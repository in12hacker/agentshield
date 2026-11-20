# 主模块技术细节

公共引用：[模块唯一职责](../../architecture.md)。公共定义以链接目标为准，本页不另定义一份。

产品理由见[产品设计](design.md)，模块关系见[架构](architecture.md)，规则接入目标见[规则技术细节](../rules/technical.md)。本文件只记录实现契约，不记录进度。

## 执行与入口

MainThreads要求调用方使用Tokio current-thread运行时：调用方主线程承载串行main worker，as-callbacks承载fast worker，as-messages承载messages worker。MainWorkers三个字段为fast／main／messages；慢阶段不创建并发prompt集合。异步await保留原操作，只释放执行线程供其他任务运行。

主模块首先调用ConfigModule::initialize()，通过snapshot()读取配置并以shared_paths()组合只读MainContext，再作为接口参数传给规则等模块初始化。start准备线程和唯一进程树，传递树读取的同一宿主boot_id连接实际DriverAdapter／DriverSession，完成真实连接鉴权并绑定驱动控制端后，调用安全日志模块打开固定库，再分别调用配置和规则的镜像导出；自保写需要该实际控制身份，日志不得在冷恢复连接之前抢先初始化。主模块不内联这些模块实现。原接收和回调先接好，必需初始化确认后才宣布业务就绪；失败停止并退役已取得资源。完整顺序见[通信架构](../../contracts/driver-communication/architecture.md)。

