// SPDX-License-Identifier: GPL-2.0-only
/* Actual libuv callback occurrences and their live object handoff. No Agent code,
 * fixed PC/CB/LR, tool-field prerequisite, or deferred target-memory sampling. */
#include <linux/elf.h>
#include <linux/err.h>
#include <linux/fs.h>
#include <linux/hashtable.h>
#include <linux/mm.h>
#include <linux/mutex.h>
#include <linux/pid_namespace.h>
#include <linux/refcount.h>
#include <linux/sched/mm.h>
#include <linux/sched/signal.h>
#include <linux/slab.h>
#include <linux/uaccess.h>
#include <linux/unaligned.h>
#include <linux/uprobes.h>
#include <linux/workqueue.h>
#include <linux/file.h>
#include <linux/sched/task.h>
#include <asm/processor.h>
#include "as_runtime.h"
#include "as_runtime_capture.h"

enum runtime_field {
#define AS_RUNTIME_FIELD(name) RT_##name,
#include "as_runtime_fields.h"
#undef AS_RUNTIME_FIELD
    RT_FIELDS
};
enum runtime_probe { UV_OPEN, UV_SUBMIT, UV_WORK, UV_DONE, UV_CLEANUP, UV_SPAWN, UV_CONNECT, UV_WRITE2, UV_UDP_SEND, UV_WRITE, UV_TRY_WRITE, UV_TRY_WRITE2, UV_PROBES };
static bool immediate_write(enum runtime_probe role)
{ return role == UV_TRY_WRITE || role == UV_TRY_WRITE2; }
struct runtime_probe_entry {
    struct uprobe_consumer consumer;
    struct uprobe *registered;
    struct as_runtime_profile *profile;
    enum runtime_probe role;
};
struct as_runtime_profile {
    struct list_head link;
    struct work_struct release;
    struct workqueue_struct *release_queue;
    struct task_struct *task;
    struct file *file;
    struct mm_struct *mm;
    u32 tgid;
    u64 birth, exec_cookie;
    u16 mask;
    u64 offsets[UV_PROBES];
    bool active;
    u64 available[DIV_ROUND_UP(RT_FIELDS, 64)], values[RT_FIELDS];
    struct runtime_probe_entry probes[UV_PROBES];
};
struct runtime_request {
    struct hlist_node link;
    refcount_t refs;
    struct as_runtime_profile *profile;
    u64 req, work, result_offset;
    u32 operation;
    enum runtime_probe role;
    struct as_runtime_key key;
    bool active, work_known, key_assigned, result_known, ambiguous;
};
struct as_runtime_scope {
    struct hlist_node link;
    refcount_t refs;
    struct as_runtime_profile *profile;
    struct task_struct *worker;
    struct runtime_request *request;
    /* A fork transfers this identity to the real child task. It must not
     * retain the parent's profile or request after the parent retires. */
    struct as_runtime_key inherited_request;
    u64 work;
    u64 cookie, consumer_id;
    enum runtime_probe role;
    bool active, inherited, inherited_key_assigned;
};
static DEFINE_SPINLOCK(runtime_lock);
static LIST_HEAD(profiles);
static DEFINE_HASHTABLE(requests, 8);
static DEFINE_HASHTABLE(workers, 8);
static unsigned int request_count;
static u64 scope_cookie;
static u8 runtime_source[16];

