# 驱动hook接入

所属大模块：内核驱动。产品边界见[驱动设计](../design.md)，组合和职责见[驱动架构](../architecture.md)，唯一入口见[驱动总契约](../technical.md)。驱动内部只读上下文见[DriverContext](context.md)，跨模块协议见[通信契约](../../contracts/driver-communication/technical.md)。本页定义本职责的唯一技术细节；不导入底层实现。

安装包保留当前发行版内核；内核接入使用BPF-LSM与一个可加载原生支撑模块，不新增内建LSM。满足需求的成熟机制优先复用，TOMOYO不是必选依赖。原生LSM可在允许睡眠的调用路径使用内核等待队列；BPF-LSM只能使用验证器允许的程序、helper及kfunc。sleepable BPF不等于存在通用“等用户答复”接口；忙循环不是挂起，长时间内核忙等可能造成调度停滞。原生实现也不能在原子上下文、自旋锁或不允许睡眠的调用链上等待。

每个操作接入时固定：实际hook与调用者、原对象、可取得字段、允许／拒绝返回语义、能否等待、原请求期限／取消、失败处理及结果可见范围。这是实现正确性所需信息，不另造测试矩阵或复杂准入框架。

## 回调内主动事实采集

hook 参数是采集起点，不是信息上限。当前驱动已经在同一回调内通过内核 API 主动取得调用者、凭据、命名空间、路径和执行现场。公共字段的含义、类型及 TLV 只在[事件契约](../../contracts/events.md)维护；本节只说明获取入口、锁和寿命，不建立第二套事件或进程缓存。

同一回调不是全系统原子快照。每项字段按下述对象引用和锁取得；不得因为它们同属一条记录，就宣称其他线程不能修改共享 socket、fs 或 namespace 状态。原 task／对象寿命与跨字段一致性分别核实，不能在回调外重读补成某个较早时点的完整状态。

### 现有公共采集链

普通进程／网络入口经 `start()`，普通文件入口经 `file_gate()`，统一进入 [start_at → capture](../../../drivers/linux/as_core.c)。BPF 在原 hook 中先经只读 `as_capture_requested` 检查既有开关／候选掩码，有候选才用 CO-RE 读取 current 的 mount namespace 并调用 `bpf_get_stack` 取得用户原生 IP 栈。它不作规则裁决或持有规则快照；最终 `start_at` 独立采用原规则状态，变化后允许本次可选预采值缺失，不补取或改变安全动作。

`start_at` 先检查开关与本操作是否有规则候选；有候选才保管原快照并分配原 `as_original`。`capture` 与 `append_process_info` 填同一公共头及进程／线程事实，各 hook 随后填自己的差异载荷，最后由 `finish` 完成驱动快速匹配。只有确实需要上报的 Controlled／Direct，才由同一 `capture_execution` 在等待或终止前补入该原记录的 syscall／kernel_stack 及已经取得的用户栈结果。无候选不捕用户栈，不报告的快速命中不展开内核栈；存在候选但最终不报告的调用可能已经预采用户栈，局部数据在该次调用结束释放。

生命周期 `observe(actor, child, ...)` 复用 `capture(actor)` 和相同编码；Create 的原 actor 是 tracepoint 给出的真实创建者，child 属性从原 child 取得，不能用 `current`、创建者 argv 或创建者身份代替。当前 Create 只报告新线程组，Exit 只报告 group_dead，不宣称已提供所有线程的创建／退出。

Self 审计有不可睡眠的回调。可归属的 actor 仍经 `capture`／`append_process_info`；I/O worker 等原主体不明确时只保留已知来源头和对象字段，不把当前工作线程冒充原调用者，也不强制获取普通 gate 的用户寄存器和栈。cwd 的 path 引用在不可睡眠路径交既有释放工作队列退役；普通可睡眠路径实际释放。这里的上下文分支是已有来源边界，不是另建采集器。

### 已取得的 API 与生命周期

