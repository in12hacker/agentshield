/* SPDX-License-Identifier: GPL-2.0-only */
#include "vmlinux.h"
#include <bpf/bpf_core_read.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>
#include "as_hook_facts.h"
enum { AS_LINUX_MNT_DETACH = 2 };
extern long as_gate_file_open(struct file *file, const struct as_hook_facts *facts) __ksym;
extern long as_gate_file_mkdir(const struct path *dir, struct dentry *entry, umode_t mode, const struct as_hook_facts *facts) __ksym;
extern long as_gate_file_unlink(const struct path *dir, struct dentry *entry, const struct as_hook_facts *facts) __ksym;
extern long as_gate_file_rmdir(const struct path *dir, struct dentry *entry, const struct as_hook_facts *facts) __ksym;
extern long as_gate_file_rename(const struct path *old_dir, struct dentry *old_entry, const struct path *new_dir, struct dentry *new_entry, const struct as_hook_facts *facts) __ksym;
extern long as_gate_exec(struct linux_binprm *bprm, const struct as_hook_facts *facts) __ksym;
extern long as_gate_connect(struct socket *sock, struct sockaddr *address, int length, const struct as_hook_facts *facts) __ksym;
extern long as_gate_sendmsg(struct socket *sock, struct msghdr *message, int length, const struct as_hook_facts *facts) __ksym;
extern __u32 as_capture_requested(__u32 kind, __u32 operation) __ksym;
extern void as_observe_file_close(struct file *file) __ksym;
SEC("lsm/file_release")
int BPF_PROG(as_file_close, struct file *file)
{
    as_observe_file_close(file);
    return 0;
}
/* Stack storage belongs to this hook only. Native copies selected values into
 * its original event before any wait and never retains this input pointer. */
static __always_inline void capture_facts(void *ctx, __u32 kind, __u32 operation,
                                          struct as_hook_facts *facts)
{
    struct task_struct *task;
    if (!as_capture_requested(kind, operation)) return;
    task = bpf_get_current_task_btf();
    if (!BPF_CORE_READ_INTO(&facts->mount_namespace_inode, task, nsproxy, mnt_ns, ns.inum))
        facts->available |= AS_HOOK_MOUNT_NAMESPACE;
    facts->user_stack_result = bpf_get_stack(ctx, facts->user_stack,
                                            sizeof(facts->user_stack), BPF_F_USER_STACK);
    facts->available |= AS_HOOK_USER_STACK;
}
/* The verifier cannot infer a native kfunc's errno range from its C body. */
static __always_inline int verdict(long result) {
    if (result > 0 || result < -4095) return -5;
    return result;
}
SEC("lsm.s/file_open")
int BPF_PROG(as_file_open, struct file *file, int prior) {
    struct as_hook_facts facts = {};
    if (prior) return verdict(prior);
    capture_facts(ctx, 2, 1, &facts);
    return verdict(as_gate_file_open(file, &facts));
}
SEC("lsm.s/bprm_check_security")
int BPF_PROG(as_exec, struct linux_binprm *bprm, int prior) {
    struct as_hook_facts facts = {};
    if (prior) return verdict(prior);
    capture_facts(ctx, 1, 1, &facts);
    return verdict(as_gate_exec(bprm, &facts));
}
SEC("lsm.s/socket_connect")
int BPF_PROG(as_connect, struct socket *sock, struct sockaddr *address, int length, int prior) {
    struct as_hook_facts facts = {};
    if (prior) return verdict(prior);
    capture_facts(ctx, 3, 1, &facts);
    return verdict(as_gate_connect(sock, address, length, &facts));
}
SEC("lsm.s/socket_sendmsg")
int BPF_PROG(as_sendmsg, struct socket *sock, struct msghdr *message, int length, int prior) {
    struct as_hook_facts facts = {};
    if (prior) return verdict(prior);
    capture_facts(ctx, 3, 2, &facts);
    return verdict(as_gate_sendmsg(sock, message, length, &facts));
}
extern long as_guard_file_permission(struct file *file,int mask) __ksym;
extern long as_guard_file_truncate(struct file *file) __ksym;
extern long as_guard_path(const struct path *path,__u32 operation) __ksym;
extern long as_guard_entry(const struct path *dir,struct dentry *entry,__u32 operation) __ksym;
extern long as_guard_create(const struct path *dir, struct dentry *entry, umode_t mode) __ksym;
extern long as_gate_inode_create(struct inode *dir, struct dentry *entry, umode_t mode, const struct as_hook_facts *facts) __ksym;
extern long as_guard_link(struct dentry *from,const struct path *to_dir,struct dentry *to) __ksym;
extern long as_guard_metadata(struct dentry *dentry) __ksym;
extern long as_gate_inode_setattr(struct dentry *dentry, struct iattr *attr, const struct as_hook_facts *facts) __ksym;
extern long as_gate_truncate_path(const struct path *path, const struct as_hook_facts *facts) __ksym;
extern long as_gate_mmap(struct file *file, unsigned long reqprot, unsigned long prot,
                        unsigned long flags, const struct as_hook_facts *facts) __ksym;
