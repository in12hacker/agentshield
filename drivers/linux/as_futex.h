/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef AS_FUTEX_H
#define AS_FUTEX_H

#include "as_protocol.h"

struct task_struct;

int as_futex_start(void);
void as_futex_stop(void);
void as_futex_task_exit(struct task_struct *task);
bool as_futex_session(u8 session[16]);
void as_futex_publish(const u8 session[16], struct as_futex_selection *fact);

#endif
