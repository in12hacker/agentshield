/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef AS_TLS_INGRESS_H
#define AS_TLS_INGRESS_H
#include "as_flow.h"
/* Userspace adapter-private SOCK_SEQPACKET/SCM_RIGHTS response. The kernel
 * neither consumes this record nor transfers descriptors through as_protocol.h. */
struct as_flow_wire {
    unsigned int kind; /* 1 ready, 2 admitted connection, 3 stopped. */
    unsigned int status; /* Current records require zero. */
    struct as_flow_binding binding; /* Actual original socket; zero outside kind 2. */
    unsigned long long accepted_cookie; /* SO_COOKIE of passed FD; zero without FD. */
};
_Static_assert(sizeof(struct as_flow_wire) == 112, "flow reply size");
#endif