static bool field(const struct as_runtime_profile *p, enum runtime_field f)
{ return p->available[f / 64] & BIT_ULL(f % 64); }
static void request_put(struct runtime_request *r)
{ if (r && refcount_dec_and_test(&r->refs)) kfree(r); }
void as_runtime_scope_put(struct as_runtime_scope *s)
{
    if (s && refcount_dec_and_test(&s->refs)) { put_task_struct(s->worker); request_put(s->request); kfree(s); }
}
static bool matches(const struct as_runtime_profile *p)
{
    return READ_ONCE(p->active) && current->mm == p->mm &&
        task_tgid_nr(current) == p->tgid && current->group_leader->start_boottime == p->birth &&
        READ_ONCE(current->group_leader->self_exec_id) == p->exec_cookie;
}
static bool filter(struct uprobe_consumer *consumer, struct mm_struct *mm)
{
    struct runtime_probe_entry *probe = container_of(consumer, struct runtime_probe_entry, consumer);
    return mm == probe->profile->mm;
}
static struct runtime_request *find_request(struct as_runtime_profile *p, u64 address, bool work)
{
    struct runtime_request *r, *found = NULL;
    u64 req = address;
    if (work) {
        if (!field(p, RT_UV_REQ_WORK) || address < p->values[RT_UV_REQ_WORK]) return NULL;
        req -= p->values[RT_UV_REQ_WORK];
    }
    hash_for_each_possible(requests, r, link, (unsigned long)req) {
        if (r->profile != p || !r->active || r->ambiguous ||
            (work ? !r->work_known || r->work != address : r->req != address)) continue;
        if (found) return NULL;
        found = r;
    }
    if (found) refcount_inc(&found->refs);
    return found;
}
bool as_runtime_scope_active(const struct as_runtime_scope *s)
{ return s && READ_ONCE(s->active); }
/* The kernel may decline to arm a uretprobe after our entry callback. Only its
 * actual public return-consumer session can retain a syscall association. */
static bool return_session(const struct as_runtime_scope *s, bool bound_stack)
{
    struct return_instance *ri;
    struct uprobe_task *utask = current->utask;
    unsigned long sp = user_stack_pointer(task_pt_regs(current));
    int i;
    if (!utask) return false;
    for (ri = READ_ONCE(utask->return_instances); ri; ri = ri->next) {
        if (bound_stack && sp >= ri->stack) continue;
        for (i = 0; i < ri->cons_cnt; i++) {
            const struct return_consumer *c = i ? &ri->extra_consumers[i - 1] : &ri->consumer;
            if (c->id == s->consumer_id && c->cookie == s->cookie) return true;
        }
    }
    return false;
}
static const struct as_runtime_key *request_reference(const struct runtime_request *r)
{ return r && READ_ONCE(r->active) && !READ_ONCE(r->ambiguous) && READ_ONCE(r->key_assigned) ? &r->key : NULL; }
static u32 request_operation(const struct runtime_request *r)
{ return r && READ_ONCE(r->active) && !READ_ONCE(r->ambiguous) ? r->operation : 0; }
/* Enclosing write APIs may flush older queued requests: their dynamic scope is
 * retained as evidence, but is never asserted as owning the emitted packet. */
