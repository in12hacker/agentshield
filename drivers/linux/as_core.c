// SPDX-License-Identifier: GPL-2.0-only
/* Current native driver. The controller and kernel exchange only as_protocol.h.
 * Waiting follows TOMOYO's original-operation lifetime; kfunc registration uses
 * the kernel BPF selftest mechanism. No new built-in LSM or legacy decoder.
 */
#include <linux/module.h>
#include <linux/bpf.h>
#include <linux/btf.h>
#include <linux/btf_ids.h>
#include <linux/binfmts.h>
#include <linux/completion.h>
#include <linux/cgroup.h>
#include <linux/cred.h>
#include <linux/dcache.h>
#include <linux/device.h>
#include <linux/file.h>
#include <linux/fprobe.h>
#include <linux/fs.h>
#include <linux/fs_struct.h>
#include <linux/fsnotify.h>
#include <linux/fsnotify_backend.h>
#include <linux/hashtable.h>
#include <linux/highmem.h>
#include <linux/in.h>
#include <linux/in6.h>
#include <linux/miscdevice.h>
#include <linux/mm.h>
#include <linux/mutex.h>
#include <linux/namei.h>
#include <linux/nsproxy.h>
#include <linux/poll.h>
#include <linux/pid_namespace.h>
#include <linux/random.h>
#include <linux/refcount.h>
#include <linux/sched/signal.h>
#include <linux/slab.h>
#include <linux/sock_diag.h>
#include <linux/socket.h>
#include <linux/tracepoint.h>
#include <linux/uaccess.h>
#include <linux/uio.h>
#include <linux/user_namespace.h>
#include <linux/unaligned.h>
#include <linux/vmalloc.h>
#include <crypto/sha2.h>
#include <net/sock.h>
#include <net/inet_sock.h>
#include <net/net_namespace.h>
#include <trace/events/sched.h>
#include "as_protocol.h"
#include "as_hook_facts.h"
#include "as_self_protection.h"
#include "as_tools.h"
#include "as_futex.h"
#include "as_group_tcp.h"
#include "as_runtime.h"
#include <linux/mman.h>
#include <linux/ptrace.h>
#include <linux/sched/task_stack.h>
#include <linux/stacktrace.h>
#include <asm/syscall.h>

#define AS_RULES_MAX 4096u
#define AS_CAPTURE_MAX 128u
#define AS_OBSERVERS_MAX 64u
#define AS_ORDINARY_MAX 256u
#define AS_HIT_BATCH_MAX 8u
#define AS_HIT_BATCH_BODY 3968u
#define AS_REPLIES_MAX 128u
#define AS_QUERY_MAX 64u
#define AS_CGROUP_LINEAGE_MAX 32u
#define AS_TLV_BYTES(value_bytes) (8u+(value_bytes))
#define AS_EXECUTION_BYTES (AS_TLV_BYTES(sizeof(struct as_syscall_snapshot)) + AS_TLV_BYTES(sizeof(struct as_stack_snapshot)) + AS_TLV_BYTES(sizeof(struct as_user_stack_snapshot)))
_Static_assert(AS_HOOK_STACK_SLOTS == AS_KERNEL_STACK_LIMIT, "hook stack slot count");
/* Fixed common facts, including the largest supported namespace-TGID list.
 * Variable path/argv fields use only the remaining event capacity. */
#define AS_REQUIRED_COMMON_BYTES (2u*AS_TLV_BYTES(TASK_COMM_LEN)+10u*AS_TLV_BYTES(4u)+7u*AS_TLV_BYTES(8u)+2u*AS_TLV_BYTES(4u+(MAX_PID_NS_LEVEL+1u)*4u)+AS_TLV_BYTES(AS_CGROUP_LINEAGE_MAX*8u)+AS_EXECUTION_BYTES)
#ifdef CONFIG_MMU
#define AS_EXEC_ARGUMENT_CONDITIONS AS_FIELD_BIT(14)
#else
#define AS_EXEC_ARGUMENT_CONDITIONS 0ULL
#endif
#define AS_CONDITIONS (AS_FIELD_BIT(1)|AS_FIELD_BIT(2)|AS_FIELD_BIT(3)|AS_FIELD_BIT(4)|AS_FIELD_BIT(5)|AS_FIELD_BIT(6)|AS_FIELD_BIT(7)|AS_FIELD_BIT(8)|AS_FIELD_BIT(9)|AS_FIELD_BIT(10)|AS_FIELD_BIT(11)|AS_FIELD_BIT(12)|AS_FIELD_BIT(13)|AS_EXEC_ARGUMENT_CONDITIONS|AS_FIELD_BIT(15)|AS_FIELD_BIT(16)|AS_FIELD_BIT(17)|AS_FIELD_BIT(18)|AS_FIELD_BIT(19)|AS_FIELD_BIT(20)|AS_FIELD_BIT(21)|AS_FIELD_BIT(22)|AS_FIELD_BIT(23)|AS_FIELD_BIT(24)|AS_FIELD_BIT(26))
enum as_source_state { DORMANT, RUNNING, QUIESCING, FAILED };
enum as_original_state { HELD, BOUND, COMPLETING };
struct as_close_directory {
    struct fsnotify_mark mark;
    bool alive; /* The group's initial mark reference is still owned here. */
};
struct as_close_scope {
    refcount_t refs;
    u8 description[32];
    u32 count;
    struct fsnotify_group *group;
    atomic_t pending_marks;
    struct completion marks_done;
    struct as_close_directory directories[];
};
struct as_snapshot {
    refcount_t refs;
    u8 *bytes;
    size_t length;
    u8 config[32], rules[32];
    u64 generation;
    u32 defaults, max_pending, max_hold_ms, count[3], max_hit_bytes[3];
    u32 driver_operations[3], backend_operations[3], backend_route_operations[3], backend_route_count;
    const u8 *backend_routes;
    bool adopted;
    struct as_message *retirement;
    struct as_self_protection *self;
    struct as_close_scope *close_scope;
    const struct cred *authority;
    bool self_log;
    u32 group_ports, gateway_ws_port;
    struct work_struct release;
    const struct as_rule **pool[3];
};
static u32 candidate_operations(const struct as_snapshot *snapshot, u16 kind)
{
    return snapshot->driver_operations[kind-1] | snapshot->backend_operations[kind-1] |
        snapshot->backend_route_operations[kind-1];
}
/* One callback's adopted view, not a second writable state or stored event. */
struct as_gate_view {
    struct as_snapshot *snapshot;
    u64 revision, started;
    bool enabled;
};
/* DriverContext is native, immutable for one actual authenticated connection. */
struct as_context { u8 instance[16], session[16]; };
struct as_message { struct list_head link; size_t bytes, last_hit; struct as_original *original; u8 data[]; };
/* One queued close record owns its path until the actual reader/release ends.
 * The common message remains the last member, with one payload allocation. */
struct as_close_message {
    struct work_struct release;
    struct path path;
    refcount_t refs;
    struct as_message message;
};
struct as_path_release { struct work_struct work; struct path path; atomic_t *count; };
struct as_original {
    struct list_head link;
    wait_queue_head_t wait;
    struct as_snapshot *snapshot;
    struct as_message *receipt, *notice;
    struct pid *actor, *parent, *controller;
    enum as_original_state state;
    bool delivered, capture_token, facts_token, direct_observation;
    unsigned facts_bucket;
    u64 id, started, deadline, revision;
    u32 decision, fallback, terminate, fallback_terminate;
    int actor_signal, parent_signal;
    int error;
    const struct as_rule *hit;
    size_t length, capacity;
    u8 event[];
};
struct as_reader {
    struct as_context context;
    struct mutex read_lock, write_lock;
    bool hello, installing, allocating, stop_facts, control_credit;
    u32 writes;
    atomic_t cancel_install;
    struct as_message *quiesce_reply;
    u8 *transfer;
    u64 transfer_id, transfer_total, transfer_bytes, transfer_epoch;
    u16 transfer_type;
    u64 cancelled_id, cancelled_total;
    u16 cancelled_type;
    u8 record[AS_FRAME_MAX];
};
static DEFINE_SPINLOCK(core_lock);
static DECLARE_WAIT_QUEUE_HEAD(read_wait);
static LIST_HEAD(pending);
static LIST_HEAD(replies);
static LIST_HEAD(ordinary);
static struct pid *controller;
static struct as_reader *connection;
static struct as_context context;
static void queue_clear(struct list_head *head);
static struct as_snapshot *active;
static enum as_source_state source_state;
static u8 source[16], instance[16];
static u64 sequence, original_sequence, generation, state_revision, dropped, group_ordinal, group_generation;
static u64 runtime_sequence;
static u32 runtime_inflight;
static u32 group_ports, gateway_ws_port, group_pending;
static u32 lifecycle_epoch;
static bool facts_enabled, facts_accepting;
static u64 facts_epoch, facts_marker, facts_loss, facts_cutoff;
static u64 hit_loss;
static unsigned facts_bucket, facts_pending_bucket;
static u32 facts_inflight[2], facts_marker_count;
static struct as_message *facts_pending;
static u32 held, readers, capture_count, observer_count, control_writes, receipt_count, reply_count, ordinary_count, retired_count, overrides;
static u32 hit_batches;
static int source_error;
static struct workqueue_struct *release_queue;
static atomic_t cwd_references=ATOMIC_INIT(0);
static atomic_t create_references = ATOMIC_INIT(0);
static dev_t control_device;
static const struct file_operations operations;
static struct tracepoint *tp_fork, *tp_exec, *tp_exit;

static void retire_snapshot(struct as_snapshot *snapshot);
static void close_scope_put(struct as_close_scope *scope);
static void message_free(struct as_message *message);
static size_t required_payload(u16 kind, u32 operations);
static void note_request_wait(const struct as_event *event, u64 wait_id);
static void release_snapshot(struct work_struct *work)
{
    struct as_snapshot *snapshot = container_of(work, struct as_snapshot, release);
    as_self_put(snapshot->self);
    close_scope_put(snapshot->close_scope);
    put_cred(snapshot->authority);
    kfree(snapshot->pool[0]); kfree(snapshot->pool[1]); kfree(snapshot->pool[2]);
    kvfree(snapshot->bytes); kfree(snapshot);
}
static void snapshot_put(struct as_snapshot *snapshot)
{
    if (snapshot && refcount_dec_and_test(&snapshot->refs)) {
        if (snapshot->adopted) retire_snapshot(snapshot); else message_free(snapshot->retirement);
        queue_work(release_queue, &snapshot->release);
    }
}
static void frame_header(struct as_frame *h, u16 class, u16 type, u64 id, size_t bytes,
                         const u8 session[16])
{
    memset(h, 0, sizeof(*h)); h->magic = cpu_to_le32(AS_MAGIC); h->format = cpu_to_le16(AS_FORMAT);
    h->class = cpu_to_le16(class); h->type = cpu_to_le16(type); h->body_bytes = cpu_to_le32(bytes);
    memcpy(h->session, session, 16); h->request_id = cpu_to_le64(id); h->total_bytes = cpu_to_le64(bytes);
}
static struct as_message *message_alloc(size_t body, gfp_t flags)
{
    struct as_message *m;
    if (body > AS_FRAME_MAX - sizeof(struct as_frame)) return NULL;
    m = kzalloc(sizeof(*m) + sizeof(struct as_frame) + body, flags);
    if (m) { INIT_LIST_HEAD(&m->link); m->bytes = sizeof(struct as_frame) + body; }
    return m;
}
static void release_close_message(struct work_struct *work)
{
    struct as_close_message *close = container_of(work, struct as_close_message, release);
    unsigned long flags;
    path_put(&close->path);
    kfree(close);
    spin_lock_irqsave(&core_lock, flags); observer_count--; spin_unlock_irqrestore(&core_lock, flags);
    wake_up_interruptible(&read_wait);
}
static void message_free(struct as_message *message)
{
    const struct as_frame *frame;
    struct as_close_message *close;
    if (!message) return;
    frame = (const void *)message->data;
    if (le16_to_cpu(frame->type) != AS_FILE_CLOSE) { kfree(message); return; }
    close = container_of(message, struct as_close_message, message);
    if (refcount_dec_and_test(&close->refs)) queue_work(release_queue, &close->release);
}
/* Only the actual device reader resolves a path; never reopen the original file.
 * The caller holds a close-message reference and its per-reader mutex. */
static size_t prepare_close_record(const struct as_message *message, u8 *record)
{
    const struct as_close_message *close = container_of(message, struct as_close_message, message);
    struct as_frame *frame = (void *)record;
    struct as_file_close *body = (void *)(frame + 1);
    char *buffer = (void *)(body + 1), *path;
    size_t bytes = 0;
    memcpy(record, message->data, sizeof(*frame) + sizeof(*body));
    path = current->fs ? d_path(&close->path, buffer, PATH_MAX) : ERR_PTR(-ESRCH);
    body->path_error = cpu_to_le32(IS_ERR(path) ? PTR_ERR(path) : 0);
    if (!IS_ERR(path)) {
        bytes = strlen(path);
        memmove(buffer, path, bytes);
    }
    body->path_bytes = cpu_to_le32(bytes);
    frame->body_bytes = cpu_to_le32(sizeof(*body) + bytes);
    frame->total_bytes = cpu_to_le64(sizeof(*body) + bytes);
    return sizeof(*frame) + sizeof(*body) + bytes;
}
/* Ordinary FIFO owns its container accounting. Payload producers neither
 * classify dequeue counters nor maintain another copy of queue occupancy. */
static void ordinary_account_locked(const struct as_message *message, int delta)
{
    const struct as_frame *frame = (const void *)message->data;
    u16 type = le16_to_cpu(frame->type);
    switch (type) {
    case AS_CONTROLLED: return;
    case AS_PERSISTENT_RETIRED: retired_count += delta; return;
    case AS_FACTS_MARKER: facts_marker_count += delta; return;
    case AS_HIT_BATCH: hit_batches += delta; return;
    default:
        ordinary_count += delta;
        if (type == AS_GROUP_TCP) group_pending += delta;
    }
}
static void ordinary_queue_locked(struct as_message *message)
{
    list_add_tail(&message->link, &ordinary); ordinary_account_locked(message, 1);
    wake_up_interruptible(&read_wait);
}
static void ordinary_batch_locked(struct list_head *batch)
{
    struct as_message *message;
    list_for_each_entry(message, batch, link) ordinary_account_locked(message, 1);
    list_splice_tail_init(batch, &ordinary); wake_up_interruptible(&read_wait);
}
static void ordinary_remove_locked(struct as_message *message)
{
    list_del_init(&message->link); ordinary_account_locked(message, -1);
}
/* Admission closes on the existing authenticated reader fence. Already owned
 * records keep their original in-flight lifetime and may still be published. */
static bool observations_open_locked(void)
{ return controller && connection && connection->hello && !connection->stop_facts; }
static bool runtime_accepting_locked(void)
{ return observations_open_locked() && (facts_accepting || source_state == RUNNING) && source_state != FAILED && source_state != QUIESCING; }
static bool runtime_header_locked(struct as_message *message, u32 phase,
    const struct as_runtime_key *request, u32 operation, size_t phase_bytes,
    void **phase_body, struct as_runtime_key *observation)
{
    struct as_runtime_handoff *h = (void *)(message->data + sizeof(struct as_frame));
    size_t prefix;
    if (!runtime_accepting_locked()) return false;
    if (runtime_sequence == U64_MAX || runtime_inflight == U32_MAX) { dropped++; return false; }
    if (request && memcmp(request->source, source, 16)) request = NULL;
    prefix = sizeof(*h) + (request ? sizeof(*request) : 0);
    frame_header((void *)message->data, AS_CLASS_EVENT, AS_RUNTIME_HANDOFF, 0, prefix + phase_bytes, context.session);
    memcpy(h->observation.source, source, 16); h->observation.domain = cpu_to_le64(2);
    h->observation.sequence = cpu_to_le64(++runtime_sequence);
    h->actor.tgid = cpu_to_le32(task_tgid_nr(current)); h->actor.tid = cpu_to_le32(task_pid_nr(current));
    h->actor.birth_boottime_ns = cpu_to_le64(current->start_boottime);
    h->observed_boottime_ns = cpu_to_le64(ktime_get_boottime_ns());
    h->presence = cpu_to_le32((request ? 1 : 0) | (operation ? 2 : 0));
    h->phase = cpu_to_le32(phase); h->operation = cpu_to_le32(operation); h->reserved = 0;
    if (request) memcpy(h + 1, request, sizeof(*request));
    *phase_body = (void *)h + prefix;
    if (observation) *observation = h->observation;
    runtime_inflight++;
    return true;
}
struct as_message *as_runtime_message_alloc(u32 phase,
    const struct as_runtime_key *request, u32 operation, size_t phase_bytes,
    void **phase_body, struct as_runtime_key *observation)
{
    size_t prefix = sizeof(struct as_runtime_handoff) + (request ? sizeof(*request) : 0);
    struct as_message *message;
    unsigned long flags;
    if (phase_bytes > AS_FRAME_MAX - sizeof(struct as_frame) - prefix) {
        spin_lock_irqsave(&core_lock, flags); if (runtime_accepting_locked()) dropped++; spin_unlock_irqrestore(&core_lock, flags);
        return NULL;
    }
    message = message_alloc(prefix + phase_bytes, GFP_ATOMIC);
    if (!message) {
        spin_lock_irqsave(&core_lock, flags); if (runtime_accepting_locked()) dropped++; spin_unlock_irqrestore(&core_lock, flags);
        return NULL;
    }
    spin_lock_irqsave(&core_lock, flags);
    if (!runtime_header_locked(message, phase, request, operation, phase_bytes, phase_body, observation)) {
        message_free(message); message = NULL;
    }
    spin_unlock_irqrestore(&core_lock, flags);
    return message;
}
static void runtime_queue_locked(struct as_message *message, size_t phase_bytes)
{
    struct as_frame *frame = (void *)message->data;
    struct as_runtime_handoff *h = (void *)(frame + 1);
    size_t body = sizeof(*h) + ((le32_to_cpu(h->presence) & 1) ? sizeof(struct as_runtime_key) : 0) + phase_bytes;
    runtime_inflight--;
    /* Stop admission does not destroy an already owned callback's record.
     * Its original source remains on the body across a real source change. */
    if (!controller || memcmp(frame->session, context.session, 16)) { message_free(message); wake_up_interruptible(&read_wait); return; }
    if (body > message->bytes - sizeof(*frame) || ordinary_count >= AS_ORDINARY_MAX) { dropped++; message_free(message); wake_up_interruptible(&read_wait); return; }
    frame->body_bytes = cpu_to_le32(body); frame->total_bytes = cpu_to_le64(body);
    message->bytes = sizeof(*frame) + body;
    ordinary_queue_locked(message);
}
void as_runtime_message_publish(struct as_message *message, size_t phase_bytes)
{
    unsigned long flags;
    spin_lock_irqsave(&core_lock, flags); runtime_queue_locked(message, phase_bytes); spin_unlock_irqrestore(&core_lock, flags);
}
static void facts_value_locked(struct as_facts_marker *value)
{
    value->epoch=cpu_to_le64(facts_epoch); value->marker=cpu_to_le64(facts_marker);
    value->loss_epoch=cpu_to_le64(facts_loss); value->pid_namespace=cpu_to_le64(init_pid_ns.ns.inum);
    value->cutoff_boottime_ns=cpu_to_le64(facts_cutoff);
    value->hit_loss=cpu_to_le64(hit_loss);
}
static void facts_publish_locked(void)
{
    struct as_message *message=facts_pending;
    if(!message || facts_inflight[facts_pending_bucket])return;
    facts_pending=NULL;
    if(!connection || !controller || !facts_enabled){message_free(message);return;}
    frame_header((void *)message->data,AS_CLASS_EVENT,AS_FACTS_MARKER,0,sizeof(struct as_facts_marker),context.session);
    facts_value_locked((void *)(message->data+sizeof(struct as_frame)));
    ordinary_queue_locked(message);
}
static void facts_lost_locked(void)
{
    if(!facts_enabled)return;
    if(facts_loss==U64_MAX){facts_enabled=false;return;}
    facts_loss++;
}
static void facts_release_locked(struct as_original *original)
{
    if(!original->facts_token)return;
    original->facts_token=false;facts_inflight[original->facts_bucket]--;facts_publish_locked();
}
static void facts_close_locked(void)
{
    facts_enabled=facts_accepting=false;kfree(facts_pending);facts_pending=NULL;
}
static int facts_begin_locked(struct as_reader *reader,u16 type,struct as_message **reserved,struct as_facts_marker *value)
{
    if(!task_is_in_init_pid_ns(current))return -EACCES;
    if(reader->stop_facts || source_state==QUIESCING)return -ESHUTDOWN;
    if(facts_pending || facts_marker_count>=AS_REPLIES_MAX)return -EBUSY;
    if(type==AS_START_FACTS&&!facts_enabled){
        if(facts_epoch==U64_MAX)return -EOVERFLOW;
        facts_enabled=true;facts_epoch++;facts_loss=0;
    }
    if(!facts_enabled)return -ENODATA;
    facts_accepting=true;
    if(facts_marker==U64_MAX || facts_loss==U64_MAX)return -EOVERFLOW;
    facts_marker++;facts_pending_bucket=facts_bucket;facts_cutoff=ktime_get_boottime_ns();facts_bucket^=1;
    facts_pending=*reserved;*reserved=NULL;facts_value_locked(value);facts_publish_locked();return 0;
}
static int inspect_processes(const u8 *body,size_t bytes,void *output)
{
    u32 count;
    if(bytes<8 || get_unaligned_le32(body+4))return -EINVAL;
    count=get_unaligned_le32(body);
    if(!count || count>AS_INSPECT_MAX || bytes!=8+(size_t)count*4)return -EINVAL;
    put_unaligned_le32(count,output);put_unaligned_le32(0,output+4);
    return as_tools_inspect_processes((const void *)(body+8),count,output+8);
}
static void retire_snapshot(struct as_snapshot *snapshot)
{
    struct as_message *m = snapshot->retirement; unsigned long flags;
    if (!m) return;
    spin_lock_irqsave(&core_lock, flags);
    if (!controller || !connection || !connection->hello) { spin_unlock_irqrestore(&core_lock,flags); message_free(m); return; }
    frame_header((void *)m->data, AS_CLASS_EVENT, AS_PERSISTENT_RETIRED, 0, 8, context.session);
    put_unaligned_le64(snapshot->generation, m->data + sizeof(struct as_frame));
    ordinary_queue_locked(m);
    spin_unlock_irqrestore(&core_lock, flags);
}
static u32 effective_mask(void)
{
    u32 values = 0, i;
    if (!active) return 0;
    for (i = 0; i < 3; i++) {
        u32 setting = (overrides >> (i * 2)) & 3;
        bool enabled = setting == 1 || (setting == 0 && (active->defaults & (1u << i)));
        values |= (enabled ? (source_state == RUNNING ? 1u : 0u) : 2u) << (i * 2);
    }
    return values;
}
static void status_locked(struct as_status *status)
{
    memset(status, 0, sizeof(*status)); memcpy(status->instance, instance, 16);
    status->state = cpu_to_le32(source_state); status->flags = cpu_to_le32(active ? 1 : 0);
    status->generation = cpu_to_le64(generation); status->revision = cpu_to_le64(state_revision);
    if (active) { memcpy(status->config, active->config, 32); memcpy(status->rules, active->rules, 32);
        status->defaults = cpu_to_le32(active->defaults); }
    status->overrides = cpu_to_le32(overrides); status->effective = cpu_to_le32(effective_mask());
    /* Report only the actual adopted native binding. */
    status->self_protection = cpu_to_le32(active && active->self ? 1 : 0); status->pending = cpu_to_le32(held);
    status->receipts = cpu_to_le32(receipt_count); status->dropped = cpu_to_le64(dropped);
    status->error = cpu_to_le32(source_error); status->lifecycle_epoch = cpu_to_le32(lifecycle_epoch);
}
static void capabilities(struct as_capabilities *caps)
{
    memset(caps, 0, sizeof(*caps)); caps->max_frame = cpu_to_le32(AS_FRAME_MAX);
    caps->max_pending = cpu_to_le32(AS_PENDING_MAX); caps->max_transfer = cpu_to_le64(AS_TRANSFER_MAX);
    caps->max_hold_ms = cpu_to_le32(AS_HOLD_MAX_MS);
    caps->operations = cpu_to_le32(AS_OP_EXEC|AS_OP_OPEN|AS_OP_CONNECT|AS_OP_SEND|AS_OP_CREATED|AS_OP_EXECUTED|AS_OP_EXITED|AS_OP_FILE_MKDIR|AS_OP_FILE_UNLINK|AS_OP_FILE_RMDIR|AS_OP_FILE_RENAME|AS_OP_FILE_TRUNCATE_PATH|AS_OP_FILE_MAP_SHARED_WRITABLE|AS_OP_FILE_TRUNCATE_OPEN|AS_OP_FILE_CREATE);
    caps->conditions = cpu_to_le64(AS_CONDITIONS); caps->features = cpu_to_le32(AS_FEAT_SELF_PROTECTION|AS_FEAT_PROCESS_FACTS|AS_FEAT_PROCESS_KILL|AS_FEAT_KILL_ACTOR|AS_FEAT_KILL_PARENT|AS_FEAT_CONTROL_PRIORITY|AS_FEAT_FILE_CLOSE_AUDIT|AS_FEAT_DNS_QUERY_SNAPSHOT|AS_FEAT_SELF_MOUNT_RULES);
}
static void state_notice_locked(void)
{
    struct as_message *m;
    if (!observations_open_locked()) return;
    if (ordinary_count >= AS_ORDINARY_MAX) { dropped++; return; }
    m = message_alloc(sizeof(struct as_status), GFP_ATOMIC);
    if (!m) { dropped++; return; }
    frame_header((void *)m->data, AS_CLASS_EVENT, AS_STATE_CHANGED, 0, sizeof(struct as_status), context.session);
    status_locked((void *)(m->data + sizeof(struct as_frame)));
    ordinary_queue_locked(m);
}
static bool drained_locked(const struct as_reader *reader)
{
    return !readers && !capture_count && !observer_count && !held && !receipt_count &&
           list_empty(&ordinary) && !runtime_inflight && !reader->writes;
}
static void quiesced_locked(struct as_reader *reader)
{
    struct as_reply *reply;
    if (!reader->quiesce_reply || !drained_locked(reader)) return;
    reply = (void *)(reader->quiesce_reply->data + sizeof(struct as_frame));
    reply->error = 0; status_locked((void *)(reply + 1));
    list_add_tail(&reader->quiesce_reply->link, &replies); reader->quiesce_reply = NULL;
    wake_up_interruptible(&read_wait);
}