`MainAssembly::log_storage() -> &Arc<LogStorageBudget>`只交出从同一配置快照组装的共同容量协调对象，供原业务库初始化复用；start把同一个对象交安全日志owner。容量字段与跨库分工只在[配置契约](../configuration/technical.md#日志共享保留预算)定义。Main不执行SQL淘汰、不复制日志或另派消费线程。正式宿主在原诊断槽观察独立的维护延期计数，停机排空后输出实际文件占用；维护延期不能改原SQL提交或驱动状态。

FastFacts读取原事件并返回ProcessInfo补充，主模块统一fill_missing后，RulesFactory::select_version按事件类别及OriginalOperation::policy_version选择快照，RulesModule::fast检查记住与普通快速规则，返回FastMatch::Matched／NeedsFacts／NoMatch／NoReviewMatch四种明确结果。NeedsFacts不与NoMatch混用，即使渠道在匹配后断开也不能因此放行；NoReviewMatch表示当前事实已排除所有询问候选，无需慢处理。有可用交互渠道且快阶段未决定时进入串行主循环；主循环在有实际慢事实实现时调用SlowFacts补空，再调用prompt；当前装配没有慢事实实现。无渠道仍先做快速内存补全，但不调用SlowFacts或prompt、不产生询问命中／日志；排队请求在渠道断开后也跳过慢处理。

prepare_driver_policy同时返回不可变PolicyLease；DriverPort在候选到达前登记并持有镜像及租约，0x105确认原生最后消费者释放后退役该代。到达事件先同步克隆同一租约，再异步处理。select_version拒绝无法取得的指定版本，不退回当前版本；历史索引只弱引用仍被租约保管的快照，释放租约后不保留无消费者的历史库。参考匹配不代表驱动实际采用。

接口位于[main_ports.rs](../../../crates/contracts/as-types/src/main_ports.rs)，调度位于[dispatch.rs](../../../crates/backend/as-main/src/dispatch.rs)，线程入口位于[runtime.rs](../../../crates/backend/as-main/src/runtime.rs)。软件调度与真实驱动接入分别验证，不能将软件冒烟视为内核保护。

## 统一事件编码

事件字段、字节编码、操作编号、进程生命周期及原执行现场只在[回调事件契约](../../contracts/events.md)定义；Rust声明与解码复用同一EventData。主模块不再复制字段表或另定义“进程树专用事件”。

快速／慢速补全只在成功时填写原ProcessInfo缺项，保留已有值、原参数边界与字段类型；瞬时调用现场、目标载荷和来源身份不由主模块补写。Arc所有权及必要的写时复制见[主模块架构](architecture.md#模块接口与状态)。

原驱动会话用唯一`DriverCallbacks`接入两个同原callbacks容量的职责队列：`controlled`只传原`DispatchInput`，独立消费者位于既有Fast线程，按原bind_host_boot／PolicyLease→native_work→快速池→原Main主循环／同一Interaction进入问询；`events`保留辅助`DriverEvent`，顺序消费者位于既有Messages线程。辅助SQL、上报结果或安全日志的异步背压不能占住受控接收入口；进程观察、计数及事实fence仍调用原Fast ingress及屏障，不在Messages复制进程树。原件在原队列／消费future保管，出队移交所有权，未新增OS线程、分类询问池或丢弃降级。辅助RequestHandoff／RequestReturn／FutexSelection沿同一events队列直接交MessageModule.consume_native_fact，由原业务日志owner写入，不再转投完成／状态notices队列。受控入口先领取同一Original已有终态；已完成不重新取旧租约或宣告来源故障。仍活请求按原版本取租约，失败时再核同一原完成状态，只有仍活且无租约才走来源故障。两个任务由同一MainThreads保管，实际来源错误沿共用收尾，退役在原会话关闭后逐一join两个真实结果，再关闭Fast／Messages线程。

生命周期Observation进入FastFacts维护真实身份，不进入RulesFactory、慢查询或人工等待，也不生成RuleHit。执行前的受控请求和Allow回执不能提升成成功exec；原Create、成功Execute及最终Exit各自的来源语义见[进程生命周期](../../contracts/events.md#进程载荷)。普通防护暂停或前端离线不关闭其他模块仍必需的基础事实。

原操作控制能力只由实际驱动适配提供；事件可解码或软件供给通过均不是内核捕获、等待或保护效果的验收结论。

## 取消与退出实现

进程树补全接口和当前接线状态见[下节](#进程树补全接入状态)，实体模块设计见[进程树契约](../process-tree/technical.md)。

准备阶段取消可退役原请求。执行进入`OriginalOperation::apply`以后，丢弃回执接收方不丢弃执行future；主模块等待原执行结束，随后发送原回执和消息。`DispatchOutcome::Execution`只转交原执行者的`OperationReceipt`，不会把Unknown升级为已允许或已阻止。LSM返回允许只表示AS不拦截，不保证后续其他安全检查、文件系统或网络执行成功；终止进程的结果也不能从拒绝回执推定。

shutdown调用时立即关闭外部管理入口、撤销会话授权及问询，并启动独立退役任务。先等待已接纳有限管理命令；保持驱动回调入口、Fast接收泵和主循环，直到Flow出口／连接、凭证和进程树刷新完成实际清理，因为这些清理仍会触发普通文件／进程回调。不等待尚由原生生产者保活的长活消息消费者。随后封闭回调接纳，stop_admission共享一次Quiesce，继续接收原请求结果；排空fast／main处理，再真实shutdown原生会话和加载器，之后join Fast线程、消息消费者与Messages线程，最后释放代租约。清理不依赖额外Allow规则，各阶段失败保留实际来源。丢弃shutdown返回的等待future不取消真实退役任务。连接对象drop只请求关闭，不冒充已经join；普通日志入队不是刷盘成功。

直决计数批沿同一快速输入队列消费。已入队的批随快速处理排空。外部查询已关且快速接收端有序退役后，原生接收泵继续从驱动取走剩余计数批并释放其数据，直到同一 Quiesce 与真实退役结果。计数批没有原操作或回执，快速接收端的有序关闭不能让这类数据触发 DeclareFailure。运行中的接收端异常关闭仍报告失败。

原操作句柄、FD和租约只存在运行时，不从事件bytes或PID重建。内核不可挂起的直接裁决仍在原hook完成；这份技术契约不声明平台覆盖或产品验收。

## 进程树补全接入状态

当前FastFacts／SlowFacts只接受`&EventData`，返回`Result<ProcessInfo>`；旧的可变事件接口已同批删除。主模块只在成功后合并None字段，事实实现不能改写事件头、文件／网络目标或驱动已有值；慢查询错误／取消不留下部分修改。快补全发生在任何Remembered／Fast匹配之前，进程树实例由主模块初始化并提供。

`DispatchInput.event`、`ReviewRequest.event`、`Prompt.event`、`MainNotice.event` 和 `RuleLogRequest.event` 共享最终形成的 `Arc<EventData>`。主模块快速入口从唯一进程树取得内存补充，移动已核实的补充值，仅填原缺失字段。没有补充值时继续共享原事件。原操作用不可变原记录逐字段借用校验；确有新事实时，补全事件才与原记录分开。慢匹配阶段保留现有 `SlowFacts` 接口，当前正式装配为 `None`。事件的运行时所有权和释放顺序见[主模块架构](architecture.md#队列与并发)。

已结束的原操作也先维护快速事实，不再依赖transfer_hit是否存在；观察回执被放弃也不跳过基础事实维护。驱动直接结果走相同只读输入／补空规则，补全失败保留原事件与原内核结果。当前正式入口使用真实ProcessTree，完成观察前置、整机扫描、消费栅栏后才采用正式防护；查询／刷新由已鉴权消息入口调用同一实例。软件供给只用于明确的软件测试，接口以[唯一技术文档](../process-tree/technical.md)为准。

## 启动校验与鉴权契约

DriverAdapter／DriverSession定义于[driver.rs](../../../crates/contracts/as-types/src/driver.rs)。connect取得已核验的会话及Hello实际能力；initialize另外提交两个模块镜像。MainAssembly仅在来源Running且SelfProtection Enabled后返回；失败不开放业务并实际清理。资源由MainThreads保管至真实排空。软件测试供给明确标记，不能作为物理加载或防护证据。

前端／IM的鉴权归主模块内消息封装，在第三线程通讯入口首次接受连接时完成。它从真实UnixStream.peer_cred取得内核[SO_PEERCRED](https://man7.org/linux/man-pages/man7/unix.7.html)，按配置模块middleware.peer的Unix UID校验身份并授予ReadOnly／ManageRules／ManageService；通过后创建持有原传输的Connection，并登记规则交互渠道可用性。同一连接复用该会话，不在每条规则或每个普通消息上重复鉴权。传输任务遇到EOF／错误时丢弃Connection，规则模块通过轻量连接状态按原超时语义收尾；重新连接重新鉴权。

规则模块Interaction::connect只是消息模块完成鉴权后的内部生命周期回调，不接受网络凭据、不读socket、不产生用户通知；断开回调也只改变渠道可用性。内部Rust队列不承担对外认证。
middleware.peer属于服务端可信配置，不能从客户请求获取；Unix UID认证不等价于HTTP／TCP鉴权。远程浏览器／IM等接口必须由接口模块完成令牌／会话与传输验证才可暴露；不提供未认证入口。双方需要相互验证的本地特权通道，由各自端点分别执行真实对端校验。内部Rust队列不额外发明鉴权协议。

配置／规则管理消息分别转交所属模块；规则单条查询／编辑调用get_rule／edit，完整替换与重载调用update／reload；消息封装不直接写文件，中间层也不写自保文件。首次鉴权完成后复用会话，每条修改仍检查管理权限。接纳后的写入在第三线程的异步任务中等待规则模块确认，不阻塞快速回调或主循环。

所有MainMessages构造经同一MainThreads的management_entry返回同一Arc，原commands tracker由该MainThreads在退役时close／drain，不存在例子或直接构造绕开的第二生命周期。宿主停止请求只交接当前宿主，不在命令任务内等待自己的shutdown；一次性监听、运行时回执能力和真实退出的边界见[唯一停止请求契约](messages/technical.md#停止请求与真实退出)。

## 回调、共享出口与临界区的源码依据

Linux [LSM hook定义](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/include/linux/lsm_hook_defs.h)中的bprm_check_security、file_open／file_permission、socket_connect参数不同，必须独立实现相应回调。TOMOYO的[同一注册表](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/security/tomoyo/tomoyo.c#L556)集中注册这些入口；不要求一个hook对应一个模块／动态库／线程。

KubeArmor的[同一enforcer对象](https://github.com/kubearmor/KubeArmor/blob/3b5c8f74476ec7bcbdb2bf451a96ab3318b2ef72/KubeArmor/enforcer/bpflsm/enforcer.go#L135)加载多个LSM program/link，多个回调共享ring reader。[shared.h](https://github.com/kubearmor/KubeArmor/blob/3b5c8f74476ec7bcbdb2bf451a96ab3318b2ef72/KubeArmor/BPF/shared.h#L83)的暂存map为PERCPU_ARRAY；事件每次reserve取得独立记录指针。共享出口不是共用一份全局event结构。其reserve失败仍返回原裁决，不能将审计投递失败变成允许。

Linux [ringbuf.c的reserve](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/kernel/bpf/ringbuf.c#L405)在推进producer_pos时持短spinlock，解锁后填充独立记录；[commit](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/kernel/bpf/ringbuf.c#L470)以原子更新释放busy标记。[内核说明](https://docs.kernel.org/bpf/ringbuf.html)明确慢producer可暂时阻挡后续已提交记录，容量满会预留失败。并发正确不等于无锁争用、无队头等待或无容量挤占。

真实驱动可按实际来源／可靠性需求分有界缓冲，由独立接收／快速回调线程的poll管理器轮询；不提前写死所有数据共用一大环，也不为每个hook开线程。BPF回调只做有界采样与本地裁决，禁止在reserve未提交期间等用户、解析完整MCP或做慢查询。原生LSM等待路径另见[驱动技术细节](../../drivers/technical.md)，不能把BPF ring上报当成挂起协议。吞吐及容量取值必须实测，不能从引用项目架构推出AS已达到某性能。

## 双向消息接线

中间层DTO见[消息技术文档](messages/technical.md)，统一驱动封套和分段buffer见[驱动通信契约](../../contracts/driver-communication/technical.md)；两端接口收口在主模块消息封装。规则问询上行、用户答复下行；底层返回operation_result中的真实决定来源和执行结果或失败，不发送“问询结束／关闭窗口”指令。普通日志不主动上抛；LLM聊天由Flow提供Started／Delta／Finished及真实缺口，原始事实结构和订阅端口只依赖[公共契约](../../contracts/flow.md)，不是只有完整回答才上抛。下行控制及查询只调用各自owner，未接入返回Unavailable。

显式[软件冒烟端](../../../crates/middleware/as-middleware/examples/configuration_smoke.rs)使用同一当前DTO观察管理回复；[正式中间层接入](../../middleware/technical.md)复用这些端口，不嵌入主循环。关闭主模块、功能回调暂停、全部配置／规则重载和驱动更新要分别等待真实owner确认，不能把消息接纳当成功；主循环不实现各子模块业务。

## Flow 组合边界

Main 已组合驱动受控入口、Flow、凭证和原聊天消费端口。受控 TLS／phantom 与原聊天采集共用同一链路，消息消费者将业务事实交中间层数据库。TLS／HTTP 处理和凭证写入分别归 Flow 与凭证模块。AS 内部按真实就绪顺序启用入口，受控 TLS 只接纳启用后新建的连接，目标进程可以先运行。具体契约见[联合架构](../flow/architecture.md)和[受控出口](../flow/tls-egress.md#支持范围与成熟依据)。

## 原请求等待的接入约束

OriginalOperation的decision_id、policy_version、hold_window、stop_signal、validate、bind_review、apply与cancel共同描述用户态原请求引用。内核保留原调用上下文和必要对象引用，用户态只拿不可伪造的会话关联及事实；不会把内核栈、指针或FD值序列化给中间层。

实际执行者保管原规则版本、询问规则及其超时动作。前端失联执行规则超时动作，驱动／控制服务失效另按原生故障事实处理。88字节事件帧只编码事实；[真实适配](../driver-adapter/technical.md)负责封装保管协议。OriginalOperation::try_result／result只读取同一次原生完成，不发送决定。内核先完成超时动作时，后来的主循环领取同一次真实回执，不重新发决定、不以已退役为由丢弃真实结果。协议、睡眠上下文和错误返回见[驱动技术细节](../../drivers/technical.md)。

无交互渠道与驱动连接失效分别处理；读取、显示、规则重载、日志及重连不续期。允许无限期等待的内核原语不意味着AS取消已设计的规则超时与断线收尾。

主模块仅组合接口与数据，MainContext的公共声明与模块依赖见[上下文契约](../../contracts/main-context.md)。配置、规则、驱动、事实和日志各自处理所属功能，不通过模块内部交叉引用、全局配置或服务定位器拼接；未明确的细节先[存档](../../archive/unresolved-module-contracts.md)。

完成路径统一产生MainNotice：成功、失败、取消均有真实ID和实际源结果。内部业务处理错误可与源已完成结果不同，trace分别记录；通知不把源Allowed改报Failed，不重复发决定。已接纳处理ctx取得原回执或明确源失败后，通过已有有界notices通道send.await保管完成，直到消息消费者接纳；满队列不丢该完成，也不另spawn、增队列或扩大额度。接收端关闭才计rejected_notices并告警，原结果仍不改写。Direct实际结果、状态及来源失效也经同一个有界异步notice接口；满时由当前来源上下文保管，不按普通遥测丢弃，消费者关闭返回明确错误。接纳不表示前端收到、SQL提交或日志刷盘。

快速ctx仍由原pending Futures及queues.callbacks限额保管，慢ctx仍顺序执行。原生Controlled接纳直接把原输入与PolicyLease交给同一Work；无外部reply订阅者用明确Native答复模式，不为每次回调spawn观察任务。缺代或已停止接纳的原请求以Retire类型进入原FastInput队列，同一pending预算执行取消、等待实际结果及通知；Fast入口已关闭时由唯一原生pump保管这一项完成同样退役，不新开补偿任务池。退出时Messages消费者在处理ctx排空、原驱动producer退役之前继续消费，不能先停止consumer再等待满队列sender；外部常驻Ingress只持Weak，不保活已结束的消费者。

已读FileClose由`driver_file_close`向原日志队列`send.await`，队列暂满时由原接收上下文持有，接纳后移交，不另建任务或缓存。日志消费者真实关闭只计日志拒收，不伪造驱动失效；普通RuleLogRequest仍走原快速日志发布接口。

快速规则未命中且有审核渠道时，规则模块用同代规则和已取得事实判断 Ask 候选。每个 Ask 候选均有明确条件不符时返回 NoReviewMatch，Main 在现有快速任务中用原 NoMatch 结果收尾。匹配或缺字段的候选继续进入原串行问询链。该分流使目标端口不符的 WS 答复发送可独立完成，原 Send 等待和期限仍由原操作保管。

受控TLS的生命周期由[FlowRuntime](../../../crates/backend/as-main/src/flow_runtime.rs)组合：凭证材料初始化后准备Flow及有界入口队列，将原唯一ChatReceiver交[同消息职责的聊天消费端口](#消息与聊天消费端口)，再启动真实驱动适配入口。网络异步工作归消息执行职责，不占快速回调或单人工问询。退出先停止导流、沿原I/O或显式取消收敛TLS连接，等原聊天队列到EOF及当前消费完成，随后沿原驱动退役流程收尾；不另建事件总线或服务定位器。字段／解析和秘密写入分别归所属模块。

完成／驱动状态通知与SecurityLogEntry审计记录使用两个独立有界mpsc，容量均由既有queues.messages派生，属于同一消息执行职责。`MessageModule::run` 和底层 `SecurityLogSink` 分别接收各自通道。两者独立推进，均结束才完成排空。真实producer保管两发送端至已开始工作结束；外部Ingress只持Weak。日志丢弃只累计rejected_logs，不占rejected_notices容量。没有为该分流新增线程、配置门槛或总线。

## 消息与聊天消费端口

本节是MessageModule消费端口的唯一文档定义，声明位于[main_ports.rs](../../../crates/contracts/as-types/src/main_ports.rs)；ChatReceiver／ChatEnvelope／ChatCursor的唯一值结构见[Flow公共契约](../../contracts/flow.md)。MainModules.messages仍为同一个显式传入的Arc<dyn MessageModule>，不新增聊天Owner、服务表或MainContext可写状态。

`MessageModule: Send + Sync`的完整签名以共享声明为准，消费入口分别为：

- `run`：取得唯一`Receiver<MainMessage>`，消费实际完成和状态到通道关闭。
- `consume_native_fact`：接收单个原`NativeFact`并等待该事实的实际存储结果，不转投完成／状态队列。
- `consume_group`：取得原`Receiver<NativeGroupTcp>`及同一宿主boot ID；在现有Messages运行时消费公开TCP原件。默认实现明确返回Unsupported，不能将未装配的来源说成已消费。
- `consume_chat`：取得Flow唯一`ChatReceiver`及可选的同实例`FlowFeedbackPort`，消费原聊天队列。没有默认丢弃实现。

run负责实际完成／状态。规则日志持久化归[底层安全日志](../security-log/technical.md)。consume_chat取得Flow唯一原队列的唯一接收端，逐条交所属业务日志消费者，消费完释放；没有默认丢弃实现。这些消费入口同属Messages OS线程中的异步工作，不按数据类别增加OS线程。主模块仅传递已有消息能力和原接收端，不解析聊天、不复制落库或授权实现。

FlowRuntime 在初始化真实 Flow 后，把同一 FlowFeedbackPort 和唯一 ChatReceiver 一起传给 consume_chat；正式 Flow 为 Some，独立事实供给没有执行端口时为 None。该端口只安装一次，属于消息模块本实例的不可变依赖，不是前端权限或第二份聊天状态。FlowRuntime保管唯一聊天消费任务的JoinHandle；该任务直接调用同一消息模块的consume_chat，不再向MainMessage或另一Chat队列转发。控制消费者仍先于驱动正式接纳运行，之后Flow初始化时直接启动聊天消费，再启用TLS入口，不为后初始化的Flow另建接收端安装队列。Flow未启用时不创建该聊天消费者或空成功来源。

无前端、未订阅或展示暂缓均不停止聊天事实消费。持久化、提交回执、数据库游标读取及提交唤醒只归[中间层业务日志](../../middleware/business-log/technical.md)；前端不从该源接收端分走一条记录。普通聊天不占完成／状态的notice容量，也不因其展示背压关闭问询控制身份。单一socket已部分写出的帧不能插入另一帧，控制优先仅在完整帧边界生效；这不是改写HIPS无渠道或原规则timeout语义的理由。

原Ask的逻辑期限不因JSONL开始写出而延长。期限内已开始的部分帧可继续写完整以保持同一控制流的帧边界，实际授权撤销、EOF或I/O失败仍结束真实传输；完整Ask可能晚于原期限才可显示，不能据显示推定原请求仍可答复。尚未写出的过期Ask只释放自身过期答复能力，不以普通数据压力或期限到达关闭控制身份。

退出时保持消费端运行，先停止TLS新增接纳、收敛Flow原任务并丢发送端，再由consume_chat排完原队列到EOF及当前实际消费，FlowRuntime等待该真实任务结束。初始化失败也先关原producer、再join已开始的消费者；不能丢JoinHandle、提前关闭receiver或以发出停止信号冒称排空。消费错误关闭原接收端并返回明确ModuleError，Flow发布失败及主模块真实join结果均须保留；业务库真实失败不得混成普通not_forwarded。控制／日志消费者依原依赖继续到native和处理ctx收敛，最后才结束Messages线程；业务库随后关闭并等待其已接纳SQL工作。

### 过期问询答复能力清理

`MainMessages::retire_expired_question(&self, connection: &Connection, decision_id: DecisionId) -> Result<(), ModuleError>`是同一原question槽的窄同步操作，不是关闭连接或发裁决的命令。它在原authentication临界区复核ManageRules授权，并在原question mutex下检查同一连接grant、同一decision_id及原deadline已到或reply已closed，只有确已退役才取走原Prompt。空槽幂等Ok，不同连接Forbidden，已换为另一ID返回Conflict，仍有效且可答复返回Busy；均不得删除别人的／后来问题。

该接口只释放过期oneshot答复能力，不发送Decision／AskClosed、不改review_ready或渠道计数，不重新计算期限。writer跳过自己的旧帧时可调用；旧ID或已由Answer清走的空槽不能成为关闭Peer的理由。原连接撤权／真正断开仍由原Connections退役接口清理，不与过期帧混称。

## 异步诊断输出与控制活性

进程诊断复用公共`as_types::diagnostics`异步stdio原语，不建立事件总线、日志数据库或历史缓存。stdout／stderr各自仅保管当前已接纳的一行字节及实际写入进度；慢输出通过Tokio异步I/O让出运行时，不在回调或消息OS线程同步写journald。原操作、业务原件和失败通知先交原所属模块，诊断不更改其裁决或错误。未接纳的调用仍由调用任务保管；已接纳行在调用方取消后由后续写入或最终flush继续，不重复已写前缀。明确输出错误由最终flush报告，不能假称写出成功。

正式宿主主循环保管一份在途诊断future，与停止信号、现有会话和正常WS处理一起轮询；诊断慢时不会阻塞OS线程或把停止信号等待改成同步日志写。退役继续由原Main／Flow／消息所有者执行，实际收尾后显式flush stdio；不增加诊断超时、无界队列或丢弃策略。

宿主的会话关闭／认证拒绝诊断只有有限的协议错误类别，重复类别在原宿主计数后异步报告`count`；不保存连接ID、正文或无界闭包链。慢stdio不参与新管理连接接纳，也不阻止已结束WS任务回收。真实业务事件仍由原日志owner逐条保留，不能用这些辅助计数替代事件。
