/* SPDX-License-Identifier: GPL-2.0-only WITH Linux-syscall-note */
#ifndef AS_PROTOCOL_H
#define AS_PROTOCOL_H
#include <linux/types.h>
#define AS_MAGIC 0x32435341u
#define AS_FORMAT 2
#define AS_FRAME_MAX 16384u
#define AS_TRANSFER_MAX (4u * 1024u * 1024u)
#define AS_PENDING_MAX 64u
#define AS_HOLD_MAX_MS 300000u
#define AS_BPF_PIN_ROOT "/sys/fs/bpf/agentshield"
#define AS_CLASS_REQUEST 1
#define AS_CLASS_REPLY 2
#define AS_CLASS_EVENT 3
#define AS_HELLO 1
#define AS_INITIALIZE 2
#define AS_REPLACE 3
#define AS_STATUS 4
#define AS_QUIESCE 5
#define AS_FAILURE 6
#define AS_BIND_REVIEW 7
#define AS_DECIDE 8
#define AS_CANCEL 9
#define AS_SET_FEATURE 10
#define AS_RESUME_FEATURE 11
#define AS_ABORT_TRANSFER 12
#define AS_START_FACTS 13
#define AS_INSPECT_PROCESSES 14
#define AS_FACTS_BARRIER 15
#define AS_KILL_PROCESS 16
#define AS_INSTALL_RUNTIME 17
#define AS_CONTROL_CREDIT 18
#define AS_CONTROLLED 0x100
#define AS_RESULT 0x101
#define AS_DIRECT 0x102
#define AS_STATE_CHANGED 0x103
#define AS_OBSERVATION 0x104
#define AS_PERSISTENT_RETIRED 0x105
#define AS_FACTS_MARKER 0x106
#define AS_HIT_BATCH 0x107
#define AS_REQUEST_RETURN 0x108
#define AS_FUTEX_SELECTION 0x109
#define AS_GROUP_TCP 0x10a
#define AS_FILE_CLOSE 0x10b
#define AS_RUNTIME_HANDOFF 0x10c
#define AS_ALLOW 0
#define AS_DENY 1
#define AS_TO_USER 2
#define AS_FEAT_SELF_PROTECTION 1u
#define AS_FEAT_CONTAINERS 2u
#define AS_FEAT_KILL_ACTOR 4u
#define AS_FEAT_KILL_PARENT 8u
#define AS_FEAT_PIPE_CONTROL 16u
#define AS_FEAT_PROCESS_FACTS 32u
#define AS_FEAT_PROCESS_KILL 64u
#define AS_FEAT_CONTROL_PRIORITY 512u
#define AS_FEAT_SELF_MOUNT_RULES 2048u
#define AS_FEAT_DNS_QUERY_SNAPSHOT 1024u
#define AS_FEAT_FILE_CLOSE_AUDIT 256u
#define AS_INSPECT_MAX 64u
#define AS_SYSCALL_ARGUMENTS 6u
#define AS_KERNEL_STACK_LIMIT 32u
#define AS_COMMON_SYSCALL 27u
#define AS_COMMON_KERNEL_STACK 28u
#define AS_COMMON_NAMESPACE_TIDS 29u
#define AS_COMMON_USER_STACK 30u
/* Original register values, not dereferenceable pointers or inferred handles. */
struct as_syscall_snapshot {
    __le32 audit_arch, number;
    __le64 instruction_pointer, stack_pointer;
    __le64 arguments[AS_SYSCALL_ARGUMENTS];
} __attribute__((packed));
struct as_stack_snapshot {
    __le32 flags, count;
    __le64 instruction_pointers[AS_KERNEL_STACK_LIMIT];
} __attribute__((packed));
struct as_user_stack_snapshot {
    __le32 result, flags;
    __le64 instruction_pointers[AS_KERNEL_STACK_LIMIT];
} __attribute__((packed));
_Static_assert(sizeof(struct as_syscall_snapshot) == 72, "syscall snapshot size");
#define AS_OP_EXEC (1u << 0)
#define AS_OP_OPEN (1u << 1)
#define AS_OP_CONNECT (1u << 2)
#define AS_OP_SEND (1u << 3)
#define AS_OP_CREATED (1u << 4)
#define AS_OP_EXECUTED (1u << 5)
#define AS_OP_EXITED (1u << 6)
#define AS_OP_FILE_MKDIR (1u << 7)
#define AS_OP_FILE_UNLINK (1u << 8)
#define AS_OP_FILE_RMDIR (1u << 9)
#define AS_OP_FILE_RENAME (1u << 10)
#define AS_OP_FILE_TRUNCATE_PATH (1u << 11)
#define AS_OP_FILE_MAP_SHARED_WRITABLE (1u << 12)
#define AS_OP_FILE_TRUNCATE_OPEN (1u << 13)
#define AS_OP_FILE_CREATE (1u << 14)
#define AS_FILE_OPEN 1u
#define AS_FILE_CREATE 4u
#define AS_FILE_MKDIR 5u
#define AS_FILE_UNLINK 6u
#define AS_FILE_RMDIR 7u
#define AS_FILE_LINK 8u
#define AS_FILE_RENAME 9u
#define AS_FILE_TRUNCATE 10u
#define AS_FILE_CHMOD 11u
#define AS_FILE_CHOWN 12u
#define AS_FILE_WRITABLE_MAPPING 13u
#define AS_FILE_MOUNT 14u
#define AS_FILE_UNMOUNT 15u
#define AS_FILE_MOVE_MOUNT 16u
#define AS_FILE_METADATA 17u
#define AS_FILE_TRUNCATE_PATH 18u
#define AS_FILE_MAP_SHARED_WRITABLE 19u
#define AS_FILE_OPERATION_BIT(operation) (1u << ((operation) - 1u))
#define AS_FILE_CONTROLLED_OPERATIONS (AS_FILE_OPERATION_BIT(AS_FILE_OPEN) | AS_FILE_OPERATION_BIT(AS_FILE_CREATE) | AS_FILE_OPERATION_BIT(AS_FILE_MKDIR) | AS_FILE_OPERATION_BIT(AS_FILE_UNLINK) | AS_FILE_OPERATION_BIT(AS_FILE_RMDIR) | AS_FILE_OPERATION_BIT(AS_FILE_RENAME) | AS_FILE_OPERATION_BIT(AS_FILE_TRUNCATE_PATH) | AS_FILE_OPERATION_BIT(AS_FILE_MAP_SHARED_WRITABLE) | AS_FILE_OPERATION_BIT(AS_FILE_TRUNCATE))
#define AS_FILE_TLV_RENAME_FLAGS 7u
#define AS_FILE_TLV_CREATION_MODE 8u
#define AS_FILE_TLV_MMAP_PARAMETERS 9u
#define AS_FILE_TLV_TRUNCATE_PARAMETERS 10u
#define AS_FILE_TLV_MOUNT_EFFECT 11u
#define AS_MOUNT_INDEPENDENT_VIEW 1u
#define AS_MOUNT_PROTECTED_TREE 2u
#define AS_MOUNT_UNKNOWN 3u
#define AS_FILE_DRIVER_OPERATIONS (AS_FILE_CONTROLLED_OPERATIONS | AS_FILE_OPERATION_BIT(AS_FILE_MOUNT))
#define AS_FIELD_BIT(field) (1ULL << ((field) - 1))
/* All integers are explicit little endian. No pointer, long, FD or Rust layout. */
struct as_frame {
    __le32 magic;
    __le16 format, class;
    __le16 type, reserved16;
    __le32 body_bytes;
    __u8 session[16];
    __le64 request_id, total_bytes, offset, reserved64;
} __attribute__((packed));
struct as_reply { __le32 error, reserved; };
struct as_truncate_parameters { __le64 size; __le32 attributes; } __attribute__((packed));
struct as_status {
    __u8 instance[16];
    __le32 state, flags;
    __le64 generation, revision;
    __u8 config[32], rules[32];
    __le32 defaults, overrides, effective, self_protection;
    __le32 pending, receipts;
    __le64 dropped;
    __le32 error, lifecycle_epoch;
} __attribute__((packed));
struct as_capabilities {
    __le32 max_frame, max_pending;
    __le64 max_transfer;
    __le32 max_hold_ms, operations;
    __le64 conditions;
    __le32 features, reserved;
} __attribute__((packed));
struct as_images {
    __le64 expected_generation;
    __le16 platform, reserved16;
    __le32 config_encoding, rules_encoding, expected_epoch;
    __le64 config_bytes, rules_bytes;
    __u8 config[32], rules[32], sha256[32];
} __attribute__((packed));
/* Only the current image layouts are accepted; no previous-layout decoder. */
struct as_configuration {
    __le32 defaults, max_pending, max_hold_ms, required_features, root_bytes;
    __le32 backend_bytes, loader_bytes, self_protection_log, group_ports, gateway_ws_port;
    __le32 close_directories, close_directory_bytes;
} __attribute__((packed));
/* Path is resolved only when the AS device reader consumes the held object.
 * Zero path_error means success, including a successful empty byte string. */
