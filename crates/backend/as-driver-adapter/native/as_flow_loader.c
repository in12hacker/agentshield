/* SPDX-License-Identifier: GPL-2.0-only */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <bpf/bpf.h>
#include <bpf/libbpf.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/magic.h>
#include <linux/netfilter_ipv4.h>
#include <linux/netfilter_ipv6/ip6_tables.h>
#include <linux/nsfs.h>
#include <poll.h>
#include <sched.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/random.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/vfs.h>
#include <sys/wait.h>
#include <unistd.h>
#include "as_tls_ingress.h"
struct plan_wire { uint32_t all, subjects, destinations, max_connections; uint64_t cgroup_id; };
_Static_assert(sizeof(struct plan_wire) == 24, "flow startup size");
static int nft(const char *input, int ownerfd)
{
    int p[2], status;
    if (pipe2(p, O_CLOEXEC)) return -1;
    pid_t child = fork();
    if (child == 0) {
        if (dup2(p[0], 0) < 0 || dup2(2, 1) < 0) _exit(126);
        close(p[0]); close(p[1]);
        execl("/usr/sbin/nft", "nft", "-f", "/proc/self/fd/0", (char *)NULL);
        fprintf(stderr, "AS_TLS_INGRESS_NFT_EXEC_FAILED errno=%d\n", errno);
        _exit(127);
    }
    close(p[0]);
    size_t n = strlen(input), offset = 0;
    while (child > 0 && offset < n) {
        ssize_t rc = write(p[1], input + offset, n - offset);
        if (rc < 0 && errno == EINTR) continue;
        if (rc <= 0) break;
        offset += rc;
    }
    close(p[1]);
    if (child < 0) return -1;
    int pidfd = syscall(SYS_pidfd_open, child, 0), failed = 0, cancelled = 0;
    if (pidfd < 0) {
        fprintf(stderr, "AS_TLS_INGRESS_NFT_PIDFD_FAILED errno=%d\n", errno);
        failed = 1;
    } else {
        /* Startup observes the same authenticated owner stop as the main loop.
         * Teardown passes -1: its already-owned deletion must finish even after
         * that stop or parent exit, before the helper reports retirement. */
        struct pollfd fds[3] = {{pidfd,POLLIN,0},{ownerfd >= 0 ? 0 : -1,POLLIN,0},{ownerfd,POLLIN,0}};
        for (;;) {
            int waited = poll(fds,3,-1);
            if (waited < 0) {
                if (errno == EINTR) continue;
                fprintf(stderr, "AS_TLS_INGRESS_NFT_POLL_FAILED errno=%d\n", errno);
                failed = 1;
                break;
            }
            if (fds[1].revents || fds[2].revents) {
                fprintf(stderr, "AS_TLS_INGRESS_NFT_CANCELLED control_revents=%x parent_revents=%x\n",
                    (unsigned)fds[1].revents, (unsigned)fds[2].revents);
                cancelled = 1;
                break;
            }
            if (fds[0].revents & POLLIN) break;
            if (fds[0].revents) {
                fprintf(stderr, "AS_TLS_INGRESS_NFT_WAIT_EVENT_FAILED revents=%x\n", (unsigned)fds[0].revents);
                failed = 1;
                break;
            }
        }
        close(pidfd);
    }
    if ((failed || cancelled) && kill(child,SIGKILL) && errno != ESRCH)
        fprintf(stderr, "AS_TLS_INGRESS_NFT_CANCEL_FAILED errno=%d\n", errno);
    while (waitpid(child, &status, 0) < 0) if (errno != EINTR) return -1;
    return !failed && !cancelled && offset == n && WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : -1;
}
static int emit(unsigned int kind, unsigned int status, const struct as_flow_binding *binding, int fd)
{
    struct as_flow_wire value = { .kind = kind, .status = status };
    if (binding) value.binding = *binding;
    if (fd >= 0) {
        socklen_t size=sizeof(value.accepted_cookie);
        if (getsockopt(fd,SOL_SOCKET,SO_COOKIE,&value.accepted_cookie,&size) || !value.accepted_cookie) return -1;
    }
    struct iovec io = { &value, sizeof(value) };
    char control[CMSG_SPACE(sizeof(int))];
    struct msghdr msg = { .msg_iov = &io, .msg_iovlen = 1 };
    if (fd >= 0) {
        memset(control, 0, sizeof(control));
        msg.msg_control = control; msg.msg_controllen = sizeof(control);
        struct cmsghdr *c = CMSG_FIRSTHDR(&msg);
        c->cmsg_level = SOL_SOCKET; c->cmsg_type = SCM_RIGHTS; c->cmsg_len = CMSG_LEN(sizeof(int));
        memcpy(CMSG_DATA(c), &fd, sizeof(fd));
    }
    return sendmsg(0, &msg, MSG_NOSIGNAL | MSG_DONTWAIT) == sizeof(value) ? 0 : -1;
}
static int valid_endpoint(const struct as_flow_endpoint *endpoint)
{
    if (!endpoint->port_be || endpoint->port_be > UINT16_MAX) return 0;
    if (endpoint->family == AS_FLOW_IPV4) {
        uint32_t ip = ntohl(endpoint->ip_be[0]);
        return ip && !IN_MULTICAST(ip) && !endpoint->ip_be[1] && !endpoint->ip_be[2] && !endpoint->ip_be[3];
    }
    if (endpoint->family != AS_FLOW_IPV6) return 0;
    struct in6_addr ip;
    memcpy(&ip, endpoint->ip_be, sizeof(ip));
    return !IN6_IS_ADDR_UNSPECIFIED(&ip) && !IN6_IS_ADDR_MULTICAST(&ip) &&
           !IN6_IS_ADDR_LINKLOCAL(&ip) && !IN6_IS_ADDR_V4MAPPED(&ip);
}

