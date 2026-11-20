/* SPDX-License-Identifier: GPL-2.0-only */
/* cgroup marking + actual TCP establishment; nft performs packet DNAT, not socket rewriting. */
/* Verifier contexts use the fixed BPF UAPI. Relocate only kernel-internal
 * task fields via explicit BPF_CORE_READ. Implicit relocation of context
 * array members can turn a constant field access into context-pointer arithmetic. */
#define BPF_NO_PRESERVE_ACCESS_INDEX
#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_core_read.h>
#include <bpf/bpf_endian.h>
#include "as_flow.h"
/* Linux UAPI asm-generic/socket.h, shared by supported arm64/x86. */
#define SOL_SOCKET 1
#define SO_MARK 36
#define AF_INET 2
#define AF_INET6 10
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY); __uint(max_entries, 1);
    __type(key, __u32); __type(value, struct as_flow_plan);
} flow_plan SEC(".maps");
struct {
    __uint(type, BPF_MAP_TYPE_HASH); __uint(max_entries, AS_FLOW_MAX);
    __type(key, struct as_flow_subject); __type(value, __u32);
} flow_subjects SEC(".maps");
struct {
    __uint(type, BPF_MAP_TYPE_HASH); __uint(max_entries, AS_FLOW_MAX);
    __type(key, struct as_flow_endpoint); __type(value, __u32);
} flow_destinations SEC(".maps");
struct {
    __uint(type, BPF_MAP_TYPE_SK_STORAGE); __uint(map_flags, BPF_F_NO_PREALLOC);
    __type(key, int); __type(value, struct as_flow_binding);
} flow_storage SEC(".maps");
struct {
    __uint(type, BPF_MAP_TYPE_HASH); __uint(max_entries, AS_FLOW_MAX);
    __type(key, struct as_flow_tuple); __type(value, struct as_flow_binding);
} flow_live SEC(".maps");
static __always_inline int mark_connection(struct bpf_sock_addr *ctx, const struct as_flow_endpoint *dest)
{
    struct bpf_sock *sk = ctx->sk;
    if (!sk) return 0;
    __u32 zero = 0;
    struct as_flow_plan *plan = bpf_map_lookup_elem(&flow_plan, &zero);
    if (!plan || !plan->enabled || ctx->protocol != IPPROTO_TCP) return 1;
    __u64 ids = bpf_get_current_pid_tgid();
    __u32 tgid = ids >> 32;
    if (tgid == plan->controller) return 1;
    struct task_struct *task = bpf_get_current_task_btf();
    __u64 cgroup_id = bpf_get_current_cgroup_id();
    if (plan->cgroup_id && plan->cgroup_id != cgroup_id) return 1;
    int same_task_net = BPF_CORE_READ(task, nsproxy, net_ns, ns.inum) == plan->netns;
    if (!plan->cgroup_id && plan->all_subjects && !same_task_net) return 1;
    struct as_flow_subject subject = { .tgid = tgid, .birth_ns = BPF_CORE_READ(task, group_leader, start_boottime) };
    if (!plan->all_subjects && !bpf_map_lookup_elem(&flow_subjects, &subject)) return 1;
    if (!bpf_map_lookup_elem(&flow_destinations, dest)) return 1;
    if (!same_task_net || bpf_get_netns_cookie(ctx) != plan->netns_cookie) return 0;
    __u32 mark = 0;
    if (bpf_getsockopt(ctx, SOL_SOCKET, SO_MARK, &mark, sizeof(mark)) || mark) return 0;
    struct as_flow_binding value = {
        .cookie = bpf_get_socket_cookie(ctx), .birth_ns = subject.birth_ns,
        .thread_birth_ns = BPF_CORE_READ(task, start_boottime),
        .observed_ns = bpf_ktime_get_boot_ns(), .cgroup_id = cgroup_id,
        .tgid = tgid, .tid = (__u32)ids,
        .tuple.family = dest->family,
        .tuple.remote_port = bpf_ntohs((__u16)dest->port_be),
    };
    __builtin_memcpy(value.tuple.remote_ip_be, dest->ip_be, sizeof(dest->ip_be));
    if (!value.cookie || !value.birth_ns) return 0;
    if (bpf_sk_storage_get(&flow_storage, sk, 0, 0)) return 0;
    struct as_flow_binding *stored = bpf_sk_storage_get(&flow_storage, sk, &value, BPF_SK_STORAGE_GET_F_CREATE);
    if (!stored) return 0;
    /* The same socket may only make one selected connection attempt. */
    if (stored->cookie != value.cookie || stored->tgid != value.tgid || stored->birth_ns != value.birth_ns) return 0;
    mark = plan->mark;
    return bpf_setsockopt(ctx, SOL_SOCKET, SO_MARK, &mark, sizeof(mark)) ? 0 : 1;
}
SEC("cgroup/connect4") int as_flow_connect4(struct bpf_sock_addr *ctx)
{
    struct as_flow_endpoint dest = {
        .family = AS_FLOW_IPV4, .ip_be = { ctx->user_ip4 }, .port_be = ctx->user_port,
    };
    return mark_connection(ctx, &dest);
}
SEC("cgroup/connect6") int as_flow_connect6(struct bpf_sock_addr *ctx)
{
    struct as_flow_endpoint dest = {
        .family = AS_FLOW_IPV6,
        .ip_be = { ctx->user_ip6[0], ctx->user_ip6[1], ctx->user_ip6[2], ctx->user_ip6[3] },
        .port_be = ctx->user_port,
    };
    if (!dest.ip_be[0] && !dest.ip_be[1] && dest.ip_be[2] == bpf_htonl(0xffff)) {
        dest.family = AS_FLOW_IPV4;
        dest.ip_be[0] = dest.ip_be[3];
        dest.ip_be[2] = dest.ip_be[3] = 0;
    }
    return mark_connection(ctx, &dest);
}
SEC("sockops") int as_flow_sockops(struct bpf_sock_ops *ctx)
{
    struct bpf_sock *sk = ctx->sk;
    if (!sk) return 1;
    struct as_flow_binding *stored = bpf_sk_storage_get(&flow_storage, sk, 0, 0);
    if (!stored) return 1;
    if (ctx->op == BPF_SOCK_OPS_ACTIVE_ESTABLISHED_CB) {
        if (bpf_sock_ops_cb_flags_set(ctx, BPF_SOCK_OPS_STATE_CB_FLAG)) return 1;
        /* Read fixed context offsets before address-family branches. Otherwise
         * LLVM can merge them into a modified context pointer the verifier rejects. */
        const volatile struct bpf_sock_ops *source = ctx;
        __u32 remote4 = source->remote_ip4, local4 = source->local_ip4;
        __u32 remote6[4] = { source->remote_ip6[0], source->remote_ip6[1],
                            source->remote_ip6[2], source->remote_ip6[3] };
        __u32 local6[4] = { source->local_ip6[0], source->local_ip6[1],
                           source->local_ip6[2], source->local_ip6[3] };
        /* DNAT preserves the socket's original peer. Never use the proxy endpoint as evidence. */
        if (bpf_ntohl(ctx->remote_port) != stored->tuple.remote_port) return 1;
        if (stored->tuple.family == AS_FLOW_IPV4) {
            if (ctx->family != AF_INET && !(ctx->family == AF_INET6 &&
                !remote6[0] && !remote6[1] && remote6[2] == bpf_htonl(0xffff))) return 1;
            if (remote4 != stored->tuple.remote_ip_be[0]) return 1;
            stored->tuple.local_ip_be[0] = local4;
        } else {
            if (stored->tuple.family != AS_FLOW_IPV6 || ctx->family != AF_INET6) return 1;
#pragma unroll
            for (int i = 0; i < 4; i++) {
                if (remote6[i] != stored->tuple.remote_ip_be[i]) return 1;
                stored->tuple.local_ip_be[i] = local6[i];
            }
        }
        stored->tuple.local_port = ctx->local_port;
        bpf_map_update_elem(&flow_live, &stored->tuple, stored, BPF_NOEXIST);
    } else if (ctx->op == BPF_SOCK_OPS_STATE_CB && ctx->args[1] != BPF_TCP_ESTABLISHED) {
        struct as_flow_binding *live = bpf_map_lookup_elem(&flow_live, &stored->tuple);
        if (live && live->cookie == stored->cookie) bpf_map_delete_elem(&flow_live, &stored->tuple);
    }
    return 1;
}
char LICENSE[] SEC("license") = "GPL";