static int append(struct as_original *r, u16 field, const void *value, u32 bytes)
{
    __le16 tag[2] = { cpu_to_le16(field), 0 }; __le32 length = cpu_to_le32(bytes);
    if (bytes > r->capacity - r->length || r->capacity - r->length - bytes < 8) return -E2BIG;
    memcpy(r->event + r->length, tag, 4); memcpy(r->event + r->length + 4, &length, 4);
    memcpy(r->event + r->length + 8, value, bytes); r->length += bytes + 8; return 0;
}
static int append_u32(struct as_original *r, u16 field, u32 value)
{ __le32 v = cpu_to_le32(value); return append(r, field, &v, sizeof(v)); }
static int append_u64(struct as_original *r, u16 field, u64 value)
{ __le64 v = cpu_to_le64(value); return append(r, field, &v, sizeof(v)); }
static int append_path_leaf(struct as_original *r, u16 field, const struct path *path,
                            const struct qstr *leaf)
{
    char *buffer, *text;
    size_t bytes, reserve = leaf ? (size_t)leaf->len + 1 : 0;
    int result = 0;

    if (!path) return 0;
    if (reserve >= PATH_MAX) return 0;
    buffer = kmalloc(PATH_MAX, GFP_ATOMIC); if (!buffer) return 0;
    text = d_path(path, buffer, PATH_MAX - reserve);
    if (!IS_ERR(text)) {
        bytes = strlen(text);
        if (leaf) {
            if (bytes && text[bytes - 1] != '/')
                text[bytes++] = '/';
            memcpy(text + bytes, leaf->name, leaf->len);
            bytes += leaf->len;
        }
        result = append(r, field, text, bytes);
    }
    kfree(buffer);
    return result == -E2BIG ? 0 : result;
}
static int append_native_path(struct as_original *r,u16 field,const struct path *path)
{
    return append_path_leaf(r, field, path, NULL);
}
static int append_path(struct as_original *r,u16 field,struct file *file)
{ return file?append_native_path(r,field,&file->f_path):0; }
static int append_exe(struct as_original *r, u16 field, struct task_struct *task)
{
    struct file *file = NULL; struct mm_struct *mm; int result;
    rcu_read_lock(); task_lock(task); mm = task->mm;
    if (mm) file = get_file_rcu(&mm->exe_file);
    task_unlock(task); rcu_read_unlock();
    result = append_path(r, field, file); if (file) fput(file); return result;
}
static int actor_arguments(struct as_original *r)
{
    unsigned long start, end; u8 *raw, *wire; u32 count = 0; size_t at, begin = 0, out = 4; int error = 0;
    if (!current->mm) return 0;
    start = READ_ONCE(current->mm->arg_start); end = READ_ONCE(current->mm->arg_end);
    if (end <= start || end - start > 4096) return 0;
    raw = kmalloc(end - start, GFP_ATOMIC); wire = kmalloc(4 * (end - start) + 4, GFP_ATOMIC);
    if (!raw || !wire) goto done;
    if (copy_from_user_nofault(raw, (const void __user *)start, end - start) || raw[end - start - 1]) goto done;
    for (at = 0; at < end - start; at++) if (!raw[at]) {
        put_unaligned_le32(at - begin, wire + out); memcpy(wire + out + 4, raw + begin, at - begin);
        out += 4 + at - begin; begin = at + 1; count++;
    }
    put_unaligned_le32(count, wire); if (out + 8 <= r->capacity - r->length) error = append(r, 3, wire, out);
done: kfree(raw); kfree(wire); return error;
}
static void release_path(struct work_struct *work)
{
    struct as_path_release *held=container_of(work,struct as_path_release,work);
    atomic_t *count = held->count;

    path_put(&held->path);
    kfree(held);
    atomic_dec(count);
}
static struct as_path_release *reserve_path_reference(atomic_t *count)
{
    struct as_path_release *held = NULL;

    if (atomic_inc_return(count) <= AS_CAPTURE_MAX)
        held = kmalloc(sizeof(*held), GFP_ATOMIC);
    if (!held) {
        atomic_dec(count);
        return NULL;
    }
    held->count = count;
    INIT_WORK(&held->work, release_path);
    return held;
}
static void defer_path_put(struct as_path_release *held)
{
    if (held)
        queue_work(release_queue, &held->work);
}
static void actor_cwd(struct as_original *r,struct task_struct *actor,u16 stage)
{
    struct path pwd;struct as_path_release *held=NULL;bool found=false;size_t capacity=r->capacity;
    /* Self Direct capacity already excludes its envelope. Preserve room for
     * a second target path as well as capture()'s first path/metadata reserve. */
    size_t reserve=stage==3?PATH_MAX+8:0;
    if(reserve>=r->capacity-r->length)return;
    /* dput() may sleep. Non-sleepable Self/trace callbacks only retain a
     * bounded path reference; the existing resource queue performs its put. */
    if(stage!=2){
        held=reserve_path_reference(&cwd_references);
        if(!held)return;
    }
    task_lock(actor);
    if(actor->fs){get_fs_pwd(actor->fs,&pwd);found=true;}
    task_unlock(actor);
    if(!found){if(held){kfree(held);atomic_dec(&cwd_references);}return;}
    r->capacity-=reserve;(void)append_native_path(r,4,&pwd);r->capacity=capacity;
    if(held){held->path=pwd;defer_path_put(held);}else path_put(&pwd);
}
static int append_pid_namespace_ids(struct as_original *r, struct task_struct *actor,
                                    enum pid_type type, u16 field)
{
    struct pid *pid = get_task_pid(actor, type);
    __le32 ids[MAX_PID_NS_LEVEL+2];
    unsigned int count, i;
    int error = 0;

    if (!pid) return 0;
    if (pid->level <= MAX_PID_NS_LEVEL) {
        count = pid->level + 1;
        if (type == PIDTYPE_TGID) error = append_u64(r, 16, ns_of_pid(pid)->ns.inum);
        ids[0] = cpu_to_le32(count);
        for (i = 0; i < count; i++) ids[i+1] = cpu_to_le32(pid->numbers[i].nr);
        error = error ?: append(r, field, ids, (count+1)*sizeof(*ids));
    }
    put_pid(pid);
    return error;
}
static int actor_namespaces(struct as_original *r,struct task_struct *actor)
{
    u64 net_inode=0;bool has_net=false;int error=0;
    /* nsproxy is replaced under task_lock. Copy only the stable scalar while
     * the actor holds its reference; no namespace reference escapes this lock. */
    task_lock(actor);
    if(actor->nsproxy&&actor->nsproxy->net_ns){net_inode=actor->nsproxy->net_ns->ns.inum;has_net=true;}
    task_unlock(actor);
    if(has_net)error=append_u64(r,17,net_inode);
    return error ?: append_pid_namespace_ids(r, actor, PIDTYPE_TGID, 20);
}
static void target_arguments(struct as_original *r,struct linux_binprm *bprm)
{
#ifdef CONFIG_MMU
    u8 *wire;size_t capacity,out=4,length_at=0,argument_at=0;u32 index=0,argc;unsigned long pos;
    bool complete=true;
    if(!bprm->mm || bprm->argc<0 || r->capacity-r->length<12)return;
    capacity=r->capacity-r->length-8;
    argc=bprm->argc;if(argc>(capacity-4)/4)return;
    wire=kmalloc(capacity,GFP_NOFS);if(!wire)return;
    put_unaligned_le32(argc,wire);
    if(argc){length_at=out;out+=4;argument_at=out;}
    pos=bprm->p;
    /* Same source and page lifetime as TOMOYO tomoyo_dump_page(): this is the
     * nascent exec mm, never current's old argv or a syscall argument pointer.
     * The sleepable exec hook holds no AS spinlock across mmap/GUP. */
    while(index<argc && complete){
        struct page *page;void *mapped;unsigned offset;long obtained;
        mmap_read_lock(bprm->mm);
        obtained=get_user_pages_remote(bprm->mm,pos,1,FOLL_FORCE,&page,NULL);
        mmap_read_unlock(bprm->mm);
        if(obtained<=0){complete=false;break;}
        mapped=kmap_local_page(page);
        for(offset=offset_in_page(pos);offset<PAGE_SIZE && index<argc;offset++){
            u8 value=((const u8 *)mapped)[offset];
            if(value){if(out==capacity){complete=false;break;}wire[out++]=value;}
            else{
                put_unaligned_le32(out-argument_at,wire+length_at);index++;
                if(index<argc){if(capacity-out<4){complete=false;break;}length_at=out;out+=4;argument_at=out;}
            }
            if(pos==ULONG_MAX){complete=false;break;}pos++;
        }
        kunmap_local(mapped);put_page(page);
    }
    if(complete && index==argc)(void)append(r,2,wire,out);
    kfree(wire);
#endif
}
static int append_process_info(struct as_original *r,struct task_struct *actor,struct task_struct *parent,bool arguments,u16 stage,size_t reserve_payload)
{
    const struct cred *cred=get_task_cred(actor);char name[TASK_COMM_LEN];size_t capacity;int error;
    get_task_comm(name, actor); error = append(r, 1, name, strnlen(name, sizeof(name)));
    error = error ?: append_u32(r, 5, from_kuid(&init_user_ns, cred->uid));
    error = error ?: append_u32(r, 6, from_kuid(&init_user_ns, cred->euid));
    error = error ?: append_u32(r, 7, from_kgid(&init_user_ns, cred->gid));
    error = error ?: append_u32(r, 8, from_kgid(&init_user_ns, cred->egid));
    error = error ?: append_u64(r,18,cred->user_ns->ns.inum);
    error = error ?: append_u32(r,21,from_kuid(&init_user_ns,cred->suid));
    error = error ?: append_u32(r,22,from_kuid(&init_user_ns,cred->fsuid));
    error = error ?: append_u32(r,23,from_kgid(&init_user_ns,cred->sgid));
    error = error ?: append_u32(r,24,from_kgid(&init_user_ns,cred->fsgid));put_cred(cred);
    if(parent){
        error = error ?: append_u32(r, 9, task_tgid_nr(parent));
        error = error ?: append_u64(r, 10, parent->start_boottime);
        get_task_comm(name, parent);error=error?:append(r,11,name,strnlen(name,sizeof(name)));
    }
    error = error ?: append_u32(r,14,(actor->flags&PF_KTHREAD)?1:0);
    error = error ?: append_u64(r,15,READ_ONCE(actor->self_exec_id));
    error = error ?: append_u64(r,25,READ_ONCE(actor->start_boottime));
    error = error ?: actor_namespaces(r,actor);
    capacity=r->capacity;
    r->capacity=reserve_payload>=r->capacity-r->length?r->length:r->capacity-reserve_payload;
    error = error ?: append_exe(r, 2, actor);
    if(parent)error=error?:append_exe(r,12,parent);
    if (arguments && actor == current) error = error ?: actor_arguments(r);
    if(!error)actor_cwd(r,actor,stage);
    r->capacity=capacity;return error==-E2BIG?0:error;
}
static void child_process_info(struct as_original *r,struct task_struct *child)
{
    size_t before=r->length;
    if(r->capacity-r->length<8)return;
    put_unaligned_le16(9,r->event+before);put_unaligned_le16(0,r->event+before+2);r->length+=8;
    /* Same common-field encoding, without another header/EventKey, ownership
     * reference or duplicate child parent identity. Foreign argv is not read. */
    if(append_process_info(r,child,NULL,false,1,0)){r->length=before;return;}
    put_unaligned_le32(r->length-before-8,r->event+before+4);
}
static void capture_cgroup_lineage(struct as_original *r, struct cgroup *cg)
{
    __le64 ids[AS_CGROUP_LINEAGE_MAX];
    unsigned int i, count;
    if ((unsigned int)cg->level >= AS_CGROUP_LINEAGE_MAX) return;
    count = cg->level + 1;
    for (i = 0; i < count; i++) {
        struct cgroup *ancestor = cgroup_ancestor(cg, i);
        if (!ancestor) return;
        ids[i] = cpu_to_le64(cgroup_id(ancestor));
        if (!ids[i]) return;
    }
    (void)append(r, 26, ids, count * sizeof(*ids));
}
static int capture(struct as_original *r, struct task_struct *actor, u16 kind, u32 operation, u16 stage, bool arguments, u64 reserved_sequence, const struct as_hook_facts *facts)
{
    struct as_event *e = (void *)r->event; struct task_struct *parent;
    struct cgroup *cg;unsigned long flags;int error;
    if (!operation || operation > 32) return -EINVAL;
    if (facts && (facts->available & ~(AS_HOOK_MOUNT_NAMESPACE | AS_HOOK_USER_STACK))) return -EINVAL;
    memset(e, 0, sizeof(*e)); e->format = cpu_to_le16(1); e->header_bytes = cpu_to_le16(sizeof(*e));
    e->kind = cpu_to_le16(kind); e->stage = cpu_to_le16(stage); e->operation = cpu_to_le32(operation);
    spin_lock_irqsave(&core_lock, flags);
    if (!reserved_sequence && sequence == U64_MAX) { spin_unlock_irqrestore(&core_lock, flags); return -EOVERFLOW; }
    memcpy(e->source, source, 16);
    e->sequence = cpu_to_le64(reserved_sequence ?: ++sequence);
    spin_unlock_irqrestore(&core_lock, flags);
    e->domain = cpu_to_le64(1); e->boottime = cpu_to_le64(ktime_get_boottime_ns());
    e->tgid = cpu_to_le32(task_tgid_nr(actor)); e->tid = cpu_to_le32(task_pid_nr(actor));
    e->birth = cpu_to_le64(actor->group_leader->start_boottime);
    r->length = sizeof(*e);
    rcu_read_lock(); cg = task_dfl_cgroup(actor); e->cgroup = cpu_to_le64(cgroup_id(cg));
    if(stage==2)capture_cgroup_lineage(r,cg);
    parent = rcu_dereference(actor->real_parent)->group_leader; get_task_struct(parent);
    r->parent = get_pid(task_tgid(parent));
    if(stage==2)r->actor=get_pid(task_tgid(actor));
    rcu_read_unlock();
    e->available = cpu_to_le32(31);
    error = append_pid_namespace_ids(r, actor, PIDTYPE_PID, AS_COMMON_NAMESPACE_TIDS);
    if (facts && (facts->available & AS_HOOK_MOUNT_NAMESPACE))
        error = error ?: append_u64(r, 19, facts->mount_namespace_inode);
    error=error?:append_process_info(r,actor,parent,arguments,stage,
        kind == 2 ? required_payload(kind, AS_FILE_OPERATION_BIT(operation)) : PATH_MAX+64);
    put_task_struct(parent);
    e->common_bytes=cpu_to_le32(r->length-sizeof(*e));return error;
}
static const u8 *tlv(const u8 *data, size_t bytes, u16 wanted, size_t *length)
{
    while (bytes >= 8) {
        u16 id = get_unaligned_le16(data); u32 size = get_unaligned_le32(data + 4);
        if (size > bytes - 8) return NULL;
        if (id == wanted) { *length = size; return data + 8; }
        data += size + 8; bytes -= size + 8;
    }
    return NULL;
}
static const u8 *value(const struct as_original *r, u16 field, size_t *bytes)
{
    const struct as_event *e = (const void *)r->event; u32 common = le32_to_cpu(e->common_bytes);
    static const u8 fields[] = {0,0,9,5,7,6,8,1,2,3,4,11,12,1,2,1,2,3,4,6,1,2,3,4,5,6,AS_FILE_TLV_MOUNT_EFFECT};
    if (field == 1) { *bytes = 4; return (const u8 *)e + offsetof(struct as_event, tgid); }
    if (field <= 12) return tlv(r->event + sizeof(*e), common, fields[field], bytes);
    return tlv(r->event + sizeof(*e) + common, r->length - sizeof(*e) - common, fields[field], bytes);
}
static int condition_matches(const struct as_original *r, const struct as_condition *condition)
{
    u16 field = le16_to_cpu(condition->field); size_t length; const u8 *actual = value(r, field, &length);
    const u8 *expected = (const u8 *)(condition + 1); u32 bytes = le32_to_cpu(condition->bytes);
    if (!actual) return -ENODATA;
    if (field == 9 || field == 14) {
        u32 index = le32_to_cpu(condition->index), count; size_t at = 4;
        if (length < 4) return -EINVAL;
        count = get_unaligned_le32(actual);
        if (index >= count) return 0;
        while (index--) { if (at + 4 > length) return -EINVAL; at += 4 + get_unaligned_le32(actual + at); }
        if (at + 4 > length || get_unaligned_le32(actual + at) > length - at - 4) return -EINVAL;
        length = get_unaligned_le32(actual + at); actual += at + 4;
    }
    switch (condition->operation) {
    case 1: return length == bytes && !memcmp(actual, expected, bytes);
    case 2: return length >= bytes && !memcmp(actual, expected, bytes);
    case 3:
        if (field == 24) return length == 1 && *actual == get_unaligned_le32(expected);
        return length == 4 && get_unaligned_le32(actual) == get_unaligned_le32(expected);
    case 4: return length == 8 && get_unaligned_le64(actual) == get_unaligned_le64(expected);
    case 5: return length == 4 && (get_unaligned_le32(actual) & get_unaligned_le32(expected)) == get_unaligned_le32(expected);
    case 6: return length == 8 && !memcmp(actual, expected, 8);
    case 7: return length == 2 && get_unaligned_le16(actual) >= get_unaligned_le16(expected) && get_unaligned_le16(actual) <= get_unaligned_le16(expected + 2);
    case 8: {
        u32 prefix = expected[0], whole = prefix / 8, remain = prefix % 8;
        if (length + 1 != bytes || memcmp(actual, expected + 1, whole)) return 0;
        return !remain || !((actual[whole] ^ expected[whole + 1]) & (0xff << (8 - remain)));
    }
    default: return -EINVAL;
    }
}
static int match(const struct as_snapshot *snapshot, const struct as_original *r, const struct as_rule **hit)
{
    const struct as_event *e = (const void *)r->event; u16 kind = le16_to_cpu(e->kind); u32 i;
    u32 operation = 1u << (le32_to_cpu(e->operation) - 1);
    for (i = 0; i < snapshot->count[kind - 1]; i++) {
        const struct as_rule *rule = snapshot->pool[kind - 1][i]; const u8 *next; u32 n; bool missing = false, no = false;
        if (!(le32_to_cpu(rule->operations) & operation)) continue;
        next = (const u8 *)(rule + 1) + le16_to_cpu(rule->group_bytes) + le16_to_cpu(rule->id_bytes);
        for (n = 0; n < le32_to_cpu(rule->conditions); n++) {
            const struct as_condition *condition = (const void *)next; int yes = condition_matches(r, condition);
            if (yes == 0) { no = true; break; }
            if (yes == -ENODATA) missing = true; else if (yes < 0) return yes;
            next += sizeof(*condition) + le32_to_cpu(condition->bytes);
        }
        if (no) continue;
        if (missing) return -ENODATA;
        *hit = rule; return le16_to_cpu(rule->action);
    }
    *hit = NULL;
    if (snapshot->backend_operations[kind - 1] & operation) return AS_TO_USER;
    if (snapshot->backend_route_operations[kind-1] & operation) {
        const u8 *at = snapshot->backend_routes;
        for (i = 0; i < snapshot->backend_route_count; i++) {
            const struct as_backend_route *route = (const void *)at;
            if (le16_to_cpu(route->kind) == kind &&
                (le32_to_cpu(route->operations) & operation) && condition_matches(r, &route->condition) != 0)
                return AS_TO_USER;
            at += sizeof(*route) + le32_to_cpu(route->condition.bytes);
        }
    }
    return AS_ALLOW;
}
static bool condition_valid(const struct as_condition *c, u16 kind, u32 operations)
{
    u16 f = le16_to_cpu(c->field); u32 n = le32_to_cpu(c->bytes); const u8 *v = (const void *)(c + 1);
    if (!f || f > 26 || !(AS_CONDITIONS & AS_FIELD_BIT(f)) || c->reserved8 || c->reserved32) return false;
    if ((f >= 13 && f <= 14 && kind != 1) || (f >= 15 && f <= 19 && kind != 2) || (f >= 20 && f <= 25 && kind != 3) || (f == 26 && kind != 2)) return false;
    if(f==26)return operations==AS_FILE_OPERATION_BIT(AS_FILE_MOUNT)&&c->operation==3&&n==4&&get_unaligned_le32(v)==AS_MOUNT_INDEPENDENT_VIEW;
    if (kind == 2 && ((f == 16 && (operations & ~AS_FILE_OPERATION_BIT(AS_FILE_RENAME))) ||
        (f == 19 && (operations & ~AS_FILE_OPERATION_BIT(AS_FILE_OPEN))) ||
        (f == 18 && (operations & (AS_FILE_OPERATION_BIT(AS_FILE_CREATE) |
                                   AS_FILE_OPERATION_BIT(AS_FILE_MKDIR)))))) return false;
    if (f != 9 && f != 14 && c->index) return false;
    if ((f >= 7 && f <= 16 && f != 17) && f != 18 && f != 19) return c->operation == 1 || c->operation == 2;
    if (f == 17) return c->operation == 6 && n == 8;
    if (f == 18) return c->operation == 4 && n == 8;
    if (f == 19) return c->operation == 5 && n == 4 && get_unaligned_le32(v);
    if (f == 20 || f == 21) return c->operation == 8 && (n == 5 || n == 17) && v[0] <= (n - 1) * 8;
    if (f == 22 || f == 23) return c->operation == 7 && n == 4 && get_unaligned_le16(v) <= get_unaligned_le16(v + 2);
    return c->operation == 3 && n == 4 && (f != 24 || get_unaligned_le32(v) <= 255);
}
static size_t required_payload(u16 kind, u32 operations)
{
    /* IPv6 local/remote, two ports, protocol, socket cookie and socket netns. */
    if(kind==3)return 2*AS_TLV_BYTES(16)+2*AS_TLV_BYTES(2)+AS_TLV_BYTES(1)+2*AS_TLV_BYTES(8)+(operations&2u?AS_TLV_BYTES(sizeof(struct as_dns_send_snapshot)):0);
    if(kind==2 && (operations & (AS_FILE_OPERATION_BIT(AS_FILE_RENAME)|AS_FILE_OPERATION_BIT(AS_FILE_LINK)|AS_FILE_OPERATION_BIT(AS_FILE_MOVE_MOUNT))))
        return 2*AS_TLV_BYTES(PATH_MAX)+2*AS_TLV_BYTES(8)+AS_TLV_BYTES(4);
    if (kind == 2 && (operations & AS_FILE_OPERATION_BIT(AS_FILE_MAP_SHARED_WRITABLE)))
        return AS_TLV_BYTES(PATH_MAX) + 2*AS_TLV_BYTES(8) + 2*AS_TLV_BYTES(4) + AS_TLV_BYTES(24);
    if (kind == 2 && (operations & AS_FILE_OPERATION_BIT(AS_FILE_TRUNCATE)))
        return AS_TLV_BYTES(PATH_MAX) + 2*AS_TLV_BYTES(8) + 2*AS_TLV_BYTES(4) +
               AS_TLV_BYTES(sizeof(struct as_truncate_parameters));
    return PATH_MAX+64;
}
/* The mark tracks only directory lifetime. Close facts have one producer:
 * file_release. In particular, this group never watches FS_CLOSE_WRITE. */