static int open_listener(int family, int backlog, uint16_t *port)
{
    int fd = socket(family, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (fd < 0) return -1;
    if (family == AF_INET) {
        struct sockaddr_in addr = { .sin_family = AF_INET, .sin_addr.s_addr = htonl(INADDR_LOOPBACK) };
        socklen_t size = sizeof(addr);
        if (bind(fd, (void *)&addr, size) || getsockname(fd, (void *)&addr, &size)) goto failed;
        *port = ntohs(addr.sin_port);
    } else {
        struct sockaddr_in6 addr = { .sin6_family = AF_INET6, .sin6_addr = IN6ADDR_LOOPBACK_INIT };
        socklen_t size = sizeof(addr);
        int only = 1;
        if (setsockopt(fd, IPPROTO_IPV6, IPV6_V6ONLY, &only, sizeof(only)) ||
            bind(fd, (void *)&addr, size) || getsockname(fd, (void *)&addr, &size)) goto failed;
        *port = ntohs(addr.sin6_port);
    }
    if (*port && !listen(fd, backlog)) return fd;
failed:
    close(fd);
    return -1;
}

static int original_tuple(int fd, int family, struct as_flow_tuple *tuple)
{
    if (family == AF_INET) {
        struct sockaddr_in peer, original;
        socklen_t peer_size = sizeof(peer), original_size = sizeof(original);
        if (getpeername(fd, (void *)&peer, &peer_size) ||
            getsockopt(fd, IPPROTO_IP, SO_ORIGINAL_DST, &original, &original_size) ||
            peer_size != sizeof(peer) || original_size != sizeof(original) ||
            peer.sin_family != AF_INET || original.sin_family != AF_INET) return -1;
        tuple->family = AS_FLOW_IPV4;
        tuple->local_ip_be[0] = peer.sin_addr.s_addr;
        tuple->remote_ip_be[0] = original.sin_addr.s_addr;
        tuple->local_port = ntohs(peer.sin_port);
        tuple->remote_port = ntohs(original.sin_port);
    } else {
        struct sockaddr_in6 peer, original;
        socklen_t peer_size = sizeof(peer), original_size = sizeof(original);
        if (getpeername(fd, (void *)&peer, &peer_size) ||
            getsockopt(fd, IPPROTO_IPV6, IP6T_SO_ORIGINAL_DST, &original, &original_size) ||
            peer_size != sizeof(peer) || original_size != sizeof(original) ||
            peer.sin6_family != AF_INET6 || original.sin6_family != AF_INET6 ||
            peer.sin6_scope_id || original.sin6_scope_id) return -1;
        tuple->family = AS_FLOW_IPV6;
        memcpy(tuple->local_ip_be, &peer.sin6_addr, sizeof(peer.sin6_addr));
        memcpy(tuple->remote_ip_be, &original.sin6_addr, sizeof(original.sin6_addr));
        tuple->local_port = ntohs(peer.sin6_port);
        tuple->remote_port = ntohs(original.sin6_port);
    }
    return 0;
}

int main(int argc, char **argv)
{
    int rc = 1, cgroup = -1, netns = -1, parentfd = -1, installed = 0;
    struct bpf_object *object = NULL;
    struct bpf_link *connect4 = NULL, *connect6 = NULL, *state_link = NULL;
    int listener[2] = {-1, -1};
    uint16_t ports[2] = {0};
    const int families[2] = {AF_INET, AF_INET6};
    const char *nfproto[2] = {"ipv4", "ipv6"};
    unsigned enabled = 0;
    struct stat own_ns, wanted_ns, cg_stat;
    struct statfs cg_fs;
    struct ucred parent; socklen_t plen = sizeof(parent);
    uint32_t mark = 0, zero = 0, one = 1;
    char table[64] = {0}, command[512];
    signal(SIGPIPE, SIG_IGN);
    if (argc != 2 || geteuid() != 0 ||
        getsockopt(0, SOL_SOCKET, SO_PEERCRED, &parent, &plen) || parent.pid != getppid() || parent.uid != 0 ||
        stat("/proc/self/ns/net", &own_ns) ||
        getrandom(&mark, sizeof(mark), 0) != sizeof(mark) || !mark) goto done;
    parentfd = syscall(SYS_pidfd_open, parent.pid, 0);
    if (parentfd < 0) goto done;
    int pass = 1;
    if (setsockopt(0, SOL_SOCKET, SO_PASSCRED, &pass, sizeof(pass))) goto done;
    size_t capacity = sizeof(struct plan_wire) + AS_FLOW_MAX * (sizeof(struct as_flow_subject) + sizeof(struct as_flow_endpoint));
    unsigned char *plan_bytes = calloc(1, capacity);
    if (!plan_bytes) goto done;
    char ancillary[CMSG_SPACE(sizeof(struct ucred)) + CMSG_SPACE(2 * sizeof(int))];
    struct iovec io = { plan_bytes, capacity };
    struct msghdr packet = { .msg_iov=&io, .msg_iovlen=1, .msg_control=ancillary, .msg_controllen=sizeof(ancillary) };
    ssize_t count = recvmsg(0, &packet, MSG_CMSG_CLOEXEC);
    int authenticated = 0;
    for (struct cmsghdr *c = CMSG_FIRSTHDR(&packet); c; c = CMSG_NXTHDR(&packet,c)) {
        if (c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_CREDENTIALS && c->cmsg_len == CMSG_LEN(sizeof(struct ucred))) {
            struct ucred actual; memcpy(&actual,CMSG_DATA(c),sizeof(actual));
            authenticated = actual.pid == parent.pid && actual.uid == parent.uid;
        } else if (c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS && c->cmsg_len == CMSG_LEN(2 * sizeof(int))) {
            int handles[2]; memcpy(handles, CMSG_DATA(c), sizeof(handles));
            cgroup = handles[0]; netns = handles[1];
        }
    }
    if (!authenticated || packet.msg_flags & (MSG_TRUNC|MSG_CTRUNC) || count < (ssize_t)sizeof(struct plan_wire)) { free(plan_bytes); goto done; }
    struct plan_wire wire; memcpy(&wire,plan_bytes,sizeof(wire));
    size_t expected = sizeof(wire) + wire.subjects * sizeof(struct as_flow_subject) + wire.destinations * sizeof(struct as_flow_endpoint);
    if (wire.subjects > AS_FLOW_MAX || !wire.destinations || wire.destinations > AS_FLOW_MAX || !wire.max_connections || wire.max_connections > AS_FLOW_MAX || wire.all > 1 || (wire.all ? wire.subjects != 0 : wire.subjects == 0) || count != (ssize_t)expected) { free(plan_bytes); goto done; }
    if (cgroup < 0 || netns < 0 || fstatfs(cgroup, &cg_fs) || cg_fs.f_type != CGROUP2_SUPER_MAGIC ||
        fstat(cgroup, &cg_stat) || !S_ISDIR(cg_stat.st_mode) ||
        (wire.cgroup_id && cg_stat.st_ino != wire.cgroup_id) ||
        ioctl(netns, NS_GET_NSTYPE) != CLONE_NEWNET || fstat(netns, &wanted_ns)) { free(plan_bytes); goto done; }
    int same_ns = wanted_ns.st_dev == own_ns.st_dev && wanted_ns.st_ino == own_ns.st_ino;
    if (!!wire.cgroup_id == same_ns || (!same_ns && setns(netns, CLONE_NEWNET)) ||
        stat("/proc/self/ns/net", &own_ns) || own_ns.st_dev != wanted_ns.st_dev ||
        own_ns.st_ino != wanted_ns.st_ino) { free(plan_bytes); goto done; }
    object = bpf_object__open_file(argv[1], NULL);
    if (libbpf_get_error(object)) { object = NULL; free(plan_bytes); goto done; }
    struct bpf_map *sm = bpf_object__find_map_by_name(object, "flow_subjects");
    struct bpf_map *dm = bpf_object__find_map_by_name(object, "flow_destinations");
    struct bpf_map *lm = bpf_object__find_map_by_name(object, "flow_live");
    if (!sm || !dm || !lm || bpf_map__set_max_entries(sm, wire.subjects ? wire.subjects : 1) ||
        bpf_map__set_max_entries(dm, wire.destinations) || bpf_map__set_max_entries(lm, wire.max_connections)) {
        free(plan_bytes); goto done;
    }
    if (bpf_object__load(object)) { free(plan_bytes); goto done; }
    int subjects = bpf_object__find_map_fd_by_name(object, "flow_subjects");
    int destinations = bpf_object__find_map_fd_by_name(object, "flow_destinations");
    int plans = bpf_object__find_map_fd_by_name(object, "flow_plan");
    int live = bpf_object__find_map_fd_by_name(object, "flow_live");
    if (subjects < 0 || destinations < 0 || plans < 0 || live < 0) { free(plan_bytes); goto done; }
    size_t off = sizeof(wire);
    for (unsigned i=0;i<wire.subjects;i++,off+=sizeof(struct as_flow_subject)) {
        struct as_flow_subject item; memcpy(&item,plan_bytes+off,sizeof(item));
        if (!item.tgid || item.reserved || !item.birth_ns || bpf_map_update_elem(subjects,&item,&one,BPF_NOEXIST)) { free(plan_bytes); goto done; }
    }
    for (unsigned i=0;i<wire.destinations;i++,off+=sizeof(struct as_flow_endpoint)) {
        struct as_flow_endpoint item; memcpy(&item,plan_bytes+off,sizeof(item));
        if (!valid_endpoint(&item) || bpf_map_update_elem(destinations,&item,&one,BPF_NOEXIST)) { free(plan_bytes); goto done; }
        enabled |= item.family == AS_FLOW_IPV4 ? 1 : 2;
    }
    free(plan_bytes);
    snprintf(table,sizeof(table),"as_tls_%x_%x",(unsigned)getpid(),mark);
    int length = snprintf(command, sizeof(command),
        "add table inet %s\nadd chain inet %s output { type nat hook output priority -105; policy accept; }\n",
        table, table);
    unsigned long long netns_cookie = 0;
    for (unsigned i = 0; i < 2; i++) {
        if (!(enabled & (1U << i))) continue;
        listener[i] = open_listener(families[i], (int)wire.max_connections, &ports[i]);
        if (listener[i] < 0) goto done;
        unsigned long long cookie = 0;
        socklen_t size = sizeof(cookie);
        if (getsockopt(listener[i], SOL_SOCKET, SO_NETNS_COOKIE, &cookie, &size) ||
            !cookie || (netns_cookie && netns_cookie != cookie)) goto done;
        netns_cookie = cookie;
        if (length < 0 || (size_t)length >= sizeof(command)) goto done;
        int added = snprintf(command + length, sizeof(command) - (size_t)length,
            "add rule inet %s output meta nfproto %s meta l4proto tcp meta mark 0x%x redirect to :%u\n",
            table, nfproto[i], mark, ports[i]);
        if (added < 0) goto done;
        length += added;
    }
    if (!netns_cookie || (size_t)length >= sizeof(command)) goto done;
    installed = 1;
    if (nft(command,parentfd)) goto done;
    struct bpf_program *cp4=bpf_object__find_program_by_name(object,"as_flow_connect4");
    struct bpf_program *cp6=bpf_object__find_program_by_name(object,"as_flow_connect6");
    struct bpf_program *sp=bpf_object__find_program_by_name(object,"as_flow_sockops");
    if (!cp4 || !cp6 || !sp) goto done;
    state_link=bpf_program__attach_cgroup(sp,cgroup);
    if (libbpf_get_error(state_link)) { state_link=NULL; goto done; }
    connect4=bpf_program__attach_cgroup(cp4,cgroup);
    if (libbpf_get_error(connect4)) { connect4=NULL; goto done; }
    connect6=bpf_program__attach_cgroup(cp6,cgroup);
    if (libbpf_get_error(connect6)) { connect6=NULL; goto done; }
    struct as_flow_plan plan = { .enabled=1, .mark=mark, .netns=(uint32_t)own_ns.st_ino, .controller=(uint32_t)parent.pid, .all_subjects=wire.all, .netns_cookie=netns_cookie, .cgroup_id=wire.cgroup_id };
    if (bpf_map_update_elem(plans,&zero,&plan,BPF_ANY) || emit(1,0,NULL,-1)) goto done;
    struct pollfd fds[4] = {{0,POLLIN,0},{parentfd,POLLIN,0},{listener[0],POLLIN,0},{listener[1],POLLIN,0}};
    for (;;) {
        if (poll(fds,4,-1)<0) { if(errno==EINTR)continue; goto done; }
        if (fds[0].revents || fds[1].revents) { rc=0; break; }
        for (unsigned i = 0; i < 2; i++) {
            if (fds[i+2].revents & (POLLERR|POLLHUP|POLLNVAL)) goto done;
            if (!(fds[i+2].revents & POLLIN)) continue;
            int client=accept4(listener[i],NULL,NULL,SOCK_CLOEXEC|SOCK_NONBLOCK);
            if (client<0) { if (errno==EAGAIN||errno==EINTR) continue; goto done; }
            struct as_flow_tuple tuple = {0};
            struct as_flow_binding binding;
            if (!original_tuple(client,families[i],&tuple) && !bpf_map_lookup_and_delete_elem(live,&tuple,&binding) &&
                binding.cookie && binding.tgid && binding.birth_ns) {
                (void)emit(2,0,&binding,client);
            }
            close(client);
        }
    }
 done:
    if (object) {
        int map=bpf_object__find_map_fd_by_name(object,"flow_plan");
        struct as_flow_plan disabled={0};
        if(map>=0) (void)bpf_map_update_elem(map,&zero,&disabled,BPF_ANY);
    }
    bpf_link__destroy(connect6); bpf_link__destroy(connect4); bpf_link__destroy(state_link);
    if(installed) { snprintf(command,sizeof(command),"delete table inet %s\n",table); if(nft(command,-1)) rc=1; }
    for (unsigned i = 0; i < 2; i++) if (listener[i] >= 0) close(listener[i]);
    if(cgroup>=0)close(cgroup);
    if(netns>=0)close(netns);
    if(parentfd>=0)close(parentfd);
    bpf_object__close(object);
    if(rc==0) (void)emit(3,0,NULL,-1);
    return rc;
}
