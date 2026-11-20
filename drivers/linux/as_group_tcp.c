// SPDX-License-Identifier: GPL-2.0-only
#include <linux/fprobe.h>
#include <linux/ftrace.h>
#include <linux/netfilter_ipv4.h>
#include <linux/sched/signal.h>
#include <linux/sock_diag.h>
#include <net/ip.h>
#include <net/tcp.h>
#include "as_group_tcp.h"

struct as_group_call {
    struct sock *sk;
    u64 generation, cookie;
    u32 start;
    u16 sport, dport;
};
static bool attached;

static int entered(struct fprobe *probe, unsigned long ip, unsigned long parent,
                   struct ftrace_regs *regs, void *data);
static void returned(struct fprobe *probe, unsigned long ip, unsigned long parent,
                     struct ftrace_regs *regs, void *data);

static struct fprobe send_probe = {
    .entry_handler = entered, .exit_handler = returned,
    .entry_data_size = sizeof(struct as_group_call),
};
static struct fprobe recv_probe = {
    .entry_handler = entered, .exit_handler = returned,
    .entry_data_size = sizeof(struct as_group_call),
};
static struct fprobe read_sock_probe = {
    .entry_handler = entered, .exit_handler = returned,
    .entry_data_size = sizeof(struct as_group_call),
};

static bool loopback(__be32 ip)
{
    return ip == htonl(INADDR_LOOPBACK);
}

static u64 socket_cookie(struct sock *sk)
{
    u32 cookie[2];

    sock_diag_save_cookie(sk, cookie);
    return ((u64)cookie[1] << 32) | cookie[0];
}

static void observe_first_send(const struct as_group_call *call)
{
    const struct tcp_sock *tcp = tcp_sk(call->sk);
    struct as_group_tcp fact = {};

    if (!as_group_otlp(call->dport, call->generation) ||
        call->sk->sk_state != TCP_ESTABLISHED || tcp->bytes_sent ||
        !tcp_rtx_and_write_queues_empty(call->sk) ||
        tcp->snd_una != call->start || tcp->snd_nxt != call->start)
        return;
    fact.generation = cpu_to_le64(call->generation);
    fact.sport = cpu_to_le16(call->sport);
    fact.dport = cpu_to_le16(call->dport);
    fact.sequence = cpu_to_le32(call->start);
    fact.cookie = cpu_to_le64(call->cookie);
    fact.kind = 3;
    as_group_publish(&fact, NULL, 0);
}

static int entered(struct fprobe *probe, unsigned long ip, unsigned long parent,
                   struct ftrace_regs *regs, void *data)
{
    struct as_group_call *call = data;
    struct sock *sk = (void *)ftrace_regs_get_argument(regs, 0);
    struct msghdr *msg = (void *)ftrace_regs_get_argument(regs, 1);
    bool received = probe != &send_probe;
    bool read_sock = probe == &read_sock_probe;
    u32 flags;
    u16 local, remote;

    if (!call || !sk || (!read_sock && !msg) || !in_task() || sock_net(sk) != &init_net ||
        !loopback(sk->sk_rcv_saddr) || !loopback(sk->sk_daddr))
        return 1;
    if (sk->sk_family != AF_INET &&
        (sk->sk_family != AF_INET6 || !ipv6_addr_v4mapped(&sk->sk_v6_daddr)))
        return 1;
    /* The consuming read_sock path has noack=false and advances this socket's
     * copied_seq. Its second argument is a descriptor, not an msghdr. */
    if (read_sock && ((u8)ftrace_regs_get_argument(regs, 3) ||
        (u32 *)ftrace_regs_get_argument(regs, 4) != &tcp_sk(sk)->copied_seq))
        return 1;
    flags = read_sock ? 0 : received ? ftrace_regs_get_argument(regs, 3) : msg->msg_flags;
    if (tcp_sk(sk)->repair ||
        flags & (MSG_OOB | MSG_PEEK | MSG_TRUNC | MSG_ERRQUEUE | MSG_FASTOPEN))
        return 1;
    local = sk->sk_num;
    remote = ntohs(sk->sk_dport);
    call->sport = received ? remote : local;
    call->dport = received ? local : remote;
    call->generation = as_group_scope(call->sport, call->dport);
    if (!call->generation)
        return 1;
    call->sk = sk;
    call->cookie = socket_cookie(sk);
    call->start = received ? tcp_sk(sk)->copied_seq : tcp_sk(sk)->write_seq;
    if (!received && ftrace_regs_get_argument(regs, 2))
        observe_first_send(call);
    return 0;
}