static int close_directory_event(struct fsnotify_mark *mark, u32 mask,
    struct inode *inode, struct inode *dir, const struct qstr *name, u32 cookie)
{ return 0; }
static void close_directory_freeing(struct fsnotify_mark *mark, struct fsnotify_group *group)
{
    struct as_close_directory *directory = container_of(mark, struct as_close_directory, mark);
    unsigned long flags;
    bool owned;
    spin_lock_irqsave(&core_lock, flags);
    owned = directory->alive; directory->alive = false;
    spin_unlock_irqrestore(&core_lock, flags);
    /* Invalidate under the same lock as membership lookup before dropping
     * the last listener reference: an inode address cannot become an ABA key. */
    if (owned) fsnotify_put_mark(mark);
}
static void close_directory_freed(struct fsnotify_mark *mark)
{
    struct as_close_scope *scope = mark->group->private;
    if (atomic_dec_and_test(&scope->pending_marks)) complete(&scope->marks_done);
}
static const struct fsnotify_ops close_directory_ops = {
    .handle_inode_event = close_directory_event,
    .freeing_mark = close_directory_freeing,
    .free_mark = close_directory_freed,
};
static void close_scope_put(struct as_close_scope *scope)
{
    u32 i;
    if (!scope || !refcount_dec_and_test(&scope->refs)) return;
    for (i = 0; i < scope->count; i++) {
        struct as_close_directory *directory = &scope->directories[i];
        unsigned long flags;
        bool owned;
        /* Transfer, rather than duplicate, the initial listener reference.
         * A concurrent kernel invalidation owns it if it won this lock first. */
        spin_lock_irqsave(&core_lock, flags);
        owned = directory->alive; directory->alive = false;
        spin_unlock_irqrestore(&core_lock, flags);
        if (owned) {
            fsnotify_destroy_mark(&directory->mark, scope->group);
            fsnotify_put_mark(&directory->mark);
        }
    }
    wait_for_completion(&scope->marks_done);
    /* free_mark signals our own lifetime; this exported barrier also waits
     * for the reaper callback to return before module-owned memory is freed. */
    fsnotify_wait_marks_destroyed();
    fsnotify_put_group(scope->group);
    kvfree(scope);
}
static struct as_close_scope *close_scope_prepare(const u8 *description, size_t bytes,
    u32 count, bool initialize, struct as_self_protection *self, const atomic_t *cancelled)
{
    struct as_close_scope *scope = NULL;
    const u8 *at = description, *end = description + bytes;
    u8 digest[32];
    unsigned long flags;
    u32 i;
    int error = 0;
    if ((!count && bytes) || (count && count > bytes / 5)) return ERR_PTR(-EINVAL);
    sha256(description, bytes, digest);
    if (!initialize) {
        spin_lock_irqsave(&core_lock, flags);
        if (active && active->close_scope) {
            scope = active->close_scope;
            refcount_inc(&scope->refs);
        }
        spin_unlock_irqrestore(&core_lock, flags);
        if ((!scope && !count) || (scope && scope->count == count &&
            !memcmp(scope->description, digest, sizeof(digest)))) return scope;
        close_scope_put(scope);
        return ERR_PTR(-EINVAL); /* Scope changes require full initialization. */
    }
    if (!count) return NULL;
    scope = kvzalloc(struct_size(scope, directories, count), GFP_KERNEL);
    if (!scope) return ERR_PTR(-ENOMEM);
    scope->group = fsnotify_alloc_group(&close_directory_ops, 0);
    if (IS_ERR(scope->group)) { error = PTR_ERR(scope->group); kvfree(scope); return ERR_PTR(error); }
    scope->group->private = scope;
    refcount_set(&scope->refs, 1); scope->count = count;
    atomic_set(&scope->pending_marks, count); init_completion(&scope->marks_done);
    memcpy(scope->description, digest, sizeof(digest));
    for (i = 0; i < count; i++) {
        scope->directories[i].alive = true;
        fsnotify_init_mark(&scope->directories[i].mark, scope->group);
        scope->directories[i].mark.mask = FS_UNMOUNT;
    }
    for (i = 0; i < count; i++) {
        struct path path;
        char *name;
        u32 length;
        if (atomic_read(cancelled)) { error = -ECANCELED; break; }
        if (end - at < 4) { error = -EINVAL; break; }
        length = get_unaligned_le32(at); at += 4;
        if (!length || length >= PATH_MAX || length > end - at || *at != '/' ||
            memchr(at, 0, length)) { error = -EINVAL; break; }
        name = kmemdup_nul(at, length, GFP_KERNEL);
        if (!name) { error = -ENOMEM; break; }
        error = kern_path(name, LOOKUP_DIRECTORY | LOOKUP_NO_SYMLINKS, &path);
        kfree(name);
        if (error) break;
        if (as_self_protected_path(self, &path)) error = -EINVAL;
        if (!error) error = fsnotify_add_inode_mark(&scope->directories[i].mark, d_inode(path.dentry), 0);
        path_put(&path); /* The mark releases the inode on delete/unmount. */
        if (error) break;
        at += length;
    }
    if (!error && at != end) error = -EINVAL;
    if (error) { close_scope_put(scope); return ERR_PTR(error); }
    return scope;
}
static struct as_snapshot *images_parse(const u8 *data, size_t length, u64 *expected, const atomic_t *cancelled, bool initialize)
{
    const struct as_images *images = (const void *)data; const struct as_configuration *cfg;
    const struct as_policy *policy; const u8 *at, *end; struct as_snapshot *s; u64 config_bytes, rule_bytes;
    u32 i, counts[3] = {}, prior_group[3] = {}, prior_number[3] = {}; u8 hash[32];
    u32 backend_routes, route_operations[3] = {}; size_t route_offset;
    if (length < sizeof(*images)) return ERR_PTR(-EINVAL);
    config_bytes = le64_to_cpu(images->config_bytes); rule_bytes = le64_to_cpu(images->rules_bytes);
    if (images->reserved16 || le16_to_cpu(images->platform) != 1 ||
        le32_to_cpu(images->config_encoding) != 1 || le32_to_cpu(images->rules_encoding) != 1 ||
        config_bytes > length - sizeof(*images) || rule_bytes != length - sizeof(*images) - config_bytes ||
        config_bytes < sizeof(*cfg) || rule_bytes < sizeof(*policy)) return ERR_PTR(-EINVAL);
    sha256(data + sizeof(*images), length - sizeof(*images), hash);
    if (memcmp(hash, images->sha256, 32)) return ERR_PTR(-EBADMSG);
    cfg = (const void *)(images + 1);
    if (le32_to_cpu(cfg->defaults) & ~7u || !le32_to_cpu(cfg->max_pending) || le32_to_cpu(cfg->max_pending) > AS_PENDING_MAX ||
        !le32_to_cpu(cfg->max_hold_ms) || le32_to_cpu(cfg->max_hold_ms) > AS_HOLD_MAX_MS ||
        !as_group_ports_valid(le32_to_cpu(cfg->group_ports), le32_to_cpu(cfg->gateway_ws_port)) || le32_to_cpu(cfg->self_protection_log) > 1 ||
        (u64)le32_to_cpu(cfg->root_bytes) + le32_to_cpu(cfg->backend_bytes) + le32_to_cpu(cfg->loader_bytes) + le32_to_cpu(cfg->close_directory_bytes) != config_bytes - sizeof(*cfg)) return ERR_PTR(-EINVAL);
    if ((cfg->group_ports || cfg->gateway_ws_port) && !as_group_tcp_available()) return ERR_PTR(-EOPNOTSUPP);
    /* Reject a requested unsupported policy as a whole; no false self-protection. */
    if (le32_to_cpu(cfg->required_features) & ~(AS_FEAT_SELF_PROTECTION | AS_FEAT_FILE_CLOSE_AUDIT)) return ERR_PTR(-EOPNOTSUPP);
    if ((cfg->root_bytes || cfg->backend_bytes || cfg->loader_bytes) && (!cfg->root_bytes || !cfg->backend_bytes || !cfg->loader_bytes)) return ERR_PTR(-EINVAL);
    if ((le32_to_cpu(cfg->required_features) & AS_FEAT_SELF_PROTECTION) && !cfg->root_bytes) return ERR_PTR(-EINVAL);
    policy = (const void *)((const u8 *)cfg + config_bytes);
    if (le32_to_cpu(policy->header_bytes) != sizeof(*policy) ||
        le64_to_cpu(policy->bytes) != rule_bytes || le32_to_cpu(policy->rules) > AS_RULES_MAX) return ERR_PTR(-EINVAL);
    backend_routes = le32_to_cpu(policy->backend_routes);
    if (backend_routes > AS_RULES_MAX - le32_to_cpu(policy->rules)) return ERR_PTR(-E2BIG);
    for (i = 0; i < 3; i++)
        if (le32_to_cpu(policy->backend_operations[i]) & ~(i == 2 ? 3u : i == 1 ? AS_FILE_CONTROLLED_OPERATIONS : 1u)) return ERR_PTR(-EOPNOTSUPP);
    at = (const void *)(policy + 1); end = (const u8 *)policy + rule_bytes;
    for (i = 0; i < le32_to_cpu(policy->rules); i++) {
        const struct as_rule *r; const u8 *next, *limit; u16 kind; u32 n, ops, group, number;bool mount_effect=false;
        if (end - at < sizeof(*r)) return ERR_PTR(-EINVAL);
        r = (const void *)at; kind = le16_to_cpu(r->kind);
        if (le32_to_cpu(r->bytes) < sizeof(*r) || le32_to_cpu(r->bytes) > end - at || kind < 1 || kind > 3 ||
            le32_to_cpu(r->report_denial) > 1 ||
            (r->report_denial && le16_to_cpu(r->action) != AS_DENY) ||
            le16_to_cpu(r->action) > 2 || le32_to_cpu(r->terminate) > 2 ||
            (r->terminate && le16_to_cpu(r->action) != AS_DENY) || le32_to_cpu(r->log) > 1 ||
            !le16_to_cpu(r->group_bytes) || !le16_to_cpu(r->id_bytes)) return ERR_PTR(-EINVAL);
        ops = le32_to_cpu(r->operations); group = le32_to_cpu(r->group_number); number = le32_to_cpu(r->rule_number);
        if (!ops || (ops & ~(kind == 3 ? 3u : kind == 2 ? AS_FILE_DRIVER_OPERATIONS : 1u))) return ERR_PTR(-EOPNOTSUPP);
        if(kind==2&&(ops&AS_FILE_OPERATION_BIT(AS_FILE_MOUNT))&&(ops!=AS_FILE_OPERATION_BIT(AS_FILE_MOUNT)||le16_to_cpu(r->action)>AS_DENY||r->terminate))return ERR_PTR(-EOPNOTSUPP);
        if (!group || !number || (kind == 2 && group == 1) || group < prior_group[kind-1] ||
            (group == prior_group[kind-1] && number <= prior_number[kind-1])) return ERR_PTR(-EINVAL);
        prior_group[kind-1] = group; prior_number[kind-1] = number;
        limit = at + le32_to_cpu(r->bytes); next = (const void *)(r + 1);
        if ((u32)le16_to_cpu(r->group_bytes) + le16_to_cpu(r->id_bytes) > limit - next ||
            sizeof(struct as_frame)+sizeof(struct as_controlled)+sizeof(struct as_hit)+(u32)le16_to_cpu(r->group_bytes)+le16_to_cpu(r->id_bytes)+sizeof(struct as_result)+sizeof(struct as_event)+AS_REQUIRED_COMMON_BYTES+required_payload(kind,ops) > AS_FRAME_MAX) return ERR_PTR(-E2BIG);
        next += le16_to_cpu(r->group_bytes) + le16_to_cpu(r->id_bytes);
        for (n = 0; n < le32_to_cpu(r->conditions); n++) {
            const struct as_condition *c;
            if (limit - next < sizeof(*c)) return ERR_PTR(-EINVAL);
            c = (const void *)next;
            if (le32_to_cpu(c->bytes) > limit - next - sizeof(*c) || !condition_valid(c, kind, ops)) return ERR_PTR(-EOPNOTSUPP);
            if(le16_to_cpu(c->field)==26)mount_effect=true;
            next += sizeof(*c) + le32_to_cpu(c->bytes);
        }
        if (next != limit || (kind==2&&(ops&AS_FILE_OPERATION_BIT(AS_FILE_MOUNT))&&!mount_effect)) return ERR_PTR(-EINVAL);
        counts[kind-1]++; at = limit;
    }
    route_offset = at - data;
    for (i = 0; i < backend_routes; i++) {
        const struct as_backend_route *route = (const void *)at; u32 operations, value_bytes; u16 kind, field;
        if (end - at < sizeof(*route)) return ERR_PTR(-EINVAL);
        kind = le16_to_cpu(route->kind); field = le16_to_cpu(route->condition.field);
        operations = le32_to_cpu(route->operations); value_bytes = le32_to_cpu(route->condition.bytes);
        if (kind < 1 || kind > 3 || route->reserved || !operations ||
            (operations & ~(kind == 3 ? 3u : kind == 2 ? AS_FILE_CONTROLLED_OPERATIONS : 1u)) ||
            value_bytes > end - at - sizeof(*route) ||
            !((kind == 1 && field == 13) || (kind == 2 && (field == 15 || field == 19)) || (kind == 3 && field == 23)) ||
            !condition_valid(&route->condition, kind, operations)) return ERR_PTR(-EINVAL);
        route_operations[kind-1] |= operations;
        at += sizeof(*route) + value_bytes;
    }
    if (at != end) return ERR_PTR(-EINVAL);
    s = kzalloc(sizeof(*s), GFP_KERNEL); if (!s) return ERR_PTR(-ENOMEM);
    s->bytes = kvmemdup(data, length, GFP_KERNEL); if (!s->bytes) { kfree(s); return ERR_PTR(-ENOMEM); }
    s->retirement = message_alloc(8, GFP_KERNEL);
    if (!s->retirement) { kvfree(s->bytes); kfree(s); return ERR_PTR(-ENOMEM); }
    s->length = length; refcount_set(&s->refs, 1); INIT_WORK(&s->release, release_snapshot); memcpy(s->config, images->config, 32); memcpy(s->rules, images->rules, 32);
    s->authority=get_current_cred();
    s->defaults = le32_to_cpu(cfg->defaults); s->max_pending = le32_to_cpu(cfg->max_pending); s->max_hold_ms = le32_to_cpu(cfg->max_hold_ms);
    for (i = 0; i < 3; i++) s->backend_operations[i] = le32_to_cpu(policy->backend_operations[i]);
    s->backend_routes = s->bytes + route_offset;
    s->backend_route_count = backend_routes;
    for (i = 0; i < 3; i++) s->backend_route_operations[i] = route_operations[i];
    for (i = 0; i < 3; i++) if (counts[i]) {
        s->pool[i] = kmalloc_array(counts[i], sizeof(*s->pool[i]), GFP_KERNEL);
        if (!s->pool[i]) { snapshot_put(s); return ERR_PTR(-ENOMEM); }
    }
    at = s->bytes + ((const u8 *)(policy + 1) - data);
    for (i = 0; i < le32_to_cpu(policy->rules); i++) {
        const struct as_rule *r = (const void *)at; u16 kind = le16_to_cpu(r->kind);
        s->pool[kind-1][s->count[kind-1]++]=r;
        s->driver_operations[kind-1] |= le32_to_cpu(r->operations);
        s->max_hit_bytes[kind-1]=max_t(u32,s->max_hit_bytes[kind-1],sizeof(struct as_hit)+(u32)le16_to_cpu(r->group_bytes)+le16_to_cpu(r->id_bytes));
        at+=le32_to_cpu(r->bytes);
    }
    for (i = 0; i < 3; i++) {
        u32 ops = candidate_operations(s,i+1);
        if (ops && sizeof(struct as_frame)+sizeof(struct as_controlled)+s->max_hit_bytes[i]+sizeof(struct as_result)+sizeof(struct as_event)+AS_REQUIRED_COMMON_BYTES+required_payload(i+1,ops) > AS_FRAME_MAX) {
            snapshot_put(s); return ERR_PTR(-E2BIG);
        }
    }
    s->group_ports = le32_to_cpu(cfg->group_ports);
    s->gateway_ws_port = le32_to_cpu(cfg->gateway_ws_port);
    if (cfg->root_bytes) {
        struct as_self_protection *previous = NULL; unsigned long flags;
        spin_lock_irqsave(&core_lock,flags);if(active&&active->self){previous=active->self;as_self_get(previous);}spin_unlock_irqrestore(&core_lock,flags);
        const u8 *root = (const void *)(cfg + 1);
        s->self = as_self_prepare(root, le32_to_cpu(cfg->root_bytes), root + le32_to_cpu(cfg->root_bytes), le32_to_cpu(cfg->backend_bytes),
            root + le32_to_cpu(cfg->root_bytes) + le32_to_cpu(cfg->backend_bytes), le32_to_cpu(cfg->loader_bytes),
            task_tgid(current), previous, release_queue, cancelled);
        as_self_put(previous);s->self_log=le32_to_cpu(cfg->self_protection_log)!=0;
        if (IS_ERR(s->self)) { int err = PTR_ERR(s->self); s->self = NULL; snapshot_put(s); return ERR_PTR(err); }
    }
    {
        const u8 *directories = (const u8 *)(cfg + 1) + le32_to_cpu(cfg->root_bytes) +
            le32_to_cpu(cfg->backend_bytes) + le32_to_cpu(cfg->loader_bytes);
        s->close_scope = close_scope_prepare(directories, le32_to_cpu(cfg->close_directory_bytes),
            le32_to_cpu(cfg->close_directories), initialize, s->self, cancelled);
        if (IS_ERR(s->close_scope)) {
            int error = PTR_ERR(s->close_scope);
            s->close_scope = NULL; if (s->self) as_self_finish(s->self); snapshot_put(s); return ERR_PTR(error);
        }
    }
    *expected = le64_to_cpu(images->expected_generation); return s;
}
static size_t hit_bytes(const struct as_rule *hit)
{ return hit ? sizeof(struct as_hit) + le16_to_cpu(hit->group_bytes) + le16_to_cpu(hit->id_bytes) : 0; }
static struct as_hit rule_hit(const struct as_rule *hit)
{
    return (struct as_hit){ hit->group_number, hit->rule_number, hit->log,
                            cpu_to_le32(2), hit->group_bytes, hit->id_bytes };
}
static u8 *write_hit(u8 *out, const struct as_hit *hit, const void *ids)
{
    size_t size = le16_to_cpu(hit->group_bytes) + le16_to_cpu(hit->id_bytes);
    memcpy(out, hit, sizeof(*hit)); out += sizeof(*hit);
    memcpy(out, ids, size);
    return out + size;
}
static void hit_lost_locked(void)
{
    if (hit_loss != U64_MAX) hit_loss++;
}
static void count_hit_locked(const struct as_event *event, const struct as_hit *hit,
                             const void *ids, const u8 *version)
{
    struct as_hit_count record = {};
    struct as_message *message = list_last_entry_or_null(&ordinary, struct as_message, link);
    struct as_frame *header;
    size_t ids_bytes = le16_to_cpu(hit->group_bytes) + le16_to_cpu(hit->id_bytes);
    size_t entry_bytes = sizeof(record) + sizeof(*hit) + ids_bytes;
    bool append_to_tail = message && message->last_hit;

    if (!controller || !facts_enabled || !event->tgid) return;
    if (append_to_tail) {
        struct as_hit_count *last = (void *)(message->data + message->last_hit);
        if (last->tgid == event->tgid && last->birth == event->birth && last->kind == event->kind &&
            le16_to_cpu(last->hit_bytes) == sizeof(*hit) + ids_bytes &&
            !memcmp(last->version, version, sizeof(last->version)) &&
            !memcmp(last + 1, hit, sizeof(*hit)) &&
            !memcmp((u8 *)(last + 1) + sizeof(*hit), ids, ids_bytes)) {
            u64 count = le64_to_cpu(last->count), sequence = le64_to_cpu(event->sequence);
            if (count == U64_MAX) { hit_lost_locked(); return; }
            last->first = cpu_to_le64(min(le64_to_cpu(last->first), sequence));
            last->last = cpu_to_le64(max(le64_to_cpu(last->last), sequence));
            last->count = cpu_to_le64(count + 1);
            return;
        }
        append_to_tail = message->bytes + entry_bytes <= sizeof(*header) + AS_HIT_BATCH_BODY;
    }
    if (!append_to_tail) {
        if (hit_batches == AS_HIT_BATCH_MAX) {
            hit_lost_locked(); return;
        }
        message = message_alloc(max_t(size_t, AS_HIT_BATCH_BODY, entry_bytes + sizeof(source)), GFP_ATOMIC);
        if (!message) { hit_lost_locked(); return; }
        message->bytes = sizeof(*header) + sizeof(source);
        frame_header((void *)message->data, AS_CLASS_EVENT, AS_HIT_BATCH, 0, sizeof(source), context.session);
        memcpy(message->data + sizeof(*header), source, sizeof(source));
        ordinary_queue_locked(message);
    }
    record.first = record.last = event->sequence;
    record.birth = event->birth;
    record.count = cpu_to_le64(1);
    record.tgid = event->tgid;
    record.kind = event->kind;
    record.hit_bytes = cpu_to_le16(sizeof(*hit) + ids_bytes);
    memcpy(record.version, version, sizeof(record.version));
    message->last_hit = message->bytes;
    memcpy(message->data + message->bytes, &record, sizeof(record));
    write_hit(message->data + message->bytes + sizeof(record), hit, ids);
    message->bytes += entry_bytes;
    header = (void *)message->data;
    header->body_bytes = cpu_to_le32(message->bytes - sizeof(*header));
    header->total_bytes = cpu_to_le64(message->bytes - sizeof(*header));
}
static size_t controlled_body(u8 *out, const struct as_original *r)
{
    struct as_controlled h = {}; size_t hit = hit_bytes(r->hit);
    h.id = cpu_to_le64(r->id); h.started_ns = cpu_to_le64(r->started); h.deadline_ns = cpu_to_le64(r->deadline);
    h.generation = cpu_to_le64(r->snapshot->generation); h.revision = cpu_to_le64(r->revision);
    memcpy(h.config, r->snapshot->config, 32); memcpy(h.rules, r->snapshot->rules, 32);
    h.event_bytes = cpu_to_le32(r->length); h.hit_bytes = cpu_to_le32(hit);
    memcpy(out, &h, sizeof(h)); out += sizeof(h);
    if (r->hit) {
        struct as_hit metadata = rule_hit(r->hit);
        out = write_hit(out, &metadata, r->hit + 1);
    }
    memcpy(out, r->event, r->length);
    return sizeof(h) + hit + r->length;
}
static void result_body(struct as_result *out, const struct as_original *r, int result)
{
    memset(out, 0, sizeof(*out)); out->id = cpu_to_le64(r->id); out->generation = cpu_to_le64(r->snapshot->generation);
    out->result = cpu_to_le32(result); out->flags = cpu_to_le32(r->terminate);
    out->actor_signal = cpu_to_le32(r->terminate ? r->actor_signal : -ENODATA); out->parent_signal = cpu_to_le32(r->terminate==2 ? r->parent_signal : -ENODATA);
    memcpy(out->rules, r->snapshot->rules, 32);
}
static void terminate_original(struct as_original *r)
{
    if(!r->terminate)return;
    r->actor_signal=as_tools_kill_bound_process(r->actor,r->controller,r->snapshot->authority);
    if(r->terminate==2)r->parent_signal=as_tools_kill_bound_process(r->parent,r->controller,r->snapshot->authority);
}
static bool reports_direct(const struct as_original *r, int result)
{
    return r->direct_observation && r->hit && (r->hit->log ||
        (result == -EPERM && le16_to_cpu(r->hit->action) == AS_DENY && r->hit->report_denial));
}

