/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef AS_GROUP_TCP_H
#define AS_GROUP_TCP_H
#include "as_protocol.h"

struct sk_buff;
int as_group_tcp_start(void);
void as_group_tcp_stop(void);
bool as_group_tcp_available(void);
u64 as_group_scope(u16 sport, u16 dport);
bool as_group_otlp(u16 dport, u64 generation);
void as_group_publish(struct as_group_tcp *fact, const struct sk_buff *skb, int offset);

static inline bool as_group_ports_valid(u32 ports, u32 gateway)
{
    u16 a2a = ports, otlp = ports >> 16;

    return gateway <= U16_MAX &&
        (!a2a || !otlp || a2a != otlp) &&
        (!gateway || (gateway != a2a && gateway != otlp));
}
#endif