static bool exact_scope(const struct as_runtime_scope *s)
{
    const struct runtime_request *r = s->request;
    if (s->inherited) return READ_ONCE(s->inherited_key_assigned);
    if (!r || READ_ONCE(r->ambiguous) || !READ_ONCE(r->key_assigned)) return false;
    if (!READ_ONCE(r->active)) return false;
    return s->role == UV_OPEN || s->role == UV_CONNECT || immediate_write(s->role) ||
           (s->role == UV_WORK && r->role == UV_OPEN);
}
bool as_runtime_scope_reference(const struct as_runtime_scope *s,
                                struct as_runtime_key *request)
{
    if (!s || !READ_ONCE(s->active)) return false;
    if (!exact_scope(s)) return false;
    memcpy(request, s->inherited ? &s->inherited_request : &s->request->key, sizeof(*request));
    return true;
}
bool as_runtime_scope_enclosing(const struct as_runtime_scope *s, struct as_runtime_key *request)
{
    if (!s || !READ_ONCE(s->active) || s->inherited || s->role == UV_WORK ||
        !s->request || !READ_ONCE(s->request->active) || READ_ONCE(s->request->ambiguous) ||
        !READ_ONCE(s->request->key_assigned)) return false;
    memcpy(request, &s->request->key, sizeof(*request)); return true;
}
struct as_runtime_scope *as_runtime_scope_get(void)
{
    struct as_runtime_scope *s, *found = NULL, *enclosing = NULL;
    unsigned long flags;
    spin_lock_irqsave(&runtime_lock, flags);
    hash_for_each_possible(workers, s, link, (unsigned long)current)
        if (s->worker == current && s->active && (s->inherited || return_session(s, true))) {
            if (exact_scope(s)) { found = s; break; }
            if (!enclosing) enclosing = s;
        }
    if (!found) found = enclosing;
    if (found) refcount_inc(&found->refs);
    spin_unlock_irqrestore(&runtime_lock, flags);
    return found;
}
static void purge_profile(struct as_runtime_profile *p)
{
    struct runtime_request *r;
    struct as_runtime_scope *s;
    struct hlist_node *next;
    unsigned int bucket;
    hash_for_each_safe(workers, bucket, next, s, link) if (s->profile == p) {
        hash_del(&s->link); WRITE_ONCE(s->active, false); as_runtime_scope_put(s);
    }
    hash_for_each_safe(requests, bucket, next, r, link) if (r->profile == p) {
        hash_del(&r->link); WRITE_ONCE(r->active, false); request_count--; request_put(r);
    }
}
static void release_profile(struct work_struct *work)
{
    struct as_runtime_profile *p = container_of(work, struct as_runtime_profile, release);
    unsigned int i;
    for (i = 0; i < UV_PROBES; i++) if (p->probes[i].registered)
        uprobe_unregister_nosync(p->probes[i].registered, &p->probes[i].consumer);
    uprobe_unregister_sync();
    fput(p->file); mmdrop(p->mm); put_task_struct(p->task); kfree(p);
}
static void retire_profile(struct as_runtime_profile *p)
{
    WRITE_ONCE(p->active, false); list_del_init(&p->link); purge_profile(p);
    queue_work(p->release_queue, &p->release);
}
void as_runtime_adopt(struct as_runtime_profile *p)
{
    struct as_runtime_profile *old, *next;
    unsigned long flags;
    spin_lock_irqsave(&runtime_lock, flags);
    list_for_each_entry_safe(old, next, &profiles, link)
        if (old->tgid == p->tgid) retire_profile(old);
    list_add_tail(&p->link, &profiles); WRITE_ONCE(p->active, true);
    spin_unlock_irqrestore(&runtime_lock, flags);
}
void as_runtime_discard(struct as_runtime_profile *p)
{ if (p) release_profile(&p->release); }
void as_runtime_clear(struct workqueue_struct *queue)
{
    as_runtime_retire();
    flush_workqueue(queue);
}
void as_runtime_retire(void)
{
    struct as_runtime_profile *p, *next;
    struct as_runtime_scope *s;
    struct hlist_node *node;
    unsigned int bucket;
    unsigned long flags;
    spin_lock_irqsave(&runtime_lock, flags);
    list_for_each_entry_safe(p, next, &profiles, link) retire_profile(p);
    hash_for_each_safe(workers, bucket, node, s, link) if (s->inherited) {
        hash_del(&s->link); WRITE_ONCE(s->active, false); as_runtime_scope_put(s);
    }
    spin_unlock_irqrestore(&runtime_lock, flags);
}
void as_runtime_source_changed(const u8 source[16])
{
    struct runtime_request *r;
    struct as_runtime_scope *s;
    unsigned int bucket;
    unsigned long flags;
    spin_lock_irqsave(&runtime_lock, flags);
    hash_for_each(requests, bucket, r, link) WRITE_ONCE(r->key_assigned, false);
    hash_for_each(workers, bucket, s, link)
        if (s->inherited) WRITE_ONCE(s->inherited_key_assigned, false);
        else if (s->request) WRITE_ONCE(s->request->key_assigned, false);
    memcpy(runtime_source, source, sizeof(runtime_source));
    spin_unlock_irqrestore(&runtime_lock, flags);
}
void as_runtime_forget_task(struct task_struct *task, bool end_profile)
{
    struct as_runtime_profile *p, *next;
    struct as_runtime_scope *s;
    struct hlist_node *node;
    unsigned long flags;
    spin_lock_irqsave(&runtime_lock, flags);
    hash_for_each_possible_safe(workers, s, node, link, (unsigned long)task) if (s->worker == task) {
        hash_del(&s->link); WRITE_ONCE(s->active, false); as_runtime_scope_put(s);
    }
    list_for_each_entry_safe(p, next, &profiles, link)
        if (end_profile && p->tgid == task_tgid_nr(task)) retire_profile(p);
    spin_unlock_irqrestore(&runtime_lock, flags);
}
static void emit_work(u32 phase, u64 work,
                      struct runtime_request *r)
{
    struct as_message *message;
    void *body;
    message = as_runtime_message_alloc(phase, request_reference(r), request_operation(r), 8, &body, NULL);
    if (message) { put_unaligned_le64(work, body); as_runtime_message_publish(message, 8); }
}
/* One API occurrence owns its site, declared arguments and technical projection.
 * The output allocation itself is the capture owner; no second blob is copied. */