/* Capture once on the original caller, after fast matching and before waiting
 * or termination. Only this owned event receives the snapshot. No target
 * memory is dereferenced and no address-indexed state survives the event. */
static int capture_execution(struct as_original *r, const struct as_hook_facts *facts)
{
    struct as_event *event = (void *)r->event;
    struct as_syscall_snapshot call;
    struct as_stack_snapshot stack;
    struct as_user_stack_snapshot user_stack;
    unsigned long arguments[AS_SYSCALL_ARGUMENTS];
#ifdef CONFIG_STACKTRACE
    unsigned long frames[AS_KERNEL_STACK_LIMIT];
    unsigned int count;
#endif
    struct pt_regs *regs;
    size_t original_length = r->length;
    size_t common_end = sizeof(*event) + le32_to_cpu(event->common_bytes);
    size_t extra = 0, stack_bytes = 0, user_stack_bytes = 0;
    bool has_call = false;
    unsigned int i;
    int error = 0;

    /* start_at withheld this space while variable paths/argv were filled. */
    r->capacity += AS_EXECUTION_BYTES;
    if (!(current->flags & PF_KTHREAD) && current->mm) {
        regs = task_pt_regs(current);
        if (user_mode(regs)) {
            call.audit_arch = cpu_to_le32(syscall_get_arch(current));
            call.number = cpu_to_le32(syscall_get_nr(current, regs));
            call.instruction_pointer = cpu_to_le64(instruction_pointer(regs));
            call.stack_pointer = cpu_to_le64(user_stack_pointer(regs));
            syscall_get_arguments(current, regs, arguments);
            for (i = 0; i < AS_SYSCALL_ARGUMENTS; i++)
                call.arguments[i] = cpu_to_le64(arguments[i]);
            has_call = true;
            extra += AS_TLV_BYTES(sizeof(call));
        }
    }
#ifdef CONFIG_STACKTRACE
    {
        count = stack_trace_save(frames, ARRAY_SIZE(frames), 0);
        stack.flags = cpu_to_le32(count == ARRAY_SIZE(frames) ? 1 : 0);
        stack.count = cpu_to_le32(count);
        for (i = 0; i < count; i++)
            stack.instruction_pointers[i] = cpu_to_le64(frames[i]);
        stack_bytes = 8 + count * sizeof(stack.instruction_pointers[0]);
        extra += AS_TLV_BYTES(stack_bytes);
    }
#endif
    if (facts && (facts->available & AS_HOOK_USER_STACK)) {
        int result = facts->user_stack_result;
        if (result < -MAX_ERRNO || result > (int)sizeof(facts->user_stack) ||
            (result >= 0 && result % sizeof(facts->user_stack[0])))
            return -EINVAL;
        user_stack.result = cpu_to_le32((u32)result);
        user_stack.flags = cpu_to_le32(result == sizeof(facts->user_stack) ? 1 : 0);
        user_stack_bytes = 8;
        if (result > 0) {
            for (i = 0; i < result / sizeof(facts->user_stack[0]); i++)
                user_stack.instruction_pointers[i] = cpu_to_le64(facts->user_stack[i]);
            user_stack_bytes += result;
        }
        extra += AS_TLV_BYTES(user_stack_bytes);
    }
    if (common_end > original_length || original_length > r->capacity ||
        extra > r->capacity - original_length)
        return -E2BIG;
    /* Insert into the one existing common region; move the payload in place. */
    memmove(r->event + common_end + extra, r->event + common_end,
            original_length - common_end);
    r->length = common_end;
    if (has_call) error = append(r, AS_COMMON_SYSCALL, &call, sizeof(call));
    if (!error && stack_bytes)
        error = append(r, AS_COMMON_KERNEL_STACK, &stack, stack_bytes);
    if (!error && user_stack_bytes)
        error = append(r, AS_COMMON_USER_STACK, &user_stack, user_stack_bytes);
    if (error) return error;
    r->length = original_length + extra;
    event->common_bytes = cpu_to_le32(common_end - sizeof(*event) + extra);
    event->bytes = cpu_to_le32(r->length);
    return 0;
}
static void complete_direct(struct as_original *r, int result, const struct as_hook_facts *facts)
{
    struct as_message *m; struct as_event *event = (void *)r->event; size_t bytes;
    unsigned long flags; u8 session[16];
    bool report = reports_direct(r, result);
    if (report && capture_execution(r, facts)) {
        spin_lock_irqsave(&core_lock, flags); dropped++; spin_unlock_irqrestore(&core_lock, flags);
        report = false;
    }
    /* Observation never changes the already chosen action or skips effects. */
    terminate_original(r);
    if (!report) return;
    bytes = sizeof(struct as_controlled) + hit_bytes(r->hit) + r->length + sizeof(struct as_result);
    spin_lock_irqsave(&core_lock, flags);
    if (!controller || !connection || !connection->hello || ordinary_count >= AS_ORDINARY_MAX || bytes > AS_FRAME_MAX - sizeof(struct as_frame)) {
        dropped++; spin_unlock_irqrestore(&core_lock, flags); return;
    }
    memcpy(session, context.session, 16); spin_unlock_irqrestore(&core_lock, flags);
    m = message_alloc(bytes, GFP_ATOMIC); if (!m) { spin_lock_irqsave(&core_lock,flags); dropped++; spin_unlock_irqrestore(&core_lock,flags); return; }
    event->bytes = cpu_to_le32(r->length); event->stage = cpu_to_le16(3); r->id = 0;
    spin_lock_irqsave(&core_lock, flags); note_request_wait(event, 0); spin_unlock_irqrestore(&core_lock, flags);
    frame_header((void *)m->data, AS_CLASS_EVENT, AS_DIRECT, 0, bytes, session);
    result_body((void *)(m->data + sizeof(struct as_frame) + controlled_body(m->data + sizeof(struct as_frame), r)), r, result);
    spin_lock_irqsave(&core_lock, flags);
    if (!controller || memcmp(session, context.session, 16) || ordinary_count >= AS_ORDINARY_MAX) { dropped++; message_free(m); }
    else { ordinary_queue_locked(m); }
    spin_unlock_irqrestore(&core_lock, flags);
}
static int source_admission_error_locked(void)
{
    if (!controller || source_state == FAILED) return -EIO;
    return source_state == RUNNING ? 0 : -ESHUTDOWN;
}

bool as_futex_session(u8 session[16])
{
    unsigned long flags;
    bool active_session;

    spin_lock_irqsave(&core_lock, flags);
    /* Avoid feeding the observer's own wakeups back into its message queue. */
    active_session = controller && source_state == RUNNING && task_tgid(current) != controller;
    if (active_session)
        memcpy(session, context.session, 16);
    spin_unlock_irqrestore(&core_lock, flags);
    return active_session;
}

void as_futex_publish(const u8 session[16], struct as_futex_selection *fact)
{
    struct as_message *message = message_alloc(sizeof(*fact), GFP_ATOMIC);
    unsigned long flags;

    spin_lock_irqsave(&core_lock, flags);
    if (!controller || source_state != RUNNING || memcmp(session, context.session, 16)) {
        message_free(message);
    } else if (!message || ordinary_count >= AS_ORDINARY_MAX || sequence == U64_MAX) {
        dropped++;
        message_free(message);
    } else {
        memcpy(fact->source, source, 16);
        fact->sequence = cpu_to_le64(++sequence);
        frame_header((void *)message->data, AS_CLASS_EVENT, AS_FUTEX_SELECTION, 0, sizeof(*fact), session);
        memcpy(message->data + sizeof(struct as_frame), fact, sizeof(*fact));
        ordinary_queue_locked(message);
    }
    spin_unlock_irqrestore(&core_lock, flags);
}
/* Both packet and completed I/O records share this authenticated source sequence. */
u64 as_group_scope(u16 sport, u16 dport)
{
    unsigned long irq;
    u64 selected = 0;

    spin_lock_irqsave(&core_lock, irq);
    if (controller && source_state == RUNNING && (group_ports || gateway_ws_port)) {
        u16 a2a = group_ports, otlp = group_ports >> 16;
        if ((a2a && (sport == a2a || dport == a2a)) ||
            (otlp && (sport == otlp || dport == otlp)) ||
            (gateway_ws_port && (sport == gateway_ws_port || dport == gateway_ws_port)))
            selected = group_generation;
    }
    spin_unlock_irqrestore(&core_lock, irq);
    return selected;
}

bool as_group_otlp(u16 dport, u64 generation)
{
    unsigned long irq;
    bool selected;

    spin_lock_irqsave(&core_lock, irq);
    selected = group_generation == generation && (group_ports >> 16) && (group_ports >> 16) == dport;
    spin_unlock_irqrestore(&core_lock, irq);
    return selected;
}

void as_group_publish(struct as_group_tcp *fact, const struct sk_buff *skb, int offset)
{
    u32 length = skb ? le32_to_cpu(fact->length) : 0;
    size_t total = sizeof(*fact) + (size_t)length, position = 0;
    const size_t capacity = AS_FRAME_MAX - sizeof(struct as_frame);
    struct as_message *message;
    LIST_HEAD(batch);
    u32 frames = 0;
    bool complete = total <= AS_TRANSFER_MAX && DIV_ROUND_UP(total, capacity) <= 32 &&
        (!skb || (offset >= 0 && length <= (u32)(INT_MAX - offset)));
    unsigned long irq;

    /* The hook owns skb until this returns. Copy directly into the original
     * physical messages; no large intermediate buffer or deferred skb reader. */
    while (complete && position < total) {
        size_t bytes = min_t(size_t, capacity, total - position);
        size_t prefix = position ? 0 : sizeof(*fact);
        size_t payload_offset = position ? position - sizeof(*fact) : 0;

        message = message_alloc(bytes, GFP_ATOMIC);
        if (!message) { complete = false; break; }
        if (skb && bytes > prefix && skb_copy_bits(skb, offset + (int)payload_offset,
                message->data + sizeof(struct as_frame) + prefix, bytes - prefix)) {
            message_free(message);
            complete = false;
            break;
        }
        list_add_tail(&message->link, &batch);
        frames++;
        position += bytes;
    }
    spin_lock_irqsave(&core_lock, irq);
    if (controller && source_state == RUNNING &&
        group_generation == le64_to_cpu(fact->generation) && group_ordinal != U64_MAX) {
        fact->ordinal = cpu_to_le64(++group_ordinal);
        if (complete && frames && ordinary_count <= AS_ORDINARY_MAX - frames &&
            group_pending <= 32 - frames) {
            memcpy(fact->source, source, 16);
            position = 0;
            list_for_each_entry(message, &batch, link) {
                struct as_frame *header = (void *)message->data;
                size_t bytes = message->bytes - sizeof(*header);

                frame_header(header, AS_CLASS_EVENT, AS_GROUP_TCP, 0, bytes, context.session);
                header->total_bytes = cpu_to_le64(total);
                header->offset = cpu_to_le64(position);
                if (!position) memcpy(message->data + sizeof(*header), fact, sizeof(*fact));
                position += bytes;
            }
            ordinary_batch_locked(&batch);
        }
    }
    spin_unlock_irqrestore(&core_lock, irq);
    queue_clear(&batch);
}

static struct as_original *start_at(u16 kind, u32 operation, const struct as_gate_view *view, const struct as_hook_facts *facts)
{
    struct as_original *r; struct as_snapshot *s; unsigned long flags; unsigned bucket; u32 setting; u64 revision, started; bool enabled, report;size_t capacity,reserve;
    spin_lock_irqsave(&core_lock, flags); s = view ? view->snapshot : active;
    if (!s) { spin_unlock_irqrestore(&core_lock, flags); return NULL; }
    setting = (overrides >> ((kind-1)*2)) & 3;
    enabled = view ? view->enabled : setting == 1 || (!setting && (s->defaults & (1u << (kind-1))));
    if (!enabled) { spin_unlock_irqrestore(&core_lock, flags); return NULL; }
    /* Like TOMOYO's per-operation disabled check, decide whether any policy
     * needs these facts before allocation/path/argv work. No fake Allow rule. */
    if (!(candidate_operations(s,kind) & (1u << (operation-1)))) {
        int error = source_admission_error_locked();
        spin_unlock_irqrestore(&core_lock, flags); return error ? ERR_PTR(error) : NULL;
    }
    if (capture_count >= AS_CAPTURE_MAX) { spin_unlock_irqrestore(&core_lock,flags); return ERR_PTR(-EAGAIN); }
    report=observations_open_locked();refcount_inc(&s->refs); readers++; capture_count++;bucket=facts_bucket;facts_inflight[bucket]++; revision = view ? view->revision : state_revision; started = view ? view->started : ktime_get_ns(); spin_unlock_irqrestore(&core_lock, flags);
    if (current->flags & PF_IO_WORKER) { r = ERR_PTR(-EOPNOTSUPP); goto failure; }
    if (current->flags & PF_KTHREAD) { r = NULL; goto failure; }
    r = kzalloc(sizeof(*r) + AS_FRAME_MAX - sizeof(struct as_frame) - sizeof(struct as_controlled), GFP_NOWAIT); if (!r) { r = ERR_PTR(-ENOMEM); goto failure; }
    spin_lock_irqsave(&core_lock,flags);r->controller=get_pid(controller);spin_unlock_irqrestore(&core_lock,flags);
    capacity=AS_FRAME_MAX-sizeof(struct as_frame)-sizeof(struct as_controlled);
    reserve=s->max_hit_bytes[kind-1]+sizeof(struct as_result);
    /* Adopted images were checked with the full Direct envelope and fixed
     * common/payload budget; no clamp may hide an impossible message. */
    capacity-=reserve+AS_EXECUTION_BYTES;
    r->snapshot = s; r->revision = revision; r->direct_observation = report; r->capture_token = true;r->facts_token=true;r->facts_bucket=bucket; r->capacity = capacity; INIT_LIST_HEAD(&r->link); init_waitqueue_head(&r->wait); r->fallback = U32_MAX;
    r->started = started; r->deadline = r->started + (u64)s->max_hold_ms * NSEC_PER_MSEC;
    r->error = capture(r, current, kind, operation, 2, true, 0, facts); return r;
failure:
    snapshot_put(s);
    spin_lock_irqsave(&core_lock,flags); readers--; capture_count--;facts_inflight[bucket]--;facts_publish_locked(); spin_unlock_irqrestore(&core_lock,flags); wake_up_interruptible(&read_wait); return r;
}
static struct as_original *start(u16 kind, u32 operation, const struct as_hook_facts *facts)
{ return start_at(kind, operation, NULL, facts); }
static void original_free(struct as_original *r)
{
    unsigned long flags; bool token = r->capture_token;
    spin_lock_irqsave(&core_lock,flags);facts_release_locked(r);spin_unlock_irqrestore(&core_lock,flags);
    put_pid(r->actor);put_pid(r->parent);put_pid(r->controller);
    snapshot_put(r->snapshot); message_free(r->receipt); message_free(r->notice); kfree(r);
    /* Release the resource only after the actual object and its scratch ownership ended. */
    if (token) { spin_lock_irqsave(&core_lock,flags); capture_count--; spin_unlock_irqrestore(&core_lock,flags); wake_up_interruptible(&read_wait); }
}
static void control_timeout(const struct as_original *r, const char *stage)
{
    const struct as_event *event = (const void *)r->event;
    /* Original source facts only. Never print paths, argv or private payloads,
     * and never run console output while holding core_lock. */
    pr_warn_ratelimited("as_wait: control timeout stage=%s kind=%u operation=%u tgid=%u tid=%u sequence=%llu elapsed_ns=%llu max_hold_ms=%u delivered=%u\n",
        stage, le16_to_cpu(event->kind), le32_to_cpu(event->operation),
        le32_to_cpu(event->tgid), le32_to_cpu(event->tid),
        (unsigned long long)le64_to_cpu(event->sequence),
        (unsigned long long)(ktime_get_ns() - r->started), r->snapshot->max_hold_ms, r->delivered);
}
/* One native delivery is authorized only by a reserved userspace queue slot.
 * Unpublished notices remain owned by the existing bounded pending originals. */
