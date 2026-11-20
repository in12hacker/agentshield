# 进程树数据与接口

产品语义见[设计](design.md)，所有者和线程见[架构](architecture.md)，公共上下文见[MainContext](../../contracts/main-context.md)。本页是进程树跨模块结构、查询和补全语义的唯一出处。ProcessTree及扫描、补全、刷新、查询已按本页实现。公开只读输出在as-process-tree，跨模块身份、设置、查询输入及原生事实端口在as-types；私有索引和OS读取留在模块内部。

## 1. 进程身份和共用字段

```text
ProcessKey { boot_id: [u8;16], host_tgid: u32, birth_boottime_ns: u64 }
EventKey { source_instance: [u8;16], sequence_domain: u64, sequence: u64 }
ProcessKind = User | Kernel
Origin = StartupScan | DriverEvent { event: EventKey, actor_tid: Option<u32>,
    actor_thread_birth_boottime_ns: Option<u64> } | Refresh
SampleWindow { begin_boottime_ns: u64, end_boottime_ns: u64 }
```

ProcessKey使用当前事件头中的TGID和真实组birth，再绑定本次启动已核实的boot_id；驱动重连不改变进程身份。TID只说明触发事件的线程；不能把裸PID、名称、容器名或驱动source_instance当进程身份。缺少强身份时可返回未关联事件事实，不能把它合入同PID节点。

受控 TLS 接入同样使用这次进程树已读取的 boot_id。Main 在进程树prepare取得该值后按原值转交给驱动适配；prepare不表示观察或全量扫描完成。

共同字段、编码和实际来源只按[回调字段](../../contracts/events.md)。公共属性复用现有[ProcessInfo](../../../crates/contracts/as-types/src/dispatch_event.rs)，不再设计同义的进程名／路径／argv／UID字段；父身份从既有parent_pid＋parent_birth组合。ProcessInfo中的父名称等是当前进程这次观察到的父信息，不是父节点的可写别名。cgroup复用事件头的真实cgroup ID；Workload／ContainerInstance复用[既有类型](../../../crates/contracts/as-types/src/workload.rs)，事件的 Workload 来自适配对原事件 cgroup 的核对。扫描以同次原生 cgroup 样本调用事实端口；无有效绑定保持 None。规则匹配不从进程树的历史 Workload 回填当次事件。

/proc/stat的starttime以时钟tick计，不能乘常量伪造当前驱动纳秒birth。Linux读取器先经窄查询取得真实ProcessKey，再打开/proc/<pid>目录相对读取，最后核对相同ProcessKey；两次之间发生PID复用则整份候选丢弃。扫描目录FD、可用的pidfd只由读取任务短期保管，不把每个进程永久钉一个FD，不传到中间层。pidfd本身不含本项目要求的全部纳秒出生事实。

## 2. 两份属性视图

```text
FieldState = Known | Missing | Inaccessible | Truncated | Stale
FieldStamp { evidence_epoch: u64, origin: Origin, window: SampleWindow, state: FieldState }
ProcessSnapshot {
    process: ProcessInfo,
    cgroup_id: Option<u64>,
    leader_state: Option<u8>,
    stamps: 每个已定义字段的FieldStamp
}
Lifecycle = Present | ExitObserved | NoLongerPresent
ProcessRecord {
    key: ProcessKey,
    initial: Arc<ProcessSnapshot>,
    latest: Arc<ProcessSnapshot>,
    lifecycle: Lifecycle,
    first_observed: SampleWindow,
    exit_observed: Option<SampleWindow>,
    exit_code: Option<i32>,
    creator: Option<ProcessKey>,
    revision: u64
}
```

stamps采用与固定字段枚举一一对应的结构，不是开放key/value扩展包。None仅表示未获取到该字段值；Missing等状态解释其原因，不能另存一份不同的值。leader_state只描述/proc所见的组长状态，组长Z或退出不等于整个线程组退出。NoLongerPresent表示重新核对原实例不存在，不能冒称捕获到了Exit或知道退出码。

initial在节点首次建立时冻结，允许有None；晚到历史事件不改写它，也不声称它是最早内核事件。latest按字段来源时序合并：缺省字段不是主动删除，不能抹掉仍有效的值；明确失效字段置None/Stale。采样区间重叠且值冲突时标记待核对，不能按入队顺序猜真值。exec_cookie是同ProcessKey内内核self_exec_id的只读样本，用于发现跨exec拼接；不作为新的进程键或接口版本。