static struct runtime_request *submitted(struct as_runtime_profile *p,
        struct pt_regs *regs, enum runtime_probe role)
{
    const size_t capacity = AS_FRAME_MAX - sizeof(struct as_frame) - sizeof(struct as_runtime_handoff);
    struct as_runtime_key key;
    struct as_message *message;
    struct runtime_request *r = NULL, *old;
    u64 args[6] = {}, request;
    u32 registers = 0, api, operation, argc;
    size_t site = 48, payload, path_length = 0, total, projection = 0;
    unsigned long flags;
    void *body, *out;
    bool observed = false, allocation_failed = false;
#ifdef CONFIG_ARM64
    registers = 3;
#elif defined(CONFIG_X86_64)
    registers = 2;
#endif
    site += hweight32(registers) * 8;
    switch (role) {
    case UV_OPEN: api = 1; argc = 6; operation = AS_REQUEST_OPEN; payload = 52; break;
    case UV_SPAWN: api = 2; argc = 3; operation = AS_REQUEST_EXECUTE; payload = 24; break;
    case UV_CONNECT: api = 3; argc = 4; operation = AS_REQUEST_CONNECT; payload = 32; break;
    case UV_WRITE2: api = 4; argc = 6; operation = AS_REQUEST_SEND; payload = 48; break;
    case UV_UDP_SEND: api = 5; argc = 6; operation = AS_REQUEST_SEND; payload = 48; break;
    case UV_WRITE: api = 6; argc = 5; operation = AS_REQUEST_SEND; payload = 40; break;
    case UV_TRY_WRITE: api = 7; argc = 3; operation = AS_REQUEST_SEND; payload = 24; break;
    case UV_TRY_WRITE2: api = 8; argc = 4; operation = AS_REQUEST_SEND; payload = 32; break;
    default: return NULL;
    }
    for (u32 i = 0; i < argc; i++) args[i] = regs_get_kernel_argument(regs, i);
    request = immediate_write(role) ? 0 : role == UV_OPEN || role == UV_SPAWN ? args[1] : args[0];
    message = as_runtime_message_alloc(1, NULL, 0, capacity, &body, &key);
    if (!message) {
        /* Still retain available register facts under the same current schema.
         * This is an allocation failure, not a fake empty projection/path. */
        message = as_runtime_message_alloc(1, NULL, 0, site + payload, &body, &key);
        allocation_failed = message != NULL;
    }
    if (message) {
        put_unaligned_le32(api, body); put_unaligned_le32(0, body + 4);
        put_unaligned_le32(allocation_failed ? 1 : 0, body + 8); put_unaligned_le32(registers, body + 12);
        put_unaligned_le64(p->birth, body + 16); put_unaligned_le64(p->exec_cookie, body + 24);
        put_unaligned_le64(instruction_pointer(regs), body + 32); put_unaligned_le64(user_stack_pointer(regs), body + 40);
#ifdef CONFIG_ARM64
        put_unaligned_le64(regs->regs[30], body + 48); put_unaligned_le64(regs->regs[29], body + 56);
#elif defined(CONFIG_X86_64)
        put_unaligned_le64(regs->bp, body + 48);
#endif
        out = body + site;
        if (role == UV_OPEN) {
            u32 status = 0;
            int error = 0;
            size_t limit = allocation_failed ? 0 : capacity - site - payload - 64;
            put_unaligned_le64(args[1], out); put_unaligned_le64(args[0], out + 8); put_unaligned_le64(args[2], out + 16);
            put_unaligned_le32((u32)args[3], out + 24); put_unaligned_le32((u32)args[4], out + 28);
            put_unaligned_le64(args[5], out + 32);
            while (path_length < limit) {
                u8 byte;
                if (args[2] > U64_MAX - path_length) { error = -EFAULT; status = 1; break; }
                error = copy_from_user_nofault(&byte, (void __user *)(args[2] + path_length), 1);
                if (error) { status = 1; break; }
                if (!byte) break;
                *((u8 *)out + payload + path_length++) = byte;
            }
            if (path_length == limit) status = 2;
            put_unaligned_le32(status, out + 40); put_unaligned_le32(status == 1 ? error : 0, out + 44);
            put_unaligned_le32(path_length, out + 48);
        } else if (immediate_write(role)) {
            put_unaligned_le64(args[0], out); put_unaligned_le64(args[1], out + 8);
            put_unaligned_le32((u32)args[2], out + 16); put_unaligned_le32(0, out + 20);
            if (role == UV_TRY_WRITE2) put_unaligned_le64(args[3], out + 24);
        } else if (role == UV_SPAWN || role == UV_CONNECT) {
            for (u32 i = 0; i < argc; i++) put_unaligned_le64(args[i], out + i * 8);
        } else {
            put_unaligned_le64(args[0], out); put_unaligned_le64(args[1], out + 8); put_unaligned_le64(args[2], out + 16);
            put_unaligned_le32((u32)args[3], out + 24); put_unaligned_le32(0, out + 28);
            if (role == UV_WRITE) put_unaligned_le64(args[4], out + 32);
            else { put_unaligned_le64(args[4], out + 32); put_unaligned_le64(args[5], out + 40); }
        }
        total = site + payload + path_length;
        if (!allocation_failed) projection = as_runtime_capture_current(p->available, p->values, body + total, capacity - total);
        put_unaligned_le32(projection, body + 4);
        as_runtime_message_publish(message, total + projection); observed = true;
    }
    if (request || immediate_write(role)) r = kzalloc(sizeof(*r), GFP_ATOMIC);
    if (!r) return NULL;
    r->profile = p; r->req = request; r->operation = operation; r->role = role; r->active = true; refcount_set(&r->refs, 1);
    r->key_assigned = observed; if (observed) r->key = key;
    if (role == UV_OPEN && field(p, RT_UV_REQ_WORK) && request <= U64_MAX - p->values[RT_UV_REQ_WORK]) {
        r->work = request + p->values[RT_UV_REQ_WORK]; r->work_known = true;
    }
    r->result_known = role == UV_OPEN && field(p, RT_UV_REQ_RESULT); r->result_offset = p->values[RT_UV_REQ_RESULT];
    /* A try-write is this call occurrence, not a uv_req or a queued stream
     * identity. Its sole reference moves directly into the actual scope. */
    if (immediate_write(role)) return r;
    spin_lock_irqsave(&runtime_lock, flags);
    if (!p->active || request_count == AS_PENDING_MAX ||
        (observed && memcmp(key.source, runtime_source, 16))) { spin_unlock_irqrestore(&runtime_lock, flags); kfree(r); return NULL; }
    hash_for_each_possible(requests, old, link, (unsigned long)request) if (old->profile == p && old->req == request) {
        old->ambiguous = true; r->ambiguous = true;
    }
    hash_add(requests, &r->link, (unsigned long)request); request_count++;
    refcount_inc(&r->refs); /* caller and hash own separate actual lifetimes */
    spin_unlock_irqrestore(&runtime_lock, flags);
    return r;
}
static bool scope_enter(struct runtime_probe_entry *probe, struct runtime_request *r,
                        u64 address, u64 *cookie)
{
    struct as_runtime_scope *s;
    struct hlist_node *node;
    unsigned long flags;
    bool established = false;
    spin_lock_irqsave(&runtime_lock, flags);
    hash_for_each_possible_safe(workers, s, node, link, (unsigned long)current)
        if (s->worker == current && !s->inherited && !return_session(s, false)) {
            hash_del(&s->link); WRITE_ONCE(s->active, false); as_runtime_scope_put(s);
        }
    spin_unlock_irqrestore(&runtime_lock, flags);
    s = kzalloc(sizeof(*s), GFP_ATOMIC);
    if (!s) { request_put(r); return false; }
    s->profile = probe->profile; s->worker = current; get_task_struct(current);
    s->work = address; s->request = r; s->role = probe->role;
    s->active = true; refcount_set(&s->refs, 1);
    spin_lock_irqsave(&runtime_lock, flags);
    if (matches(probe->profile) && scope_cookie != U64_MAX) {
        if (r && r->key_assigned && memcmp(r->key.source, runtime_source, 16))
            WRITE_ONCE(r->key_assigned, false);
        s->cookie = ++scope_cookie; s->consumer_id = probe->consumer.id; *cookie = s->cookie;
        hash_add(workers, &s->link, (unsigned long)current);
        established = true;
    } else { s->active = false; as_runtime_scope_put(s); }
    spin_unlock_irqrestore(&runtime_lock, flags);
    return established;
}
static void end_request(struct as_runtime_profile *p, u64 address)
{
    struct runtime_request *r;
    struct hlist_node *node;
    unsigned long flags;
    spin_lock_irqsave(&runtime_lock, flags);
    hash_for_each_possible_safe(requests, r, node, link, (unsigned long)address)
        if (r->profile == p && r->req == address) {
            hash_del(&r->link); WRITE_ONCE(r->active, false); request_count--; request_put(r);
        }
    spin_unlock_irqrestore(&runtime_lock, flags);
}
/* The tracepoint supplies the actual new task object before it can run. No
 * PID lookup, ancestry guess or later target-memory query participates. */
