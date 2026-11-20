/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef AS_HOOK_FACTS_H
#define AS_HOOK_FACTS_H

/* Same-hook scalar input, borrowed only for one native kfunc invocation.
 * Never put kernel pointers here: the original hook pointers keep their
 * verifier-checked kfunc arguments. This is not the device wire protocol. */
#define AS_HOOK_MOUNT_NAMESPACE 1u
#define AS_HOOK_USER_STACK 2u
#define AS_HOOK_STACK_SLOTS 32u
struct as_hook_facts {
    __u32 available;
    __u32 mount_namespace_inode;
    __u32 rename_flags; /* Original path_rename flags, zero in other hooks. */
    __s32 user_stack_result; /* Actual bpf_get_stack bytes or negative errno. */
    __u64 user_stack[AS_HOOK_STACK_SLOTS];
};

#endif
