/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef AS_TOOLS_H
#define AS_TOOLS_H
#include <linux/types.h>
struct pid;
struct cred;
struct as_process_binding;
/* DriverCore owns authentication, admission and output capacity. No retained state. */
int as_tools_inspect_processes(const __le32 *pids, u32 count, struct as_process_binding *output);
/* Zero means signal accepted, not process exit. -ESRCH means the object is gone. */
int as_tools_kill_process(u32 host_tgid, u64 birth_boottime_ns, struct pid *controller);
/* Caller owns both pid references and the adopted policy credential for this synchronous call. */
int as_tools_kill_bound_process(struct pid *target, struct pid *controller, const struct cred *authority);
#endif
