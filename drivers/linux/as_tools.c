// SPDX-License-Identifier: GPL-2.0-only
/* Stateless process primitives; lifetime/signalling follow kernel pid.c/signal.c. */
#include <linux/cgroup.h>
#include <linux/cred.h>
#include <linux/errno.h>
#include <linux/ktime.h>
#include <linux/pid_namespace.h>
#include <linux/sched/signal.h>
#include <linux/unaligned.h>
#include "as_protocol.h"
#include "as_tools.h"

static struct pid *host_pid(u32 number)
{
    struct pid *pid;
    rcu_read_lock();
    pid = get_pid(find_pid_ns(number, &init_pid_ns));
    rcu_read_unlock();
    return pid;
}

int as_tools_inspect_processes(const __le32 *pids, u32 count, struct as_process_binding *output)
{
    u32 i;
    if (!task_is_in_init_pid_ns(current)) return -EACCES;
    for (i = 0; i < count; i++) if (!get_unaligned_le32(pids + i)) return -EINVAL;
    for (i = 0; i < count; i++) {
        struct as_process_binding *entry = output + i;
        struct task_struct *task, *parent;
        u32 number = get_unaligned_le32(pids + i);
        struct pid *pid;
        memset(entry, 0, sizeof(*entry));
        entry->pid = cpu_to_le32(number); entry->begin_ns = cpu_to_le64(ktime_get_boottime_ns());
        pid = host_pid(number);
        task = pid ? get_pid_task(pid, PIDTYPE_TGID) : NULL;
        put_pid(pid);
        if (!task) entry->status = cpu_to_le32(-ESRCH);
        else {
            entry->birth = cpu_to_le64(task->start_boottime);
            entry->kind = cpu_to_le32((task->flags & PF_KTHREAD) ? 1 : 0);
            entry->exec_cookie = cpu_to_le64(READ_ONCE(task->self_exec_id));
            rcu_read_lock(); parent = rcu_dereference(task->real_parent)->group_leader;
            entry->parent_pid = cpu_to_le32(task_tgid_nr(parent));
            entry->parent_birth = cpu_to_le64(parent->start_boottime);
            entry->cgroup = cpu_to_le64(cgroup_id(task_dfl_cgroup(task)));
            rcu_read_unlock(); put_task_struct(task);
        }
        entry->end_ns = cpu_to_le64(ktime_get_boottime_ns());
    }
    return 0;
}

static int kill_bound(struct pid *pid, const u64 *birth, struct pid *controller, const struct cred *authority)
{
    struct task_struct *task;
    const struct cred *saved = NULL;
    int error;
    if (!pid) return -ESRCH;
    if (pid_nr_ns(pid, &init_pid_ns) == 1) return -EPERM;
    task = get_pid_task(pid, PIDTYPE_TGID);
    if (!task) return -ESRCH;
    if (birth && task->start_boottime != *birth) error = -ESTALE;
    else if ((task->flags & PF_KTHREAD) || pid == controller) error = -EPERM;
    else error = 0;
    put_task_struct(task);
    /* Adopted rules use their authenticated issuer, never the blocked actor's privilege. */
    if (!error && authority) saved = override_creds(authority);
    /* priv=0 retains both credential and LSM checks. No numeric relookup. */
    if (!error) error = kill_pid(pid, SIGKILL, 0);
    if (saved) revert_creds(saved);
    return error;
}

int as_tools_kill_process(u32 number, u64 birth, struct pid *controller)
{
    struct pid *pid;
    int error;
    if (!number || number > INT_MAX) return -EINVAL;
    if (!task_is_in_init_pid_ns(current)) return -EACCES;
    pid = host_pid(number);
    error = kill_bound(pid, &birth, controller, NULL);
    put_pid(pid);
    return error;
}

int as_tools_kill_bound_process(struct pid *target, struct pid *controller, const struct cred *authority)
{
    if (!authority) return -EACCES;
    return kill_bound(target, NULL, controller, authority);
}