void as_runtime_child_created(struct task_struct *parent, struct task_struct *child)
{
    struct as_runtime_scope *source, *next = NULL;
    struct runtime_request *request = NULL;
    struct as_runtime_key key;
    struct as_message *message;
    unsigned long flags;
    void *body;
    bool known = false;
    if (current != parent || !thread_group_leader(child)) return;
    spin_lock_irqsave(&runtime_lock, flags);
    hash_for_each_possible(workers, source, link, (unsigned long)parent) {
        if (source->worker != parent || !source->active || source->role != UV_SPAWN ||
            source->inherited || !return_session(source, true) || !source->request) continue;
        request = source->request;
        if (request->ambiguous) break;
        next = kzalloc(sizeof(*next), GFP_ATOMIC);
        if (!next) break;
        next->worker = child; get_task_struct(child);
        next->role = UV_SPAWN; next->active = true; next->inherited = true;
        next->inherited_key_assigned = request->key_assigned;
        if (next->inherited_key_assigned) next->inherited_request = request->key;
        refcount_set(&next->refs, 1); hash_add(workers, &next->link, (unsigned long)child);
        known = request->key_assigned; if (known) key = request->key;
        break;
    }
    spin_unlock_irqrestore(&runtime_lock, flags);
    if (!next) return;
    message = as_runtime_message_alloc(9, known ? &key : NULL, 0, 16, &body, NULL);
    if (message) {
        put_unaligned_le32(task_tgid_nr(child), body); put_unaligned_le32(0, body + 4);
        put_unaligned_le64(child->group_leader->start_boottime, body + 8);
        as_runtime_message_publish(message, 16);
    }
}
static int entered(struct uprobe_consumer *consumer, struct pt_regs *regs, u64 *cookie)
{
    struct runtime_probe_entry *probe = container_of(consumer, struct runtime_probe_entry, consumer);
    struct as_runtime_profile *p = probe->profile;
    struct runtime_request *r = NULL, *old;
    struct as_message *message;
    struct hlist_node *node;
    unsigned long flags;
    u64 address;
    void *body;
    if (!matches(p)) return UPROBE_HANDLER_IGNORE;
    if (probe->role == UV_OPEN || probe->role >= UV_SPAWN) {
        r = submitted(p, regs, probe->role);
        return scope_enter(probe, r, r ? r->req : 0, cookie) ? 0 : UPROBE_HANDLER_IGNORE;
    }
    address = regs_get_kernel_argument(regs, probe->role == UV_SUBMIT ? 1 : 0);
    spin_lock_irqsave(&runtime_lock, flags); r = find_request(p, address, probe->role != UV_CLEANUP); spin_unlock_irqrestore(&runtime_lock, flags);
    if (probe->role == UV_SUBMIT) {
        message = as_runtime_message_alloc(2, request_reference(r), request_operation(r), 32, &body, NULL);
        if (message) {
            put_unaligned_le64(regs_get_kernel_argument(regs, 0), body); put_unaligned_le64(address, body + 8);
            put_unaligned_le64(regs_get_kernel_argument(regs, 3), body + 16);
            put_unaligned_le64(regs_get_kernel_argument(regs, 4), body + 24);
            as_runtime_message_publish(message, 32);
        }
    } else if (probe->role == UV_WORK) {
        emit_work(3, address, r);
        return scope_enter(probe, r, address, cookie) ? 0 : UPROBE_HANDLER_IGNORE;
    } else if (probe->role == UV_DONE) {
        s64 result = 0;
        bool attempted = r && r->result_known && r->req <= U64_MAX - r->result_offset;
        int read_error = attempted ? copy_from_user_nofault(&result, (void __user *)(r->req + r->result_offset), sizeof(result)) : 0;
        bool present = attempted && !read_error;
        size_t bytes = 16 + (present ? 8 : read_error ? 4 : 0);
        message = as_runtime_message_alloc(5, request_reference(r), request_operation(r), bytes, &body, NULL);
        if (message) {
            put_unaligned_le64(address, body); put_unaligned_le32(present ? 1 : read_error ? 2 : 0, body + 8); put_unaligned_le32(0, body + 12);
            if (present) put_unaligned_le64(result, body + 16);
            else if (read_error) put_unaligned_le32(read_error, body + 16);
            as_runtime_message_publish(message, bytes);
        }
    } else {
        message = as_runtime_message_alloc(6, request_reference(r), request_operation(r), 8, &body, NULL);
        if (message) { put_unaligned_le64(address, body); as_runtime_message_publish(message, 8); }
        spin_lock_irqsave(&runtime_lock, flags);
        hash_for_each_possible_safe(requests, old, node, link, (unsigned long)address) if (old->profile == p && old->req == address) {
            hash_del(&old->link); WRITE_ONCE(old->active, false); request_count--; request_put(old);
        }
        spin_unlock_irqrestore(&runtime_lock, flags);
    }
    request_put(r); return UPROBE_HANDLER_IGNORE;
}
static int returned(struct uprobe_consumer *consumer, unsigned long function, struct pt_regs *regs, u64 *cookie)
{
    struct runtime_probe_entry *probe = container_of(consumer, struct runtime_probe_entry, consumer);
    struct as_runtime_scope *s, *found = NULL;
    unsigned long flags;
    spin_lock_irqsave(&runtime_lock, flags);
    hash_for_each_possible(workers, s, link, (unsigned long)current)
        if (s->profile == probe->profile && s->worker == current && s->cookie == *cookie) {
            hash_del(&s->link); WRITE_ONCE(s->active, false); found = s; break;
        }
    spin_unlock_irqrestore(&runtime_lock, flags);
    if (found && matches(probe->profile)) {
        if (probe->role == UV_WORK) emit_work(4, found->work, found->request);
        else {
            struct as_message *message;
            void *body;
            message = as_runtime_message_alloc(8, request_reference(found->request), 0, 8, &body, NULL);
            if (message) {
                put_unaligned_le32((s32)regs_return_value(regs), body); put_unaligned_le32(0, body + 4);
                as_runtime_message_publish(message, 8);
            }
            if (immediate_write(probe->role) && found->request)
                WRITE_ONCE(found->request->active, false);
            else if (probe->role != UV_OPEN && found->request)
                end_request(probe->profile, found->request->req);
        }
    }
    as_runtime_scope_put(found); return 0;
}
struct as_runtime_profile *as_runtime_prepare(const void *body, size_t bytes, struct workqueue_struct *queue)
{
    const struct as_runtime_plan *plan = body;
    struct as_runtime_profile *p;
    struct task_struct *task = NULL;
    struct mm_struct *mm = NULL;
    struct file *file = NULL;
    struct pid *pid;
    struct elfhdr elf;
    loff_t position = 0;
    unsigned int i, count, bitmap;
    u16 probes;
    int error = -EINVAL;
    if (bytes < sizeof(*plan) || plan->reserved) return ERR_PTR(-EINVAL);
    count = le32_to_cpu(plan->field_count); bitmap = DIV_ROUND_UP(count, 64);
    if ((count && count != RT_FIELDS) || bytes != sizeof(*plan) + (bitmap + count) * 8) return ERR_PTR(-EINVAL);
    probes = le16_to_cpu(plan->probe_present);
    if (!probes || probes & ~GENMASK(UV_PROBES - 1, 0)) return ERR_PTR(-EOPNOTSUPP);
    rcu_read_lock(); pid = get_pid(find_pid_ns(le32_to_cpu(plan->tgid), &init_pid_ns)); rcu_read_unlock();
    if (pid) task = get_pid_task(pid, PIDTYPE_TGID);
    put_pid(pid);
    if (!task) return ERR_PTR(-ESRCH);
    if (task->start_boottime != le64_to_cpu(plan->birth) || READ_ONCE(task->self_exec_id) != le64_to_cpu(plan->exec_cookie)) { error = -ESTALE; goto fail; }
    mm = get_task_mm(task);
    rcu_read_lock(); if (mm) file = get_file_rcu(&mm->exe_file); rcu_read_unlock();
    if (!mm || !file) { error = -ESRCH; goto fail; }
    if (file_inode(file)->i_ino != le64_to_cpu(plan->inode) ||
        MAJOR(file_inode(file)->i_sb->s_dev) != le32_to_cpu(plan->dev_major) ||
        MINOR(file_inode(file)->i_sb->s_dev) != le32_to_cpu(plan->dev_minor)) { error = -ESTALE; goto fail; }
    if (kernel_read(file, &elf, sizeof(elf), &position) != sizeof(elf) ||
        memcmp(elf.e_ident, ELFMAG, SELFMAG) || elf.e_ident[EI_CLASS] != ELFCLASS64 ||
        elf.e_machine != le16_to_cpu(plan->elf_machine)) { error = -ENOEXEC; goto fail; }
    p = kzalloc(sizeof(*p), GFP_KERNEL); if (!p) { error = -ENOMEM; goto fail; }
    INIT_LIST_HEAD(&p->link); INIT_WORK(&p->release, release_profile);
    p->task = task; p->file = file; p->mm = mm; p->release_queue = queue;
    p->tgid = le32_to_cpu(plan->tgid); p->birth = task->start_boottime; p->exec_cookie = READ_ONCE(task->self_exec_id);
    p->mask = probes;
    mmgrab(mm); mmput(mm);
    for (i = 0; i < bitmap; i++) {
        unsigned int remaining = count - i * 64;
        u64 allowed = remaining >= 64 ? U64_MAX : GENMASK_ULL(remaining - 1, 0);
        p->available[i] = get_unaligned_le64(body + sizeof(*plan) + i * 8);
        if (p->available[i] & ~allowed) { error = -EINVAL; goto discard; }
    }
    for (i = 0; i < count; i++) p->values[i] = get_unaligned_le64(body + sizeof(*plan) + (bitmap + i) * 8);
    for (i = 0; i < UV_PROBES; i++) p->offsets[i] = le64_to_cpu(plan->probes[i]);
    {
        struct as_runtime_profile *old;
        unsigned long flags;
        bool same = false;
        spin_lock_irqsave(&runtime_lock, flags);
        list_for_each_entry(old, &profiles, link)
            if (old->active && old->tgid == p->tgid && old->birth == p->birth && old->exec_cookie == p->exec_cookie &&
                old->mm == p->mm && file_inode(old->file) == file_inode(p->file) && old->mask == p->mask &&
                !memcmp(old->offsets, p->offsets, sizeof(p->offsets)) &&
                !memcmp(old->available, p->available, sizeof(p->available)) && !memcmp(old->values, p->values, sizeof(p->values))) { same = true; break; }
        spin_unlock_irqrestore(&runtime_lock, flags);
        if (same) { as_runtime_discard(p); return NULL; }
    }
    for (i = 0; i < UV_PROBES; i++) if (probes & BIT(i)) {
        struct runtime_probe_entry *probe = &p->probes[i];
        if (le64_to_cpu(plan->probes[i]) >= i_size_read(file_inode(file))) { error = -EINVAL; goto discard; }
        probe->profile = p; probe->role = i; probe->consumer.handler = entered; probe->consumer.filter = filter;
        if (i == UV_WORK || i == UV_OPEN || i >= UV_SPAWN) probe->consumer.ret_handler = returned;
        probe->registered = uprobe_register(file_inode(file), le64_to_cpu(plan->probes[i]), 0, &probe->consumer);
        if (IS_ERR(probe->registered)) { error = PTR_ERR(probe->registered); probe->registered = NULL; goto discard; }
    }
    if (READ_ONCE(task->self_exec_id) != p->exec_cookie || task->mm != p->mm) { error = -ESTALE; goto discard; }
    rcu_read_lock(); file = get_file_rcu(&p->mm->exe_file); rcu_read_unlock();
    if (!file || file_inode(file) != file_inode(p->file)) { if (file) fput(file); error = -ESTALE; goto discard; }
    fput(file);
    return p;
discard: as_runtime_discard(p); return ERR_PTR(error);
fail: if (file) fput(file); if (mm) mmput(mm); put_task_struct(task); return ERR_PTR(error);
}