struct as_file_close {
    __le32 reporter_tid, device_major, device_minor, path_error;
    __le64 inode;
    __le32 path_bytes, reserved;
} __attribute__((packed));
struct as_policy {
    __le32 header_bytes, rules;
    __le64 bytes;
    /* Bit (operation - 1): unfiltered backend candidates in each rule family.
     * This is routing metadata, never a rule hit or an inferred verdict. */
    __le32 backend_operations[3], backend_routes;
} __attribute__((packed));
struct as_rule {
    __le32 bytes;
    __le16 kind, action;
    __le32 operations, terminate, group_number, rule_number, log, conditions;
    __le16 group_bytes, id_bytes;
    __le32 report_denial;
} __attribute__((packed));
struct as_condition {
    __le16 field;
    __u8 operation, reserved8;
    __le32 bytes, index, reserved32;
} __attribute__((packed));
/* One typed necessary target condition, never a rule action. Its value follows. */
struct as_backend_route {
    __le16 kind, reserved;
    __le32 operations;
    struct as_condition condition;
} __attribute__((packed));
/* Network TLV9: one hook-read iterator prefix; bytes follow this header. */
struct as_dns_send_snapshot {
    __le64 requested_bytes;
    __le32 attempted_bytes, send_flags;
} __attribute__((packed));
/* Condition fields: actor PID,PPID,UID,GID,EUID,EGID,name,exe,argv,cwd,
 * parent name,exe,target exe,argv,file path,new path,device,inode,mask,
 * local IP,remote IP,local port,remote port,protocol,domain = 1..25.
 * Operators: bytes exact=1,prefix=2,u32 equality=3,u64 equality=4,
 * u32 bit subset=5,device pair equality=6,port inclusive range=7,
 * IP prefix=8 (one prefix-length byte followed by 4 or 16 address bytes).
 * argv's index lives in the condition header; other operators require index=0.
 */