static void publish_control_locked(struct as_reader *reader)
{
    struct as_original *original;
    if (!reader || !reader->control_credit) return;
    list_for_each_entry(original, &pending, link) {
        if (original->notice && list_empty(&original->notice->link)) {
            list_add_tail(&original->notice->link, &replies);
            reader->control_credit = false;
            wake_up_interruptible(&read_wait);
            break;
        }
    }
}
static int finish(struct as_original *r, const struct as_hook_facts *facts)
{
    unsigned long flags; int verdict, result; const struct as_rule *hit = NULL; const char *timeout_stage = NULL;
    if (IS_ERR_OR_NULL(r)) return r ? PTR_ERR(r) : 0;
    if (r->error) { result = r->error; goto leave_reader; }
    ((struct as_event *)r->event)->bytes = cpu_to_le32(r->length);
    verdict = match(r->snapshot, r, &hit); r->hit = hit;
    if (verdict != AS_TO_USER) {
        result = verdict < 0 ? verdict : verdict == AS_DENY ? -EPERM : 0;
        if (verdict == AS_ALLOW && !hit) {
            spin_lock_irqsave(&core_lock, flags); result = source_admission_error_locked(); spin_unlock_irqrestore(&core_lock, flags);
        }
        if (hit && r->direct_observation) {
            struct as_hit metadata = rule_hit(hit);
            spin_lock_irqsave(&core_lock, flags);
            count_hit_locked((const void *)r->event, &metadata, hit + 1, r->snapshot->rules);
            spin_unlock_irqrestore(&core_lock, flags);
        }
        if(verdict==AS_DENY && hit)r->terminate=le32_to_cpu(hit->terminate);
        complete_direct(r, result, facts); goto leave_reader;
    }
    result = capture_execution(r, facts);
    if (result) goto leave_reader;
    if (sizeof(struct as_frame) + sizeof(struct as_controlled) + hit_bytes(hit) + r->length > AS_FRAME_MAX) { result = -E2BIG; goto leave_reader; }
    r->receipt = message_alloc(sizeof(struct as_result), GFP_NOWAIT);
    if (!r->receipt) { result = -ENOMEM; goto leave_reader; }
    r->notice = message_alloc(sizeof(struct as_controlled) + hit_bytes(hit) + r->length, GFP_NOWAIT);
    if (!r->notice) { result = -ENOMEM; goto leave_reader; }
    spin_lock_irqsave(&core_lock, flags); readers--;
    result = source_admission_error_locked(); if (result) goto reject;
    if (held + receipt_count >= r->snapshot->max_pending || original_sequence == U64_MAX) { result = -EAGAIN; goto reject; }
    if (ktime_get_ns() >= r->deadline) { result = -ETIMEDOUT; timeout_stage = "before_admission"; goto reject; }
    r->id = ++original_sequence; r->state = HELD; held++;
    note_request_wait((const void *)r->event, r->id);
    list_add_tail(&r->link, &pending);
    frame_header((void *)r->notice->data, AS_CLASS_EVENT, AS_CONTROLLED, 0, r->notice->bytes - sizeof(struct as_frame), context.session);
    controlled_body(r->notice->data + sizeof(struct as_frame), r); r->notice->original = r;
    publish_control_locked(connection);facts_release_locked(r);
    while (r->state != COMPLETING) {
        u64 deadline = r->deadline, now = ktime_get_ns(); long waited;
        if (now >= deadline) {
            if (r->fallback <= AS_DENY) {r->decision = r->fallback;r->terminate=r->fallback_terminate;} else {r->error = -ETIMEDOUT;timeout_stage = "unbound_wait";}
            r->state = COMPLETING; break;
        }
        spin_unlock_irqrestore(&core_lock, flags);
        waited = wait_event_interruptible_timeout(r->wait, READ_ONCE(r->state) == COMPLETING || READ_ONCE(r->deadline) != deadline,
                    max_t(unsigned long, 1, nsecs_to_jiffies(deadline - now)));
        spin_lock_irqsave(&core_lock, flags);
        if (waited < 0 && r->state != COMPLETING) { r->error = waited; r->state = COMPLETING; }
    }
    list_del_init(&r->link);
    if (r->notice) {
        bool queued = !list_empty(&r->notice->link);
        list_del_init(&r->notice->link); message_free(r->notice); r->notice = NULL;
        if (queued && connection) { connection->control_credit = true; publish_control_locked(connection); }
    }
    result = r->error ?: (r->decision == AS_DENY ? -EPERM : 0);
    if(r->error || r->decision!=AS_DENY)r->terminate=0;
    /* Completing owns the chosen action. Keep held/capture/snapshot until effects finish. */
    spin_unlock_irqrestore(&core_lock,flags);
    if (timeout_stage) control_timeout(r, timeout_stage);
    terminate_original(r);
    spin_lock_irqsave(&core_lock,flags);
    held--;
    if (r->delivered && controller) {
        frame_header((void *)r->receipt->data, AS_CLASS_EVENT, AS_RESULT, 0, sizeof(struct as_result), context.session);
        result_body((void *)(r->receipt->data + sizeof(struct as_frame)), r, result);
        list_add_tail(&r->receipt->link, &replies); receipt_count++; r->receipt = NULL; wake_up_interruptible(&read_wait);
    }
    spin_unlock_irqrestore(&core_lock, flags); original_free(r); return result;
reject:
    spin_unlock_irqrestore(&core_lock, flags);
    if (timeout_stage) control_timeout(r, timeout_stage);
    original_free(r); return result;
leave_reader:
    spin_lock_irqsave(&core_lock, flags); readers--; spin_unlock_irqrestore(&core_lock, flags);
    original_free(r); wake_up_interruptible(&read_wait); return result;
}
enum self_call_kind { SELF_FILE, SELF_PATH, SELF_DENTRY, SELF_PAIR, SELF_LINK, SELF_UMOUNT };
struct self_call {
    enum self_call_kind kind; u32 operation;
    struct file *file; const struct path *path, *other;
    const struct path *creation_parent;
    struct dentry *dentry; struct vfsmount *mnt;
    unsigned long mount_flags; bool isolated_detach, mount_no_outward_propagation;
    /* Pointer presence records whether the original hook supplied this scalar; zero is data. */
    const u32 *rename_flags;
    const u32 *mount_effect;
    const umode_t *creation_mode;
    const __le64 *mmap_parameters;
    const struct as_truncate_parameters *truncate_parameters;
};
static struct as_snapshot *self_snapshot(u64 *revision)
{
    struct as_snapshot *snapshot; unsigned long flags;
    spin_lock_irqsave(&core_lock, flags); snapshot = active;
    if (snapshot && snapshot->self) { refcount_inc(&snapshot->refs); readers++; *revision = state_revision; }
    else snapshot = NULL;
    spin_unlock_irqrestore(&core_lock, flags); return snapshot;
}
static void self_snapshot_put(struct as_snapshot *snapshot)
{
    unsigned long flags;
    if (!snapshot) return;
    snapshot_put(snapshot);
    spin_lock_irqsave(&core_lock, flags); readers--; spin_unlock_irqrestore(&core_lock, flags);
    wake_up_interruptible(&read_wait);
}
static int self_evaluate(struct as_self_protection *self,const struct self_call *call,bool controller_actor)
{
    switch(call->kind) {
    case SELF_FILE:return as_self_file(self,call->file,call->operation,controller_actor);
    case SELF_PATH:
        if(call->operation==AS_FILE_MOUNT)return as_self_mount(self,call->path,call->mount_effect?*call->mount_effect:AS_MOUNT_UNKNOWN,controller_actor);
        return as_self_path(self,call->path,call->operation,controller_actor);
    case SELF_DENTRY:return as_self_dentry(self,call->dentry,call->operation,controller_actor);
    case SELF_PAIR:return as_self_pair(self,call->path,call->other,call->operation,controller_actor);
    case SELF_LINK:return as_self_link(self,call->dentry,call->other,controller_actor);
    case SELF_UMOUNT:return as_self_umount(self,call->mnt,call->isolated_detach);
    }
    return -EINVAL;
}
static int file_payload(struct as_original *r, const struct self_call *call)
{
    struct path mount_root;
    const struct path *from = call->file ? &call->file->f_path : call->path;
    struct dentry *dentry;
    struct inode *inode;
    struct super_block *sb;
    size_t capacity = r->capacity, metadata;
    int error;
    if (call->kind == SELF_UMOUNT && call->mnt) {
        mount_root.mnt = call->mnt; mount_root.dentry = call->mnt->mnt_root; from = &mount_root;
    }
    dentry = from ? from->dentry : call->dentry;
    inode = dentry ? d_inode(dentry) : NULL;
    sb = dentry ? dentry->d_sb : NULL;
    metadata = (sb ? AS_TLV_BYTES(8) : 0) + (inode ? AS_TLV_BYTES(8) : 0) +
        (call->file ? 2*AS_TLV_BYTES(4) : 0) +
        (call->rename_flags ? AS_TLV_BYTES(4) : 0) + (call->creation_mode ? AS_TLV_BYTES(4) : 0) + (call->mount_effect ? AS_TLV_BYTES(4) : 0) +
        (call->mmap_parameters ? AS_TLV_BYTES(24) : 0) +
        (call->truncate_parameters ? AS_TLV_BYTES(sizeof(*call->truncate_parameters)) : 0);
    if (metadata > r->capacity - r->length) return -E2BIG;
    r->capacity -= metadata;
    error = call->creation_parent
        ? append_path_leaf(r, 1, call->creation_parent, &dentry->d_name)
        : append_native_path(r, 1, from);
    error = error ?: append_native_path(r, 2, call->other);
    r->capacity = capacity;
    if (sb) {
        __le32 device[2] = { cpu_to_le32(MAJOR(sb->s_dev)), cpu_to_le32(MINOR(sb->s_dev)) };
        error = error ?: append(r, 3, device, sizeof(device));
    }
    if (inode) error = error ?: append_u64(r, 4, inode->i_ino);
    if (call->file) {
        error = error ?: append_u32(r, 5, call->file->f_flags);
        error = error ?: append_u32(r, 6, call->file->f_mode & (FMODE_READ|FMODE_WRITE));
    }
    if (call->rename_flags) error = error ?: append_u32(r, AS_FILE_TLV_RENAME_FLAGS, *call->rename_flags);
    if (call->creation_mode) error = error ?: append_u32(r, AS_FILE_TLV_CREATION_MODE, *call->creation_mode);
    if(call->mount_effect)error=error?:append_u32(r,AS_FILE_TLV_MOUNT_EFFECT,*call->mount_effect);
    if (call->mmap_parameters) error = error ?: append(r, AS_FILE_TLV_MMAP_PARAMETERS, call->mmap_parameters, 24);
    if (call->truncate_parameters)
        error = error ?: append(r, AS_FILE_TLV_TRUNCATE_PARAMETERS, call->truncate_parameters,
                                sizeof(*call->truncate_parameters));
    return error;
}
static void self_denied_notice(struct as_snapshot *snapshot,u64 revision,const struct self_call *call)
{
    static const struct {
        char group[sizeof("as-self-protection") - 1] __nonstring;
        char id[sizeof("deny-modification") - 1] __nonstring;
    } ids = { "as-self-protection", "deny-modification" };
    struct as_hit hit = { cpu_to_le32(1), cpu_to_le32(2), cpu_to_le32(snapshot->self_log),
                          cpu_to_le32(1), cpu_to_le16(sizeof(ids.group)), cpu_to_le16(sizeof(ids.id)) };
    struct as_original *r;struct as_message *message;struct as_controlled prefix={};struct as_result result={};
    struct as_event *event;unsigned long flags;unsigned bucket;size_t bytes;u8 session[16];int error;
    bool attributed = !(current->flags & (PF_IO_WORKER | PF_KTHREAD));
    struct as_event identity = {};
    u64 event_sequence;

    if (!attributed && !snapshot->self_log) return;
    spin_lock_irqsave(&core_lock,flags);
    if (!observations_open_locked()) { spin_unlock_irqrestore(&core_lock, flags); return; }
    if (sequence == U64_MAX) {
        if (attributed && facts_enabled && source_state == RUNNING) hit_lost_locked();
        if (snapshot->self_log) dropped++;
        spin_unlock_irqrestore(&core_lock, flags);
        return;
    }
    event_sequence = ++sequence;
    if (attributed && source_state == RUNNING) {
        identity.kind = cpu_to_le16(2);
        identity.sequence = cpu_to_le64(event_sequence);
        identity.tgid = cpu_to_le32(task_tgid_nr(current));
        identity.birth = cpu_to_le64(current->group_leader->start_boottime);
        count_hit_locked(&identity, &hit, &ids, snapshot->config);
    }
    if (!snapshot->self_log) { spin_unlock_irqrestore(&core_lock, flags); return; }
    if(ordinary_count>=AS_ORDINARY_MAX||observer_count>=AS_OBSERVERS_MAX){dropped++;spin_unlock_irqrestore(&core_lock,flags);return;}
    observer_count++;bucket=facts_bucket;facts_inflight[bucket]++;memcpy(session,context.session,16);spin_unlock_irqrestore(&core_lock,flags);
    r=kzalloc(sizeof(*r)+AS_FRAME_MAX-sizeof(struct as_frame),GFP_ATOMIC);if(!r)goto lost;
    r->capacity=AS_FRAME_MAX-sizeof(struct as_frame)-sizeof(prefix)-sizeof(hit)-sizeof(ids)-sizeof(result)-AS_EXECUTION_BYTES;
    if(!attributed){
        event=(void*)r->event;r->length=sizeof(*event);event->format=cpu_to_le16(1);event->header_bytes=cpu_to_le16(sizeof(*event));event->kind=cpu_to_le16(2);event->stage=cpu_to_le16(3);event->operation=cpu_to_le32(call->operation);
        memcpy(event->source,source,16);event->sequence=cpu_to_le64(event_sequence);
        event->domain=cpu_to_le64(1);event->boottime=cpu_to_le64(ktime_get_boottime_ns());event->available=cpu_to_le32(1);error=0;
    }else error=capture(r,current,2,call->operation,3,false,event_sequence,NULL);
    error=error?:file_payload(r,call);
    error=error?:capture_execution(r,NULL);
    if(r->parent)put_pid(r->parent);
    bytes=sizeof(prefix)+sizeof(hit)+sizeof(ids)+r->length+sizeof(result);
    if(error||bytes>AS_FRAME_MAX-sizeof(struct as_frame)){kfree(r);goto lost;}
    event=(void*)r->event;event->bytes=cpu_to_le32(r->length);
    message=message_alloc(bytes,GFP_ATOMIC);if(!message){kfree(r);goto lost;}
    frame_header((void*)message->data,AS_CLASS_EVENT,AS_DIRECT,0,bytes,session);
    prefix.generation=cpu_to_le64(snapshot->generation);prefix.revision=cpu_to_le64(revision);memcpy(prefix.config,snapshot->config,32);memcpy(prefix.rules,snapshot->rules,32);prefix.event_bytes=cpu_to_le32(r->length);prefix.hit_bytes=cpu_to_le32(sizeof(hit)+sizeof(ids));
    u8 *out=message->data+sizeof(struct as_frame);memcpy(out,&prefix,sizeof(prefix));out=write_hit(out+sizeof(prefix),&hit,&ids);memcpy(out,r->event,r->length);out+=r->length;
    result.generation=cpu_to_le64(snapshot->generation);result.result=cpu_to_le32(-EPERM);result.actor_signal=cpu_to_le32(-ENODATA);result.parent_signal=cpu_to_le32(-ENODATA);memcpy(result.rules,snapshot->rules,32);memcpy(out,&result,sizeof(result));kfree(r);
    spin_lock_irqsave(&core_lock,flags);
    if(!controller||memcmp(session,context.session,16)||ordinary_count>=AS_ORDINARY_MAX){dropped++;message_free(message);}else{ordinary_queue_locked(message);}
    observer_count--;facts_inflight[bucket]--;facts_publish_locked();spin_unlock_irqrestore(&core_lock,flags);wake_up_interruptible(&read_wait);return;
lost:spin_lock_irqsave(&core_lock,flags);observer_count--;facts_inflight[bucket]--;dropped++;facts_publish_locked();spin_unlock_irqrestore(&core_lock,flags);wake_up_interruptible(&read_wait);
}
/* Mount is Driver-only. Reuse the normal event, matcher, RuleHit and Direct
 * completion chain; no human wait, detached original or private allow list. */
static bool self_mount_driver_rule(struct as_snapshot *snapshot,u64 revision,const struct self_call *call,int *result)
{
    struct as_original *r;const struct as_rule *hit=NULL;unsigned long flags;int verdict;bool handled=false;
    if(!(snapshot->driver_operations[1]&AS_FILE_OPERATION_BIT(AS_FILE_MOUNT)))return false;
    r=kzalloc(sizeof(*r)+AS_FRAME_MAX-sizeof(struct as_frame)-sizeof(struct as_controlled),GFP_ATOMIC);
    if(!r)return false;
    refcount_inc(&snapshot->refs);r->snapshot=snapshot;r->revision=revision;
    r->capacity=AS_FRAME_MAX-sizeof(struct as_frame)-sizeof(struct as_controlled)-snapshot->max_hit_bytes[1]-sizeof(struct as_result)-AS_EXECUTION_BYTES;
    spin_lock_irqsave(&core_lock,flags);r->direct_observation=observations_open_locked();spin_unlock_irqrestore(&core_lock,flags);
    r->error=capture(r,current,2,AS_FILE_MOUNT,3,false,0,NULL);
    if(!r->error)r->error=file_payload(r,call);
    if(r->error)goto out;
    verdict=match(snapshot,r,&hit);
    if(!hit||(verdict!=AS_ALLOW&&verdict!=AS_DENY))goto out;
    r->hit=hit;*result=verdict==AS_DENY?-EPERM:0;handled=true;
    if(r->direct_observation){
        struct as_hit metadata=rule_hit(hit);
        spin_lock_irqsave(&core_lock,flags);count_hit_locked((const void *)r->event,&metadata,hit+1,snapshot->rules);spin_unlock_irqrestore(&core_lock,flags);
    }
    complete_direct(r,*result,NULL);
out:original_free(r);return handled;
}
static int self_guard_at(const struct self_call *call, struct as_snapshot *snapshot,
                         u64 revision, bool controller_actor)
{
    struct as_self_protection *fence=as_self_fence_get();int verdict=0,fenced=0;
    struct self_call classified;u32 effect=AS_MOUNT_UNKNOWN;
    if(call->kind==SELF_PATH&&call->operation==AS_FILE_MOUNT){
        classified=*call;
        if(snapshot&&snapshot->self)effect=as_self_mount_effect(snapshot->self,call->path,call->mount_flags,call->mount_no_outward_propagation);
        if(fence&&(!snapshot||fence!=snapshot->self)){
            u32 other=as_self_mount_effect(fence,call->path,call->mount_flags,call->mount_no_outward_propagation);
            if(!snapshot||!snapshot->self)effect=other;
            else if(other==AS_MOUNT_PROTECTED_TREE||effect==AS_MOUNT_PROTECTED_TREE)effect=AS_MOUNT_PROTECTED_TREE;
            else if(other!=AS_MOUNT_INDEPENDENT_VIEW||effect!=AS_MOUNT_INDEPENDENT_VIEW)effect=AS_MOUNT_UNKNOWN;
        }
        classified.mount_effect=&effect;call=&classified;
        /* An initialization fence cannot be overridden by an adopted policy. */
        if(!fence&&snapshot&&effect==AS_MOUNT_INDEPENDENT_VIEW&&self_mount_driver_rule(snapshot,revision,call,&verdict))goto out;
    }
    if(snapshot)verdict=self_evaluate(snapshot->self,call,controller_actor);
    if(fence&&(!snapshot||fence!=snapshot->self))fenced=self_evaluate(fence,call,false);
    /* Initialization-fence rejections are maintenance, never an adopted RuleHit. */
    if(fenced<0)verdict=fenced;else if(!verdict)verdict=fenced;
    /* Modification authority produces no synthetic allow notice. FileOpen
     * still delegates its actual decision to the ordinary Driver rules. */
    if(verdict==1)goto out;
    if(snapshot&&fenced>=0&&verdict==-EPERM)self_denied_notice(snapshot,revision,call);
out:
    as_self_put(fence);return verdict;
}
static int self_guard(const struct self_call *call)
{
    struct as_snapshot *snapshot;u64 revision=0;bool controller_actor;unsigned long flags;int verdict;
    snapshot=self_snapshot(&revision);
    spin_lock_irqsave(&core_lock,flags);controller_actor=controller&&task_tgid(current)==controller;spin_unlock_irqrestore(&core_lock,flags);
    verdict=self_guard_at(call,snapshot,revision,controller_actor);
    self_snapshot_put(snapshot);return verdict;
}
static long file_gate(const struct self_call *call, bool modifies, const struct as_hook_facts *facts)
{
    struct as_gate_view view = {};
    struct as_original *r;
    unsigned long flags;
    u32 setting;
    bool controller_actor;
    int error = 0;
    spin_lock_irqsave(&core_lock, flags);
    view.snapshot = active; view.revision = state_revision; view.started = ktime_get_ns();
    if (view.snapshot) {
        refcount_inc(&view.snapshot->refs); readers++;
        setting = (overrides >> 2) & 3;
        view.enabled = setting == 1 || (!setting && (view.snapshot->defaults & 2));
    }
    controller_actor = controller && task_tgid(current) == controller;
    spin_unlock_irqrestore(&core_lock, flags);
    if (modifies) error = self_guard_at(call, view.snapshot, view.revision, controller_actor);
    if (error < 0) { self_snapshot_put(view.snapshot); return error; }
    r = start_at(2, call->operation, &view, facts);
    self_snapshot_put(view.snapshot);
    if (!IS_ERR_OR_NULL(r) && !r->error) r->error = file_payload(r, call);
    return finish(r, facts);
}

/* The fprobe data belongs to one live VFS invocation. A held path is
 * consumed by the matching inode_create or released at the VFS boundary. */
struct as_create_scope {
    struct hlist_node link;
    struct task_struct *task;
    const struct path *parent;
    const struct dentry *dir;
    struct as_path_release *target;
};

static DEFINE_SPINLOCK(create_lock);
static DEFINE_HASHTABLE(create_scopes, 8);

static struct as_create_scope *create_scope_locked(void)
{
    struct as_create_scope *scope;

    hash_for_each_possible(create_scopes, scope, link, (unsigned long)current)
        if (scope->task == current)
            return scope;
    return NULL;
}

static int create_enter(struct fprobe *fp, unsigned long ip, unsigned long parent_ip,
                        struct ftrace_regs *regs, void *data)
{
    struct as_create_scope *scope = data;
    unsigned long flags;

    if (!scope || !READ_ONCE(active) || !try_module_get(THIS_MODULE))
        return 1;
    memset(scope, 0, sizeof(*scope));
    scope->task = current;
    spin_lock_irqsave(&create_lock, flags);
    hash_add(create_scopes, &scope->link, (unsigned long)current);
    spin_unlock_irqrestore(&create_lock, flags);
    return 0;
}

static void create_leave(struct fprobe *fp, unsigned long ip, unsigned long parent_ip,
                         struct ftrace_regs *regs, void *data)
{
    struct as_create_scope *scope = data;
    unsigned long flags;

    spin_lock_irqsave(&create_lock, flags);
    hash_del(&scope->link);
    spin_unlock_irqrestore(&create_lock, flags);
    defer_path_put(scope->target);
    module_put(THIS_MODULE);
}

static int create_path_release(struct fprobe *fp, unsigned long ip, unsigned long parent_ip,
                               struct ftrace_regs *regs, void *data)
{
    const struct path *parent = (const void *)ftrace_regs_get_argument(regs, 0);
    struct as_create_scope *scope;
    struct as_path_release *held = NULL;
    unsigned long flags;

    spin_lock_irqsave(&create_lock, flags);
    scope = create_scope_locked();
    if (scope && scope->parent == parent) {
        held = scope->target;
        scope->target = NULL;
        scope->parent = NULL;
    }
    spin_unlock_irqrestore(&create_lock, flags);
    defer_path_put(held);
    return 0;
}

static struct fprobe create_origins = {
    .entry_handler = create_enter,
    .exit_handler = create_leave,
    .entry_data_size = sizeof(struct as_create_scope),
};
static struct fprobe create_path_end = { .entry_handler = create_path_release };
static const char *create_origin_names[] = { "may_o_create", "filename_mknodat" };

/* One fprobe entry owns the lifetime of the original syscall invocation. The
 * index points into that entry, and never retains a pointer to the wait. */
struct as_request_scope {
    struct hlist_node link;
    struct task_struct *task;
    struct as_request_return record;
    u8 session[16];
    bool multiple;
    struct as_runtime_scope *runtime;
};
static DEFINE_SPINLOCK(request_lock);
static DEFINE_HASHTABLE(request_scopes, 8);

struct as_request_probe {
    struct fprobe probe;
    const char *names[2];
    u32 operation;
    bool registered;
};

static int request_enter(struct fprobe *fp, unsigned long ip, unsigned long parent_ip,
                      struct ftrace_regs *regs, void *data)
{
    struct as_request_scope *scope = data;
    unsigned long flags;

    if (!scope || !READ_ONCE(active))
        return 1;
    memset(scope, 0, sizeof(*scope));
    scope->task = current;
    scope->runtime = as_runtime_scope_get();
    scope->record.operation =
        cpu_to_le32(container_of(fp, struct as_request_probe, probe)->operation);
    spin_lock_irqsave(&request_lock, flags);
    hash_add(request_scopes, &scope->link, (unsigned long)current);
    spin_unlock_irqrestore(&request_lock, flags);
    return 0;
}

/* Called under core_lock when the existing native wait is admitted. */
static void note_request_wait(const struct as_event *event, u64 wait_id)
{
    struct as_request_scope *scope;
    unsigned long flags;
    u16 kind = le16_to_cpu(event->kind);
    u32 code = le32_to_cpu(event->operation), operation;

    if (kind == 1 && code == 1)
        operation = AS_REQUEST_EXECUTE;
    else if (kind == 2 && code == AS_FILE_OPEN)
        operation = AS_REQUEST_OPEN;
    else if (kind == 3 && code == 1)
        operation = AS_REQUEST_CONNECT;
    else if (kind == 3 && code == 2)
        operation = AS_REQUEST_SEND;
    else
        return;
    spin_lock_irqsave(&request_lock, flags);
    hash_for_each_possible(request_scopes, scope, link, (unsigned long)current) {
        if (scope->task != current)
            continue;
        /* A different nested syscall scope cannot bind an outer wait. */
        if (scope->record.operation != cpu_to_le32(operation))
            break;
        if (as_runtime_scope_active(scope->runtime)) {
            struct as_runtime_key request, enclosing, event_key;
            u32 context_flags;
            bool known = as_runtime_scope_reference(scope->runtime, &request);
            bool in_call = as_runtime_scope_enclosing(scope->runtime, &enclosing);
            if (known && in_call && !memcmp(&request, &enclosing, sizeof(request))) in_call = false;
            size_t phase_bytes = sizeof(event_key) + 8 + (wait_id ? 8 : 0) + (in_call ? sizeof(enclosing) : 0);
            size_t prefix = sizeof(struct as_runtime_handoff) + (known ? sizeof(request) : 0);
            struct as_message *message = message_alloc(prefix + phase_bytes, GFP_ATOMIC);
            void *body;
            if (message && runtime_header_locked(message, 7, known ? &request : NULL, operation, phase_bytes, &body, NULL)) {
                memcpy(event_key.source, event->source, 16); event_key.domain = event->domain; event_key.sequence = event->sequence;
                memcpy(body, &event_key, sizeof(event_key)); body += sizeof(event_key);
                context_flags = (wait_id ? 1 : 0) | (in_call ? 2 : 0);
                put_unaligned_le32(context_flags, body); put_unaligned_le32(0, body + 4); body += 8;
                if (wait_id) { put_unaligned_le64(wait_id, body); body += 8; }
                if (in_call) memcpy(body, &enclosing, sizeof(enclosing));
                runtime_queue_locked(message, phase_bytes);
            } else {
                if (!message && runtime_accepting_locked()) dropped++;
                message_free(message);
            }
        }
        if (!wait_id) break;
        if (scope->record.wait_id) {
            scope->multiple = true;
        } else {
            memcpy(scope->record.source, event->source, sizeof(event->source));
            scope->record.domain = event->domain;
            scope->record.sequence = event->sequence;
            scope->record.wait_id = cpu_to_le64(wait_id);
            memcpy(scope->session, context.session, sizeof(scope->session));
        }
        break;
    }
    spin_unlock_irqrestore(&request_lock, flags);
}