static void returned(struct fprobe *probe, unsigned long ip, unsigned long parent,
                     struct ftrace_regs *regs, void *data)
{
    const struct as_group_call *call = data;
    bool received = probe != &send_probe;
    int result = (int)ftrace_regs_get_return_value(regs);
    struct as_group_tcp fact = {};
    u32 cursor;

    if (!call || result <= 0)
        return;
    cursor = received ? tcp_sk(call->sk)->copied_seq : tcp_sk(call->sk)->write_seq;
    if (cursor - call->start != (u32)result)
        return;
    fact.generation = cpu_to_le64(call->generation);
    fact.sport = cpu_to_le16(call->sport);
    fact.dport = cpu_to_le16(call->dport);
    fact.kind = received ? 2 : 1;
    fact.sequence = cpu_to_le32(call->start);
    fact.length = cpu_to_le32(result);
    fact.tgid = cpu_to_le32(task_tgid_nr(current));
    fact.birth = cpu_to_le64(READ_ONCE(current->group_leader->start_boottime));
    fact.cookie = cpu_to_le64(call->cookie);
    as_group_publish(&fact, NULL, 0);
}

static unsigned int packet(void *unused, struct sk_buff *skb, const struct nf_hook_state *state)
{
    int offset = skb_network_offset(skb), transport, end;
    struct sock *sk = state->sk;
    struct as_group_tcp fact = {};
    struct iphdr ip_buffer;
    struct tcphdr tcp_buffer;
    const struct iphdr *ip = skb_header_pointer(skb, offset, sizeof(ip_buffer), &ip_buffer);
    const struct tcphdr *tcp;
    u64 generation;

    if (!ip || ip->version != 4 || ip->ihl < 5 || ip->protocol != IPPROTO_TCP ||
        ip_is_fragment(ip) || !loopback(ip->saddr) || !loopback(ip->daddr))
        return NF_ACCEPT;
    transport = offset + ip->ihl * 4;
    tcp = skb_header_pointer(skb, transport, sizeof(tcp_buffer), &tcp_buffer);
    if (!tcp || tcp->doff < 5)
        return NF_ACCEPT;
    generation = as_group_scope(ntohs(tcp->source), ntohs(tcp->dest));
    if (!generation)
        return NF_ACCEPT;
    end = skb_is_gso(skb) ? skb->len : offset + ntohs(ip->tot_len);
    transport += tcp->doff * 4;
    if (end < transport || end > skb->len)
        return NF_ACCEPT;
    if (sk && sk_fullsock(sk) && sk->sk_type == SOCK_STREAM && sk->sk_protocol == IPPROTO_TCP)
        fact.cookie = cpu_to_le64(socket_cookie(sk));
    if (end > transport && !fact.cookie)
        return NF_ACCEPT;
    fact.generation = cpu_to_le64(generation);
    fact.sport = cpu_to_le16(ntohs(tcp->source));
    fact.dport = cpu_to_le16(ntohs(tcp->dest));
    fact.sequence = cpu_to_le32(ntohl(tcp->seq));
    fact.acknowledgment = cpu_to_le32(ntohl(tcp->ack_seq));
    fact.length = cpu_to_le32(end - transport);
    fact.flags = ((__force u8 *)tcp)[13];
    as_group_publish(&fact, skb, transport);
    return NF_ACCEPT;
}

static const struct nf_hook_ops packet_hook = {
    .hook = packet, .pf = NFPROTO_IPV4, .hooknum = NF_INET_LOCAL_OUT,
    .priority = NF_IP_PRI_LAST,
};

int as_group_tcp_start(void)
{
    int error = register_fprobe(&send_probe, "tcp_sendmsg_locked", NULL);

    if (error)
        return error;
    error = register_fprobe(&recv_probe, "tcp_recvmsg_locked", NULL);
    if (error)
        goto recv_failed;
    error = register_fprobe(&read_sock_probe, "__tcp_read_sock", NULL);
    if (error)
        goto read_sock_failed;
    error = nf_register_net_hook(&init_net, &packet_hook);
    if (error)
        goto packet_failed;
    WRITE_ONCE(attached, true);
    return 0;

packet_failed:
    unregister_fprobe(&read_sock_probe);
read_sock_failed:
    unregister_fprobe(&recv_probe);
recv_failed:
    unregister_fprobe(&send_probe);
    return error;
}

bool as_group_tcp_available(void)
{
    return READ_ONCE(attached);
}

void as_group_tcp_stop(void)
{
    if (attached) {
        nf_unregister_net_hook(&init_net, &packet_hook);
        unregister_fprobe(&read_sock_probe);
        unregister_fprobe(&recv_probe);
        unregister_fprobe(&send_probe);
    }
}
