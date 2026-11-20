# 回调事件字段与真实来源

本页是事件字段的唯一说明，已实现字段与明确标注的目标字段分开；实际声明与解码见[dispatch_event.rs](../../crates/contracts/as-types/src/dispatch_event.rs)，固定88字节前缀见[event_frame.rs](../../crates/contracts/as-kernel-common/src/event_frame.rs)。字段迭代不增加接口版本、旧字段别名或兼容解析。目标字段不是当前代码供给或联调通过声明。

## 身份与共同区域

`EventData.host_boot_id` 是 Main 从本机进程树取得的实际启动 UUID，附加于已接纳的原生控制与直接结果事件。它独立于原驱动头和 EventKey。入库时与原事件同事务保存，未提供该来源的记录保留缺失。它限定进程寿命和 BOOTTIME 的比较范围。

固定头继续保留来源实例／序号域／序号、操作族和阶段、BOOTTIME时点、原actor宿主TGID／TID、真实组birth及实际cgroup。TID不是TGID，组birth不是调用线程birth；后者由共同TLV25解码为逻辑EventHeader.actor_thread_birth_boottime_ns，不改88字节前缀布局。

共同 TLV26 解码为 `EventHeader.actor_cgroup_lineage: Option<Vec<u64>>`。它由同一原 cgroup 内核对象取得，按根到原 actor 的 cgroup 顺序保存，最后一项等于 `actor_cgroup_id`。每项 LE u64，JSON 用十进制字符串，最多 32 项。原生未取得完整路径时省略，进程树不补写。它用于对已核验的 Docker 规则范围作排除判断；没有该字段时仍保留原未知语义。

共同字段复用ProcessInfo，全部按原字节／整数语义保存：

- 1 name、2 executable、3 argv、4 cwd：name为实际task comm；路径是来源视图原字节，不按basename／UTF-8替换或跨namespace改写。argv保持元素边界、空元素和非UTF-8字节，不拼成shell字符串。
- 5 uid、6 euid、7 gid、8 egid；21 suid、22 fsuid、23 sgid、24 fsgid：分别为真实／有效／保存／文件系统凭据，全部是宿主初始user namespace数字，不能混用。
- 9 parent_pid、10 parent_birth、11 parent_name、12 parent_executable、13 parent_argv：实际物理父线程组的身份／属性，不是creator，也不是目标进程。
- 14 kind为u32：0 User、1 Kernel；15 exec_cookie为u64 self_exec_id。真实0保留Some(0)，不转成缺失。kind和cookie只在ProcessInfo维护，树快照不另存同名顶层副本。
- 16 pid_namespace_inode为active PID namespace，17 net_namespace_inode、18 user_namespace_inode、19 mount_namespace_inode均是实际namespace inode。数字不等于Docker实例身份；不能用pid_ns_for_children代替active pid namespace。/proc namespace链接为空时仅标记该字段Missing，不因此丢弃整个节点；非空格式仍严格校验。
- 20 namespace_tgids：count:u32后接count个u32，从宿主PID namespace到内层的真实TGID向量。当前Linux最多33层项，保留完整层次，不只留下最后一个容器内PID。
- 25调用task的start_boottime：只作为事件／字段来源的线程寿命证据；不能用线程组birth冒充。

瞬时字段只描述该次原回调。原事件在途持有不等于缓存有效对象；消费结束释放内存，审计库只保存原记录，不能将历史IP、TID、参数指针或namespace数字重新解引用、当作当前值或延长原对象有效期。

共同 TLV29 为调用线程的 namespace TID 链：`count:u32` 后接 `count` 个 LE u32，从宿主 PID namespace 到原线程的 active PID namespace，1至33项；每项为真实非零TID，首项等于原头actor_tid。底层唯一字段为 `EventHeader.actor_namespace_tids: Option<Vec<u32>>`。它从同次回调保管的 `get_task_pid(actor, PIDTYPE_PID)` 读取，不由TGID推导、不由进程树后补，也不作为独立身份：匹配还需原boot、线程birth和namespace作用域。字段缺失为None，成功取得的完整链按原序保存。该字段属于本事件actor，不写入child_process公共区；子进程共同信息继续按其既定字段供给。实现与实机状态以本轮验收记录为准。