static void request_leave(struct fprobe *fp, unsigned long ip, unsigned long parent_ip,
                       struct ftrace_regs *regs, void *data)
{
    struct as_request_scope *scope = data;
    unsigned long flags;
    struct as_message *message;
    long value = ftrace_regs_get_return_value(regs);

    spin_lock_irqsave(&request_lock, flags);
    hlist_del_init(&scope->link);
    as_runtime_scope_put(scope->runtime); scope->runtime = NULL;
    spin_unlock_irqrestore(&request_lock, flags);
    if (!scope->record.wait_id || scope->multiple || value < -4095 || value > INT_MAX)
        return;

    /* Check admission, capture the fixed return scalars and enqueue under the
     * same short lock: no uncounted publisher can appear after Quiesce. */
    spin_lock_irqsave(&core_lock, flags);
    if (!observations_open_locked() || memcmp(context.session, scope->session, sizeof(scope->session))) {
        spin_unlock_irqrestore(&core_lock, flags);
        return;
    }
    message = message_alloc(sizeof(scope->record), GFP_ATOMIC);
    if (!message || ordinary_count >= AS_ORDINARY_MAX) {
        dropped++;
        message_free(message);
    } else {
        scope->record.value = cpu_to_le32((s32)value);
        scope->record.returned_boottime_ns = cpu_to_le64(ktime_get_boottime_ns());
        frame_header((void *)message->data, AS_CLASS_EVENT, AS_REQUEST_RETURN, 0,
                     sizeof(scope->record), scope->session);
        memcpy(message->data + sizeof(struct as_frame), &scope->record, sizeof(scope->record));
        ordinary_queue_locked(message);
    }
    spin_unlock_irqrestore(&core_lock, flags);
}

static void forget_request_task(struct task_struct *task)
{
    struct as_request_scope *scope;
    struct hlist_node *next;
    unsigned long flags;

    spin_lock_irqsave(&request_lock, flags);
    hash_for_each_possible_safe(request_scopes, scope, next, link, (unsigned long)task)
        if (scope->task == task) {
            hlist_del_init(&scope->link);
            as_runtime_scope_put(scope->runtime); scope->runtime = NULL;
        }
    spin_unlock_irqrestore(&request_lock, flags);
}

#if defined(CONFIG_ARM64)
#define AS_NATIVE_SYSCALL(name) "__arm64_sys_" #name
#elif defined(CONFIG_X86_64)
#define AS_NATIVE_SYSCALL(name) "__x64_sys_" #name
#endif
#define AS_REQUEST_PROBE(op, ...) { \
    .probe = { .entry_handler = request_enter, .exit_handler = request_leave, \
               .entry_data_size = sizeof(struct as_request_scope) }, \
    .names = { __VA_ARGS__ }, .operation = (op), \
}
static struct as_request_probe request_probes[] = {
    AS_REQUEST_PROBE(AS_REQUEST_OPEN, AS_NATIVE_SYSCALL(openat), AS_NATIVE_SYSCALL(openat2)),
    AS_REQUEST_PROBE(AS_REQUEST_EXECUTE, AS_NATIVE_SYSCALL(execve), AS_NATIVE_SYSCALL(execveat)),
    AS_REQUEST_PROBE(AS_REQUEST_CONNECT, AS_NATIVE_SYSCALL(connect)),
    AS_REQUEST_PROBE(AS_REQUEST_SEND, AS_NATIVE_SYSCALL(sendto), AS_NATIVE_SYSCALL(sendmsg)),
    AS_REQUEST_PROBE(AS_REQUEST_SEND, AS_NATIVE_SYSCALL(write), AS_NATIVE_SYSCALL(writev)),
    AS_REQUEST_PROBE(AS_REQUEST_SEND, AS_NATIVE_SYSCALL(sendmmsg)),
};

static bool create_has_candidate(void)
{
    unsigned long flags;
    bool needed = false;

    spin_lock_irqsave(&core_lock, flags);
    if (active) {
        u32 setting = (overrides >> 2) & 3;
        bool enabled = setting == 1 || (!setting && (active->defaults & 2));

        needed = enabled && (candidate_operations(active, 2) &
                            AS_FILE_OPERATION_BIT(AS_FILE_CREATE));
    }
    spin_unlock_irqrestore(&core_lock, flags);
    return needed;
}
/* Preparing a real exec identity grants no execution permission. The ordinary
 * Process Driver pool decides Allow/Deny after this lifecycle preparation. */
static int prepare_exec_identity(struct file *file)
{
    struct as_snapshot *snapshot;struct as_self_protection *fence;u64 revision=0;int error=0;
    snapshot=self_snapshot(&revision);fence=as_self_fence_get();
    if(snapshot)error=as_self_prepare_exec(snapshot->self,file);
    if(!error&&fence&&(!snapshot||fence!=snapshot->self))error=as_self_prepare_exec(fence,file);
    as_self_put(fence);self_snapshot_put(snapshot);return error;
}

__bpf_kfunc_start_defs();
/* The hook lends a final-release file; hold its path, never revive the file. */
__bpf_kfunc void as_observe_file_close(struct file *file)
{
    struct dentry *dentry = file->f_path.dentry;
    struct inode *inode = file_inode(file), *parent;
    struct as_close_scope *scope;
    struct fsnotify_mark *mark = NULL;
    struct as_snapshot *snapshot = NULL;
    struct as_close_message *close;
    struct as_file_close *record;
    unsigned long flags;
    u8 session[16];
    bool admitted = false;
    if (!(file->f_mode & FMODE_WRITE) || FMODE_FSNOTIFY_NONE(file->f_mode) ||
        !S_ISREG(inode->i_mode)) return;
    /* A positive child keeps its parent alive. Keep its actual parent stable
     * while checking the scope's independently referenced inode objects. */
    rcu_read_lock();
    spin_lock_irqsave(&dentry->d_lock, flags);
    parent = dentry == dentry->d_parent ? NULL : d_inode(dentry->d_parent);
    spin_lock(&core_lock);
    scope = active ? active->close_scope : NULL;
    if (parent && scope && source_state == RUNNING && observations_open_locked()) {
        struct as_close_directory *directory;
        mark = fsnotify_find_inode_mark(parent, scope->group);
        directory = mark ? container_of(mark, struct as_close_directory, mark) : NULL;
        if (directory && directory->alive) {
            if (ordinary_count >= AS_ORDINARY_MAX || observer_count >= AS_OBSERVERS_MAX) dropped++;
            else {
                snapshot = active; refcount_inc(&snapshot->refs); readers++; observer_count++;
                memcpy(session, context.session, sizeof(session)); admitted = true;
            }
        }
        /* An attached mark found while holding core_lock still owns its
         * initial listener ref: freeing_mark cannot drop it behind us. */
        if (mark) fsnotify_put_mark(mark);
    }
    spin_unlock(&core_lock);
    spin_unlock_irqrestore(&dentry->d_lock, flags);
    rcu_read_unlock();
    if (!admitted) return;
    close = kzalloc(sizeof(*close) + sizeof(struct as_frame) + sizeof(*record), GFP_ATOMIC);
    if (!close) {
        spin_lock_irqsave(&core_lock, flags); observer_count--; dropped++; spin_unlock_irqrestore(&core_lock, flags);
        self_snapshot_put(snapshot);
        wake_up_interruptible(&read_wait);
        return;
    }
    INIT_WORK(&close->release, release_close_message); refcount_set(&close->refs, 1);
    close->path = file->f_path; path_get(&close->path);
    INIT_LIST_HEAD(&close->message.link);
    close->message.bytes = sizeof(struct as_frame) + sizeof(*record);
    frame_header((void *)close->message.data, AS_CLASS_EVENT, AS_FILE_CLOSE, 0, sizeof(*record), session);
    record = (void *)(close->message.data + sizeof(struct as_frame));
    record->reporter_tid = cpu_to_le32(task_pid_nr(current));
    record->device_major = cpu_to_le32(MAJOR(inode->i_sb->s_dev));
    record->device_minor = cpu_to_le32(MINOR(inode->i_sb->s_dev));
    record->inode = cpu_to_le64(inode->i_ino);
    spin_lock_irqsave(&core_lock, flags);
    /* Already admitted callbacks retain their tail across a real Quiesce. */
    if (!controller || memcmp(session, context.session, sizeof(session)) || ordinary_count >= AS_ORDINARY_MAX) {
        dropped++; message_free(&close->message);
    } else ordinary_queue_locked(&close->message);
    spin_unlock_irqrestore(&core_lock, flags);
    self_snapshot_put(snapshot);
}

/* This only avoids unnecessary optional collection. It grants no verdict or
 * snapshot lease: the original gate still adopts and matches its own state. */
__bpf_kfunc u32 as_capture_requested(u32 kind, u32 operation)
{
    struct as_snapshot *snapshot;
    unsigned long flags;
    u32 setting;
    bool wanted = false;

    if (kind < 1 || kind > 3 || !operation || operation > 32 ||
        (current->flags & (PF_IO_WORKER | PF_KTHREAD))) return 0;
    spin_lock_irqsave(&core_lock, flags);
    snapshot = active;
    setting = (overrides >> ((kind-1)*2)) & 3;
    if (snapshot && controller && source_state == RUNNING &&
        (setting == 1 || (!setting && (snapshot->defaults & (1u << (kind-1))))))
        wanted = candidate_operations(snapshot, kind) & (1u << (operation-1));
    spin_unlock_irqrestore(&core_lock, flags);
    return wanted;
}
__bpf_kfunc long as_gate_file_open(struct file *file, const struct as_hook_facts *facts)
{
    struct inode *inode = file_inode(file);
    struct self_call call = { .kind = SELF_FILE, .operation = AS_FILE_OPEN, .file = file };
    /* security_file_open precedes chrdev_open/misc_open replacing f_op.
     * Identify only this live registered device; device_open still authenticates. */
    if (READ_ONCE(control_device) && S_ISCHR(inode->i_mode) && inode->i_rdev == READ_ONCE(control_device)) return 0;
    return file_gate(&call, (file->f_mode & FMODE_WRITE) || (file->f_flags & O_TRUNC), facts);
}
__bpf_kfunc long as_gate_file_mkdir(const struct path *dir, struct dentry *entry, umode_t mode, const struct as_hook_facts *facts)
{
    struct path path = { .mnt = dir->mnt, .dentry = entry };
    struct self_call call = { .kind = SELF_PATH, .operation = AS_FILE_MKDIR, .path = &path, .creation_mode = &mode };
    return file_gate(&call, true, facts);
}
__bpf_kfunc long as_gate_file_unlink(const struct path *dir, struct dentry *entry, const struct as_hook_facts *facts)
{
    struct path path = { .mnt = dir->mnt, .dentry = entry };
    struct self_call call = { .kind = SELF_PATH, .operation = AS_FILE_UNLINK, .path = &path };
    return file_gate(&call, true, facts);
}
__bpf_kfunc long as_gate_file_rmdir(const struct path *dir, struct dentry *entry, const struct as_hook_facts *facts)
{
    struct path path = { .mnt = dir->mnt, .dentry = entry };
    struct self_call call = { .kind = SELF_PATH, .operation = AS_FILE_RMDIR, .path = &path };
    return file_gate(&call, true, facts);
}
__bpf_kfunc long as_gate_file_rename(const struct path *old_dir, struct dentry *old_entry,
                                   const struct path *new_dir, struct dentry *new_entry, const struct as_hook_facts *facts)
{
    u32 flags = facts->rename_flags;
    struct path old_path = { .mnt = old_dir->mnt, .dentry = old_entry };
    struct path new_path = { .mnt = new_dir->mnt, .dentry = new_entry };
    struct self_call call = { .kind = SELF_PAIR, .operation = AS_FILE_RENAME, .path = &old_path,
        .other = &new_path, .rename_flags = &flags };
    return file_gate(&call, true, facts);
}
__bpf_kfunc long as_gate_exec(struct linux_binprm *bprm, const struct as_hook_facts *facts)
{
    struct as_original *r; int error;
    error=prepare_exec_identity(bprm->file);if(error)return error;
    r = start(1,1,facts);
    if (IS_ERR_OR_NULL(r)) return finish(r, facts);
    error = append_path(r, 1, bprm->file); if (error) r->error = error;
    if(!r->error)target_arguments(r,bprm);
    return finish(r, facts);
}
static int network_target(struct as_original *r, struct socket *sock, const struct sockaddr *address, int length)
{
    int error; __le16 port;u32 cookie[2]; u8 protocol = READ_ONCE(sock->sk->sk_protocol);
    if (address->sa_family == AF_INET && length >= sizeof(struct sockaddr_in)) {
        const struct sockaddr_in *v = (const void *)address; __be32 local = READ_ONCE(inet_sk(sock->sk)->inet_rcv_saddr);
        error = append(r, 1, &local, 4); error = error ?: append(r, 2, &v->sin_addr, 4);
        port = cpu_to_le16(ntohs(READ_ONCE(inet_sk(sock->sk)->inet_sport))); error = error ?: append(r, 3, &port, 2);
        port = cpu_to_le16(ntohs(v->sin_port)); error = error ?: append(r, 4, &port, 2);
    } else if (address->sa_family == AF_INET6 && length >= sizeof(struct sockaddr_in6)) {
        const struct sockaddr_in6 *v = (const void *)address; struct in6_addr local = sock->sk->sk_v6_rcv_saddr;
        error = append(r, 1, &local, 16); error = error ?: append(r, 2, &v->sin6_addr, 16);
        port = cpu_to_le16(ntohs(READ_ONCE(inet_sk(sock->sk)->inet_sport))); error = error ?: append(r, 3, &port, 2);
        port = cpu_to_le16(ntohs(v->sin6_port)); error = error ?: append(r, 4, &port, 2);
    } else return -EAFNOSUPPORT;
    error=error?:append(r,5,&protocol,1);
    /* The GPL-exported diagnostic wrapper uses the kernel's stable cookie
     * generator; never copy its private allocator or substitute actor netns. */
    sock_diag_save_cookie(sock->sk,cookie);
    error=error?:append_u64(r,7,((u64)cookie[1]<<32)|cookie[0]);
    return error?:append_u64(r,8,sock_net(sock->sk)->ns.inum);
}
/* Observe this iterator once. The original iterator and user memory remain unchanged. */
static int dns_send_snapshot(struct as_original *r, const struct msghdr *message)
{
    struct iov_iter iterator = message->msg_iter;
    size_t width, room, attempted, copied;
    const u8 *protocol = value(r, 24, &width), *port;
    u8 *field;
    struct as_dns_send_snapshot *snapshot;
    u64 requested;
    if (!protocol || width != 1 || *protocol != IPPROTO_UDP) return 0;
    port = value(r, 23, &width);
    if (!port || width != 2 || get_unaligned_le16(port) != 53) return 0;
    if (r->length > r->capacity || r->capacity - r->length < AS_TLV_BYTES(sizeof(*snapshot))) return -E2BIG;
    requested = iov_iter_count(&iterator);
    room = r->capacity - r->length - AS_TLV_BYTES(sizeof(*snapshot));
    attempted = min_t(u64, requested, room);
    field = r->event + r->length; snapshot = (void *)(field + 8);
    copied = copy_from_iter(snapshot + 1, attempted, &iterator);
    snapshot->requested_bytes = cpu_to_le64(requested);
    snapshot->attempted_bytes = cpu_to_le32(attempted);
    snapshot->send_flags = cpu_to_le32(message->msg_flags);
    put_unaligned_le16(9, field); put_unaligned_le16(0, field + 2);
    put_unaligned_le32(sizeof(*snapshot) + copied, field + 4);
    r->length += AS_TLV_BYTES(sizeof(*snapshot) + copied);
    return 0;
}
__bpf_kfunc long as_gate_connect(struct socket *sock, struct sockaddr *address, int length, const struct as_hook_facts *facts)
{
    struct as_original *r; int error;
    if (!sock->sk || (address->sa_family != AF_INET && address->sa_family != AF_INET6)) return 0;
    r = start(3, 1, facts); if (IS_ERR_OR_NULL(r)) return finish(r, facts);
    error = network_target(r, sock, address, length); if (error) r->error = error; return finish(r, facts);
}
__bpf_kfunc long as_gate_sendmsg(struct socket *sock, struct msghdr *message, int length, const struct as_hook_facts *facts)
{
    struct as_original *r; struct sockaddr_storage peer = {}; struct sockaddr *address = message->msg_name;
    int size = message->msg_namelen, error;
    if (!sock->sk || (sock->sk->sk_family != AF_INET && sock->sk->sk_family != AF_INET6)) return 0;
    if (!address) {
        if (sock->sk->sk_family == AF_INET) {
            struct sockaddr_in *v = (void *)&peer; v->sin_family = AF_INET;
            v->sin_port = READ_ONCE(inet_sk(sock->sk)->inet_dport); v->sin_addr.s_addr = READ_ONCE(inet_sk(sock->sk)->inet_daddr); size = sizeof(*v);
        } else {
            struct sockaddr_in6 *v = (void *)&peer; v->sin6_family = AF_INET6;
            v->sin6_port = READ_ONCE(inet_sk(sock->sk)->inet_dport); v->sin6_addr = sock->sk->sk_v6_daddr; size = sizeof(*v);
        }
        address = (void *)&peer;
    }
    r = start(3, 2, facts); if (IS_ERR_OR_NULL(r)) return finish(r, facts);
    error = network_target(r, sock, address, size);
    if (!error) error = dns_send_snapshot(r, message);
    if (error) r->error = error;
    return finish(r, facts);
}
__bpf_kfunc long as_guard_file_permission(struct file *file,int mask)
{struct self_call call={.kind=SELF_FILE,.operation=3,.file=file};int r;if(!(mask&MAY_WRITE))return 0;r=self_guard(&call);return r>0?0:r;}
__bpf_kfunc long as_guard_file_truncate(struct file *file)
{struct self_call call={.kind=SELF_FILE,.operation=AS_FILE_TRUNCATE,.file=file};int r=self_guard(&call);return r>0?0:r;}
__bpf_kfunc long as_guard_path(const struct path *path,u32 operation)
{struct self_call call={.kind=SELF_PATH,.operation=operation,.path=path};int r=self_guard(&call);return r>0?0:r;}
__bpf_kfunc long as_guard_entry(const struct path *dir,struct dentry *entry,u32 operation)
{struct path path={.mnt=dir->mnt,.dentry=entry};struct self_call call={.kind=SELF_PATH,.operation=operation,.path=&path};int r=self_guard(&call);return r>0?0:r;}
__bpf_kfunc long as_guard_create(const struct path *parent, struct dentry *entry, umode_t mode)
{
    struct as_create_scope *scope;
    struct as_path_release *held = NULL, *old = NULL;
    unsigned long flags;
    bool needed;
    long result = as_guard_entry(parent, entry, AS_FILE_CREATE);

    if (result || !d_is_negative(entry) || ((mode & S_IFMT) && !S_ISREG(mode)))
        return result;
    needed = create_has_candidate();
    if (needed) {
        held = reserve_path_reference(&create_references);
        if (!held)
            return -ENOMEM;
        held->path = (struct path){ .mnt = parent->mnt, .dentry = entry };
        path_get(&held->path);
    }
    spin_lock_irqsave(&create_lock, flags);
    scope = create_scope_locked();
    if (scope) {
        old = scope->target;
        scope->target = held;
        scope->parent = parent;
        scope->dir = parent->dentry;
        held = NULL;
    }
    spin_unlock_irqrestore(&create_lock, flags);
    defer_path_put(old);
    defer_path_put(held);
    return needed && !scope ? -ENODATA : 0;
}