extern long as_guard_mprotect(struct vm_area_struct *vma,unsigned long prot) __ksym;
extern long as_guard_move_mount(const struct path *from,const struct path *to) __ksym;
extern long as_guard_mount(const struct path *path,unsigned long flags,int no_outward_propagation) __ksym;
extern long as_guard_umount(struct vfsmount *mnt,int isolated_detach) __ksym;
SEC("lsm/file_permission")
int BPF_PROG(as_self_permission,struct file *file,int mask,int prior){return verdict(prior?prior:as_guard_file_permission(file,mask));}
SEC("lsm/file_truncate")
int BPF_PROG(as_self_file_truncate,struct file *file,int prior){return verdict(prior?prior:as_guard_file_truncate(file));}
SEC("lsm.s/path_truncate")
int BPF_PROG(as_truncate_path,const struct path *path,int prior){
    struct as_hook_facts facts = {};
    if (prior) return verdict(prior);
    capture_facts(ctx, 2, 18, &facts);
    return verdict(as_gate_truncate_path(path, &facts));
}
SEC("lsm/path_chmod")
int BPF_PROG(as_self_chmod,const struct path *path,umode_t mode,int prior){return verdict(prior?prior:as_guard_path(path,11));}
SEC("lsm/path_chown")
int BPF_PROG2(as_self_chown,const struct path *,path,kuid_t,uid,kgid_t,gid,int,prior){return verdict(prior?prior:as_guard_path(path,12));}
SEC("lsm.s/path_unlink")
int BPF_PROG(as_file_unlink,const struct path *dir,struct dentry *entry,int prior){
    struct as_hook_facts facts = {};
    if (prior) return verdict(prior);
    capture_facts(ctx, 2, 6, &facts);
    return verdict(as_gate_file_unlink(dir, entry, &facts));
}
SEC("lsm.s/path_rmdir")
int BPF_PROG(as_file_rmdir,const struct path *dir,struct dentry *entry,int prior){
    struct as_hook_facts facts = {};
    if (prior) return verdict(prior);
    capture_facts(ctx, 2, 7, &facts);
    return verdict(as_gate_file_rmdir(dir, entry, &facts));
}
SEC("lsm.s/path_mkdir")
int BPF_PROG(as_file_mkdir,const struct path *dir,struct dentry *entry,umode_t mode,int prior){
    struct as_hook_facts facts = {};
    if (prior) return verdict(prior);
    capture_facts(ctx, 2, 5, &facts);
    return verdict(as_gate_file_mkdir(dir, entry, mode, &facts));
}
SEC("lsm/path_mknod")
int BPF_PROG(as_self_create,const struct path *dir,struct dentry *entry,umode_t mode,unsigned int dev,int prior){return verdict(prior?prior:as_guard_create(dir,entry,mode));}
SEC("lsm.s/inode_create")
int BPF_PROG(as_file_create, struct inode *dir, struct dentry *entry, umode_t mode, int prior) {
    struct as_hook_facts facts = {};
    if (prior) return verdict(prior);
    capture_facts(ctx, 2, 4, &facts);
    return verdict(as_gate_inode_create(dir, entry, mode, &facts));
}
SEC("lsm/path_symlink")
int BPF_PROG(as_self_symlink,const struct path *dir,struct dentry *entry,const char *old_name,int prior){return verdict(prior?prior:as_guard_entry(dir,entry,4));}
SEC("lsm/path_link")
int BPF_PROG(as_self_link,struct dentry *old_entry,const struct path *dir,struct dentry *entry,int prior){return verdict(prior?prior:as_guard_link(old_entry,dir,entry));}
SEC("lsm.s/path_rename")
int BPF_PROG(as_file_rename,const struct path *old_dir,struct dentry *old_entry,const struct path *new_dir,struct dentry *new_entry,unsigned int flags,int prior){
    struct as_hook_facts facts = { .rename_flags = flags };
    if (prior) return verdict(prior);
    capture_facts(ctx, 2, 9, &facts);
    return verdict(as_gate_file_rename(old_dir, old_entry, new_dir, new_entry, &facts));
}
SEC("lsm.s/inode_setattr")
int BPF_PROG(as_inode_setattr,struct mnt_idmap *idmap,struct dentry *entry,struct iattr *attr,int prior){
    struct as_hook_facts facts = {};
    if (prior) return verdict(prior);
    capture_facts(ctx, 2, 10, &facts);
    return verdict(as_gate_inode_setattr(entry, attr, &facts));
}
SEC("lsm/inode_setxattr")
int BPF_PROG(as_self_setxattr,struct mnt_idmap *idmap,struct dentry *entry,const char *name,const void *value,size_t size,int flags,int prior){return verdict(prior?prior:as_guard_metadata(entry));}
SEC("lsm/inode_removexattr")
int BPF_PROG(as_self_removexattr,struct mnt_idmap *idmap,struct dentry *entry,const char *name,int prior){return verdict(prior?prior:as_guard_metadata(entry));}
SEC("lsm/inode_set_acl")
int BPF_PROG(as_self_set_acl,struct mnt_idmap *idmap,struct dentry *entry,const char *name,struct posix_acl *acl,int prior){return verdict(prior?prior:as_guard_metadata(entry));}
SEC("lsm/inode_remove_acl")
int BPF_PROG(as_self_remove_acl,struct mnt_idmap *idmap,struct dentry *entry,const char *name,int prior){return verdict(prior?prior:as_guard_metadata(entry));}
SEC("lsm/inode_file_setattr")
int BPF_PROG(as_self_file_setattr,struct dentry *entry,struct file_kattr *attr,int prior){return verdict(prior?prior:as_guard_metadata(entry));}
SEC("lsm.s/mmap_file")
int BPF_PROG(as_mmap_shared_write,struct file *file,unsigned long reqprot,unsigned long prot,unsigned long flags,int prior){
    /* Anonymous mappings have no file object to protect. Prove the actual
     * non-NULL hook argument before passing it to a trusted-pointer kfunc. */
    if(prior||!file)return verdict(prior);
    struct as_hook_facts facts = {};
    capture_facts(ctx, 2, 19, &facts);
    return verdict(as_gate_mmap(file, reqprot, prot, flags, &facts));
}
SEC("lsm/file_mprotect")
int BPF_PROG(as_self_mprotect,struct vm_area_struct *vma,unsigned long reqprot,unsigned long prot,int prior){return verdict(prior?prior:as_guard_mprotect(vma,prot));}
static __always_inline int no_outward_propagation(struct vfsmount *mnt)
{
    struct mount *target;
    struct hlist_node *slaves;
    int group;
    if(!mnt)return 0;
    target=(void *)mnt-bpf_core_field_offset(struct mount,mnt);
    if(BPF_CORE_READ_INTO(&group,target,mnt_group_id)||group||
        BPF_CORE_READ_INTO(&slaves,target,mnt_slave_list.first)||slaves)return 0;
    return 1;
}
SEC("lsm/sb_mount")
int BPF_PROG(as_self_mount,const char *dev,const struct path *path,const char *type,unsigned long flags,void *data,int prior)
{
    struct vfsmount *mnt=NULL;
    int isolated=0;
    if(prior)return verdict(prior);
    if(!BPF_CORE_READ_INTO(&mnt,path,mnt))isolated=no_outward_propagation(mnt);
    return verdict(as_guard_mount(path,flags,isolated));
}
SEC("lsm/move_mount")
int BPF_PROG(as_self_move_mount,const struct path *from,const struct path *to,int prior){return verdict(prior?prior:as_guard_move_mount(from,to));}
static __always_inline int isolated_detach(struct vfsmount *mnt,int flags)
{
    /* MNT_DETACH only. The kernel propagates umount through the parent mount. */
    if (flags != AS_LINUX_MNT_DETACH) return 0;
    struct mount *target = (void *)mnt - bpf_core_field_offset(struct mount, mnt);
    struct mount *parent;
    struct vfsmount *root;
    struct task_struct *task = bpf_get_current_task_btf();
    if (BPF_CORE_READ_INTO(&parent, target, mnt_parent) || !parent || parent == target ||
        BPF_CORE_READ_INTO(&root, task, fs, root.mnt) ||
        root != (void *)parent + bpf_core_field_offset(struct mount, mnt)) return 0;
    return no_outward_propagation((void *)parent+bpf_core_field_offset(struct mount,mnt));
}
SEC("lsm/sb_umount")
int BPF_PROG(as_self_umount,struct vfsmount *mnt,int flags,int prior){return verdict(prior?prior:as_guard_umount(mnt,isolated_detach(mnt,flags)));}
char LICENSE[] SEC("license") = "GPL";