收到exec成功时清除旧镜像的路径、参数及其他不再有有效依据的字段，再合入实际成功事件；只在成功Observation公共镜像缺失时采用该事件的真实target_executable；不从Execute请求或Allow结果直接推定当前镜像。argv/cwd和路径可能不经过exec就改变，不能仅因exec_cookie没变而保证它们未变。

父关系只存initial/latest中的实际父身份，查询投影为`ParentLink = Root | Known { key: ProcessKey, node_present: bool } | Unresolved { observed_pid: Option<u32> }`。只有来源明确报告根关系才用Root；缺字段不能当无父。PID存在而birth缺失时保持Unresolved；已知父实例但节点未到达／已回收为Known且node_present=false。PID 0的系统根关系是虚拟分组，不构造一个普通ProcessKey，也不把每CPU idle task当/proc进程节点。

私有children索引是实际关系的派生索引，不再有一份可独立编辑的parent字段。创建者只从Create的actor取得，孩子及其实际父身份取ProcessData的child字段；不能把创建者的父身份当孩子的父身份。

## 3. 内存布局与复杂度

采用标准HashMap按ProcessKey定位节点，另有当前PID→Key索引、父Key→子Key集合和待关联父Key索引。查询分页用按Key排序的BTreeSet索引；不手写红黑树、不复制一张完整可写树。父子只持Key，不互持强Arc，避免环和退役泄漏。

主查找预期O(1)，节点创建／删除维护有序索引O(log N)，直接子查询O(k)，祖先查询O(depth)，子树遍历／排序O(k log k)并直接按所得Key集合分页。遍历使用显式栈和visited保护，拒绝自父及循环关系；不递归吃调用栈。同值的新确认也更新来源窗口及有效性；kind／exec_cookie已统一到process，命名空间及保存／FS凭据同样逐字段保留；父属性进入快照并保留原stamp，不只临时补到事件。当前实现用不可变latest视图发布，读方共享Arc，初始快照始终不变，不保留通用版本库。

退出记录按保留期和内存预算回收；串行扫描期间保留退役证据，防止晚到采样复活退出节点；原事件仍在途时按ProcessKey钉住记录，ExitObserved还须真实源栅栏越过其源时点才可回收。仍被查询引用的不可变快照由Arc自然释放，模块将其计入占用，不能绕过预算无限挂快照。存活进程不因LRU淘汰；不能为省空间把活节点静默删掉。达到预算先清理可退役记录，仍不足则明确ResourceLimit和完整性状态；启动时不能以残缺清单返回Ready。父节点回收后子节点保留原父Key并显示未解析，不重挂到另一个同PID节点。

## 4. 启动扫描与事件合并

扫描只读宿主/proc的数字进程目录，不使用ps文本输出、递归每个线程、读取环境变量或任意远程内存。先验证真实procfs、宿主PID命名空间及可见范围；默认目录藏进程、容器内局部挂载或权限使枚举不完整时返回明确错误。内核线程可以缺少exe/argv/cwd，这是缺失属性，不是缺失节点。

扫描过程先读取真实进程绑定，再取得stat/status/comm、exe、cmdline、cwd、ns/{pid,net,user,mnt}及实际父信息；每条候选携带采样区间，结束再核对绑定与exec_cookie。读取过程中退出是正常竞争，不能让一次ENOENT导致整个初始化无限重来；具体候选撤销，生命周期或最终核对确定节点状态。权限错误与退出分开，不把所有读取失败当死亡。

扫描区间用前后两次原生核对的源BOOTTIME时点包围，不将用户时间命名空间的时钟值或/proc tick直接与驱动纳秒比较；父实例绑定以原生parent PID／birth为准。任一核对跨越进程或镜像变化都不能把两边字段拼成一个稳定样本。

观察先开、扫描后做、栅栏最后收尾。扫描期间快线程已经维护候选树，扫描结果逐批合并，不整树覆盖回调结果。Exit对同一ProcessKey保持终态，晚到Create或旧扫描不能复活它；exec和其他字段按原时点／有效性合并。结束栅栏必须覆盖已经进入采集但尚未入队的回调，不能仅在读到当前最大序号时宣布完整。

