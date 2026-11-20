// SPDX-License-Identifier: GPL-2.0-only
/* Only observe the original native FUTEX_WAIT_PRIVATE/FUTEX_WAKE_PRIVATE path. */
#include <linux/fprobe.h>
#include <linux/futex.h>
#include <linux/hashtable.h>
#include <linux/ktime.h>
#include <linux/printk.h>
#include <linux/pid.h>
#include <linux/preempt.h>
#include <linux/ptrace.h>
#include <linux/sched.h>
#include <linux/spinlock.h>
#include <asm/syscall.h>
#include "as_futex.h"

struct as_futex_wait;
struct as_futex_mark;

struct as_futex_call {
    struct hlist_node link;
    struct task_struct *task;
    struct as_futex_wait *wait;
    struct as_futex_mark *mark;
    u8 session[16];
    u32 op;
};

struct as_futex_wait {
    struct hlist_node link;
    struct as_futex_call *call;
    const void *entry;
    struct as_sync_thread thread;
    u64 id, observed_ns;
};

struct as_futex_mark {
    const void *wake_queue;
    struct as_sync_thread waiter;
    u64 wait_id, wait_ns;
    bool consumed;
};

static DEFINE_SPINLOCK(futex_lock);
static DEFINE_HASHTABLE(futex_calls, 8);
static DEFINE_HASHTABLE(futex_waits, 8);
static u64 wait_sequence;
static bool enabled;

static struct as_sync_thread thread_facts(struct task_struct *task)
{
    return (struct as_sync_thread) {
        .tgid = cpu_to_le32(task_tgid_nr(task)),
        .tid = cpu_to_le32(task_pid_nr(task)),
        .birth_boottime_ns = cpu_to_le64(READ_ONCE(task->start_boottime)),
    };
}

static struct as_futex_call *call_locked(void)
{
    struct as_futex_call *call;

    hash_for_each_possible(futex_calls, call, link, (unsigned long)current)
        if (call->task == current)
            return call;
    return NULL;
}

static void detach_call_locked(struct as_futex_call *call)
{
    if (call->wait) {
        hlist_del_init(&call->wait->link);
        call->wait = NULL;
    }
    call->mark = NULL;
    hlist_del_init(&call->link);
}

static int syscall_enter(struct fprobe *fp, unsigned long ip, unsigned long parent_ip,
                         struct ftrace_regs *regs, void *data)
{
    struct as_futex_call *call = data;
    struct pt_regs *sysregs;
    unsigned long args[6], flags;
    u32 op;

    if (!call || !READ_ONCE(enabled) || !in_task())
        return 1;
    sysregs = (void *)ftrace_regs_get_argument(regs, 0);
    if (!sysregs)
        return 1;
    syscall_get_arguments(current, sysregs, args);
    op = (u32)args[1];
    if (op != FUTEX_WAIT_PRIVATE && op != FUTEX_WAKE_PRIVATE)
        return 1;
    if (!as_futex_session(call->session))
        return 1;
    call->task = current;
    call->op = op;
    call->wait = NULL;
    call->mark = NULL;
    spin_lock_irqsave(&futex_lock, flags);
    if (!enabled || call_locked()) {
        spin_unlock_irqrestore(&futex_lock, flags);
        return 1;
    }
    hash_add(futex_calls, &call->link, (unsigned long)current);
    spin_unlock_irqrestore(&futex_lock, flags);
    return 0;
}

static void syscall_leave(struct fprobe *fp, unsigned long ip, unsigned long ret_ip,
                          struct ftrace_regs *regs, void *data)
{
    unsigned long flags;

    spin_lock_irqsave(&futex_lock, flags);
    if (enabled)
        detach_call_locked(data);
    spin_unlock_irqrestore(&futex_lock, flags);
}

static int wait_enter(struct fprobe *fp, unsigned long ip, unsigned long parent_ip,
                      struct ftrace_regs *regs, void *data)
{
    struct as_futex_wait *wait = data;
    struct as_futex_call *call;
    unsigned long flags;

    if (!wait || !READ_ONCE(enabled) || !in_task())
        return 1;
    spin_lock_irqsave(&futex_lock, flags);
    call = enabled ? call_locked() : NULL;
    if (!call || call->op != FUTEX_WAIT_PRIVATE || call->wait || wait_sequence == U64_MAX) {
        spin_unlock_irqrestore(&futex_lock, flags);
        return 1;
    }
    wait->entry = (const void *)ftrace_regs_get_argument(regs, 0);
    wait->call = call;
    wait->thread = thread_facts(current);
    wait->id = ++wait_sequence;
    wait->observed_ns = ktime_get_boottime_ns();
    call->wait = wait;
    hash_add(futex_waits, &wait->link, (unsigned long)wait->entry);
    spin_unlock_irqrestore(&futex_lock, flags);
    return 0;
}

static void wait_leave(struct fprobe *fp, unsigned long ip, unsigned long ret_ip,
                       struct ftrace_regs *regs, void *data)
{
    struct as_futex_wait *wait = data;
    unsigned long flags;

    spin_lock_irqsave(&futex_lock, flags);
    if (enabled && !hlist_unhashed(&wait->link)) {
        hlist_del_init(&wait->link);
        wait->call->wait = NULL;
    }
    spin_unlock_irqrestore(&futex_lock, flags);
}

/* At entry the kernel still holds the bucket lock and the selected q is live.
 * Copy the wait observation here because the waiter may release q before
 * wake_q_add_safe is entered. The latter only supplies its actual task. */
static int mark_enter(struct fprobe *fp, unsigned long ip, unsigned long parent_ip,
                      struct ftrace_regs *regs, void *data)
{
    struct as_futex_mark *mark = data;
    struct as_futex_wait *wait;
    struct as_futex_call *call;
    const void *entry;
    unsigned long flags;

