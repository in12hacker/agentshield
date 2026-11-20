/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef AS_FLOW_H
#define AS_FLOW_H
/* Host byte order except fields marked *_be. IPv4 uses word 0; the rest are zero. */
#define AS_FLOW_IPV4 4
#define AS_FLOW_IPV6 6
struct as_flow_subject { unsigned int tgid, reserved; unsigned long long birth_ns; };
struct as_flow_endpoint { unsigned int family, ip_be[4], port_be; };
struct as_flow_tuple {
    unsigned int family, local_ip_be[4], remote_ip_be[4], local_port, remote_port, reserved;
};
struct as_flow_binding {
    unsigned long long cookie, birth_ns, thread_birth_ns, observed_ns, cgroup_id;
    unsigned int tgid, tid;
    struct as_flow_tuple tuple;
};
struct as_flow_plan {
    unsigned int enabled, mark, netns, controller, all_subjects, reserved;
    unsigned long long netns_cookie, cgroup_id;
};
_Static_assert(sizeof(struct as_flow_subject) == 16, "flow subject size");
_Static_assert(sizeof(struct as_flow_endpoint) == 24, "flow endpoint size");
_Static_assert(sizeof(struct as_flow_tuple) == 48, "flow tuple size");
_Static_assert(sizeof(struct as_flow_binding) == 96, "flow binding size");
_Static_assert(sizeof(struct as_flow_plan) == 40, "flow plan size");
#define AS_FLOW_MAX 4096
#endif