当前EventHeader.sequence用于身份，不保证因果顺序；当前dropped还混合不同事件类别。现有事实端口提供独立事实丢失代和真正排空标记。序号不连续本身不证明进程事件丢失。队列溢出、事实丢失代变化或断线时记录Uncertain，进行一次受控核对；核对不能追回已丢失的历史瞬态进程和最初字段，必须保留历史缺口标记。

## 5. 原生事实的窄协作接口

此窄接口只为扫描、身份核对和流衔接，不让驱动维护用户态进程树，也不复制配置／规则体系。接口由主模块注入的共享契约定义，底层驱动适配在现有鉴权会话实现：

```text
ProcessTrackingStart { host_pid_namespace_inode, tracking_epoch, begin_marker, loss_epoch, backend_rejected, cutoff_boottime_ns }
ProcessBinding { host_tgid, birth_boottime_ns, parent_pid, parent_birth_boottime_ns,
                 kind, exec_cookie,
                 cgroup_id: Option<u64>, window: SampleWindow }
ProcessFactsBarrier { host_pid_namespace_inode, tracking_epoch, marker, loss_epoch, backend_rejected, cutoff_boottime_ns }
ProcessFactsPort::inspect(host_tgids) -> 每项Binding | Gone | Inaccessible
ProcessFactsPort::facts_barrier() -> ProcessFactsBarrier
```

主模块调用驱动开始观察并获得ProcessTrackingStart，之后进程树只取得inspect/barrier窄端口，无规则发布、停驱动或任意内核读取权。boot_id由进程树从已核实的宿主procfs读取，和驱动实际PID命名空间共同核对整机范围；原生只传现有TGID／birth含义，用户态组合ProcessKey，不要求内核模块再读/proc或编造boot_id。inspect在短RCU/锁范围内取得真实task引用和字段后释放；仅返回固定定义的进程事实。模块可复用当前内核已导出的find_pid_ns/get_pid_task等标准接口，不依赖未导出的pidfd私有函数或自行复制内核结构布局。

同一连接先接好事实发送队列，Initialize不能替换掉它。Start／Barrier／Inspect使用现有请求封套与关联ID；分页或批次不得阻塞原操作回复。具体消息编号和固定字节布局只在驱动通信文档及as_protocol.h定义，不并存新旧协议。当前源端已通过同一已鉴权会话接入；唯一固定LE消息定义见驱动通信文档。

基础生命周期已覆盖内核线程组，普通可裁决操作仍保留原actor与上下文保护检查。字段能取才填；创建／退出等事实不制造控制能力。父重定向、凭据、cwd、argv没有当前完整回调覆盖，由明确刷新维护最后观察值，不新造未经确认的hook。