struct as_controlled {
    __le64 id, started_ns, deadline_ns, generation, revision;
    __u8 config[32], rules[32];
    __le32 event_bytes, hit_bytes;
    __u8 reserved[16];
} __attribute__((packed));
struct as_result {
    __le64 id, generation;
    __le32 result, flags, actor_signal, parent_signal;
    __u8 rules[32];
} __attribute__((packed));
#define AS_REQUEST_OPEN 1u
#define AS_REQUEST_EXECUTE 2u
#define AS_REQUEST_CONNECT 3u
#define AS_REQUEST_SEND 4u
/* One native syscall invocation containing an admitted matching gate wait. */
struct as_request_return {
    __u8 source[16];
    __le64 domain, sequence, wait_id, returned_boottime_ns;
    __le32 value, operation;
} __attribute__((packed));
struct as_sync_thread {
    __le32 tgid, tid;
    __le64 birth_boottime_ns;
} __attribute__((packed));
struct as_runtime_key {
    __u8 source[16];
    __le64 domain, sequence;
} __attribute__((packed));
struct as_runtime_plan {
    __le32 tgid;
    __le16 elf_machine, probe_present;
    __le64 birth, exec_cookie, inode;
    __le32 dev_major, dev_minor;
    __le64 probes[12];
    __le32 field_count, reserved;
} __attribute__((packed));
struct as_runtime_handoff {
    struct as_runtime_key observation;
    struct as_sync_thread actor;
    __le64 observed_boottime_ns;
    __le32 presence, phase, operation, reserved;
} __attribute__((packed));
/* Passive Group TCP wire bytes or one actual I/O interval. */
struct as_group_tcp {
    __u8 source[16];
    __le64 ordinal, generation;
    __le32 sequence, acknowledgment, length, tgid;
    __le64 birth;
    __le16 sport, dport;
    __u8 kind, flags;
    __le16 reserved;
    __le64 cookie;
} __attribute__((packed));
/* Selection in a private wake. A zero wait_id has no paired wait observation. */
struct as_futex_selection {
    __u8 source[16];
    __le64 sequence, selected_boottime_ns;
    struct as_sync_thread waker, waiter;
    __le64 wait_id, wait_boottime_ns;
} __attribute__((packed));
struct as_review {
    __le64 id;
    __u8 rules[32];
    __le64 deadline_ns;
    __le32 decision, terminate;
} __attribute__((packed));
struct as_decision { __le64 id; __le32 decision, terminate; };
/* One observation generation and a FIFO completion marker. The reply's loss
 * sample may precede the final marker's sample; epoch/marker identify it. */