__bpf_kfunc long as_gate_inode_create(struct inode *dir, struct dentry *entry, umode_t mode, const struct as_hook_facts *facts)
{
    struct as_create_scope *scope;
    struct as_path_release *held = NULL;
    unsigned long flags;
    long result;

    spin_lock_irqsave(&create_lock, flags);
    scope = create_scope_locked();
    if (scope && scope->target && scope->dir == entry->d_parent &&
        d_inode(entry->d_parent) == dir && scope->target->path.dentry == entry) {
        held = scope->target;
        scope->target = NULL;
    }
    spin_unlock_irqrestore(&create_lock, flags);
    if (!held)
        return 0;
    {
        /* An unhashed negative dentry can render as " (deleted)" in d_path.
         * Use the actual mounted parent and its locked target name. */
        struct path parent = { .mnt = held->path.mnt, .dentry = entry->d_parent };
        struct self_call call = {
            .kind = SELF_PATH, .operation = AS_FILE_CREATE,
            .path = &held->path, .creation_parent = &parent, .creation_mode = &mode,
        };
        result = file_gate(&call, false, facts);
    }
    defer_path_put(held);
    return result;
}
__bpf_kfunc long as_guard_link(struct dentry *from,const struct path *to_dir,struct dentry *to)
{struct path path={.mnt=to_dir->mnt,.dentry=to};struct self_call call={.kind=SELF_LINK,.operation=AS_FILE_LINK,.dentry=from,.other=&path};int r=self_guard(&call);return r>0?0:r;}
__bpf_kfunc long as_guard_metadata(struct dentry *dentry)
{struct self_call call={.kind=SELF_DENTRY,.operation=AS_FILE_METADATA,.dentry=dentry};int r=self_guard(&call);return r>0?0:r;}
__bpf_kfunc long as_gate_inode_setattr(struct dentry *dentry, struct iattr *attr, const struct as_hook_facts *facts)
{
    struct self_call call = { .kind = SELF_DENTRY, .operation = AS_FILE_METADATA, .dentry = dentry };
    struct as_truncate_parameters parameters;
    struct file *file;
    u32 flags = attr->ia_valid;

    if ((flags & (ATTR_SIZE | ATTR_FILE)) != (ATTR_SIZE | ATTR_FILE))
        return as_guard_metadata(dentry);
    file = attr->ia_file;
    if (!file || file->f_path.dentry != dentry || !S_ISREG(file_inode(file)->i_mode) || attr->ia_size < 0)
        return as_guard_metadata(dentry);

    /* The original notify_change call owns both objects and its inode lock. */
    parameters.size = cpu_to_le64(attr->ia_size);
    parameters.attributes = cpu_to_le32(flags);
    call.operation = AS_FILE_TRUNCATE;
    call.file = file;
    call.truncate_parameters = &parameters;
    return file_gate(&call, true, facts);
}
__bpf_kfunc long as_gate_truncate_path(const struct path *path, const struct as_hook_facts *facts)
{
    struct self_call call = { .kind = SELF_PATH, .operation = AS_FILE_TRUNCATE_PATH, .path = path };
    return file_gate(&call, true, facts);
}
__bpf_kfunc long as_gate_mmap(struct file *file, unsigned long reqprot, unsigned long prot,
                             unsigned long flags, const struct as_hook_facts *facts)
{
    __le64 parameters[] = { cpu_to_le64(reqprot), cpu_to_le64(prot), cpu_to_le64(flags) };
    struct self_call call = { .kind = SELF_FILE, .operation = AS_FILE_MAP_SHARED_WRITABLE,
                            .file = file, .mmap_parameters = parameters };
    unsigned long type = flags & MAP_TYPE;
    int result;

    if (!(prot & PROT_WRITE) || (type != MAP_SHARED && type != MAP_SHARED_VALIDATE))
        return 0;
    if (S_ISREG(file_inode(file)->i_mode))
        return file_gate(&call, true, facts);
    result = self_guard(&call);
    return result > 0 ? 0 : result;
}
__bpf_kfunc long as_guard_mprotect(struct vm_area_struct *vma,unsigned long prot)
{struct self_call call={.kind=SELF_FILE,.operation=AS_FILE_WRITABLE_MAPPING,.file=vma->vm_file};int r;if(!vma->vm_file||!(prot&PROT_WRITE)||!(vma->vm_flags&VM_SHARED)||(vma->vm_flags&VM_WRITE))return 0;r=self_guard(&call);return r>0?0:r;}
__bpf_kfunc long as_guard_move_mount(const struct path *from,const struct path *to)
{struct self_call call={.kind=SELF_PAIR,.operation=AS_FILE_MOVE_MOUNT,.path=from,.other=to};int r=self_guard(&call);return r>0?0:r;}
__bpf_kfunc long as_guard_mount(const struct path *path,unsigned long flags,int no_outward_propagation)
{struct self_call call={.kind=SELF_PATH,.operation=AS_FILE_MOUNT,.path=path,.mount_flags=flags,.mount_no_outward_propagation=no_outward_propagation==1};int r=self_guard(&call);return r>0?0:r;}
__bpf_kfunc long as_guard_umount(struct vfsmount *mnt,int isolated_detach)
{struct self_call call={.kind=SELF_UMOUNT,.operation=AS_FILE_UNMOUNT,.mnt=mnt,.isolated_detach=isolated_detach==1};int r=self_guard(&call);return r>0?0:r;}
__bpf_kfunc_end_defs();
BTF_KFUNCS_START(as_gate_functions)
BTF_ID_FLAGS(func, as_capture_requested)
BTF_ID_FLAGS(func, as_observe_file_close)
BTF_ID_FLAGS(func, as_gate_file_open, KF_SLEEPABLE)
BTF_ID_FLAGS(func, as_gate_file_mkdir, KF_SLEEPABLE)
BTF_ID_FLAGS(func, as_gate_file_unlink, KF_SLEEPABLE)
BTF_ID_FLAGS(func, as_gate_file_rmdir, KF_SLEEPABLE)
BTF_ID_FLAGS(func, as_gate_file_rename, KF_SLEEPABLE)
BTF_ID_FLAGS(func, as_gate_exec, KF_SLEEPABLE)
BTF_ID_FLAGS(func, as_gate_connect, KF_SLEEPABLE)
BTF_ID_FLAGS(func, as_gate_sendmsg, KF_SLEEPABLE)
BTF_ID_FLAGS(func, as_guard_file_permission)
BTF_ID_FLAGS(func, as_guard_file_truncate)
BTF_ID_FLAGS(func, as_guard_path)
BTF_ID_FLAGS(func, as_guard_entry)
BTF_ID_FLAGS(func, as_guard_create)
BTF_ID_FLAGS(func, as_gate_inode_create, KF_SLEEPABLE)
BTF_ID_FLAGS(func, as_guard_link)
BTF_ID_FLAGS(func, as_guard_metadata)
BTF_ID_FLAGS(func, as_gate_inode_setattr, KF_SLEEPABLE)
BTF_ID_FLAGS(func, as_gate_truncate_path, KF_SLEEPABLE)
BTF_ID_FLAGS(func, as_gate_mmap, KF_SLEEPABLE)
BTF_ID_FLAGS(func, as_guard_mprotect)
BTF_ID_FLAGS(func, as_guard_move_mount)
BTF_ID_FLAGS(func, as_guard_mount)
BTF_ID_FLAGS(func, as_guard_umount)
BTF_KFUNCS_END(as_gate_functions)
static int gate_filter(const struct bpf_prog *prog, u32 id)
{
    const char *name = prog->aux->attach_func_name;
    if (!name) return -EACCES;
    if (!strcmp(name, "bpf_lsm_file_release")) return 0;
    if (!strcmp(name,"bpf_lsm_file_permission")||!strcmp(name,"bpf_lsm_file_truncate")||!strcmp(name,"bpf_lsm_path_chmod")||!strcmp(name,"bpf_lsm_path_chown")||!strcmp(name,"bpf_lsm_path_mknod")||!strcmp(name,"bpf_lsm_path_symlink")||!strcmp(name,"bpf_lsm_path_link")||!strcmp(name,"bpf_lsm_inode_setxattr")||!strcmp(name,"bpf_lsm_inode_removexattr")||!strcmp(name,"bpf_lsm_inode_set_acl")||!strcmp(name,"bpf_lsm_inode_remove_acl")||!strcmp(name,"bpf_lsm_inode_file_setattr")||!strcmp(name,"bpf_lsm_file_mprotect")||!strcmp(name,"bpf_lsm_sb_mount")||!strcmp(name,"bpf_lsm_move_mount")||!strcmp(name,"bpf_lsm_sb_umount")) return 0;
    if (!prog->sleepable) return -EACCES;
    if (!strcmp(name, "bpf_lsm_file_open") || !strcmp(name, "bpf_lsm_bprm_check_security") ||
        !strcmp(name, "bpf_lsm_socket_connect") || !strcmp(name, "bpf_lsm_socket_sendmsg") ||
        !strcmp(name, "bpf_lsm_path_mkdir") || !strcmp(name, "bpf_lsm_path_unlink") ||
        !strcmp(name, "bpf_lsm_path_rmdir") || !strcmp(name, "bpf_lsm_path_rename") ||
        !strcmp(name, "bpf_lsm_path_truncate") || !strcmp(name, "bpf_lsm_mmap_file") ||
        !strcmp(name, "bpf_lsm_inode_setattr") || !strcmp(name, "bpf_lsm_inode_create")) return 0;
    return -EACCES;
}
static const struct btf_kfunc_id_set gate_functions = { .owner = THIS_MODULE, .set = &as_gate_functions, .filter = gate_filter };
static int advance_epoch_locked(void)
{
    if (lifecycle_epoch == U32_MAX) { source_state = FAILED; source_error = -EOVERFLOW; return -EOVERFLOW; }
    lifecycle_epoch++; return 0;
}
static void fail_locked(void)
{
    struct as_original *r;
    as_runtime_retire();
    if (source_state != FAILED) { source_state = FAILED; source_error = -EIO; state_revision++; }
    list_for_each_entry(r, &pending, link) {
        if (r->state == COMPLETING) continue;
        r->error = -EIO; r->state = COMPLETING; wake_up_interruptible(&r->wait);
    }
}
static void queue_clear(struct list_head *head)
{
    struct as_message *m, *next;
    list_for_each_entry_safe(m, next, head, link) { if (head == &ordinary) ordinary_remove_locked(m); else list_del(&m->link); if (m->original) m->original->notice = NULL; message_free(m); }
}
static int device_open(struct inode *inode, struct file *file)
{
    struct as_reader *r; unsigned long flags;
    if (!capable(CAP_SYS_ADMIN)) return -EPERM;
    { u64 revision=0;struct as_snapshot *snapshot=self_snapshot(&revision);bool authorized=!snapshot||as_self_backend_instance(snapshot->self);self_snapshot_put(snapshot);if(!authorized)return -EACCES; }
    r = kzalloc(sizeof(*r), GFP_KERNEL); if (!r) return -ENOMEM;
    mutex_init(&r->read_lock); mutex_init(&r->write_lock); atomic_set(&r->cancel_install,0);
    memcpy(r->context.instance, instance, 16); get_random_bytes(r->context.session, 16);
    if (!memchr_inv(r->context.session, 0, 16)) r->context.session[0] = 1;
    spin_lock_irqsave(&core_lock, flags);
    if (controller || readers || capture_count || observer_count || control_writes || held || receipt_count) { spin_unlock_irqrestore(&core_lock, flags); kfree(r); return -EBUSY; }
    original_sequence = 0; controller = get_pid(task_tgid(current)); connection = r; context = r->context; file->private_data = r;
    sequence=0;runtime_sequence=0;group_ordinal=0;hit_loss=0;get_random_bytes(source,16);if(!memchr_inv(source,0,16))source[0]=1;
    as_runtime_source_changed(source);
    spin_unlock_irqrestore(&core_lock, flags); return 0;
}
static int device_release(struct inode *inode, struct file *file)
{
    struct as_reader *r = file->private_data; struct pid *old;
    struct as_snapshot *retired = NULL; unsigned long flags;
    bool runtime_cleanup = false;
    spin_lock_irqsave(&core_lock, flags);
    old = NULL;
    if (connection == r && !memcmp(r->context.session, context.session, 16)) {
        as_runtime_retire(); runtime_cleanup = true;
        quiesced_locked(r);
        if (source_state == QUIESCING && !r->quiesce_reply && !held && !receipt_count && !r->writes) {
            /* Quiesce stops new held requests. Short direct/self callbacks
             * retain their snapshot; they are not a controller failure.
             * Unpublish first so they can finish without admitting new users. */
            retired = active; active = NULL; source_state = DORMANT;
            overrides = 0; source_error = 0; state_revision++;
            /* Cover the detached owner's final put as well: a new session
             * cannot receive a retirement belonging to this closed one. */
            if (retired) readers++;
        } else if (source_state == RUNNING || held || receipt_count || readers || capture_count || observer_count || r->writes) {
            advance_epoch_locked(); fail_locked();
        }
        old = controller; controller = NULL; connection = NULL;
        facts_close_locked();
        dropped += ordinary_count; queue_clear(&replies); queue_clear(&ordinary);
        reply_count = receipt_count = 0;
    }
    spin_unlock_irqrestore(&core_lock, flags);
    if (runtime_cleanup) flush_workqueue(release_queue);
    if (retired) {
        snapshot_put(retired);
        spin_lock_irqsave(&core_lock, flags); readers--; spin_unlock_irqrestore(&core_lock, flags);
    }
    put_pid(old); kvfree(r->transfer); message_free(r->quiesce_reply); kfree(r);
    wake_up_interruptible(&read_wait); return 0;
}
static ssize_t device_read(struct file *file, char __user *buffer, size_t bytes, loff_t *offset)
{
    struct as_reader *r = file->private_data; struct as_message *m;
    struct as_close_message *close = NULL;
    unsigned long flags; ssize_t result = -EAGAIN;
    if (!mutex_trylock(&r->read_lock)) return -EAGAIN;
    spin_lock_irqsave(&core_lock, flags);
    if (task_tgid(current) != controller || connection != r || memcmp(r->context.session, context.session, 16)) { result = -EACCES; goto done; }
    quiesced_locked(r);
    m = list_first_entry_or_null(&replies, struct as_message, link);
    if (m) {
        const struct as_frame *header = (const void *)m->data;
        if (bytes < m->bytes) { result = -EMSGSIZE; goto done; }
        memcpy(r->record, m->data, m->bytes); result = m->bytes; list_del(&m->link);
        if (le16_to_cpu(header->class) == AS_CLASS_REPLY) reply_count--;
        else if (le16_to_cpu(header->class) == AS_CLASS_EVENT && le16_to_cpu(header->type) == AS_RESULT) receipt_count--;
        if (m->original) { m->original->delivered = true; m->original->notice = NULL; }
        message_free(m); quiesced_locked(r); goto done;
    }
    m = list_first_entry_or_null(&ordinary, struct as_message, link);
    if (m) {
        if (bytes < m->bytes) { result = -EMSGSIZE; goto done; }
        if (le16_to_cpu(((struct as_frame *)m->data)->type) == AS_FILE_CLOSE) {
            size_t prepared;
            close = container_of(m, struct as_close_message, message);
            refcount_inc(&close->refs);
            spin_unlock_irqrestore(&core_lock, flags);
            prepared = prepare_close_record(m, r->record);
            spin_lock_irqsave(&core_lock, flags);
            if (task_tgid(current) != controller || connection != r ||
                memcmp(r->context.session, context.session, 16)) { result = -EACCES; goto done; }
            if (list_first_entry_or_null(&ordinary, struct as_message, link) != m) goto done;
            if (bytes < prepared) { result = -EMSGSIZE; goto done; }
            result = prepared;
        } else {
            memcpy(r->record, m->data, m->bytes); result = m->bytes;
        }
        if (m->original) { m->original->delivered = true; m->original->notice = NULL; }
        ordinary_remove_locked(m);
        message_free(m); quiesced_locked(r);
    }
done:
    spin_unlock_irqrestore(&core_lock, flags);
    if (result > 0 && copy_to_user(buffer, r->record, result)) result = -EFAULT;
    if (close) message_free(&close->message);
    mutex_unlock(&r->read_lock); return result;
}
static struct as_original *original_locked(u64 id)
{
    struct as_original *r;
    list_for_each_entry(r, &pending, link) if (r->id == id && r->state != COMPLETING) return r;
    return NULL;
}
static int small_locked(struct as_reader *reader, u16 type, const u8 *body, size_t bytes)
{
    struct as_original *r; u32 feature, enabled, shift, next;
    if (type == AS_HELLO) {
        if (bytes) return -EINVAL;
        if (reader->hello) return -EALREADY;
        reader->hello = true; return 0;
    }
    if (!reader->hello) return -EPROTO;
    switch (type) {
    case AS_STATUS: return bytes ? -EINVAL : 0;
    case AS_CONTROL_CREDIT:
        if (bytes || reader->control_credit) return -EINVAL;
        reader->control_credit = true;
        publish_control_locked(reader);
        return 0;
    case AS_QUIESCE:
        if (bytes) return -EINVAL;
        reader->stop_facts=true;
        facts_accepting=false;
        atomic_set(&reader->cancel_install,1);
        if (advance_epoch_locked()) { fail_locked(); return -EOVERFLOW; }
        if (source_state == RUNNING) source_state = QUIESCING;
        state_revision++; state_notice_locked(); return 0;
    case AS_FAILURE:
        if (bytes) return -EINVAL;
        atomic_set(&reader->cancel_install,1);
        if (advance_epoch_locked()) { fail_locked(); return -EOVERFLOW; }
        fail_locked(); state_notice_locked(); return 0;
    case AS_BIND_REVIEW: {
        const struct as_review *review = (const void *)body; u64 deadline;
        if (bytes != sizeof(*review)) return -EINVAL;
        if (le32_to_cpu(review->decision) > AS_DENY || le32_to_cpu(review->terminate)>2 ||
            (review->terminate && le32_to_cpu(review->decision)!=AS_DENY)) return -EINVAL;
        r = original_locked(le64_to_cpu(review->id)); if (!r) return -ENOENT;
        if (memcmp(review->rules, r->snapshot->rules, 32)) return -ESTALE;
        if (r->state == BOUND) return -EALREADY;
        deadline = le64_to_cpu(review->deadline_ns);
        if (ktime_get_ns() >= r->deadline || deadline <= ktime_get_ns()) return -ETIME;
        r->deadline = min(r->deadline, deadline); r->fallback = le32_to_cpu(review->decision); r->fallback_terminate=le32_to_cpu(review->terminate); r->state = BOUND;
        wake_up_interruptible(&r->wait); return 0;
    }
    case AS_DECIDE: {
        const struct as_decision *decision = (const void *)body;
        if (bytes != sizeof(*decision)) return -EINVAL;
        if (le32_to_cpu(decision->decision) > AS_DENY || le32_to_cpu(decision->terminate)>2 ||
            (decision->terminate && le32_to_cpu(decision->decision)!=AS_DENY)) return -EINVAL;
        r = original_locked(le64_to_cpu(decision->id)); if (!r) return -ENOENT;
        if (ktime_get_ns() >= r->deadline) return -ETIME;
        r->decision = le32_to_cpu(decision->decision); r->terminate=le32_to_cpu(decision->terminate); r->state = COMPLETING;
        wake_up_interruptible(&r->wait); return 0;
    }
    case AS_CANCEL:
        if (bytes != 8) return -EINVAL;
        r = original_locked(get_unaligned_le64(body)); if (!r) return -ENOENT;
        if (r->state == BOUND) {r->decision = r->fallback;r->terminate=r->fallback_terminate;} else r->error = -ECANCELED;
        r->state = COMPLETING; wake_up_interruptible(&r->wait); return 0;
    case AS_SET_FEATURE:
    case AS_RESUME_FEATURE:
        if (bytes != (type == AS_SET_FEATURE ? 8 : 4)) return -EINVAL;
        if (!active) return -ENODATA;
        feature = get_unaligned_le32(body); if (feature < 1 || feature > 3) return -EINVAL;
        shift = (feature - 1) * 2; enabled = type == AS_SET_FEATURE ? get_unaligned_le32(body + 4) : 0;
        if (enabled > 1) return -EINVAL;
        next = type == AS_SET_FEATURE ? (enabled ? 1 : 2) : 0;
        if (((overrides >> shift) & 3) != next) { overrides = (overrides & ~(3u << shift)) | (next << shift); state_revision++; state_notice_locked(); }
        return 0;
    default: return -EOPNOTSUPP;
    }
}
static bool state_reply(u16 type)
{ return type == AS_INITIALIZE || type == AS_REPLACE || type == AS_STATUS || type == AS_QUIESCE || type == AS_FAILURE || type == AS_SET_FEATURE || type == AS_RESUME_FEATURE; }
static ssize_t device_write(struct file *file, const char __user *buffer, size_t bytes, loff_t *offset)
{
    struct as_reader *r = file->private_data;
    struct as_frame *frame;
    struct as_message *reply = NULL, *cancel_reply = NULL, *fact_marker = NULL;
    struct as_snapshot *candidate = NULL, *old = NULL, *applied = NULL;
    struct as_runtime_profile *runtime_candidate = NULL;
    u8 *input = NULL, *complete = NULL, *allocation = NULL, *aborted = NULL;
    const u8 *body;
    u64 total, position, id, expected = 0, epoch = 0;
    size_t body_bytes, complete_bytes = 0, reply_body;
    unsigned long flags;
    u16 type;
    int error = 0;
    bool reply_reserved = false;
    bool runtime_cleanup = false;
    bool locked = false, complete_message = false, status, installation_owner = false, early = false, write_owner = false;
    if (bytes < sizeof(*frame) || bytes > AS_FRAME_MAX) return -EMSGSIZE;
    spin_lock_irqsave(&core_lock, flags);
    error = task_tgid(current) == controller && connection == r ? 0 : -EACCES;
    if (!error && (r->writes == U32_MAX || control_writes == U32_MAX)) error = -EAGAIN;
    if (!error) { r->writes++; control_writes++; write_owner = true; }
    spin_unlock_irqrestore(&core_lock, flags);
    if (error) return error;
    input = memdup_user(buffer, bytes);
    if (IS_ERR(input)) { error = PTR_ERR(input); input = NULL; goto done; }
    frame = (void *)input; body = input + sizeof(*frame); body_bytes = bytes - sizeof(*frame);
    type = le16_to_cpu(frame->type); total = le64_to_cpu(frame->total_bytes);
    position = le64_to_cpu(frame->offset); id = le64_to_cpu(frame->request_id);
    if (le32_to_cpu(frame->magic) != AS_MAGIC || le16_to_cpu(frame->format) != AS_FORMAT ||
        le16_to_cpu(frame->class) != AS_CLASS_REQUEST || frame->reserved16 || frame->reserved64 ||
        !id || type < 1 || type > AS_CONTROL_CREDIT || le32_to_cpu(frame->body_bytes) != body_bytes ||
        total > AS_TRANSFER_MAX || position > total || body_bytes > total - position) { error = -EPROTO; goto done; }
    if ((position || total != body_bytes) && (type != AS_INITIALIZE && type != AS_REPLACE)) { error = -EPROTO; goto done; }
    if ((type == AS_INITIALIZE || type == AS_REPLACE) && (!body_bytes || (!position && body_bytes < sizeof(struct as_images)))) { error = -EINVAL; goto done; }
    status = state_reply(type);
    reply_body = sizeof(struct as_reply) + (type == AS_HELLO ? sizeof(struct as_status) + sizeof(struct as_capabilities) : status ? sizeof(struct as_status) : 0);
    if(type==AS_START_FACTS || type==AS_FACTS_BARRIER)reply_body+=sizeof(struct as_facts_marker);
    if(type==AS_KILL_PROCESS)reply_body+=8;
    if(type==AS_INSPECT_PROCESSES){
        u32 count=body_bytes>=8?get_unaligned_le32(body):0;
        if(!count || count>AS_INSPECT_MAX || body_bytes!=8+(size_t)count*4){error=-EINVAL;goto done;}
        reply_body+=8+(size_t)count*sizeof(struct as_process_binding);
    }
    reply = message_alloc(reply_body, GFP_NOWAIT);
    if (!reply) { error = -ENOMEM; goto done; }
    if(type==AS_START_FACTS || type==AS_FACTS_BARRIER){
        fact_marker=message_alloc(sizeof(struct as_facts_marker),GFP_NOWAIT);
        if(!fact_marker){error=-ENOMEM;goto done;}
    }
    if ((type == AS_QUIESCE || type == AS_FAILURE) && !body_bytes) {
        cancel_reply = message_alloc(sizeof(struct as_reply), GFP_NOWAIT);
        if (!cancel_reply) { error = -ENOMEM; goto done; }
    }
    if (mutex_lock_interruptible(&r->write_lock)) { error = -ERESTARTSYS; goto done; }
    locked = true;
    spin_lock_irqsave(&core_lock, flags);
    if (task_tgid(current) != controller || connection != r ||
        (type == AS_HELLO ? memchr_inv(frame->session, 0, 16) != NULL : memcmp(frame->session, context.session, 16) != 0)) error = -EACCES;
    if (!error && type != AS_HELLO && !r->hello) error = -EPROTO;
    epoch = lifecycle_epoch;
    spin_unlock_irqrestore(&core_lock, flags);
    if (error) goto done;
    if (type == AS_INSTALL_RUNTIME) {
        spin_lock_irqsave(&core_lock, flags);
        if ((!facts_accepting && source_state != RUNNING) || source_state == FAILED || source_state == QUIESCING) error = -ESHUTDOWN;
        spin_unlock_irqrestore(&core_lock, flags);
        if (!error) {
            /* Inode/VMA preparation can wait for mmap writers. Keep the
             * original write lifetime, but leave urgent Decide/Cancel free. */
            mutex_unlock(&r->write_lock); locked = false;
            runtime_candidate = as_runtime_prepare(body, body_bytes, release_queue);
            if (IS_ERR(runtime_candidate)) { error = PTR_ERR(runtime_candidate); runtime_candidate = NULL; }
            mutex_lock(&r->write_lock); locked = true;
            spin_lock_irqsave(&core_lock, flags);
            if (connection != r || task_tgid(current) != controller || memcmp(frame->session, context.session, 16)) error = -EACCES;
            else if (!error && epoch != lifecycle_epoch) error = -ESTALE;
            else if (!error && !runtime_accepting_locked()) error = -ESHUTDOWN;
            spin_unlock_irqrestore(&core_lock, flags);
        }
        early = true;
    }
    if(type==AS_KILL_PROCESS){
        const struct as_process_target *target=(const void *)body;
        if(body_bytes!=sizeof(*target) || target->reserved)error=-EINVAL;
        spin_lock_irqsave(&core_lock,flags);
        if(!error && memcmp(target->instance,instance,16))error=-ESTALE;
        if(!error && source_state!=RUNNING)error=-ESHUTDOWN;
        if(!error && reply_count>=AS_QUERY_MAX)error=-EAGAIN;
        /* Reserve the existing reply capacity before the irreversible effect. */
        if(!error){reply_count++;reply_reserved=true;}
        spin_unlock_irqrestore(&core_lock,flags);
        if(!error){
            error=as_tools_kill_process(le32_to_cpu(target->pid),le64_to_cpu(target->birth),task_tgid(current));
            put_unaligned_le32(error==-ESRCH?1:0,reply->data+sizeof(struct as_frame)+sizeof(struct as_reply));
            if(error==-ESRCH)error=0;
        }
    }
    if(type==AS_INSPECT_PROCESSES){
        spin_lock_irqsave(&core_lock,flags);error=facts_enabled?0:-ENODATA;spin_unlock_irqrestore(&core_lock,flags);
        if(!error)error=inspect_processes(body,body_bytes,reply->data+sizeof(struct as_frame)+sizeof(struct as_reply));
    }
    if (type == AS_INITIALIZE || type == AS_REPLACE) {
        if (position && id == r->cancelled_id && total == r->cancelled_total && type == r->cancelled_type) goto done;
        if (!position) {
            if (r->transfer || r->installing || r->allocating) { error = -EBUSY; early = true; }
            else if (le32_to_cpu(((const struct as_images *)body)->expected_epoch) != epoch) { error = -ESTALE; early = true; }
            if (early) { r->cancelled_id = id; r->cancelled_total = total; r->cancelled_type = type; goto respond; }
            r->allocating = true; r->transfer_id = id; r->transfer_type = type;
            r->transfer_total = total; r->transfer_bytes = 0; r->transfer_epoch = epoch;
            mutex_unlock(&r->write_lock); locked = false;
            /* One reserved allocation per real connection; urgent writes never wait for reclaim. */
            allocation = kvmalloc(total, GFP_KERNEL);
            mutex_lock(&r->write_lock); locked = true;
            r->allocating = false; wake_up_interruptible(&read_wait);
            if (r->cancelled_id == id && r->cancelled_total == total) goto done;
            if (!allocation) { error = -ENOMEM; early = true; goto respond; }
            spin_lock_irqsave(&core_lock, flags);
            if (connection != r || task_tgid(current) != controller) error = -EACCES;
            else if (lifecycle_epoch != epoch) error = -ESTALE;
            spin_unlock_irqrestore(&core_lock, flags);
            if (error) { early = true; goto respond; }
            r->transfer = allocation; allocation = NULL;
        }
        if (!r->transfer || r->transfer_id != id || r->transfer_type != type || r->transfer_total != total || r->transfer_bytes != position) { error = -EPROTO; goto done; }
        memcpy(r->transfer + position, body, body_bytes); r->transfer_bytes += body_bytes;
        if (r->transfer_bytes != total) goto done;
        complete = r->transfer; complete_bytes = total; epoch = r->transfer_epoch;
        r->transfer = NULL; r->installing = true; atomic_set(&r->cancel_install,0); installation_owner = true; complete_message = true;
        mutex_unlock(&r->write_lock); locked = false;
        candidate = images_parse(complete, complete_bytes, &expected, &r->cancel_install, type == AS_INITIALIZE);
        if (IS_ERR(candidate)) { error = PTR_ERR(candidate); candidate = NULL; }
    }
    if (type == AS_ABORT_TRANSFER) {
        if (body_bytes != 8 || !r->transfer || get_unaligned_le64(body) != r->transfer_id) error = -ENOENT;
        else { aborted = r->transfer; r->transfer = NULL; }
    }
respond:
    spin_lock_irqsave(&core_lock, flags);
    if (connection != r || task_tgid(current) != controller) { error = -EACCES; goto unlock; }
    if (!reply_reserved && reply_count + (cancel_reply ? 1 : 0) >= ((type == AS_STATUS || type == AS_HELLO || (type >= AS_START_FACTS && type != AS_CONTROL_CREDIT)) ? AS_QUERY_MAX : AS_REPLIES_MAX)) { error = -EAGAIN; goto unlock; }
    frame_header((void *)reply->data, AS_CLASS_REPLY, type, id, sizeof(struct as_reply), context.session);
    if (!error && cancel_reply && (r->transfer || r->allocating)) {
        r->cancelled_id = r->transfer_id; r->cancelled_total = r->transfer_total; r->cancelled_type = r->transfer_type;
        aborted = r->transfer; r->transfer = NULL;
        frame_header((void *)cancel_reply->data, AS_CLASS_REPLY, r->transfer_type, r->transfer_id, sizeof(struct as_reply), context.session);
        ((struct as_reply *)(cancel_reply->data + sizeof(struct as_frame)))->error = cpu_to_le32(-ECANCELED);
        list_add_tail(&cancel_reply->link, &replies); reply_count++; cancel_reply = NULL;
    }
    if (!error && complete_message) {
        if (expected != generation || epoch != lifecycle_epoch || le32_to_cpu(((const struct as_images *)complete)->expected_epoch) != lifecycle_epoch) error = -ESTALE;
        else if (type == AS_INITIALIZE && (held || readers || receipt_count)) error = -EBUSY;
        else if (retired_count >= AS_PENDING_MAX) error = -EAGAIN;
        else if (generation == U64_MAX || state_revision == U64_MAX || lifecycle_epoch == U32_MAX) error = -EOVERFLOW;
        else if (type == AS_REPLACE && source_state != RUNNING) error = -ESHUTDOWN;
        else if (type == AS_REPLACE && (candidate->group_ports != group_ports ||
                 candidate->gateway_ws_port != gateway_ws_port)) error = -EINVAL;
        else if (active && active->self && (!candidate->self || !as_self_same_root(active->self,candidate->self))) error = -EXDEV;
        else {
            old = active; active = candidate; candidate = NULL; active->generation = ++generation; active->adopted = true; applied=active;refcount_inc(&applied->refs);
            if (type == AS_INITIALIZE) {
                group_ports = active->group_ports; gateway_ws_port = active->gateway_ws_port;
                group_generation = active->generation;
                overrides = 0; source_state = RUNNING; source_error = 0;
                r->stop_facts=false;facts_accepting=facts_enabled;
                if(!facts_enabled){sequence=0;runtime_sequence=0;get_random_bytes(source,16);if(!memchr_inv(source,0,16))source[0]=1;as_runtime_source_changed(source);}
                lifecycle_epoch++;
            }
            state_revision++; state_notice_locked();
        }
    } else if (!error && type == AS_INSTALL_RUNTIME) {
        if (epoch != lifecycle_epoch || (!facts_accepting && source_state != RUNNING) || source_state == FAILED || source_state == QUIESCING) error = -ESHUTDOWN;
        else if (runtime_candidate) { as_runtime_adopt(runtime_candidate); runtime_candidate = NULL; }
    } else if(!error && (type==AS_START_FACTS || type==AS_FACTS_BARRIER)) {
        error=body_bytes?-EINVAL:facts_begin_locked(r,type,&fact_marker,(void *)(reply->data+sizeof(struct as_frame)+sizeof(struct as_reply)));
    } else if (!error && !early && type != AS_ABORT_TRANSFER && type != AS_INSPECT_PROCESSES && type != AS_KILL_PROCESS) error = small_locked(r, type, body, body_bytes);
    if (!error && (type == AS_QUIESCE || type == AS_FAILURE)) { as_runtime_retire(); runtime_cleanup = true; }
    ((struct as_reply *)(reply->data + sizeof(struct as_frame)))->error = cpu_to_le32(error);
    if (!error) {
        if (type == AS_HELLO || status) status_locked((void *)(reply->data + sizeof(struct as_frame) + sizeof(struct as_reply)));
        if (type == AS_HELLO) capabilities((void *)(reply->data + sizeof(struct as_frame) + sizeof(struct as_reply) + sizeof(struct as_status)));
        ((struct as_frame *)reply->data)->body_bytes = cpu_to_le32(reply_body);
        ((struct as_frame *)reply->data)->total_bytes = cpu_to_le64(reply_body); reply->bytes = sizeof(struct as_frame) + reply_body;
    } else reply->bytes = sizeof(struct as_frame) + sizeof(struct as_reply);
    if(!reply_reserved)reply_count++;
    reply_reserved=false;
    if (!error && type == AS_QUIESCE && !drained_locked(r)) {
        if (r->quiesce_reply) {
            ((struct as_reply *)(reply->data + sizeof(struct as_frame)))->error = cpu_to_le32(-EBUSY);
            reply->bytes = sizeof(struct as_frame) + sizeof(struct as_reply);
            ((struct as_frame *)reply->data)->body_bytes = cpu_to_le32(sizeof(struct as_reply));
            ((struct as_frame *)reply->data)->total_bytes = cpu_to_le64(sizeof(struct as_reply));
        } else { r->quiesce_reply = reply; reply = NULL; }
    }
    if (reply) { list_add_tail(&reply->link, &replies); reply = NULL; }
    error = 0; wake_up_interruptible(&read_wait);
unlock:
    spin_unlock_irqrestore(&core_lock, flags);
done:
    if (locked) mutex_unlock(&r->write_lock);
    as_runtime_discard(runtime_candidate);
    if (runtime_cleanup) flush_workqueue(release_queue);
    if (candidate && candidate->self) as_self_finish(candidate->self);
    if (applied && applied->self) as_self_finish(applied->self);
    if (installation_owner) { mutex_lock(&r->write_lock); r->installing = false; mutex_unlock(&r->write_lock); wake_up_interruptible(&read_wait); }
    snapshot_put(old); snapshot_put(candidate); snapshot_put(applied); kvfree(complete); kvfree(allocation); kvfree(aborted);
    message_free(reply); message_free(cancel_reply); message_free(fact_marker); kfree(input);
    if (write_owner) { spin_lock_irqsave(&core_lock,flags); if(reply_reserved)reply_count--; r->writes--; control_writes--; spin_unlock_irqrestore(&core_lock,flags); wake_up_interruptible(&read_wait); }
    return error ?: bytes;
}
static __poll_t device_poll(struct file *file, poll_table *wait)
{
    struct as_reader *reader = file->private_data; __poll_t events = 0; unsigned long flags;
    poll_wait(file, &read_wait, wait); spin_lock_irqsave(&core_lock, flags);
    if (connection != reader || memcmp(reader->context.session, context.session, 16)) { spin_unlock_irqrestore(&core_lock, flags); return EPOLLERR|EPOLLHUP; }
    quiesced_locked(reader);
    if (!list_empty(&replies)) events |= EPOLLPRI;
    if (!list_empty(&replies) || !list_empty(&ordinary)) events |= EPOLLIN|EPOLLRDNORM;
    if (reply_count < AS_REPLIES_MAX) events |= EPOLLOUT|EPOLLWRNORM;
    spin_unlock_irqrestore(&core_lock, flags); return events;
}
static const struct file_operations operations = {
    .owner = THIS_MODULE, .open = device_open, .release = device_release,
    .read = device_read, .write = device_write, .poll = device_poll,
};
static struct miscdevice device = { .minor = MISC_DYNAMIC_MINOR, .name = "as_wait", .fops = &operations, .mode = 0600 };
/* Lifecycle observation uses the same native event structure and the same
 * authenticated endpoint. Tracepoint callbacks never wait or acquire a mutex.
 */