    if (!mark || !READ_ONCE(enabled) || !in_task())
        return 1;
    entry = (const void *)ftrace_regs_get_argument(regs, 1);
    spin_lock_irqsave(&futex_lock, flags);
    call = enabled ? call_locked() : NULL;
    if (!call || call->op != FUTEX_WAKE_PRIVATE || call->mark) {
        spin_unlock_irqrestore(&futex_lock, flags);
        return 1;
    }
    mark->wake_queue = (const void *)ftrace_regs_get_argument(regs, 0);
    mark->consumed = false;
    mark->wait_id = 0;
    hash_for_each_possible(futex_waits, wait, link, (unsigned long)entry) {
        if (wait->entry == entry && !memcmp(wait->call->session, call->session, 16)) {
            if (mark->wait_id) {
                mark->wait_id = 0;
                break;
            }
            mark->wait_id = wait->id;
            mark->wait_ns = wait->observed_ns;
            mark->waiter = wait->thread;
        }
    }
    call->mark = mark;
    spin_unlock_irqrestore(&futex_lock, flags);
    return 0;
}

static void mark_leave(struct fprobe *fp, unsigned long ip, unsigned long ret_ip,
                       struct ftrace_regs *regs, void *data)
{
    struct as_futex_mark *mark = data;
    struct as_futex_call *call;
    unsigned long flags;

    spin_lock_irqsave(&futex_lock, flags);
    call = enabled ? call_locked() : NULL;
    if (call && call->mark == mark)
        call->mark = NULL;
    spin_unlock_irqrestore(&futex_lock, flags);
}

static int selected_task(struct fprobe *fp, unsigned long ip, unsigned long parent_ip,
                         struct ftrace_regs *regs, void *data)
{
    struct as_futex_selection fact = {};
    struct as_futex_call *call;
    struct as_futex_mark *mark;
    unsigned long flags;
    u8 session[16];

    if (!READ_ONCE(enabled) || !in_task())
        return 0;
    spin_lock_irqsave(&futex_lock, flags);
    call = enabled ? call_locked() : NULL;
    mark = call ? call->mark : NULL;
    if (!mark || mark->consumed ||
        mark->wake_queue != (const void *)ftrace_regs_get_argument(regs, 0)) {
        spin_unlock_irqrestore(&futex_lock, flags);
        return 0;
    }
    mark->consumed = true;
    fact.waker = thread_facts(current);
    fact.waiter = thread_facts((void *)ftrace_regs_get_argument(regs, 1));
    fact.selected_boottime_ns = cpu_to_le64(ktime_get_boottime_ns());
    if (mark->wait_id && !memcmp(&mark->waiter, &fact.waiter, sizeof(fact.waiter))) {
        fact.wait_id = cpu_to_le64(mark->wait_id);
        fact.wait_boottime_ns = cpu_to_le64(mark->wait_ns);
    }
    memcpy(session, call->session, sizeof(session));
    spin_unlock_irqrestore(&futex_lock, flags);
    as_futex_publish(session, &fact);
    return 0;
}

void as_futex_task_exit(struct task_struct *task)
{
    struct as_futex_call *call;
    struct hlist_node *next;
    unsigned long flags;

    if (!READ_ONCE(enabled))
        return;
    spin_lock_irqsave(&futex_lock, flags);
    hash_for_each_possible_safe(futex_calls, call, next, link, (unsigned long)task)
        if (call->task == task)
            detach_call_locked(call);
    spin_unlock_irqrestore(&futex_lock, flags);
}

#if defined(CONFIG_ARM64)
#define AS_FUTEX_WRAPPER "__arm64_sys_futex"
#elif defined(CONFIG_X86_64)
#define AS_FUTEX_WRAPPER "__x64_sys_futex"
#endif
static struct {
    const char *name;
    struct fprobe probe;
} sources[] = {
    { AS_FUTEX_WRAPPER, { .entry_handler = syscall_enter, .exit_handler = syscall_leave,
                         .entry_data_size = sizeof(struct as_futex_call) } },
    { "futex_wake_mark", { .entry_handler = mark_enter, .exit_handler = mark_leave,
                         .entry_data_size = sizeof(struct as_futex_mark) } },
    { "wake_q_add_safe", { .entry_handler = selected_task } },
};
static unsigned int registered;
static bool wait_registered;
static struct fprobe wait_source = {
    .entry_handler = wait_enter,
    .exit_handler = wait_leave,
    .entry_data_size = sizeof(struct as_futex_wait),
};

int as_futex_start(void)
{
    unsigned long flags;
    int error;

    for (; registered < ARRAY_SIZE(sources); registered++) {
        error = register_fprobe_syms(&sources[registered].probe, &sources[registered].name, 1);
        if (error) {
            as_futex_stop();
            return error;
        }
    }
    error = register_fprobe(&wait_source, "futex_do_wait", NULL);
    wait_registered = !error;
    if (error)
        pr_info("as_wait: private futex wait proof unavailable: %d\n", error);
    spin_lock_irqsave(&futex_lock, flags);
    WRITE_ONCE(enabled, true);
    spin_unlock_irqrestore(&futex_lock, flags);
    return 0;
}

void as_futex_stop(void)
{
    unsigned long flags;

    spin_lock_irqsave(&futex_lock, flags);
    WRITE_ONCE(enabled, false);
    hash_init(futex_calls);
    hash_init(futex_waits);
    spin_unlock_irqrestore(&futex_lock, flags);
    if (wait_registered) {
        unregister_fprobe(&wait_source);
        wait_registered = false;
    }
    while (registered)
        unregister_fprobe(&sources[--registered].probe);
}
