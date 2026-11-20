// SPDX-License-Identifier: GPL-2.0-only
/* File self-protection owns native objects, not a second rule source.
 * VFS path ancestry/inode identity follows Landlock/AppArmor-style object
 * mediation. A temporary initialization fence stabilizes the protected tree
 * while its existing inode objects (including external hardlinks) are pinned.
 */
#include <linux/module.h>
#include <linux/cred.h>
#include <linux/dcache.h>
#include <linux/elf.h>
#include <linux/file.h>
#include <linux/fs.h>
#include <linux/hashtable.h>
#include <linux/mount.h>
#include <linux/magic.h>
#include <linux/namei.h>
#include <linux/refcount.h>
#include <linux/sched/signal.h>
#include <linux/slab.h>
#include <linux/workqueue.h>
#include <uapi/linux/mount.h>
#include "as_self_protection.h"
#include "as_protocol.h"
#define AS_SELF_OBJECTS_MAX 65536u
#define AS_SELF_INSTANCES_MAX 128u
struct protected_inode { struct hlist_node node; struct inode *inode; };
struct exec_instance {
    struct list_head link; struct pid *tgid; u64 birth, exec_id; bool pending; u8 role;
};
struct as_self_protection {
    refcount_t refs; struct work_struct release; struct workqueue_struct *release_queue;
    struct path root; struct file *backend, *loader;
    spinlock_t lock; DECLARE_HASHTABLE(objects,10); u32 object_count, instance_count;
    struct list_head instances;
};
static DEFINE_SPINLOCK(fence_lock);
static struct as_self_protection *fence;
static struct file *task_exe(struct task_struct *task)
{
    struct mm_struct *mm; struct file *file = NULL;
    rcu_read_lock(); task_lock(task); mm = task->mm;
    if (mm) file = get_file_rcu(&mm->exe_file);
    task_unlock(task); rcu_read_unlock(); return file;
}
static bool same_file(const struct file *a,const struct file *b)
{ return a && b && path_equal(&a->f_path,&b->f_path); }
static bool static_elf(struct file *file)
{
    struct elfhdr header; struct elf_phdr ph; loff_t offset = 0; u32 i;
    if (kernel_read(file,&header,sizeof(header),&offset)!=sizeof(header) || memcmp(header.e_ident,ELFMAG,SELFMAG) ||
        header.e_ident[EI_CLASS]!=ELF_CLASS || header.e_phentsize!=sizeof(ph) || !header.e_phnum || header.e_phnum>1024) return false;
    for(i=0;i<header.e_phnum;i++) {
        offset=(loff_t)header.e_phoff+(loff_t)i*sizeof(ph);
        if(offset<0 || kernel_read(file,&ph,sizeof(ph),&offset)!=sizeof(ph) || ph.p_type==PT_INTERP) return false;
    }
    return true;
}
static bool known_inode(struct as_self_protection *self,struct inode *inode)
{
    struct protected_inode *item; unsigned long flags; bool found=false;
    if(!inode)return false;
    spin_lock_irqsave(&self->lock,flags);
    hash_for_each_possible(self->objects,item,node,(unsigned long)inode) if(item->inode==inode){found=true;break;}
    spin_unlock_irqrestore(&self->lock,flags);return found;
}
static int remember_inode(struct as_self_protection *self,struct inode *inode)
{
    struct protected_inode *item,*existing;struct inode *held;unsigned long flags;
    if(!inode || known_inode(self,inode))return 0;
    /* Overlay's external upper/lower aliases are different inode objects. */
    if(inode->i_sb->s_magic==OVERLAYFS_SUPER_MAGIC)return -EOPNOTSUPP;
    item=kmalloc(sizeof(*item),GFP_NOWAIT);if(!item)return -ENOMEM;
    held=igrab(inode);if(!held){kfree(item);return -ESTALE;}item->inode=held;
    spin_lock_irqsave(&self->lock,flags);
    hash_for_each_possible(self->objects,existing,node,(unsigned long)inode) if(existing->inode==inode){spin_unlock_irqrestore(&self->lock,flags);iput(held);kfree(item);return 0;}
    if(self->object_count>=AS_SELF_OBJECTS_MAX){spin_unlock_irqrestore(&self->lock,flags);iput(held);kfree(item);return -E2BIG;}
    hash_add(self->objects,&item->node,(unsigned long)inode);self->object_count++;spin_unlock_irqrestore(&self->lock,flags);return 0;
}
static bool inside(struct as_self_protection *self,const struct path *path)
{ return path && (path_is_under(path,&self->root) || is_subdir(path->dentry,self->root.dentry)); }
static bool protected(struct as_self_protection *self,const struct path *path)
{ return inside(self,path) || (path&&known_inode(self,d_inode(path->dentry))); }
bool as_self_protected_path(struct as_self_protection *self,const struct path *path)
{ return self && protected(self,path); }
static bool ancestor(struct as_self_protection *self,const struct path *path)
{ return path&&(path_is_under(&self->root,path)||is_subdir(self->root.dentry,path->dentry)); }
static void release_self(struct work_struct *work)
{
    struct as_self_protection *self=container_of(work,struct as_self_protection,release);
    struct protected_inode *item;struct hlist_node *tmp;struct exec_instance *instance,*next;unsigned b;
    hash_for_each_safe(self->objects,b,tmp,item,node){hash_del(&item->node);iput(item->inode);kfree(item);}
    list_for_each_entry_safe(instance,next,&self->instances,link){list_del(&instance->link);put_pid(instance->tgid);kfree(instance);}
    if(self->backend)fput(self->backend);
    if(self->loader)fput(self->loader);
    if(self->root.dentry)path_put(&self->root);
    kfree(self);
}
void as_self_get(struct as_self_protection *self){if(self)refcount_inc(&self->refs);}
void as_self_put(struct as_self_protection *self){if(self&&refcount_dec_and_test(&self->refs))queue_work(self->release_queue,&self->release);}
struct as_self_protection *as_self_fence_get(void)
{struct as_self_protection *self;unsigned long flags;spin_lock_irqsave(&fence_lock,flags);self=fence;as_self_get(self);spin_unlock_irqrestore(&fence_lock,flags);return self;}
void as_self_finish(struct as_self_protection *self)
{bool release=false;unsigned long flags;spin_lock_irqsave(&fence_lock,flags);if(fence==self){fence=NULL;release=true;}spin_unlock_irqrestore(&fence_lock,flags);if(release)as_self_put(self);}
bool as_self_same_root(const struct as_self_protection *a,const struct as_self_protection *b){return a&&b&&path_equal(&a->root,&b->root);}
static u8 verified_role(struct as_self_protection *self,struct task_struct *task)
{
    struct exec_instance *instance;struct file *file;u8 role=0;unsigned long flags;
    if(task->flags&(PF_KTHREAD|PF_IO_WORKER))return 0;
    file=task_exe(task);if(!file)return 0;
    spin_lock_irqsave(&self->lock,flags);
    list_for_each_entry(instance,&self->instances,link) if(instance->tgid==task_tgid(task)&&!instance->pending&&
        instance->birth==task->group_leader->start_boottime&&instance->exec_id==task->self_exec_id&&
        same_file(file,instance->role==1?self->backend:self->loader)){role=instance->role;break;}
    spin_unlock_irqrestore(&self->lock,flags);fput(file);return role;
}
bool as_self_backend_instance(struct as_self_protection *self)
{ return self && verified_role(self,current)==1; }
static int decision(struct as_self_protection *self,struct inode *inode,bool controller)
{
    if(!controller || verified_role(self,current)!=1)return -EPERM;
    if(inode&&(inode==file_inode(self->backend)||inode==file_inode(self->loader)))return -EPERM;
    return 1;
}
int as_self_path(struct as_self_protection *self,const struct path *path,u32 operation,bool controller)
{
    if(!self||!path)return 0;
    if((operation==AS_FILE_MOUNT||operation==AS_FILE_MOVE_MOUNT)&&(ancestor(self,path)||inside(self,path)))return -EPERM;
    if((operation==AS_FILE_RENAME||operation==AS_FILE_RMDIR)&&ancestor(self,path))return -EPERM;
    if(!protected(self,path))return 0;
    return decision(self,d_inode(path->dentry),controller);
}
int as_self_dentry(struct as_self_protection *self,struct dentry *dentry,u32 operation,bool controller)
{
    if(!self||!dentry)return 0;
    if(is_subdir(dentry,self->root.dentry)||known_inode(self,d_inode(dentry)))return decision(self,d_inode(dentry),controller);
    return 0;
}
int as_self_file(struct as_self_protection *self,struct file *file,u32 operation,bool controller)
{
    int result;
    if(!self||!file||!protected(self,&file->f_path))return 0;
    result=decision(self,file_inode(file),controller);
    if(result>0){int error=remember_inode(self,file_inode(file));if(error)return error;}
    return result;
}
int as_self_pair(struct as_self_protection *self,const struct path *from,const struct path *to,u32 operation,bool controller)
{
    bool a,b;
    if(!self)return 0;
    if((from&&ancestor(self,from))||(to&&ancestor(self,to)))return -EPERM;
    if(operation==AS_FILE_MOVE_MOUNT&&((from&&inside(self,from))||(to&&inside(self,to))))return -EPERM;
    a=from&&protected(self,from);b=to&&protected(self,to);
    if(!a&&!b)return 0;
    if(a!=b)return -EPERM;
    if(!controller||verified_role(self,current)!=1)return -EPERM;
    if((from&&d_inode(from->dentry)&&(d_inode(from->dentry)==file_inode(self->backend)||d_inode(from->dentry)==file_inode(self->loader)))||
       (to&&d_inode(to->dentry)&&(d_inode(to->dentry)==file_inode(self->backend)||d_inode(to->dentry)==file_inode(self->loader))))return -EPERM;
    return 1;
}
int as_self_link(struct as_self_protection *self,struct dentry *from,const struct path *to,bool controller)
{
    bool a,b;
    if(!self)return 0;
    a=is_subdir(from,self->root.dentry)||known_inode(self,d_inode(from));b=protected(self,to);
    if(!a&&!b)return 0;
    if(a!=b)return -EPERM;
    return decision(self,d_inode(from),controller);
}
static bool mount_overlaps_root(struct as_self_protection *self,const struct path *path)
{ return path_is_under(&self->root,path)||path_is_under(path,&self->root); }
static bool protected_superblock(struct as_self_protection *self,struct super_block *sb)
{
    struct protected_inode *item;unsigned long flags;unsigned bucket;bool found=false;
    if(self->root.dentry->d_sb==sb)return true;
    spin_lock_irqsave(&self->lock,flags);
    hash_for_each(self->objects,bucket,item,node)if(item->inode->i_sb==sb){found=true;break;}
    spin_unlock_irqrestore(&self->lock,flags);return found;
}
int as_self_umount(struct as_self_protection *self,struct vfsmount *mnt,bool isolated_detach)
{
    struct path path;
    if(!self||!mnt)return 0;
    path.mnt=mnt;path.dentry=mnt->mnt_root;
    if(isolated_detach&&!mount_overlaps_root(self,&path))return 0;
    return ancestor(self,&path)||inside(self,&path)?-EPERM:0;
}
u32 as_self_mount_effect(struct as_self_protection *self,const struct path *path,unsigned long flags,bool no_outward_propagation)
{
    unsigned long propagation=flags&~(MS_REC|MS_SILENT);
    if (!self||!path) return AS_MOUNT_UNKNOWN;
    if(mount_overlaps_root(self,path))return AS_MOUNT_PROTECTED_TREE;
    /* Follow path_mount's operation precedence; dentry aliases do not identify
     * an original mount, but a filesystem remount still affects shared inodes. */
    if((flags&(MS_BIND|MS_REMOUNT))==(MS_BIND|MS_REMOUNT))return AS_MOUNT_INDEPENDENT_VIEW;
    if(flags&MS_REMOUNT){
        if(protected_superblock(self,path->mnt->mnt_sb))return AS_MOUNT_PROTECTED_TREE;
        return AS_MOUNT_UNKNOWN;
    }
    if(!(flags&MS_BIND)&&(propagation==MS_SHARED||propagation==MS_PRIVATE||
        propagation==MS_SLAVE||propagation==MS_UNBINDABLE))return AS_MOUNT_INDEPENDENT_VIEW;
    if((flags&MS_BIND)||!(flags&(MS_SHARED|MS_PRIVATE|MS_SLAVE|MS_UNBINDABLE|MS_MOVE))){
        if(no_outward_propagation)return AS_MOUNT_INDEPENDENT_VIEW;
    }
    return AS_MOUNT_UNKNOWN;
}
int as_self_mount(struct as_self_protection *self,const struct path *path,u32 effect,bool controller)
{
    if(!self||!path)return 0;
    if(effect==AS_MOUNT_PROTECTED_TREE)return -EPERM;
    return as_self_path(self,path,AS_FILE_MOUNT,controller);
}
int as_self_prepare_exec(struct as_self_protection *self,struct file *file)
{
    struct exec_instance *instance,*next,*entry;unsigned long flags;u8 role;
    if(!self||!file||!capable(CAP_SYS_ADMIN))return 0;
    role=same_file(file,self->backend)?1:same_file(file,self->loader)?2:0;
    spin_lock_irqsave(&self->lock,flags);
    /* A failed exec leaves the last successful identity valid. Only replace
     * this task group's uncommitted candidate, never its active instance. */
    list_for_each_entry_safe(instance,next,&self->instances,link)if(instance->tgid==task_tgid(current)&&instance->pending){
        list_del(&instance->link);self->instance_count--;put_pid(instance->tgid);kfree(instance);
    }
    spin_unlock_irqrestore(&self->lock,flags);
    if(!role)return 0;
    entry=kzalloc(sizeof(*entry),GFP_NOWAIT);if(!entry)return -ENOMEM;
    entry->tgid=get_task_pid(current,PIDTYPE_TGID);entry->birth=current->group_leader->start_boottime;
    entry->exec_id=current->self_exec_id;entry->pending=true;entry->role=role;
    spin_lock_irqsave(&self->lock,flags);
    if(self->instance_count>=AS_SELF_INSTANCES_MAX){spin_unlock_irqrestore(&self->lock,flags);put_pid(entry->tgid);kfree(entry);return -ENOSPC;}
    list_add(&entry->link,&self->instances);self->instance_count++;spin_unlock_irqrestore(&self->lock,flags);return 0;
}
void as_self_exec_seen(struct as_self_protection *self,struct task_struct *task)
{
    struct exec_instance *instance,*next;struct file *file;unsigned long flags;
    if(!self)return;
    file=task_exe(task);if(!file)return;
    spin_lock_irqsave(&self->lock,flags);
    list_for_each_entry_safe(instance,next,&self->instances,link)if(instance->tgid==task_tgid(task)){
        if(instance->pending&&instance->birth==task->group_leader->start_boottime&&instance->exec_id!=task->self_exec_id&&same_file(file,instance->role==1?self->backend:self->loader)){
            instance->pending=false;instance->exec_id=task->self_exec_id;
        }else if(instance->birth!=task->group_leader->start_boottime||instance->exec_id!=task->self_exec_id||!same_file(file,instance->role==1?self->backend:self->loader)){
            list_del(&instance->link);self->instance_count--;put_pid(instance->tgid);kfree(instance);
        }
    }
    spin_unlock_irqrestore(&self->lock,flags);fput(file);
}
void as_self_exit_seen(struct as_self_protection *self,struct task_struct *task)
{
    struct exec_instance *instance,*next;unsigned long flags;
    if(!self)return;
    spin_lock_irqsave(&self->lock,flags);
    list_for_each_entry_safe(instance,next,&self->instances,link)if(instance->tgid==task_tgid(task)){
        list_del(&instance->link);self->instance_count--;put_pid(instance->tgid);kfree(instance);
    }
    spin_unlock_irqrestore(&self->lock,flags);
}
struct scan_name{struct list_head link;u32 bytes;char name[];};
struct scan_dir{struct list_head link;struct path path;};
struct scanner{struct dir_context context;struct list_head names;u32 count;int error;};
static bool collect(struct dir_context *ctx,const char *name,int bytes,loff_t offset,u64 ino,unsigned type)
{
    struct scanner *scan=container_of(ctx,struct scanner,context);struct scan_name *entry;
    if((bytes==1&&name[0]=='.')||(bytes==2&&name[0]=='.'&&name[1]=='.'))return true;
    if(scan->count>=AS_SELF_OBJECTS_MAX){scan->error=-E2BIG;return false;}
    entry=kmalloc(sizeof(*entry)+bytes,GFP_KERNEL);if(!entry){scan->error=-ENOMEM;return false;}
    entry->bytes=bytes;memcpy(entry->name,name,bytes);list_add_tail(&entry->link,&scan->names);scan->count++;return true;
}
static int inventory(struct as_self_protection *self,const atomic_t *cancelled)
{
    LIST_HEAD(directories);struct scan_dir *dir,*next;int error=0;
    dir=kmalloc(sizeof(*dir),GFP_KERNEL);if(!dir)return -ENOMEM;dir->path=self->root;path_get(&dir->path);list_add(&dir->link,&directories);
    while(!list_empty(&directories)){
        struct file *file;struct scanner scan={.context.actor=collect};struct scan_name *name,*tmp;
        if(signal_pending(current)||atomic_read(cancelled)){error=-ECANCELED;break;}
        dir=list_first_entry(&directories,struct scan_dir,link);list_del(&dir->link);INIT_LIST_HEAD(&scan.names);
        file=dentry_open(&dir->path,O_RDONLY|O_DIRECTORY|O_NOFOLLOW,current_cred());
        if(IS_ERR(file)){error=PTR_ERR(file);path_put(&dir->path);kfree(dir);break;}
        error=iterate_dir(file,&scan.context);fput(file);if(!error)error=scan.error;
        list_for_each_entry_safe(name,tmp,&scan.names,link){
            struct qstr q=QSTR_INIT(name->name,name->bytes);struct dentry *dentry;struct path path;
            list_del(&name->link);
            if(!error){dentry=lookup_one_positive_unlocked(mnt_idmap(dir->path.mnt),&q,dir->path.dentry);
                if(IS_ERR(dentry))error=PTR_ERR(dentry);else{
                    path.mnt=mntget(dir->path.mnt);path.dentry=dentry;
                    error = follow_down(&path, LOOKUP_FOLLOW);
                    dentry = path.dentry;
                    bool seen = known_inode(self, d_inode(dentry));
                    if(!error)error=remember_inode(self,d_inode(dentry));
                    if(!error&&!seen&&S_ISDIR(d_inode(dentry)->i_mode)){
                        next=kmalloc(sizeof(*next),GFP_KERNEL);if(!next)error=-ENOMEM;
                        else{next->path=path;path_get(&path);list_add_tail(&next->link,&directories);}
                    }
                    path_put(&path);
                }
            }
            kfree(name);
        }
        path_put(&dir->path);kfree(dir);if(error)break;
    }
    list_for_each_entry_safe(dir,next,&directories,link){list_del(&dir->link);path_put(&dir->path);kfree(dir);}return error;
}
static struct file *bind_file(const u8 *bytes,u32 count)
{char *name;struct file *file;if(!count||count>PATH_MAX||memchr(bytes,0,count))return ERR_PTR(-EINVAL);name=kmemdup_nul(bytes,count,GFP_KERNEL);if(!name)return ERR_PTR(-ENOMEM);file=filp_open(name,O_RDONLY|O_NOFOLLOW,0);kfree(name);return file;}
struct as_self_protection *as_self_prepare(const u8 *root,u32 root_bytes,const u8 *backend,u32 backend_bytes,const u8 *loader,u32 loader_bytes,struct pid *controller,struct as_self_protection *previous,struct workqueue_struct *release_queue,const atomic_t *cancelled)
{
    struct as_self_protection *self;struct file *actual;struct exec_instance *entry;char *name;
    unsigned long flags;int error;
    if(!root_bytes||root_bytes>PATH_MAX||memchr(root,0,root_bytes))return ERR_PTR(-EINVAL);
    if(task_tgid(current)!=controller)return ERR_PTR(-EACCES);
    if(previous){
        struct path root_path;struct file *b,*l;
        name=kmemdup_nul(root,root_bytes,GFP_KERNEL);if(!name)return ERR_PTR(-ENOMEM);
        error=kern_path(name,LOOKUP_DIRECTORY|LOOKUP_FOLLOW,&root_path);kfree(name);if(error)return ERR_PTR(error);
        b=bind_file(backend,backend_bytes);l=bind_file(loader,loader_bytes);
        bool same=!IS_ERR(b)&&!IS_ERR(l)&&path_equal(&root_path,&previous->root)&&same_file(b,previous->backend)&&same_file(l,previous->loader)&&as_self_backend_instance(previous);
        path_put(&root_path);if(!IS_ERR(b))fput(b);if(!IS_ERR(l))fput(l);
        if(!same)return ERR_PTR(-EXDEV);
        as_self_get(previous);return previous;
    }
    self=kzalloc(sizeof(*self),GFP_KERNEL);if(!self)return ERR_PTR(-ENOMEM);
    refcount_set(&self->refs,1);INIT_WORK(&self->release,release_self);self->release_queue=release_queue;spin_lock_init(&self->lock);hash_init(self->objects);INIT_LIST_HEAD(&self->instances);
    name=kmemdup_nul(root,root_bytes,GFP_KERNEL);if(!name){error=-ENOMEM;goto fail;}
    error=kern_path(name,LOOKUP_DIRECTORY|LOOKUP_FOLLOW,&self->root);kfree(name);if(error)goto fail;
    self->backend=bind_file(backend,backend_bytes);if(IS_ERR(self->backend)){error=PTR_ERR(self->backend);self->backend=NULL;goto fail;}
    self->loader=bind_file(loader,loader_bytes);if(IS_ERR(self->loader)){error=PTR_ERR(self->loader);self->loader=NULL;goto fail;}
    actual=task_exe(current);if(!same_file(actual,self->backend)){if(actual)fput(actual);error=-EACCES;goto fail;}fput(actual);
    if(!inside(self,&self->backend->f_path)||!inside(self,&self->loader->f_path)||!static_elf(self->backend)||!static_elf(self->loader)){error=-EOPNOTSUPP;goto fail;}
    entry=kzalloc(sizeof(*entry),GFP_KERNEL);if(!entry){error=-ENOMEM;goto fail;}
    entry->tgid=get_task_pid(current,PIDTYPE_TGID);entry->birth=current->group_leader->start_boottime;entry->exec_id=current->self_exec_id;entry->role=1;list_add(&entry->link,&self->instances);self->instance_count=1;
    spin_lock_irqsave(&fence_lock,flags);if(fence){spin_unlock_irqrestore(&fence_lock,flags);error=-EBUSY;goto fail;}fence=self;as_self_get(self);spin_unlock_irqrestore(&fence_lock,flags);
    error=remember_inode(self,d_inode(self->root.dentry));if(!error)error=inventory(self,cancelled);
    if(error){as_self_finish(self);goto fail;}return self;
fail:as_self_put(self);return ERR_PTR(error);
}