static void observe(struct task_struct *actor, struct task_struct *child, u32 operation, struct file *image)
{
    struct as_original *r; struct as_message *m; struct as_event *event; struct task_struct *parent;
    unsigned long flags; unsigned bucket; int error; size_t bytes; u8 session[16], epoch[16];
    spin_lock_irqsave(&core_lock, flags);
    if ((!facts_accepting && source_state != RUNNING) || !observations_open_locked()) { spin_unlock_irqrestore(&core_lock, flags); return; }
    if (ordinary_count >= AS_ORDINARY_MAX || observer_count >= AS_OBSERVERS_MAX) { dropped++;facts_lost_locked();spin_unlock_irqrestore(&core_lock, flags);return; }
    bucket=facts_bucket;facts_inflight[bucket]++;observer_count++;
    memcpy(session, context.session, 16); memcpy(epoch, source, 16); spin_unlock_irqrestore(&core_lock, flags);
    r = kzalloc(sizeof(*r) + AS_FRAME_MAX - sizeof(struct as_frame), GFP_ATOMIC); if (!r) goto lost;
    r->capacity = AS_FRAME_MAX - sizeof(struct as_frame); error = capture(r, actor, 1, operation, 1, operation==1, 0, NULL); event = (void *)r->event;
    if (!error && operation == 2) {
        error = append_u32(r, 3, task_tgid_nr(child)); error = error ?: append_u64(r, 4, child->group_leader->start_boottime);
        rcu_read_lock(); parent = rcu_dereference(child->real_parent)->group_leader; get_task_struct(parent);
        error = error ?: append_u32(r, 6, task_tgid_nr(parent)); error = error ?: append_u64(r, 7, parent->group_leader->start_boottime);
        error = error ?: append_u64(r, 8, cgroup_id(task_dfl_cgroup(child))); rcu_read_unlock(); put_task_struct(parent);
        if(!error)child_process_info(r,child);
    } else if (!error && operation == 1) error = append_path(r, 1, image);
    else if(!error && operation==3 && actor==current){
        /* Linux v7 do_exit synchronizes the shared group result before live--
         * and this group_dead tracepoint. Do not use the last TID's exit_code. */
        unsigned long signal_flags;int code=0;bool known=false;
        spin_lock_irqsave(&actor->sighand->siglock,signal_flags);
        if(actor->signal->flags&SIGNAL_GROUP_EXIT){code=actor->signal->group_exit_code;known=true;}
        spin_unlock_irqrestore(&actor->sighand->siglock,signal_flags);
        if(known)error=append_u32(r,5,(u32)code);
    }
    if (r->parent) put_pid(r->parent);
    if (error) { kfree(r); goto lost; }
    bytes = r->length; event->bytes = cpu_to_le32(bytes);
    m = message_alloc(bytes, GFP_ATOMIC); if (!m) { kfree(r); goto lost; }
    frame_header((void *)m->data, AS_CLASS_EVENT, AS_OBSERVATION, 0, bytes, session);
    memcpy(m->data + sizeof(struct as_frame), r->event, bytes); kfree(r);
    spin_lock_irqsave(&core_lock, flags);
    if ((!facts_enabled && source_state != RUNNING) || !controller || memcmp(context.session, session, 16) || memcmp(source, epoch, 16) || ordinary_count >= AS_ORDINARY_MAX) { dropped++;facts_lost_locked();message_free(m); }
    else { ordinary_queue_locked(m); }
    observer_count--;facts_inflight[bucket]--;facts_publish_locked();spin_unlock_irqrestore(&core_lock, flags);wake_up_interruptible(&read_wait);return;
lost:
    spin_lock_irqsave(&core_lock, flags);observer_count--;facts_inflight[bucket]--;dropped++;facts_lost_locked();facts_publish_locked();spin_unlock_irqrestore(&core_lock, flags);wake_up_interruptible(&read_wait);
}
static void created(void *ignored, struct task_struct *parent, struct task_struct *child)
{
    as_runtime_child_created(parent, child);
    if (thread_group_leader(child)) observe(parent, child, 2, NULL);
}
static void executed(void *ignored, struct task_struct *task, pid_t old_pid, struct linux_binprm *bprm)
{
    as_runtime_forget_task(task, true);
    u64 revision=0;struct as_snapshot *snapshot=self_snapshot(&revision);struct as_self_protection *fence=as_self_fence_get();
    struct pid *old=NULL;unsigned long flags;
    if(snapshot)as_self_exec_seen(snapshot->self,task);
    if(fence&&(!snapshot||fence!=snapshot->self))as_self_exec_seen(fence,task);
    if(snapshot&&!as_self_backend_instance(snapshot->self)){
        spin_lock_irqsave(&core_lock,flags);
        if(controller&&task_tgid(task)==controller){
            advance_epoch_locked();fail_locked();old=controller;controller=NULL;connection=NULL;facts_close_locked();
            dropped+=ordinary_count;queue_clear(&replies);queue_clear(&ordinary);
            reply_count=receipt_count=0;
        }
        spin_unlock_irqrestore(&core_lock,flags);put_pid(old);
    }
    as_self_put(fence);self_snapshot_put(snapshot);observe(task,NULL,1,bprm->file);
}
static void exited(void *ignored, struct task_struct *task, bool group_dead)
{
    struct pid *old = NULL; unsigned long flags;
    forget_request_task(task);
    as_runtime_forget_task(task, group_dead);
    as_futex_task_exit(task);
    if (!group_dead) return;
    {u64 revision=0;struct as_snapshot *snapshot=self_snapshot(&revision);struct as_self_protection *fence=as_self_fence_get();if(snapshot)as_self_exit_seen(snapshot->self,task);if(fence&&(!snapshot||fence!=snapshot->self))as_self_exit_seen(fence,task);as_self_put(fence);self_snapshot_put(snapshot);}
    spin_lock_irqsave(&core_lock, flags);
    if (controller && task_tgid(task) == controller) {
        advance_epoch_locked(); fail_locked(); old = controller; controller = NULL; connection = NULL;facts_close_locked();
        dropped += ordinary_count; queue_clear(&replies); queue_clear(&ordinary);
        reply_count = receipt_count = 0;
    }
    spin_unlock_irqrestore(&core_lock, flags);
    if (old) put_pid(old);
    observe(task, NULL, 3, NULL);
}
static void locate_tracepoint(struct tracepoint *tp, void *ignored)
{
    if (!strcmp(tp->name, "sched_process_fork")) tp_fork = tp;
    else if (!strcmp(tp->name, "sched_process_exec")) tp_exec = tp;
    else if (!strcmp(tp->name, "sched_process_exit")) tp_exit = tp;
}
static int __init as_wait_init(void)
{
    int error;
    unsigned int i;
    BUILD_BUG_ON(sizeof(struct as_frame) != 64); BUILD_BUG_ON(sizeof(struct as_status) != 144);
    BUILD_BUG_ON(sizeof(struct as_capabilities) != 40); BUILD_BUG_ON(sizeof(struct as_images) != 136);
    BUILD_BUG_ON(sizeof(struct as_group_tcp) != 72);
    BUILD_BUG_ON(sizeof(struct as_runtime_plan) != 144); BUILD_BUG_ON(sizeof(struct as_runtime_handoff) != 72);
    BUILD_BUG_ON(sizeof(struct as_frame) + sizeof(struct as_file_close) + PATH_MAX > AS_FRAME_MAX);
    BUILD_BUG_ON(sizeof(struct as_configuration) != 48); BUILD_BUG_ON(sizeof(struct as_file_close) != 32); BUILD_BUG_ON(sizeof(struct as_policy) != 32);
    BUILD_BUG_ON(sizeof(struct as_rule) != 40); BUILD_BUG_ON(sizeof(struct as_condition) != 16);
    BUILD_BUG_ON(sizeof(struct as_controlled) != 128); BUILD_BUG_ON(sizeof(struct as_result) != 64);
    BUILD_BUG_ON(sizeof(struct as_review) != 56); BUILD_BUG_ON(sizeof(struct as_hit) != 20); BUILD_BUG_ON(sizeof(struct as_event) != 88);
    check_trace_callback_type_sched_process_fork(created);
    check_trace_callback_type_sched_process_exec(executed);
    check_trace_callback_type_sched_process_exit(exited);
    for_each_kernel_tracepoint(locate_tracepoint, NULL);
    if (!tp_fork || !tp_exec || !tp_exit) return -EOPNOTSUPP;
    release_queue = alloc_workqueue("as-native-release", WQ_UNBOUND|WQ_MEM_RECLAIM, 1);
    if (!release_queue) return -ENOMEM;
    get_random_bytes(instance, 16); if (!memchr_inv(instance,0,16)) instance[0]=1;
    error = tracepoint_probe_register(tp_fork, (void *)created, NULL); if (error) { destroy_workqueue(release_queue); return error; }
    error = tracepoint_probe_register(tp_exec, (void *)executed, NULL); if (error) goto fork;
    error = tracepoint_probe_register(tp_exit, (void *)exited, NULL); if (error) goto exec;
    error = register_fprobe_syms(&create_origins, create_origin_names, ARRAY_SIZE(create_origin_names));
    if (error) goto exit;
    error = register_fprobe(&create_path_end, "end_creating_path", NULL); if (error) goto create;
    error = register_btf_kfunc_id_set(BPF_PROG_TYPE_LSM, &gate_functions); if (error) goto create_end;
    error = misc_register(&device); if (error) goto create_end;
    WRITE_ONCE(control_device,device.this_device->devt);
    for (i = 0; i < ARRAY_SIZE(request_probes); i++) {
        struct as_request_probe *source = &request_probes[i];
        error = register_fprobe_syms(&source->probe, source->names, source->names[1] ? 2 : 1);
        source->registered = !error;
        if (error)
            pr_info("as_wait: request return observation %u unavailable: %d\n", source->operation, error);
    }
    error = as_group_tcp_start();
    if (error)
        pr_info("as_wait: Group TCP observation unavailable: %d\n", error);
    error = as_futex_start();
    if (error)
        pr_info("as_wait: private futex observation unavailable: %d\n", error);
    return 0;
create_end: unregister_fprobe(&create_path_end);
create: unregister_fprobe(&create_origins);
exit: tracepoint_probe_unregister(tp_exit, (void *)exited, NULL);
exec: tracepoint_probe_unregister(tp_exec, (void *)executed, NULL);
fork: tracepoint_probe_unregister(tp_fork, (void *)created, NULL); tracepoint_synchronize_unregister();
    destroy_workqueue(release_queue); return error;
}
static void __exit as_wait_exit(void)
{
    unsigned int i;
    WRITE_ONCE(control_device,0);
    misc_deregister(&device);
    as_runtime_clear(release_queue);
    as_futex_stop();
    as_group_tcp_stop();
    for (i = 0; i < ARRAY_SIZE(request_probes); i++)
        if (request_probes[i].registered)
            unregister_fprobe(&request_probes[i].probe);
    unregister_fprobe(&create_path_end);
    unregister_fprobe(&create_origins);
    tracepoint_probe_unregister(tp_fork, (void *)created, NULL);
    tracepoint_probe_unregister(tp_exec, (void *)executed, NULL);
    tracepoint_probe_unregister(tp_exit, (void *)exited, NULL);
    tracepoint_synchronize_unregister(); snapshot_put(active); destroy_workqueue(release_queue);
}
module_init(as_wait_init); module_exit(as_wait_exit);
MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("AgentShield current original-operation driver");