- **线程／进程身份和时间**：`task_pid_nr(actor)`、`task_tgid_nr(actor)`、原 task 与 group_leader 的 `start_boottime`、`ktime_get_boottime_ns()` 已进入原事件头。`get_task_comm` 读取原 task comm；`self_exec_id` 按原值保存。实际父线程组从 RCU 保护的 `real_parent` 取得，增加 task／pid 引用后离开 RCU，采集结束释放。创建者与物理父亲仍分开。[Linux PID 接口](https://github.com/torvalds/linux/blob/v7.0/include/linux/pid.h)
- **凭据与命名空间**：`get_task_cred`／`put_cred` 保管同一凭据，保存宿主初始 user namespace 下真实／有效／保存／FS UID/GID 和 user namespace。`task_lock` 保护对 nsproxy 的读取，只在锁内复制 net namespace 标量；`get_task_pid(PIDTYPE_TGID)` 保管实际 pid 对象，读取 active PID namespace 和完整 TGID 链后 `put_pid`。同一公共 pid 向量函数分别接收 PIDTYPE_TGID 和 PIDTYPE_PID，后者仅在 `capture(actor)` 填事件头的 namespace TID 链，不放入 child_process 区域或进程树。当前普通 hook 的 mount namespace 经 BPF CO-RE 读取后填入既有 `mount_namespace_inode`／TLV19；读取成功的0仍保留，失败不补零。cgroup 与其祖先在原 RCU 区间取值，不保留裸 cgroup 指针。[凭据接口](https://github.com/torvalds/linux/blob/v7.0/include/linux/cred.h) · [namespace 接口](https://github.com/torvalds/linux/blob/v7.0/include/linux/pid_namespace.h)
- **镜像、cwd 与参数**：镜像取得真实 file 引用后由同一原路径编码，结束时 `fput`；cwd 经 `get_fs_pwd` 取得 path 引用并按实际上下文释放。当前 actor argv 仅在 actor 就是 current 时一次有界复制；exec 的目标 argv 从原 `bprm->mm` 参数页获取，使用 mmap 读锁、页引用及 `kmap_local_page`，每页读完释放。不能把 foreign task 的后读、旧 mm 或 env 当成原目标参数。具体范围见[原事件字段](../../contracts/events.md#进程载荷)和现有 `target_arguments`／`actor_arguments`。
- **原操作对象**：文件字段由 hook 正在持有的 path／file／inode 取得；网络在原 socket 上读取端点、协议、`sock_diag_save_cookie` 和 socket 自己的 net namespace。不能用 actor 的 net namespace 代替 socket 所属域；cookie 并非用户 FD。上述字段已存在，不重新增加同义字段。[socket cookie 接口](https://github.com/torvalds/linux/blob/v7.0/net/core/sock_diag.c)
- **执行现场**：`task_pt_regs(current)`、`user_mode`、`syscall_get_arch/nr/arguments` 和 `user_stack_pointer` 给出原用户入口寄存器；`stack_trace_save` 给出当前内核栈。现有实现只在原 hook 内取得，不在 Ask 等待后回读。真实 `-1`、零参数、空栈及满缓冲原义不改，现场快照不进入进程树补全。[唯一现场契约](../../contracts/events.md#原操作执行现场) · [实测边界](../../evidence/execution-site-capture.md)

- **BPF 用户原生栈**：普通 hook 在上述候选预检查后通过 `bpf_get_stack(ctx, ..., BPF_F_USER_STACK)` 读取当前任务的原生 IP 序列。实际返回字节数或负 errno 与 IP 一起沿原记录保存，成功0与未取得分开；数值不解引用、不按栈形状判身份。固定272字节、只含标量的 `as_hook_facts` 仅位于本次 BPF 栈，Native 在原 `as_original` 中复制需要发布的值后不保留该指针；没有 task/per-CPU 临时表、独立通知或第二个拥有者。rename 的原标量 flags 与该局部数据一起传入，四个原内核指针继续作为 verifier 核验的独立 kfunc 参数，保持最多五个参数。

目标 BTF 已实际包含 current→nsproxy→mnt_ns→ns.inum；namespace 访问规则允许 current 在本次回调读取，其他 task 仍须按 task_lock／原对象寿命处理。Linux v7.0 的 LSM helper 分派进入 tracing／raw tracing 的 get_stack 实现，用户路径使用 current 的用户寄存器；原生模块未导出 stack_trace_save_user 不否定这条 BPF 路径。源码与编译不等于目标 verifier／实测通过，实际结果另记证据。[namespace 规则](https://github.com/torvalds/linux/blob/v7.0/include/linux/nsproxy.h) · [LSM 分派](https://github.com/torvalds/linux/blob/v7.0/kernel/bpf/bpf_lsm.c) · [tracing helper](https://github.com/torvalds/linux/blob/v7.0/kernel/trace/bpf_trace.c)

### 已核可继续补齐的入口，尚未实现

以下是具有实际内核来源的候选，不表示字段已下发、探针已附着或业务关系已成立。新增字段仍须先落入所属公共契约；优先使用已有原事件和在途拥有者，不复制已有头、身份、队列或规则池。

- **线程创建／退出**：已经注册的 `sched_process_fork` 给出真实 parent／child task，但 `created` 仅保留 thread_group_leader；`exited` 在非 group_dead 时返回。可以在这些既有回调上取得被滤掉的线程事实，原头已有创建者 TID／birth。需要先定义线程观察的归属，不能把 CLONE_THREAD 写成新的进程树节点或把每次线程退出当成进程退出。[sched 生命周期原型](https://github.com/torvalds/linux/blob/v7.0/include/trace/events/sched.h)
- **等待、唤醒与运行**：`sched_waking` 在实际唤醒上下文执行，`sched_wakeup` 记录目标成为可运行但不保证在原唤醒者上下文，`sched_switch` 给出 prev／next 和切换状态。三者是不同事实。它们可补线程交接模式；必须保留目标线程寿命及中断／内核上下文，不能将中断时的 current 认成用户发起者。当前 LSM 里的 task 状态只描述那个时点，不能回溯替代尚未采集的唤醒事件。这些 scheduler 探针尚未接入。[原型与上下文保证](https://github.com/torvalds/linux/blob/v7.0/include/trace/events/sched.h)
- **同步请求与实际完成**：正式 private futex 采集从实际原生 syscall 参数区分 WAIT_PRIVATE 和 WAKE_PRIVATE。等待探点的作用域归该次 `futex_do_wait`。唤醒方在 `futex_wake_mark` 持原 bucket 锁时核取同一活跃条目，复制所需值，再在相同 wake queue 的实际 `wake_q_add_safe` 回调核对目标线程。缺等待探点或同一条目的证明时仍可记录选择候选。原等待回调退出和任务退出清除借用引用，关闭时停止接纳并释放所有探点。控制服务自身的 futex 选择按已认证进程实例排除，防止观察重新触发自身消费。采集缺失不改变防护启动和裁决。[原事实](../../contracts/events.md#private-futex-唤醒选择) · [futex 原队列与 wake](https://github.com/torvalds/linux/blob/v7.0/kernel/futex/waitwake.c)
- **信号及网络接收**：`signal_generate` 与 `signal_deliver` 能分别保留实际目标、信号和阶段；特殊 siginfo 哨兵不能直接解引用，产生不等于已递送。`socket_recvmsg` 则提供原 socket、接收消息、大小和 flags，可与已有 socket cookie 关联接收尝试；它不是实际成功收到字节。需要 syscall／函数返回来源才报告真实完成。当前普通网络 gate 只有 Connect／Send，信号事件和接收入口未实现；这不否定 Flow 已取得的协议响应事实。[signal tracepoint](https://github.com/torvalds/linux/blob/v7.0/include/trace/events/signal.h) · [LSM socket 原型](https://github.com/torvalds/linux/blob/v7.0/include/linux/lsm_hook_defs.h)

CPU、调度优先级、当前 task 状态等也有真实来源，但不是已定义的工具归因条件。是否增加，取决于明确模式是否消费该事实，而不是因字段能够读取就全部保存。新探针不得把全系统高频调度追踪默认为所有规则的开销；采集范围与记录寿命须明确，不增加 Agent 名称过滤、时间猜测、固定栈形状匹配或第二套后台采集框架。

## 当前普通控制入口与结果边界

[as_hooks.bpf.c](../../../drivers/linux/as_hooks.bpf.c) 的 12 个 sleepable 入口调用 [as_core.c](../../../drivers/linux/as_core.c) 中对应 KF_SLEEPABLE 函数。

- as_exec → bprm_check_security → Process.Execute。
- as_file_open → file_open → File.Open。
- as_file_create → inode_create → File.CreateFile，限定已取得原创建路径的具名普通文件。
- as_file_mkdir → path_mkdir → File.CreateDirectory。
- as_file_unlink → path_unlink → File.Unlink。
- as_file_rmdir → path_rmdir → File.RemoveDirectory。
- as_file_rename → path_rename → File.Rename。
- as_truncate_path → path_truncate → File.TruncatePath。
- as_inode_setattr → inode_setattr → File.TruncateOpenFile，仅原 ATTR_SIZE＋ATTR_FILE 组合。
- as_mmap_shared_write → mmap_file → File.MapSharedWritable。
- as_connect → socket_connect → Network.Connect。
- as_sendmsg → socket_sendmsg → Network.Send。

三类规则池按对应操作筛选，在同一原hook返回Allow／Deny；ToUser保管调用并等待。BPF保留前序非零拒绝，原生函数不能把其他检查已经拒绝的操作改成允许。PF_IO_WORKER不能伪装成原actor，普通等待链对其明确返回Unsupported；不从这些入口宣称所有异步I/O的主体已完整取得。

原回执表示本次LSM权限检查如何收尾：Allow是允许原调用继续，不能等同于截断、映射、打开已完成、exec已成功、连接已建立或字节已送达。后续文件系统／协议栈仍会失败。成功exec另有事实通知，当前不伪造其余系统调用最终结果。

普通 Open 的裁决点是 `file_open`，O_CREAT 可以在进入该点前创建 inode。具名普通文件创建在实际 `inode_create` 裁决，系统自保继续在 `path_mknod` 检查。见[创建交接](#具名普通文件创建的同次请求交接)。

## 普通文件gate接线

取得原事件路径后按[驱动规则分流契约](rules.md)核对后端候选。

以下是统一接线契约。源码、目标内核实际加载和原操作效果分别验证。保留原前序LSM拒绝，同一物理hook只保一个AS程序，交[Core统一gate](../technical.md#统一file-gate流程)：

- Open沿原`file_open(file)`。真实file带path／inode及原打开信息；仍是本次打开准入，不含O_PATH，也不证明已创建／打开成功。
- CreateDirectory沿`path_mkdir(dir, dentry, mode)`，在`filename_mkdirat`调用实际`vfs_mkdir`前裁决。负dentry的目标inode尚不存在，不能填父inode作目标；路径由原dir.mnt与dentry构成，device可取该原parent path已选文件系统。
- Unlink沿`path_unlink(dir, dentry)`，在实际`vfs_unlink`前裁决；RemoveDirectory沿`path_rmdir(dir, dentry)`，在实际`vfs_rmdir`前裁决。原目标名字和对象事实可取得；移除名字不等于已打开对象立即消失。
- Rename沿`path_rename(old_dir, old_dentry, new_dir, new_dentry, flags)`，在`start_renaming`完成查找／加锁后、实际`vfs_rename`前裁决。原path／new_path保两个原角色，不用cwd／inode别名猜挂载；源inode不冒充目的inode。交换、覆盖及失败语义按原参数解释，不造逆向EventKey或第二次独立裁决。

- TruncatePath 沿 `path_truncate(path)`。Linux 7.0 的 [vfs_truncate](https://github.com/torvalds/linux/blob/v7.0/fs/open.c) 在取得挂载写引用和 inode 写访问、处理 lease 后，到实际 `do_truncate` 前调用该点。hook 只给出原 path，目标长度无来源。已有 FD 的 `ftruncate` 和打开已有文件的 `O_TRUNC` 另走文件对象链。[Linux 的 handle_truncate](https://github.com/torvalds/linux/blob/v7.0/fs/namei.c#L4018)
- TruncateOpenFile 沿 `inode_setattr(dentry, attr)`，要求 ATTR_SIZE 与 ATTR_FILE 同时存在，原 ia_file 非空，原 file.dentry 等于该次 setattr 的 dentry，且对象是普通文件。路径、文件标志和大小取原 file 与 iattr。Linux 7.0 的 `do_truncate` 对有文件对象的请求填这两位，`notify_change` 持 inode 锁在实际 setattr 前检查。此点可保管同一请求，但不能阻止其此前已发生的打开步骤。早期非睡眠 `file_truncate` 继续执行自保。其他 setattr 走既有元数据自保。等待期间答复链不能打开、变更或等待同一 inode。[do_truncate](https://github.com/torvalds/linux/blob/v7.0/fs/open.c#L45) · [notify_change](https://github.com/torvalds/linux/blob/v7.0/fs/attr.c#L405)
- MapSharedWritable 沿 `mmap_file(file, reqprot, prot, flags)`。仅真实普通文件、有效 PROT_WRITE 且 MAP_TYPE 为 MAP_SHARED 或 MAP_SHARED_VALIDATE 进入普通 gate。Linux 7.0 的 [vm_mmap_pgoff](https://github.com/torvalds/linux/blob/v7.0/mm/util.c#L529) 在取 mmap 写锁及修改 VMA 之前检查。原三项位集随事件传递，原 file 由调用者持有至返回。匿名、私有或无写权限的映射无此普通事件；自保原检查继续适用。

三项都复用 `file_gate`，先做原自保，再用同一快照、原操作和等待链；加载器在原有物理 hook 位置替换两个旧薄入口。用户答复、超时和停止不访问同一目标，尤其不能等待同一挂载写冻结或另一项针对同目标的操作完成。

所有载荷按[统一字段](../../contracts/events.md#普通文件gate字段与操作来源)供给；只有真实file参数时才填open_flags／access_mask。device取原dentry->d_sb，inode只在真实d_inode存在时保留，不强行抹掉已获取值或拿父inode替代。raw mode／flags不从后续stat补回，路径原字节不作UTF-8替换或文件名归一化。path_mkdir原mode来自[filename_mkdirat](https://github.com/torvalds/linux/blob/v7.0/fs/namei.c)调用的mode_strip_umask：匹配7.0头文件include/linux/namei.h:199—203仅在不支持POSIX ACL且无SB_I_NOUMASK时扣current_umask；其余留给文件系统处理，hook不自动补S_IFDIR。

Linux v7.0的[sleepable集合](https://github.com/torvalds/linux/blob/v7.0/kernel/bpf/bpf_lsm.c)包含 file_open、path_mkdir、path_unlink、path_rmdir、path_rename、path_truncate、mmap_file、inode_setattr。普通入口替换同 hook 的旧 self-only 入口，调用KF_SLEEPABLE同原等待支撑，kfunc过滤和加载清单同步；没有旧wrapper fallback。目标发行版是否可实际附着及生效仍按当前产物真实结果认定，不从源码或构建推定。

组织方式复用[成熟LSM分层依据](../references.md#hook到操作规则匹配的源码依据)：TOMOYO的path_perm／path2_perm与AppArmor的common_perm保真实动作／双路径，KubeArmor的[公共path matcher](https://github.com/kubearmor/KubeArmor/blob/main/KubeArmor/BPF/shared.h)复用同类检查而非每hook一套服务。这里只借薄入口和内聚匹配；TOMOYO原生等待不证明任意BPF hook可睡眠，KubeArmor直接裁决也不提供Ask能力，不复制它们的冲突优先级。

可睡眠不表示没有外层锁：[目录锁规范](https://github.com/torvalds/linux/blob/v7.0/Documentation/filesystems/directory-locking.rst)要求创建／删除持目录i_rwsem，跨目录改名还持s_vfs_rename_mutex及父目录锁。Open的atomic-create调用也不能笼统当成无锁。AS不得释放／重取这些VFS锁后重放，不在自有spinlock或普通rcu_read_lock临界区内等待；原答复、Ask发送和停止路径不能依赖目标路径的重新打开、枚举、hash或先写日志／查询完成。原事实从回调先采，补充进程信息与落库不得成为解除本次内核等待的反向依赖。

#### 写打开与写关闭

写打开复用 `file_open` 的 `File.Open` 事件、文件规则和原等待。原 `file.f_mode` 含 `FMODE_WRITE` 且命中有效 Ask 规则时，只为该次打开生成一次 Ask。答复允许时继续原打开，拒绝时原打开返回拒绝。已打开 FD 的 `write()`、`pwrite()`、`writev()` 等不逐次问询。已有 `file_permission(MAY_WRITE)` 继续检查自保，沿用 `File.Write` 事件编号。[写打开规则](../../backend/rules/technical.md#file-target) · [原写入调查](../../evidence/file-wait-extension.md#普通-fd-写入等待复核)

写关闭的统一入口是既有LSM `file_release`，经同一个BPF对象的薄入口调用Native观察函数。该点处于最后file引用释放链，不能阻止、Ask或把通知当写入成功。仅普通文件且真实FMODE_WRITE进入，并复用内核FMODE_FSNOTIFY_NONE过滤；不复制IMA的“所有打开者中最后writer”条件。语义及字段只见[安全审计契约](../../contracts/security-audit.md)。

Native按配置实际绑定的目录inode对象匹配原文件的真实父目录，范围为直接子项。目录改名后仍跟随同一对象，同名重建不自动换实例；未变配置的普通规则替换复用原绑定。自保目录的重复监控沿真实自保对象接口核验，不另拼路径。目录准备可在初始化进行I/O，内核fsnotify mark只管理该目录对象的销毁／卸载，不监听关闭事件或创建用户态fd；避免长期裸持inode妨碍superblock退出，也不永久pin配置mount。释放回调只做有界内存检查、持原path引用并入原普通FIFO。

file_release可能在没有current->fs的延迟fput工作线程执行。不得在该回调盲调d_path，也不得get_file复活最后释放对象；在AS原设备读者上下文沿同一持有path解析，明确是消费时路径。出队不提前销毁实际读取上下文，清空和释放共用原消息生命周期；路径引用在原release_queue锁外释放。安全日志退回纯持久化消费者，不自行创建fanotify来源。实际接通／加载范围以当前候选证据为准，不从hook存在推定已交付。

依据为[Linux文件释放](https://github.com/torvalds/linux/blob/v7.0/fs/file_table.c)、[fsnotify关闭选择](https://github.com/torvalds/linux/blob/v7.0/include/linux/fsnotify.h)及[IMA的file_release消费者](https://github.com/torvalds/linux/blob/v7.0/security/integrity/ima/ima_main.c)。

#### 已有共享映射增加写权限

原文件映射有 `VM_SHARED`、无 `VM_WRITE`，且本次有效 `prot` 请求 `PROT_WRITE` 时属于 `GrantWritableMapping`。内核在 `mmap_write_lock` 内逐段选定原 VMA，调用 `security_file_mprotect(vma, reqprot, prot)` 后才更新该段。跨 VMA 调用可能已更新前段，单段拒绝不回滚前段。首次取得共享可写映射仍走 `MapSharedWritable`。[Linux mprotect 调用链](https://raw.githubusercontent.com/torvalds/linux/v7.0/mm/mprotect.c)

当前 `file_mprotect` 用于自保即时裁决。目标内核在该点拒绝加载可睡眠 BPF，较早的可睡眠系统调用入口尚未选定 VMA。普通规则不支持 `GrantWritableMapping`。见[本期范围](../../archive/unresolved-module-contracts.md#已有共享映射升级的普通问询)及[目标内核实测](../../evidence/file-wait-extension.md#已有共享映射增加写权限的复核)。

#### 具名普通文件创建的同次请求交接

开发范围为目标 Linux 的 `may_o_create` 和 `filename_mknodat` 具名普通文件分支。两条调用链都先经过实际 `path_mknod`，再在父目录锁内以同一个目标 dentry 进入 `inode_create`。原自保在 `path_mknod` 完成，普通裁决在 `inode_create` 复用 `file_gate` 及原等待。Fprobe 只限定原 VFS 函数调用及清理事实，普通裁决仍由 LSM 完成。原有文件打开、匿名临时文件和特殊节点不属于本创建入口。[Linux 调用链](https://raw.githubusercontent.com/torvalds/linux/v7.0/fs/namei.c)及[Fprobe 生命周期](https://docs.kernel.org/trace/fprobe.html)。

Native 的私有创建上下文使用每次函数调用的 Fprobe 入口数据，限定同一实际任务和原 VFS 调用。普通规则有候选时，`path_mknod` 保管真实 mount 与目标 dentry 的路径引用，沿用已有的延迟释放工作队列。`inode_create` 核对同一当前任务、原子 dentry 与真实父 inode 后一次消费。`end_creating_path` 在原次尝试释放前清理未消费引用，函数返回时摘除本次上下文。任务或父 inode 相同而子对象不同的回调不能消费记录。短锁仅索引和消费，释放后沿原 gate 采样、匹配、等待和收尾。普通规则有候选时，缺原作用域的实际创建检查返回明确错误。构建与验收需要目标内核确认全部来源及等待入口。

事件继续使用 `FileOperation::CreateFile` 和 `FileData`。完整路径由原 mount 下已核实的父目录路径和原子 dentry 的实际名称组成，避免把创建中的未哈希 dentry 渲染为已删除文件。组合发生在原父目录锁保持期间。`creation_mode` 取 `inode_create` 的原 mode，目标 inode 未存在时留空。能力位 14 标识普通文件创建，沿用原操作掩码、事件封套、规则池、超时及回执。

Actor／Parent终止由原执行点复用[DriverTools](tools.md)，能力位4／8已实现。原生规则池不直接匹配容器实例。受支持的 Docker 容器事实由底层适配核验，实例范围由底层规则消费。管道消息控制当前未实现。所需能力或条件不支持时整体拒绝采用。见[容器实例绑定](../../backend/driver-adapter/technical.md#容器实例绑定)。

### BPF-LSM＋原生等待支撑

可复用的扩展方式是：可睡眠BPF-LSM在快规则需要移交时调用原生模块注册的kfunc；原生函数登记原请求并用等待队列／completion睡眠；用户态通过实际驱动端点提交决策，原生代码保存决定并唤醒；kfunc返回后BPF在同一次LSM调用中返回允许或拒绝。睡眠发生在原生代码，不能用map轮询或让BPF先返回0替代。

[kfunc官方接口](https://docs.kernel.org/bpf/kfuncs.html)支持模块提供kfunc及KF_SLEEPABLE约束。调用必须来自可睡眠程序，并限于目标内核允许的hook；Linux v7.0的[sleepable_lsm_hooks](https://github.com/torvalds/linux/blob/v7.0/kernel/bpf/bpf_lsm.c#L325)列出了bprm_check_security、file_open、path_unlink／rename、socket_connect／sendmsg、task_alloc等。名单不能外推至file_permission等未列入口，也不能代替对具体调用者持锁及等待依赖的检查。发行版内核按实际加载与行为核验。

原生等待源码位于[as_core.c](../../../drivers/linux/as_core.c)，只实现当前[as_protocol.h](../../../drivers/linux/as_protocol.h)通信记录；旧原生头与旧decoder已经删除。每个原请求独立保管，关闭／取消／超时只能收尾一次；用户态传回的是决策结果，原操作实际执行结果另行取得。原生模块满足加载／签名条件时不需要仅为注册此kfunc重编整个LSM；这不等于任意目标机都可加载。

### 系统自保附着集合

下列 16 个独立入口只调用不等待的自保判定，使用同一配置系统组和实际对象绑定，不成为另一规则池：

- as_self_permission：file_permission，实际后续MAY_WRITE。
- as_self_file_truncate：file_truncate。
- as_self_chmod：path_chmod；as_self_chown：path_chown。
- as_self_create：path_mknod；as_self_symlink：path_symlink。
- as_self_link：path_link，核对实际两端。
- as_self_setxattr：inode_setxattr；as_self_removexattr：inode_removexattr。
- as_self_set_acl：inode_set_acl；as_self_remove_acl：inode_remove_acl；as_self_file_setattr：inode_file_setattr。
- as_self_mprotect：file_mprotect，处理已有共享映射的新写授权。
- as_self_mount：sb_mount；as_self_move_mount：move_mount；as_self_umount：sb_umount。

底层加载器核对并附着12个普通入口、16个自保入口及1个写关闭事实入口，共29个。创建的早期自保入口同时保管原路径事实。全部完成后才输出 Ready。原生三个生命周期入口和创建交接的 Fprobe 由模块注册。任一必需入口缺失时初始化失败。元数据操作复用 File.SetMetadata，见[DriverSelfProtection](self-protection.md)。

mmap_file的file参数对匿名映射可以为NULL；BPF入口在调用统一 gate kfunc 前显式判空，匿名映射没有文件对象，不参与文件自保。原生kfunc仍保持非空可信指针要求，不放宽对象类型或绕过验证器；约束依据见[kfunc参数文档](https://docs.kernel.org/bpf/kfuncs.html#kfunc-parameters)。

### 进程树必需的生命周期hook

进程创建和退出回调是基础事实入口，同时保留exec成功入口维护镜像变化。LSM继续负责执行前安全裁决；生命周期补充由同一原生模块通过正式tracepoint API注册回调，不增加另一执法栈、守护进程或业务线程。取舍依据见[生命周期源码调研](../references.md#进程生命周期回调与进程树依据)。当前同一模块以for_each_kernel_tracepoint查找并以tracepoint_probe_register注册以下三个入口；check_trace_callback_type_sched_process_*核对匹配headers中的原型。

1. **创建成功：sched_process_fork**。Linux v7.0在copy_process成功后、wake_up_new_task之前调用，失败分支不进入该点，见[实际调用位置](https://github.com/torvalds/linux/blob/v7.0/kernel/fork.c#L2497)。回调原型提供创建者和子task；只为新的线程组生成Create进程事实，CLONE_THREAD不能新增一个进程节点。任务已创建不等于已经exec了新程序。
2. **执行前：bprm_check_security**。继续承担Execute的允许／拒绝／问询；命中允许不提前更新进程树中的已执行镜像。创建前的task_alloc安全检查与创建成功通知分开，不能把task_alloc当作出生事实；其后仍可能分配失败，且此处尚无最终子PID／birth，不能伪造这些字段或据此配置不支持的条件。
3. **执行成功：sched_process_exec**。只在实际成功切换镜像后更新相同进程实例的镜像事实，见[exec_binprm调用位置](https://github.com/torvalds/linux/blob/v7.0/fs/exec.c#L1594)。失败或被拒绝的exec不生成成功事实，也不新增Create节点。非主线程exec前后的TID变化不应拆成两个进程；内核de_thread保留进程原出生时间，进程身份仍以TGID及原birth为准。
4. **进程组退出：sched_process_exit**。Linux v7.0回调提供task与group_dead；仅group_dead=true时产生进程Exit事实，不能以TID==TGID、线程组长先退出或任意一次线程退出认定整个进程结束。该通知发生在退出清理过程中，不能据此宣称所有原生资源已释放，也不能单凭它确认AS终止动作的因果和完整分项结果。实际顺序见[do_exit](https://github.com/torvalds/linux/blob/v7.0/kernel/exit.c#L870)与[tracepoint参数](https://github.com/torvalds/linux/blob/v7.0/include/trace/events/sched.h#L307)。

生命周期回调只捕获必要原生事实并有界投递，不等待人工、不调用慢查询、不按日志开关过滤。它们不产生OriginalOperation或RuleHit，不走询问池，也不能通过返回一个值阻止已经观察到的创建／退出。task_free／sched_process_free是对象回收阶段，不替代进程退出通知；不为这个阶段新增一套进程树生命周期。

EventData及ProcessData沿用公共结构，只增加必要缺口字段；Create／Exit操作编号、创建者与子进程／物理父进程的区别、TLV和exec成功字段映射以[进程生命周期事件](../../contracts/events.md#进程载荷)为唯一出处。创建者不一定是新进程的物理父进程，不能直接把回调parent参数填成父子树边。

生命周期事实经[0x104观察事件](../../contracts/driver-communication/technical.md#6-消息类型与完成条件)进入原接收／快速事实路径，更新进程树接口，再按现有消息职责分发。原生观察使用主模块同一Work的Native完成模式保管到事实处理结束，不创建需要额外任务保活的外部完成订阅者，也不因临时接收端释放跳过事实维护。普通防护暂时关闭不停止仍被身份／进程树需要的这几个入口；完整AS退出再按真实生命周期解除它们。

启动时核实三个生命周期tracepoint和实际参数可用性；缺失时报对应能力错误，不把task_free、逐线程退出、轮询或另一个hook作为静默回退。StartFacts在正式规则采用前允许Dormant／Failed的基础观察，生命周期不再按PF_KTHREAD／PF_IO_WORKER过滤线程组事实；普通可裁决入口对其原actor的限制仍保留，不能把观察扩展当成新增等待能力。Quiesce停止新观察并排空已经进入的采集。

当前全局序号不是生命周期连续序列，普通dropped也不等于进程丢失。进程事实用独立loss_epoch及0x106栅栏表达完整性；其全采集桶、固定cutoff和底层快速FIFO处理确认只按[唯一通信定义](../../contracts/driver-communication/technical.md#基础进程事实的线格式)。存量扫描、纳秒身份核对和不确定修复由[进程树模块](../../backend/process-tree/technical.md)承担，原生hook不自行维护另一份进程树。

InspectBatch复用真实task引用采样TGID、组birth、物理父身份、进程类型、64位self_exec_id、cgroup及源BOOTTIME区间。它不把读/proc的时钟tick当身份、不递归枚举全系统，引用在固定批次采样后释放；失败和进程已消失分别返回。公共类型见[process_source.rs](../../../crates/contracts/as-types/src/process_source.rs)。

### 管道hook与MCP消息拦截

本方向已暂缓，以下保留未实现需求及来源限制，不作为当前LLM采集任务或已支持能力。当前TLS hook设计见下节及[来源契约](flow-binding.md)。

本需求是对MCP stdio的原消息在交付前实施裁决，允许／阻止／问询后仍使用同一执行对象；不是只收集内容、事后告警或杀进程代替原消息阻断。解析与重组仍归独立流量模块，驱动负责真实管道操作、字节来源和原操作保管，主模块只组合接口。

**管道访问的明确入口是LSM file_permission**。它收到实际file与MAY_READ／MAY_WRITE；[VFS路径](https://github.com/torvalds/linux/blob/v7.0/fs/read_write.c#L425)在正常读写分派前调用，能据真实文件对象做直接读／写权限裁决。匿名pipe与命名FIFO纳入文件类，不新增第四EventKind、Pipe规则池或MCP规则池。FileData.device／inode／access_mask继续用于事实，真实控制仍依赖原文件／管道对象引用，不能把FD编号或inode当成不可复用的管道能力。

这个hook的[原型](https://github.com/torvalds/linux/blob/v7.0/include/linux/lsm_hook_defs.h#L185)不带消息buffer；Linux v7.0的[sleepable列表](https://github.com/torvalds/linux/blob/v7.0/kernel/bpf/bpf_lsm.c#L325)也不含file_permission。因此它是已确定的直接权限控制点，不能在这里调用KF_SLEEPABLE等待人工，也不能仅凭命中权限规则就宣称按MCP消息内容阻断。文件Read／Write的操作语义见[规则契约](../../backend/rules/technical.md#管道读写与规则类别)。

**字节位置的源码依据**：[anon_pipe_read／anon_pipe_write](https://github.com/torvalds/linux/blob/v7.0/fs/pipe.c#L401)及FIFO包装能看到实际pipe与iov_iter，写入会在pipe互斥区复制／合并到管道buffer；不能持这个锁等待主模块或解析器。fexit/vfs_read／vfs_write发生在操作返回时，只能作为已发生传输的观测参考；fentry能够采样参数也不等于获得可返回拒绝或可等待的执行能力。用户buffer的早期拷贝还必须与最终实际交付字节一致，不能检查一份后由对方修改另一份。

现有[MCPSpy源码](https://github.com/alex-ilgayev/MCPSpy/blob/main/bpf/mcpspy.c)使用fexit观测读写和pipe关联，可参考捕获位置，不能作为成熟HIPS消息阻断实现。其按inode／PID关联、单次syscall筛选和截断假设不进入AS。更多边界见[调研](../references.md#管道与mcp源码依据)。

给流量模块的必要接口信息已经确定：原EventData的进程／容器事实、持有真实引用的管道实例、原方向与顺序、实际字节范围、完整／缺失标记，以及能够保管和最终提交同一字节范围的执行能力。已知进程信息继续用EventHeader／ProcessInfo，权限事实继续用FileData，裁决与期限继续用Decision／OriginalOperation／HoldWindow；不另建McpProcess或把正文塞进共享进程TLV。先前观察／工具反馈见[暂缓记录](../../archive/flow-design/runtime-feedback-paused.md)，当前LLM契约见[Flow设计](../../backend/flow/technical.md)；普通pipe操作即时拦截与完整消息控制不同，未证实的内容控制点仍[存档](../../archive/unresolved-module-contracts.md#管道mcp拦截的待定执行点)。

流量模块按[MCP stdio传输规范](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports#stdio)处理换行分隔的JSON-RPC消息，必须应对跨多次读写、一次读写含多条消息、部分读写及两个方向。不能以JSON开头、FD=0／1或进程名认定MCP；端点角色和协议识别由实际流量会话核实。驱动只提供底层字节与能力，不解释tools/call等高级业务。

当前仍未实现管道消息级拦截。联合文档已撤回受管启动／分段pipe作为通用前提，改为运行时监控与真实拒绝后的结果反馈。普通write/writev可即时阻止，但不能由此宣布完整消息保管／反馈注入已经实现；不把观察作为事前控制，不自动引入seccomp栈或内核补丁。

### 捕获能力与未明确部分

驱动只声明实际执行点的字段、动作和等待能力；不从统一结构推定完整覆盖。当前源码见[目录说明](../../../drivers/linux/README.md)，新增操作与部署缺口见[交付清单](../../backend/gaps.md)。

## 唯一职责

### TLS明文采集（待实现）

被动TLS的旧初始化接口仍属[归档草案](../../archive/flow-design/tls-capture-interface-draft.md)，已有函数与对象寿命调研保留在[来源依据](flow-binding.md#未接入方向的源码依据)；两者都不是当前生产端口，也不提供LSM人工等待或正文修改权。当前受控IPv4／IPv6新连接入口见[来源边界](flow-binding.md)，实际LLM解析和TLS中介属于底层Flow。

### 现有hook职责

DriverHooks捕获实际回调参数及同一现场通过已核内核API取得的真实事实、保持原执行点并执行DriverCore给出的返回结果；同类规则选择和是否保管由DriverCore组合。它不解析规则、读取配置、管理通讯或识别MCP。进程、文件、网络是回调接口与载荷类别，不是三个等待线程。

生命周期使用for_each_kernel_tracepoint查找实际sched入口并以tracepoint_probe_register注册；原型由匹配headers的check_trace_callback_type_sched_process_*编译核实。模块退出解除注册并同步在途回调。目标内核未导出具体__tracepoint对象不作为自造符号查找或BPF不可信task指针传递的理由。

已有Execute／Open／Connect／Send的实际规则效果分别见[文件终止验收](../../evidence/rule-termination-integration.md)与[进程／网络专项](../../evidence/process-network-rule-integration.md)。证据只覆盖列明的平台／协议／场景，不从源码支持推定其他变体已实测。


## Group 回环 TCP 原件

观察范围和跨层结构由 [Group 契约](../../middleware/agent-graph/technical.md#持续采集的来源边界)定义。驱动仅在启用范围内用 IPv4 `LOCAL_OUT` 和原 TCP 收发探针生产事实，原包放行。观察范围随完整初始化采纳，普通策略替换不切换观察范围或在途连接。源内序号先于队列承载分配；采样缺口与实际连接连续性的解释以Group唯一契约为准，不把源序号空洞推定为每条连接缺字节。Group 占用有限的原生事件槽。底层从原鉴权设备读出后直接交 Group 消费者，原裁决及状态继续使用现有队列。

## 原运行时库观察

实际ELF探点的安装、libuv原入口/提交/工作/完成/清理及现syscall scope交接，唯一见[运行时观察契约](../../backend/driver-adapter/runtime-observation-technical.md)，私有资源寿命归[驱动上下文边界](context.md#运行时观察资源的私有寿命)。它是原通道中的只读事实源，不参与HIPS风险裁决，不以工具语义或HTTP解析成功作为原件采集条件。