struct as_facts_marker { __le64 epoch, marker, loss_epoch, pid_namespace, cutoff_boottime_ns, hit_loss; };
struct as_process_binding {
    __le32 pid, status;
    __le64 birth;
    __le32 parent_pid, kind;
    __le64 parent_birth, exec_cookie, cgroup, begin_ns, end_ns;
} __attribute__((packed));
struct as_process_target {
    __u8 instance[16];
    __le32 pid, reserved;
    __le64 birth;
} __attribute__((packed));
/* A hit uses rule metadata from the actual driver snapshot, never an invented ID. */
struct as_hit {
    __le32 group_number, rule_number, log, source;
    __le16 group_bytes, id_bytes;
} __attribute__((packed));
struct as_hit_count {
    __le64 first, last, birth, count;
    __le32 tgid;
    __le16 kind, hit_bytes;
    __u8 version[32];
} __attribute__((packed));
struct as_event {
    __le16 format, header_bytes;
    __le32 bytes;
    __le16 kind, stage;
    __le32 operation;
    __u8 source[16];
    __le64 domain, sequence, boottime;
    __le32 tgid, tid;
    __le64 birth, cgroup;
    __le32 available, common_bytes;
} __attribute__((packed));
_Static_assert(sizeof(struct as_frame)==64,"frame size");
_Static_assert(sizeof(struct as_reply)==8,"reply size");
_Static_assert(sizeof(struct as_status)==144,"status size");
_Static_assert(sizeof(struct as_capabilities)==40,"capability size");
_Static_assert(sizeof(struct as_images)==136,"persistent descriptor size");
_Static_assert(sizeof(struct as_configuration)==48,"configuration size");
_Static_assert(sizeof(struct as_file_close)==32,"file close size");
_Static_assert(sizeof(struct as_policy)==32,"policy size");
_Static_assert(sizeof(struct as_rule)==40,"rule size");
_Static_assert(__builtin_offsetof(struct as_rule,report_denial)==36,"denial feedback offset");
_Static_assert(sizeof(struct as_condition)==16,"condition size");
_Static_assert(sizeof(struct as_backend_route)==24,"backend route size");
_Static_assert(sizeof(struct as_dns_send_snapshot)==16,"DNS send snapshot header size");
_Static_assert(sizeof(struct as_controlled)==128,"controlled prefix size");
_Static_assert(sizeof(struct as_result)==64,"result size");
_Static_assert(sizeof(struct as_review)==56,"review size");
_Static_assert(sizeof(struct as_decision)==16,"decision size");
_Static_assert(sizeof(struct as_facts_marker)==48,"facts marker size");
_Static_assert(sizeof(struct as_hit_count)==72,"hit count size");
_Static_assert(sizeof(struct as_process_binding)==64,"process binding size");
_Static_assert(sizeof(struct as_process_target)==32,"process target size");
_Static_assert(sizeof(struct as_hit)==20,"hit size");
_Static_assert(sizeof(struct as_event)==88,"event size");
#endif