Native直接供给kind、cookie、PID／NET／USER namespace、TGID向量及保存／FS凭据。当前普通控制hook通过BPF CO-RE在同回调读取current的mount namespace标量，Native填入既有TLV19。生命周期等没有BPF预采值的入口保留缺失；进程树的独立详情查询仍可通过/proc固定链接读取。模块不硬编码私有mnt_namespace布局或调用未导出的from_mnt_ns。

cwd从真实fs/path取得；不能睡眠的回调把path引用交既有释放队列退役，源额度不足留空，不持锁等待底层。读取foreign task argv需要不同的内存上下文，fork回调不拿creator argv当child argv。

## 进程载荷

1 target_executable、2 target_argv、3 child_pid、4 child_birth、5 exit_code、6 child_parent_pid、7 child_parent_birth、8 child_cgroup、9 child_process共同区域。

执行前的目标argv来自原bprm->mm已经复制的参数页，按p／argc逐页有界读取并释放，只有完整向量才输出；不读取env，不拿current旧mm或早期用户指针代替。目标参数和cwd已接入原有规则条件，新增namespace／凭据事实并未自动成为新规则条件。

成功exec观察报告当前新镜像与新argv。树仅对stage=Observation的Execute在公共executable缺失时使用这次真实target_executable；ControlledRequest或Allow回执绝不提升成“已经执行”。

fork的child_process只保存实际child可安全取得的属性，复用共同字段编码；parent9—13不在这个区域重复传输，真实父身份仍只取6／7。子区域的task birth若出现，必须等于该新线程组的child_birth。Rust用Box保管可选child数据，普通文件／网络事件不承担整块子结构的固定大小。

Exit仅在group_dead且共享SIGNAL_GROUP_EXIT状态成立时保留signal.group_exit_code；这是原始组wait状态码，不是最后退出TID的exit_code或用户态推测。消息解码、ProcessRecord.exit_code和查询保留同一值。

## 文件与网络载荷

文件仍为path／new_path、device主次号、inode、open_flags及access_mask；open_flags来自file.f_flags，access_mask始终是FMODE_READ／FMODE_WRITE，不改成当次MAY权限请求或mmap prot。Self路径有file时也保留这些字段；umount使用真实mount root的path／inode。仅有dentry而没有对应mount时不猜绝对旧路径。