冷恢复时，系统扫描还需要读取已绑定procfs的上述固定信息入口及boot_id；该访问归真实AS实例的维护路径设计，不能通过配置一条/proc/**普通Allow绕过。驱动只核对实际进程／procfs对象与固定读类型，不给任意文件或其他进程维护权限。固定procfs维护读取已接入真实AS实例授权，不下发泛/proc普通允许规则。

## 6. 补全与现有预留

现有FastFacts必须先接收原EventData，维护原事件事实，再查询同一ProcessKey和已确认父Key，从内存返回可用于该事件的ProcessInfo补充。主模块统一调用fill_missing，只填None。补充过程不获得修改事件头、文件／网络目标或驱动已有属性的权限。

快补全采用明确的最近确认缓存语义：ProcessKey一致、字段Known、采样结束不晚于原事件时点，且没有已知exec／父关系／凭据变化或来源缺口使其失效时，返回对应缓存值。名称、cwd、凭据及NET／USER／MOUNT namespace等线程属性还必须同TID和同task birth，扫描仅代表组长；不能跨线程或TID复用补全。可以补名称、路径、参数、cwd、凭据和已核实父信息，不把一切易变属性都拒之门外。该值是最近确认的上下文，不证明采样后操作系统绝无变化；未捕获变化的时延由刷新周期和查询时间标记如实表达。

新exec之后不能补旧镜像，已知的父或凭据变化立即使相关旧字段失效；父信息必须绑定该事件的真实父实例。采样区间与事件重叠、事件时间缺失或缓存只有事件之后的版本时，不猜时间先后。仅同PID不足以关联，来源缺口期间失效的缓存不能用于授予规则条件。初始化／刷新得到的新值仍可供详情查询；原事件无法从只保留的两份视图得到有效补充时保持None。确认缺口通过已有NeedsFacts进入慢阶段；无交互渠道仍不进行慢I/O。

快速入口只从内存取得事件缺失且通过原进程实例、采样时间和字段有效性检查的属性。读锁内取得同一节点的不可变快照引用和对应证据代，释放读锁后筛选并复制实际要补的字段，再向主模块移交。物理父进程身份有冲突时不补父属性。ProcessTree 只接入 FastFacts 与原查询接口。实际系统刷新由消息职责中的显式 Refresh／周期任务执行。主模块保留 SlowFacts 接口，hash／DB 等慢字段待设计明确后由所属实现接入。

主循环排队期间已经结束的原操作仍要更新快速事实，即使没有transfer_hit；取消回执等待也不取消生命周期事实维护。驱动直接结果和Observation都经过同一事实模块，但不再过第二轮规则或生成OriginalOperation。

当前实际接线：MainAssembly先构造树和回调消费者，再start_process_tracking、initialize全系统扫描／消费栅栏，最后采用正式驱动镜像。驱动直接结果和Observation统一维护事实；source_gap在实际断流／投递失败时立即失效缓存，原生进程事实loss_epoch变化仍由原fence／scan检查。辅助事实的正常背压按[适配唯一投递契约](../driver-adapter/technical.md#必要结果接纳与普通观测)保管，不增加backend_rejected或伪造进程事实缺失。source_gap本身不启动扫描，刷新仍归既有显式／周期任务。Work保管finish_event到所有命中处理结束。缓存自身失败只标记Uncertain并返回空补充，保留原始字段的快规则；不会因为树记录失败另造拦截门。

精确已闭合事件重放不再累计。真正无法归属的迟到项标不完整。去重闭合只依据Native翻桶时固定的cutoff_boottime_ns与Main快速事实消费ACK；保留仍在途键，不按marker ID或最大sequence猜测。Native桶覆盖所有产生EventData的采集，Controlled入队后即释放，不等待人工答复。

## 7. 对外接口和查询结构

```text
ProcessTree::prepare(settings: ProcessTreeSettings) -> Result<Arc<ProcessTree>>
initialize(start: ProcessTrackingStart, source: Arc<dyn ProcessFactsPort>) -> Result<ScanReceipt>
FastFacts::prepare(&EventData) -> Result<ProcessInfo>
FastFacts::record_hit(&EventData, &RuleHit)
get_cached_process(ProcessKey) -> Result<ProcessDetails>  // 同步、仅缓存
get_process(ProcessKey) -> async Result<ProcessDetails>  // 缺失时一次定向补取
resolve_current_pid(u32) -> Result<Option<ProcessKey>>
query(ProcessQuery::{Status,Get,ResolvePid,List,Children,Ancestors,Subtree,Hits}) -> async Result<QueryReply>
refresh(RefreshTarget::Process(ProcessKey) | System) -> async Result<ScanReceipt>
source_gap()
finish_event(EventKey)
status() -> TreeStatus
close() -> async Result
```

prepare读取宿主boot_id并分配模块所有权，不启动观察、不代表初始化完成；initialize成功才返回带采样范围、覆盖范围和栅栏的ScanReceipt。MainContext不增加可变树或当前PID。共享参数位于as-types，公开结果通过模块接口传递，私有索引／Linux读取器留在进程树crate，不让消费者导入其内部map。

ProcessDetails包含ProcessRecord、两份父关系投影和已确认子节点数量。命中摘要通过 `ProcessQuery::Hits` 按同一 `ProcessKey` 分页查询。不存在的节点返回NotFound，详情补取只处理已存在且仍Present的相同实例，查询不能生成假节点或追踪同PID后继者。ProcessFilter只包含名称／路径字节精确匹配、生命周期、进程类型及已确认Workload范围，不另实现HIPS规则编辑器。resolve_current_pid只返回当前缓存中Present实例的查找提示，不承诺操作系统此刻仍存活；后续查询使用返回的ProcessKey，操作控制仍由OriginalOperation保管。已退出历史实例按完整Key或列表过滤查询。

PageRequest包含limit和不透明cursor。默认查询是实时keyset分页，cursor绑定树实例、采集世代、查询种类、过滤条件及最后Key，不因无关进程更新就返回Conflict让人无法翻页。跨实例／重建世代返回CursorExpired；每页携带自己的tree_revision，查询期间新建／退出／重挂节点可能改变后续页，不能拼成一份全局原子快照。节点Key不复用，可稳定去重；调用者可明确重新刷新，不自动无限重试。冻结全树导出不是本轮查询承诺，不为它增加多版本库或长持读锁。

ProcessPage返回records、next_cursor、tree_revision、TreeStatus；路径查询遇到缺父返回已知路径和missing_parent，不接到另一个同名PID。TreeStatus含Building/Ready/Uncertain/Closed、当前节点数、退役数、事实流标记、扫描范围和历史缺口；Ready只表示本轮覆盖已确认，不保证任意易变字段即时正确。

下行查询经现有消息线程首次鉴权及ReadOnly授权，刷新需ManageService；消息层将这些类型转给同一模块，不直接读/proc。详细argv和路径只对已授权端可见，不抄入普通状态广播；不采集环境变量。当前Down::Processes通过ProcessCommand::Query／Refresh调用同一模块；不存在节点、游标失效、资源不足等返回明确失败，不能用空成功结果占位。

## 8. 命中归集、修复和资源

规则命中键为ProcessKey＋EventKind＋RuleSourceVersion＋组ID/号＋规则ID/号；每键累计命中数和首末观察点。重复键为EventKey＋该规则键；ToUser规则和后续底层规则各算自己的真实命中，同一命中被Completed和RuleLog再次看到不重复计数。log不参与计数键。

去重表有界，维护经真实事实栅栏确认的源时间窗口；已退役窗口的旧事件不重新计数，无法确认的迟到项标记统计不完整。仍在途的原操作对应记录保留到其处理结束，由主模块已有Work寿命保管并通知完成。

直决计数复用同一个命中键和 `RuleHitSummary`。已鉴权会话的[直决计数批](../../contracts/driver-communication/technical.md#驱动直决计数)沿主模块快处理FIFO消费，合并到已确认的原进程节点。直接结果只负责日志、回执和事件事实。已交付的受控移交与后端规则保留原EventKey去重。计数增量消费后释放；规则版本变化保留各自条目。缺少进程节点、计数资源不足、真实传输丢失或跟踪起点未确认时，查询如实标记不完整。计数只覆盖当前会话启用后的已消费事实。丢失数以最后处理的事实栅栏为准。首末为实际事件序号上下界，不能反推出区间内其他命中。

现有 `RuleHitSummary` 是实际观察到的累计命中。重复询问后的「记住」提示需要同一进程实例和询问规则版本在指定窗口内的去重问询数，首末事件与累计次数不足以推算该值。实际问询沿既有原问询身份确认，静默命中不参与此阈值。窗口、阈值及规则适用范围确定后，再设计有界近期统计，复用已有进程和规则身份。

ProcessTreeSettings只定义memory_budget_bytes、retired_retention_ms、reconcile_interval_ms、query_page_size，正整数、按运行时容量检查。配置读取／保存仍归配置模块；进程树只接收校验后的设置。预算覆盖节点、待关联项、扫描候选、去重及查询快照；当前安装配置为256MiB存储计量预算、5分钟退役保留、30秒系统核对和128条查询页；这些是明确资源策略，不是实测吞吐上限。存储计量覆盖快照数据和保守索引开销，不冒称进程RSS硬上限；快照Arc仍被查询引用时其计量继续保留。核对周期只决定调度，不兼作字段TTL。

当前只允许一个已拥有的刷新任务；并发显式请求返回Conflict，周期遇忙跳过，不制造源缺口。同目标请求共享结果的自动合并尚未实现；全系统扫描不在每个事件上重复触发。退出码、历史初始信息和丢失命中无法追回时保持未知。发生SourceUnavailable时继续显示带不确定标记的数据，不能把树错误转换成规则Allow；关闭中的工作实际收尾后释放资源。

## 9. 成熟依据及核验重点

- [Falco proc扫描源码](https://github.com/falcosecurity/libs/blob/master/userspace/libscap/linux/scap_procs.c)将系统读取与事件解析分开；[thread_manager](https://github.com/falcosecurity/libs/blob/master/userspace/libsinsp/thread_manager.cpp)维护索引、线程组及扫描后依赖。借鉴这些职责，不迁入插件框架、全部线程/FD/环境表或用裸PID满足AS实例身份要求。
- [Linux procfs文档](https://docs.kernel.org/filesystems/proc.html)说明进程信息读取接口；[stat手册](https://man7.org/linux/man-pages/man5/proc_pid_stat.5.html)规定starttime的tick单位；[pidfd手册](https://man7.org/linux/man-pages/man2/pidfd_open.2.html)说明真实任务引用和资源错误。以上不能证明一次/proc读取是全系统原子快照。
- [Rust标准集合](https://doc.rust-lang.org/std/collections/index.html)提供HashMap查找及BTree有序范围，使用成熟实现；内存/查询复杂度是设计性质，吞吐需实际实现后测量。

验证聚焦实质反例：扫描期间创建/exec/退出、PID复用和非组长exec、父重定向及晚到父节点、已知字段不被补充覆盖、迟到慢结果不污染原事件、丢失后的明确不确定状态、退役节点／查询取消无泄漏。使用真实小进程验证必要路径，不以测试数量或全新矩阵代替模块完成。

## 10. 成熟组件取舍与模块纯洁性

[sysinfo](https://docs.rs/sysinfo/latest/sysinfo/)管理自己的系统缓存，它不能替代驱动的纳秒实例绑定和事实栅栏。[procfs](https://docs.rs/procfs/latest/procfs/process/struct.Process.html)的目录FD绑定方式已采用同样原则，但本模块只读已定义的固定procfs字段和对象链接，保留原始字节及字段失败状态，没有引入完整系统解析库。[indextree](https://docs.rs/indextree/latest/indextree/)提供arena节点ID，接入后仍需ProcessKey映射、两份属性和缺父关系，不能减少本模块必要状态。

因此使用现有成熟标准集合与Tokio TaskTracker；工程接口保持一个状态所有者。规则匹配、动作执行、日志落盘、通信鉴权和配置写入都不进入进程树。未来替换存储容器可在store内部完成，不让框架身份泄露为外部进程身份，不保留双轨管理。

数据完整性缺口推进单一evidence_epoch，字段补全只用当前证据代Known值。系统重扫不会重新启用缺口前、未经重新确认的父路径／参数／容器字段；initial仍保留当时历史。查询返回当前证据代，旧latest中的历史样本不能被当作当前规则输入。

同一实例的initialize只接受首次源绑定，重复调用或关闭后调用明确失败，不替换原端口。刷新重叠是接纳冲突，不是事实丢失；周期遇忙跳过，不推进证据代。当前没有同目标刷新结果合并或运行中热重绑定实现，生命周期重建由主模块完整重新启动处理。

## 11. 详情查询的单次缺失补取

同步get_cached_process只读取缓存；异步get_process先读同一缓存，仅对已有存活节点尚未获取、且有/proc或Inspect实际来源的详情字段尝试一次定向读取。ProcessQuery::Get走该异步接口；列表／父子／祖先等查询不递归触发补取，FastFacts仍无I/O。Workload等没有真实提供者的字段不触发无效读取，退出节点不读取同PID的新实例。

补取复用现有扫描锁和TaskTracker，不新增线程、服务或第二份事实缓存。并发请求在取得处理机会后重查缓存和该节点的尝试状态，已经读取过的空值、失败结果也缓存；同一进程／镜像不机械重试。已经由启动／全系统扫描实际尝试的同代字段也记入这份节点状态，详情查询不重复同一次失败读取。真实exec或显式刷新才重新开放尝试；源恢复沿既有系统扫描或新树初始化完成，不能把任何一次读取失败推进的全局证据代当作重试许可。

返回值始终是当前实际缓存详情，不能把已接纳补取称为取得所有字段。读取失败后保留已有真实数据及原来源状态，字段None仍表示未取得。完整初始快照不因动态补取改写，成功的新事实只更新latest；新读取不能倒填为原事件发生时的事实。

子树查询沿现有children索引遍历，仅按实际访问／排队的ProcessKey增长同一查询预算，不为无关全系统节点预留。子Key分批取出后释放读锁，再协作让步；分页同样在批次间让步。取消纯缓存查询会释放临时集合及Charge，既不增加源I/O也不创建后台扫描／Owner。查询仍是带revision的实时弱一致视图，不因加入取消点承诺冻结全树快照。
