/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef AS_SELF_PROTECTION_H
#define AS_SELF_PROTECTION_H
#include <linux/types.h>
#include <linux/path.h>
#include <linux/workqueue.h>
#include <linux/atomic.h>
struct file; struct pid; struct task_struct; struct dentry; struct vm_area_struct;
struct as_self_protection;
/* Preparation owns a temporary initialization fence until finish(). It does not
 * publish ordinary rules or claim a completed persistent-state adoption. */
struct as_self_protection *as_self_prepare(const u8 *root, u32 root_bytes,
        const u8 *backend, u32 backend_bytes, const u8 *loader, u32 loader_bytes,
        struct pid *controller, struct as_self_protection *previous, struct workqueue_struct *release_queue,
        const atomic_t *cancelled);
void as_self_get(struct as_self_protection *self);
void as_self_put(struct as_self_protection *self);
void as_self_finish(struct as_self_protection *self);
struct as_self_protection *as_self_fence_get(void);
bool as_self_backend_instance(struct as_self_protection *self);
bool as_self_same_root(const struct as_self_protection *a,const struct as_self_protection *b);
/* Read-only object check for configuration-owned observation scopes. */
bool as_self_protected_path(struct as_self_protection *self,const struct path *path);
/* 0 means no protected modification, 1 verifies protected backend modification
 * authority, negative is a real rejection. Ordinary rules retain their decision. */
int as_self_file(struct as_self_protection *self,struct file *file,u32 operation,bool controller);
int as_self_path(struct as_self_protection *self,const struct path *path,u32 operation,bool controller);
int as_self_dentry(struct as_self_protection *self,struct dentry *target,u32 operation,bool controller);
int as_self_pair(struct as_self_protection *self,const struct path *from,const struct path *to,u32 operation,bool controller);
int as_self_link(struct as_self_protection *self,struct dentry *from,const struct path *to,bool controller);
int as_self_umount(struct as_self_protection *self,struct vfsmount *mnt,bool isolated_detach);
u32 as_self_mount_effect(struct as_self_protection *self,const struct path *path,unsigned long flags,bool no_outward_propagation);
int as_self_mount(struct as_self_protection *self,const struct path *path,u32 effect,bool controller);
/* Identity preparation only: 0 is a completed preparation step, never an Allow.
 * Negative is a real preparation failure. A role becomes valid only on exec_seen. */
int as_self_prepare_exec(struct as_self_protection *self,struct file *file);
void as_self_exec_seen(struct as_self_protection *self,struct task_struct *task);
void as_self_exit_seen(struct as_self_protection *self,struct task_struct *task);
#endif