`FileData.mount_effect: Option<MountEffect>`只属于Mount原回调。File专属TLV11为LE u32：1=`independent_view`表示该次实际操作只影响独立挂载视图，2=`protected_tree`表示实际保护树或受保护文件系统会受影响，3=`unknown`表示已检查但不能确认影响范围；None只表示来源未采集此事实。判定依据为同回调的实际VFS包含、操作类别、受保护superblock及必要的传播关系，详见[自保对象边界](../drivers/linux/self-protection.md#保护的修改与边界)。该字段是原事实，不代表Allow；不能由路径字符串、进程名或另一个时刻的namespace补填。后端、业务库和前端复用同一枚举，普通规则只在明确支持的Driver Mount直接裁决中消费独立视图条件。

### 普通文件gate字段与操作来源

以下扩展沿用同一 FileData，仅此处定义字段。File 专属载荷的 TLV7／8 各为4字节 LE u32，不占共同区域或其他操作族的同号TLV；缺失不生成TLV，真实0编码为4个零字节。现有六项的含义和编号保持，不建立第二个文件事件结构。源码接通与实际内核加载／保护效果分别认定。

- `rename_flags: Option<u32>`，File TLV7：只保存`path_rename`原`unsigned int flags`，单位为Linux RENAME位集，不是权限mask。存储域0..u32::MAX；当前目标内核解释RENAME_NOREPLACE=1、RENAME_EXCHANGE=2、RENAME_WHITEOUT=4，不能把任意组合或未知位说成合法：EXCHANGE与NOREPLACE／WHITEOUT互斥，实际文件系统、权限和其余VFS检查仍可能拒绝。Some(0)是实际无flag；None表示未取得，其他本轮操作不填。来源是已进入真实hook的请求，不由两路径／目标是否存在推断。消费者是原Rename事件展示、规则日志请求、结构化事件存储和执行解释；本轮不增加flags匹配DSL，也不制造交换后的第二个伪事件。
- `creation_mode: Option<u32>`，File TLV8：保存 CreateDirectory 的 `path_mkdir` 或 CreateFile 的 `inode_create` 原 `umode_t`，u32 承载实际 16 位无符号值，范围 0..65535。保留已由内核处理的 Linux 模式位，Some(0) 为实际 0，None 表示未取得。对象类别来自实际操作。该字段用于原事件、日志、结构化存储和执行解释。

File TLV9 为 24 字节，依次是原 `mmap_file` 的 requested_prot、effective_prot、flags，分别对应 `map_requested_prot`、`map_effective_prot`、`map_flags`，三个 LE u64，单位为各自 Linux 位集，保留 0 与所有原始位。仅 MapSharedWritable 携带，三个字段同时存在或同时缺失。它们不是长度、地址或匹配条件。

File TLV10 为 12 字节。依次为 `truncate_size: Option<u64>` 和 `truncate_attributes: Option<u32>`，原 `iattr.ia_size` 非负目标字节数与 `ia_valid` 属性位图，均为 LE。只允许用于 TruncateOpenFile，两字段同时存在或同时缺失。早期 file_truncate 自保通知没有 iattr，这两字段为空。

以上参数只可由原 hook 提供，不从进程树、cwd、后来的stat或相似操作补成“原值”。TLV与EventData反序列化均拒绝其他操作的rename_flags／creation_mode，creation_mode大于65535也拒绝；其他真实可缺字段仍保持None，不因此拒绝整帧。填缺失不得擦除或覆盖原Some(0)。日志请求携字段不证明已写盘；SQL NULL只对应None，INTEGER 0保留Some(0)，不另存整份JSON封套。

本轮普通规则控制操作及目标字段来源如下，列表说明原hook可供给的事实，不保证每次采样成功；采样失败仍为None，按规则原Missing语义处理。它不限制已有Self专属Link／MoveMount等通知的真实双路径或其他file对象字段。

- Open=1：path、device、inode来自原file对象；open_flags为f_flags，access_mask为下述FMODE值。没有普通new_path、rename_flags或creation_mode条件来源。
- CreateFile=4：具名普通文件创建，path 和 device 来自该次 `path_mknod` 的实际挂载与目标，creation_mode 来自匹配的原 `inode_create`。创建前目标 inode 未存在时留空。早期系统自保拒绝可能只提供该次原路径。
- CreateDirectory=5：path为原目标dentry与真实mount的完整路径，device来自目标文件系统；creation_mode来自原path_mkdir参数。创建前不能用目标inode作普通匹配条件；若原d_inode实际存在，事件仍保留该样本，绝不能借父inode填充。
- Unlink=6、RemoveDirectory=7：path、device、inode来自原被移除对象，没有普通new_path或FMODE条件来源。
- TruncateOpenFile=10：真实原 file、dentry 和目标大小来自 inode_setattr 及真实 iattr。早期 file_truncate 自保拒绝仅携原 file 事实。
- TruncatePath=18：来自 `path_truncate` 的原 path、device、inode。hook 没有请求长度和 file。
- MapSharedWritable=19：来自 `mmap_file` 的原 file 对象，保留 path、device、inode、原文件 flags/access_mask 和 TLV9；access_mask 仍只允许作为 Open 规则条件。
- Rename=9：path与new_path是原双路径；device与inode描述原被改名对象，rename_flags来自原path_rename参数。不能用目标是否存在推flag，也不把旧对象inode冒充新路径对象。

普通规则中的 new_path 只适用 Rename，access_mask 只适用 Open，inode 条件不能包含 CreateFile 或 CreateDirectory。operations 列表是 OR，每项都必须能提供其全部目标条件。flags／mode 只作原事实。

Open的access_mask仍只取FMODE_READ=1／FMODE_WRITE=2；Some(0)不是缺失，FileTarget的非零mask条件仍按全部指定bit包含匹配。后续file_permission的MAY_READ=4／MAY_WRITE=2／MAY_APPEND=8不能混入此字段。O_PATH在VFS进入security_file_open前返回，当前Open链不产生其事件，不能用0或None补造一个已放行结果。

`Write=3` 用于 `file_permission(MAY_WRITE)` 自保事件，文件对象来自该次写入持有的 `struct file`。普通文件写打开使用 `Open=1` 和 `access_mask` 的 `FMODE_WRITE` 位，问询在原打开时完成。写关闭事实由安全日志模块接收并按其独有记录类型保存。[写关闭审计契约](../backend/security-log/technical.md#写关闭审计)

`GrantWritableMapping=13` 用于已有共享映射新增写权限的自保事件。path、device、inode、打开 flags 和 access_mask 取该次原 VMA 所持 `vm_file`。TLV9 仍仅用于首次共享可写映射的 `MapSharedWritable=19`。[驱动执行边界](../drivers/linux/hooks.md#已有共享映射增加写权限)

网络仍保留真实本地／远端IP、端口及IANA协议号；连接前实际端口0仍为0。域名没有来源时None，不由反向DNS／进程名猜出。网络7保留内核socket_cookie，8保留socket自身的net namespace inode；后者不能用actor当前net namespace代替。cookie经公开sock_diag_save_cookie接口取得，不调用未导出的私有函数。

### UDP DNS Send 查询快照

网络9仅用于同次 `Send` 的 UDP／远端53 iterator 快照，定义为 `NetworkData.dns_snapshot: Option<DnsSendSnapshot>`。它是hook内一次读取的查询观察，不冻结用户内存或承诺之后协议栈消费相同字节，不指认后续网站连接。`Connect`、其他传输／端口不生成此字段；没有原件时为None。

TLV9正文为16字节小端头 `requested_bytes:u64 / attempted_bytes:u32 / send_flags:u32`，后跟实际复制的原字节data。requested是hook时 `msg_iter` 的剩余字节数，attempted是这次实际尝试复制、受原事件帧剩余空间约束的字节数；copied只由data.len()派生，满足copied<=attempted<=requested，不另存可变副本。send_flags取同次msghdr。原iterator不推进、不替换，用户内存只读一次。部分／零复制仍保留实际data及数量，不猜errno。requested按现u64十进制字符串序列化，其余数量为u32；data保持原字节。

适配在交主模块前只解释同一自持有data，填 `analysis: Option<DnsQueryAnalysis>`；None只表示尚未解释。枚举使用serde `status`标签：question{qtype:u16,qclass:u16}、response、other_opcode{opcode:u8}、no_question、multiple_questions{count:u16}、not_dns、malformed{reason:String}、short_read、capture_limited。短读先于帧前缀不足判定；完整但不足DNS固定头为not_dns。响应、非Query、零／多问题和解析失败均不丢原件或伪造成一条成功Query。

仅完整复制且解析出单个标准Query问题时，原问题名以成熟DNS解析器的转义显示格式填既有 `target_domain`（保大小写；根名成功空值保持Some空字节，不改None），QTYPE／QCLASS保原u16。原wire只在data保存。DNS域名规则只匹配这类Send查询快照，其他状态／无此来源明确不属于该规则，不把普通网络操作的缺Domain升级为NeedsFacts／Cancel。比较遵循DNS的ASCII大小写不敏感语义，原字段不折叠。规则原Allow／Deny／Ask及原操作等待链不变，快照不构成抵御并发改写的保证。

能力由窄 `DriverFeature::DnsQuerySnapshot=1024`声明，不启用Native条件25、不支持Driver域名匹配或Connect域名。现行Driver规则优先级不变；UDP53 Driver Allow若先命中，后端DNS规则不会执行。需要询问的正式数据必须通过原规则owner明确分流ToUser，不能在hook隐藏绕过。

本页没有定义任意内核内存、环境、xattr值或未设计操作的通用转储字段；DNS仅为上述一次Send快照。原事件主体不明确的I/O worker自保通知只报告已确认对象与结果，不把worker PID冒充原用户发起者。

## 原操作执行现场

`EventData` 直接承载 `syscall: Option<SyscallSnapshot>` 与 `kernel_stack: Option<StackSnapshot>`，不进入 `ProcessInfo`、进程树或第二条通知队列。None 只表示来源未提供该项，真实零值及成功取得的空栈原样保留。主模块不能补写或覆盖这些瞬时字段；在途事件及其日志消费者沿现有拥有者交接，消费结束释放。[代码与联调证据](../evidence/execution-site-capture.md)

`EventData.user_stack: Option<UserStackSnapshot>` 复用原调用事实路径，不进入进程树。`UserStackSnapshot { result: i32, snapshot: StackSnapshot }` 保留 BPF `bpf_get_stack(..., BPF_F_USER_STACK)` 的实际返回值：非负表示已复制字节数，必须等于原IP数组长度乘8，最多256；负值保留原负errno（-4095至-1），此时IP数组为空且buffer_full=false。成功0保留Some且空IP数组，不改成None；None表示本回调没有提供这项。StackSnapshot沿用有序原u64地址及32槽buffer_full含义，不作符号化、栈形状绑定或后读目标内存。

共同TLV30依次为LE i32 result、LE u32 flags和成功返回的LE u64 IP序列；flags只允许bit0=buffer_full，错误时为0，成功时恰在32槽用满时为1。它不进入child_process公共区。BPF在原普通hook内对规则候选作只读采集预检查，决定是否调用栈helper；安全裁决仍由原gate持有的实际规则视图完成。有候选但最终命中静默快速裁决时，本次用户栈可能已经读取，随后随hook临时值释放且不上报；无候选或关闭时不读取。预检查之后规则并发更新可能使该原事件没有用户栈，仍保留缺失，不补取、不改裁决。该预检查不提前运行匹配或引入第二份状态。

`SyscallSnapshot` 保存同一原调用的用户寄存器视图：`audit_arch: u32` 是 Linux `syscall_get_arch` 返回的 AUDIT_ARCH 编码；`number: i32` 是 `syscall_get_nr` 的实际值，-1 保留为原值、不宣称正在执行有效系统调用；`instruction_pointer: u64`、`stack_pointer: u64` 为 `task_pt_regs(current)` 中的用户 PC/SP；`arguments: Vec<u64>` 恰好六项，按 `syscall_get_arguments` 的原顺序保存原位模式，不解引用指针、不把数值猜成文件句柄或对象身份。u64 字段的 JSON 均用规范十进制字符串，避免前端精度丢失。只有当前非内核线程且原寄存器为用户模式时提供该结构。其余情况不补零快照。

共同 TLV27 为固定72字节：LE u32 audit_arch、LE i32 number、LE u64 PC、LE u64 SP、六个 LE u64 参数。两端读取同一当前格式，不加版本或兼容别名。

`StackSnapshot` 保存 Linux `stack_trace_save` 在原hook内返回的内核帧 IP 序列 `instruction_pointers: Vec<u64>` 和 `buffer_full: bool`。最多32项；顺序、重复项和零值不改写。buffer_full只表示本次32槽缓冲用满，不证明截断；false也不保证调用栈完整。共同 TLV28 为 LE u32 flags（仅bit0表示buffer_full）、LE u32 count，随后 count 个 LE u64 IP；count<=32，长度严格一致，bit0与count==32一致。未提供与返回零项分别表示为None和Some(空栈,false)。这些是当次现场数值，不是可回读地址、跨事件恒定身份或固定栈形状的绑定规则。

采集使用内核已公开的调用现场与栈接口，在普通驱动快速匹配完成后、确实需要上报的原 Controlled／Direct 事件中执行，且在原操作等待／终止动作之前。无候选默认放行、不需要报告的快速命中和现有生命周期／自保通知不增加栈展开。为同一原记录预留一次有界字节空间，不扩大原队列、不添加容量失败后的旧路径。用户态完整语言栈、同步对象追踪和业务多条件模式不由这两项字段冒充。

依据：[Linux ARM64 syscall接口](https://github.com/torvalds/linux/blob/v7.0/arch/arm64/include/asm/syscall.h)、[当前线程内核栈接口](https://github.com/torvalds/linux/blob/v7.0/kernel/stacktrace.c)。`stack_trace_save_user` 在当前模块导出表不可用，不绕过导出限制或复制一套运行时栈解析器。该批不依赖Node、Agent私有字段或SDK。

## 树的保留与补全

initial固定首次接受视图；latest按字段合并，所有新字段有独立来源／采样窗口。原值不被补充覆盖。父名称／镜像／argv只从同一已核父实例、有效且不晚于事件的缓存补入，并保留父样本原stamp；不将后来父属性倒填成初始历史。

回调来源保留原EventKey、TID和task birth。comm、cwd、凭据及NET／USER／MOUNT namespace等线程属性只可补给同TID且同task birth的事件；/proc扫描的来源是组长，不能拿它的属性补另一个线程。线程组共享的argv／镜像及其exec连续性仍按组身份与时序处理。

成功读取到空命令行保留为空值，不是null：字符串表示为""，本项目argv数组为Some([])；单个NUL则保留一个空参数Some([[]])。读取失败才是None。procfs空读不证明原操作argc=0，因此这种缓存空值不补成原事件的完整零参数事实；查询仍保留已获取的空值，不为它重复补取。

新增命名空间字段不会自动填Workload::Container；Docker实例映射仍须由真实运行时来源负责。所有详细数据通过现有鉴权查询／事件入口提供，不新增可写进程树或另一个Owner。

## 原生系统调用请求返回

`NativeRequestReturn` 是附于原 `EventKey` 的独立观察。它保留同一原生会话的 `wait_id` 和对应 `DecisionId`，以及外层 syscall 包装函数返回时的 BOOTTIME 纳秒和带符号原返回值。`operation` 记录实际受控 gate 的类别。0x101 仍报告原检查点的裁决结果。[帧定义](driver-communication/technical.md#6-消息类型与完成条件)

| operation | 对应的原生包装函数 | 返回值含义 |
| --- | --- | --- |
| `file_open` | `openat`、`openat2` | 文件描述符或负错误 |
| `execute` | `execve`、`execveat` | 包装函数结果，目标映像的效果独立观察 |
| `connect` | `connect` | 包装函数结果，保留 `-EINPROGRESS` 等实际返回 |
| `send` | `sendto`、`sendmsg`、socket发送中的`write`／`writev`、`sendmmsg` | 原包装函数带符号返回；前四项为接受字节数，sendmmsg为接受消息数，失败均保留原负错误 |

驱动把类别相符的唯一已接纳等待绑定在当前 syscall 探点自己的在途对象上。退出回调消费该记录，任务退出清理索引。会话变化、探点缺失、同类别多个受控等待或资源不足均可能缺少观察。该事实证明原函数包含该次驱动等待，并保留函数返回值。实际数据送达、应用同步对象和工具身份各有独立来源。

write／writev探点只有在原调用实际命中Network.Send gate并接纳等待后才发布该观察；普通文件／管道写入不因此新增File.Write Ask或发送事实。批量sendmmsg的多个已接纳等待继续按现有multiple语义报告原返回缺失，不把整批结果归给其中一个事件。返回单位按原EventKey所指事件的syscall编号解释，不把消息数冒充字节数。

sendmmsg在先前消息已成功、后续消息失败时可能返回正的成功前缀消息数。保留该实际返回，不能由某个gate的Deny反推整个syscall必定返回负错误。

## Private futex 唤醒选择

`NativeFutexSelection` 保存一次由原生 `FUTEX_WAKE_PRIVATE` 进入的实际选择。独立 `EventKey` 标识原观察，`waker` 和 `waiter` 分别保存采集时真实宿主 TGID、TID 和该线程的 BOOTTIME 出生纳秒。`selected_boottime_ns` 是实际 `wake_q_add_safe` 回调时间。该记录表达原选择和目标，线程实际恢复和 syscall 返回继续由各自来源确认。

`wait` 存在时携带采集器为该次队列条目寿命分配的唯一编号和 `futex_do_wait` 观察时间。驱动在 `futex_wake_mark` 进入且原条目仍存活时保留同一条目的等待证据，再核对实际交给唤醒队列的线程实例。缺等待观察、匹配歧义或线程不同时，`wait` 为 `None`，保留该条选择为候选。地址限于活跃回调索引，退出后清除。输出沿既有消息、业务库和活动查询，工具或 Agent 归属仍需自身证据。[线编码](driver-communication/technical.md#6-消息类型与完成条件)


## 原生库请求的原始交接

驱动可在已授权范围内保留安全取得的原始情报供后续业务扩展；当前业务是否已经使用某项事实不是采集或保留条件。原字段须有真实来源、采集时点、作用域／有效寿命和读取结果，业务按公共接口按需消费。原始采集与工具语义解释分开，未证明的关系仍如实缺失；不为未来用途延长瞬时对象寿命或回读已失效目标。

`NativeRequestHandoff` 保留本次回调实际得到的请求、提交、工作、完成及cleanup原值；定义唯一见[runtime_observation.rs](../../crates/contracts/as-types/src/runtime_observation.rs)，字节及寿命约定见[原运行时观察源](../backend/driver-adapter/runtime-observation-technical.md)。它复用原EventKey和线程实例，不把本源的observation/request编号当成LSM事件或ToolCall。只有实际EventBound载荷中的event指向原驱动等待；缺该交接的记录仍保存，不凭时间、地址相似或候选数量制造关系。

原始pathname以取得的字节前缀和实际读取终止状态一起移交，完整空值、NULL读取失败和截断分别保留。记录只沿既有DriverEvent、MainMessage及业务日志消费，不进入进程树或历史缓存。工具事实与HTTP关联不是原始记录的采集或入库条件；各自证据独立确认。
