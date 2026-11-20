# 底层驱动适配接口

所属大模块：Rust底层。模块边界见[两大模块](../../drivers/architecture.md#两个大模块的边界)，底层上下文见[MainContext](../../contracts/main-context.md)，内核入口见[驱动契约](../../drivers/technical.md)，跨边界数据见[通信契约](../../contracts/driver-communication/technical.md)。本页是底层驱动适配接口与初始化参数的唯一出处。

## 平台接口边界

- `DriverAdapter::connect(&DriverConnectInput) -> Arc<dyn DriverSession>`：底层连接并核验原生端点，完成Hello取得真实能力并启动受信加载器；`DriverSession::info()`返回同一会话的初始DriverInfo。系统模块由安装／特权生命周期实际加载，connect不执行任意shell安装。不接收或创建驱动私有DriverContext。
- `DriverSession::initialize(parts: DriverInitParts) -> AppliedPersistentState`：底层把完整输入编码为InitializeDriver，复用start_process_tracking已经绑定的唯一事件出口并等待驱动实际采用；不是直接调用内核对象方法。采用回执仅按[共享结果](../../contracts/driver-communication/technical.md#8-规则命中来源与版本)。
- 原操作适配实现现有OriginalOperation；停止接纳、排空与shutdown保管本进程的加载器、BPF link和控制FD／HANDLE。只释放自己取得的资源，不关闭外部Root broker或其他实例。

```text
DriverConnectInput {
    platform: TargetPlatform,
    host_boot_id: 进程树prepare实际读取的宿主boot_id,
    driver_assets_directory: 借用配置模块已生成的只读NativePath,
}

DriverInitParts {
    expected_persistent_generation: u64,
    expected_lifecycle_epoch: u32,
    configuration: DriverConfigImage,
    rules: DriverPolicyImage,
}
```

DriverConnectInput由主模块从已初始化配置／MainContext及已prepare的进程树取必要数据组合；不要求驱动借用MainContext对象，不再次发现目录。原生端点名字和协议固定，不接收中间层指定的设备路径。系统ko路径与安装来源尚未确定处先存档，不能把私有资源目录冒称为系统模块位置。

DriverInitParts只组合两个所属模块的不可变导出及实际状态中的并发校验值，不包含模块实现、写入器、内核指针或另一份配置。DriverPort登记候选后保管同一不可变镜像和PolicyLease；0x105真实退役后释放该代，精确到达事件另克隆同一租约。主模块组织持久更新，适配只传输及核实实际回执，不编译规则或读配置源。

## 通讯、消息与控制

进程树使用同一会话上的观察前置及固定进程事实查询／栅栏端口，语义与结构只在[进程树契约](../process-tree/technical.md#5-原生事实的窄协作接口)定义。当前DriverSession已实现start_process_tracking／inspect／facts_barrier；Initialize复用已绑定队列，不替换它。该窄端口不授予进程树规则发布／驱动生命周期控制权。

底层MainMessages.driver／DriverPort继续承担调用端语义，DriverFrame／FrameClass承担现行封套codec；底层按实际能力分片发送不可变镜像，下行镜像重组只由接收端Native保管。上行Group 0x10a沿原封套分片时，由唯一接收泵保管当前一条消息至完成后交原解码/消费者，不保留无人消费的重组状态；唯一字节契约见[驱动通信](../../contracts/driver-communication/technical.md)。连接I/O由本适配封装，消息关联仍由底层唯一接收泵完成；不新设DriverLink别名或另一个消息Owner。

DriverInfo、DriverCapabilities、DriverStatus取实际驱动回复。它们是协议数据与只读观察，不是驱动私有上下文，更不能由ConfigDocument推定。原生事实先解码成已有EventData再入主调度；只有真正保管的受控消息才创建OriginalOperation，Observation不能创建可执行能力。

原操作错误和通道失效分别处理，唯一原生结果语义见[原操作错误](../../drivers/linux/operations.md#单次原生错误与通道失效)。wire::result保留已有0／-1／-13／-125对应的Allowed／Denied／Cancelled，其余实际负码直接成为`ExecutionOutcome::Failed { native_error }`，不调用管理命令回复使用的generic error()折叠。NativeResult直接持ExecutionOutcome；Original.receipt核验原id／generation／rules后封装真实OperationReceipt。合法-110／-5只完成这个Original，不使Shared失效，也不把StopReason改成SourceFailure。

Shared.fail仍处理真实接收／会话失效并使尚未完成的原请求返回Unavailable；不能由单个errno推定这一状态。Direct结果仍复用同一解码器，但其生产契约仅允许真实Allowed／Denied，其它结果明确拒为InvalidInput，不凭errno猜原提案。Controlled保留实际失败回执与Direct帧类型校验互不替代，不新增错误总线／DTO／Owner或兼容协议。

接收泵在首个实际失败返回前向stderr输出`AS_NATIVE_RECEIVE_FAILED`，保留原ModuleError及实际失败阶段；设备读取失败另保留原I/O类别、OS错误码和错误文本。封套校验前只记录实际读取长度，校验后才记录class／type／request_id／物理帧正文长度／逻辑总长／offset，不输出正文或重新解析未校验头。`event_decode_or_dispatch`明确表示原事件处理函数返回失败，未声称已区分其内部解码与分发原因。原错误仍经过Shared.fail和实际reader join返回；正常停机且没有未完Group正文不输出故障，不依赖宿主安装tracing订阅器。

### 必要结果接纳与普通观测

受控请求与辅助事实共用一次鉴权和唯一接收泵，通过DriverCallbacks.controlled与events两个职责出口交付。泵先对controlled执行reserve_owned，将唯一OwnedPermit保管在原Ipc中，再经同一认证端点AS_CONTROL_CREDIT授予一条交付额度；Controlled到达时take该permit并立即send移交，不在Critical暂存受控上下文。该许可占原队列容量，未读Native notice仍由原pending／期限保管，防止已完成但未消费的原件脱离Native max_pending后无限累积。Shared::deliver已取得的原件全部沿原events消费链保管，不按事件枚举选择可丢类型；出口暂满或原Critical有先行项时，将同一原值move到既有Critical::Required，保持FIFO并暂停普通读取，PRI继续独立读取原请求／完成。未绑定、接收端真实关闭或协议失败明确返回错误并沿Shared.fail收尾；不丢弃原件后假成功、不无界spawn、不让日志入库成为问询发布前置。

正常背压不增加callback_loss／ProcessFactsBarrier.backend_rejected，也不使进程树失效；实际投递失败仍保留原拒收计数，不能隐藏真实失源。进程生命周期在原生源丢失仍由loss_epoch报告，独立DirectHits损失仍归命中计数缺口；Group维持其独立来源和损失记录。本段只保证该投递入口已读取原件的保管，不宣称内核任意负载下一般无损，也不以聚合计数推定某条结果丢失位置。

接收泵保管必要原件时，使用锁定rustix安全epoll与现async-io等待原设备PRI；只在设备实际报告完成可读时调用同一read，继续读取Controlled并直接关联Reply与AS_RESULT，不等待Main通知或人工答复，也不从普通POLLIN忙轮询。Hello必须实际报告ControlPriority能力位512；缺少时返回Unsupported，不接旧模块后静默等待。该位证明端点原请求及完成读取语义，不是原始研究字段门槛或用户开关。原优先／普通FIFO的唯一语义见[原生端点](../../drivers/linux/transport.md#缓冲与临界区)。Main独立原生事件泵可以等待原有处理入口接纳，把Normal／Retire及同代PolicyLease交现有Fast pending预算；取消、真实结果收集和完成通知由该执行上下文统一收尾。停机保留原生producer／reader和消费者到实际资源清理、回执处理及已读Critical尾部真实接纳完成，再关闭I/O和加载器。最终停止／排尾阶段不再申请或发布新的Controlled额度；已取得但尚未采用的许可在原Ipc锁内复核停止状态，退役则释放许可并继续排尾，不把正常退役当作投递失败。真实接收端关闭仍报错，不能伪造已投递、已持久化或已裁决。

## Linux实际接线与寿命

当前接口见[driver.rs](../../../crates/contracts/as-types/src/driver.rs)，实现见[as-driver-adapter](../../../crates/backend/as-driver-adapter/src/lib.rs)。旧DriverStartup已删除，全部调用使用分阶段接口。控制设备要求真实字符设备、root所有且无组／其他权限，并核对sysfs的设备号；不是任意同名文件。运行内核、模块签名与安装来源仍属于真实部署事实，连接不能代替安装证明。

[as_loader.c](../../../crates/backend/as-driver-adapter/native/as_loader.c)是底层用户态加载器，按实际实例核验或附着完整BPF集合并保管link。正式自保要求底层与加载器静态链接，以精确执行文件和原生对象引用建立失联后的维护边界；动态解释器／任意root程序不能成为恢复主体。正常排空后释放本实例pins；异常控制者或加载器退出保留检查入口，不能把故障变为允许。内核管理员主动卸载／解绑不属于文件自保承诺。

Linux构建统一使用锁定getrandom的官方`linux_getrandom`后端，通过libc的getrandom系统接口取熵，不保留旧内核的随机设备回退。选择依据见[getrandom 0.3.4官方后端说明](https://docs.rs/getrandom/0.3.4/getrandom/#opt-in-backends)。静态GNU程序中dlsym无法取得同一动态符号，默认兼容路径会转读/dev/random及/dev/urandom，可能在来源失效时反向阻塞恢复。项目Cargo目标配置和显式static构建脚本使用同一cfg；下游可执行程序必须采用此构建约束，库自身设置不替下游选择后端。不自制随机数实现、不扩大设备白名单。

唯一接收泵关联命令答复、真实原操作结果和上行事件。必要结果拒接纳沿上述真实来源失效路径处理，普通观测拒绝有明确计数；持久代退役有界保管，并对后续持久发布施加容量约束，不无限起任务。设备写入／镜像解析由已拥有任务保管，单帧阻塞syscall放入spawn_blocking，业务运行时不持跨await互斥锁。

`stop_admission`、Quiesce及shutdown共享同一次停止回执。失败与正常关闭分别处理；正常完整退出的内核状态以[原生寿命契约](../../drivers/linux/transport.md)为准。接收泵、真实写入、原请求、加载器及本会话TLS入口资源均实际结束后才完成shutdown。启动等待被取消仍由所属任务清理已经取得的资源。Windows尚无生产适配，不返回空会话。

DriverSession同时承载[DriverTools](../../drivers/linux/tools.md)的显式进程终止；DriverPort.kill_process仍复用同一实际连接。ProcessFactsPort保持只读，进程树未获得终止接口。工具回执单独返回KillProcessOutcome，不伪造DriverStatus变化或规则执行结果。

## 被动TLS来源（未实现）

被动 TLS 来源与下述原生工具持续采集是不同能力，不能将聊天字节采样视为工具到系统操作的交接。

当前DriverSession没有start_tls_capture／TlsCaptureInput或ABI探针接口；原草案已[归档](../../archive/flow-design/tls-capture-interface-draft.md)。不要求当前调用者构造采样预算／profile，也不返回空采样会话。当前正式受控入口见下一节。

## 原生请求持续采集与工具边界

原始libuv请求观察沿 `DriverEvent::RequestHandoff → MainMessage::RequestHandoff` 进入现有消息消费者。驱动适配只从真实执行映像的符号和libuv C DWARF取得必要探点／字段，使用原鉴权设备会话安装；命令、实际回调数据和生命周期唯一见[原运行时观察源](runtime-observation-technical.md)。没有V8对象布局、ToolCall或HTTP身份不阻止可用原生探点和原件入库。缺探点、布局、读取或接纳时分别记录实际缺口，不冒充工具语义，也不使普通InspectProcess或HIPS启动失败。

普通原操作仍沿既有Controlled／Direct事件连续携带 `EventData.syscall`、`kernel_stack` 和原用户栈。它们由原线程在快速规则匹配后、发布和等待之前一次取得，按共同TLV解码并随同一事件消费；源不补读目标、不制造工具关系。字段及范围只在[事件契约](../../contracts/events.md#原操作执行现场)定义，验证见[执行现场证据](../../evidence/execution-site-capture.md)。

职责分配为：适配核验实际进程／exec及ELF，驱动保管真实探针、原req/work及实际spawn child在途寿命，在各有效回调中一次取得该次原值；适配只解码已拥有记录，沿原Main消费者移交中间层。原地址不成为跨cleanup恒定身份或延迟解引用能力；只有实际原请求交接才能填写request，只有实际LSM检查的Controlled／Direct发布才能填写EventBound。具体业务组合仍归中间层。

`ToolExecutionFacts/NativeToolContext` 是已证工具语义的原消息／业务库契约；本libuv原始源不伪造其生产者。R12已证明特定实际映像的ToolCall→原请求→EventKey，公开共享消息格式读取器现已接入正式Submission消息和SQL原件；真实归因的验收状态见唯一量化记录，代码存在不表示三类已完成。固定PC、CB/LR、旧私有布局表、Promise扩图和独立/proc实验输出都不迁入产品。原件及待接部分见[独立复核](../../evidence/runtime-correlation-research.md#r12-同次-toolcall-native-eventkey-研究的当前复核)。OTel Profiler仅作成熟元数据实现参考，不以采样符号化、目标后读或语言支持列表冒充已证精确归因。

## 受控TLS入口

入口使用 Main 从已初始化的进程树转交的宿主 boot_id。该值用于核对计划中的 ProcessKey，并为已核实的套接字发起者组合相同的进程身份。进程树持有唯一的读取结果。

```text
TlsIngressInput {
  boot_id: [u8;16],
  scope: FlowScope, destinations: BoundedList<SocketAddr>,
  config_version: ConfigVersion, max_connections: u32
}
DriverSession::start_tls_ingress(input: TlsIngressInput, output: Sender<AdmittedConnection>)
    -> async Result<Arc<dyn TlsIngressSession>, ModuleError>
TlsIngressSession::stop_accepting() -> async Result<TlsIngressStatus, ModuleError>
TlsIngressStatus { generation: u64, accepting: bool, admitted: u64, rejected: u64 }
```

Linux 入口通过 cgroup connect4／connect6 标记和 nft OUTPUT 数据包重定向接纳 IPv4／IPv6 TCP 新连接。宿主和已核实的独立容器网络域各有实际入口，配置与分流见下文。控制进程自身按真实 TGID 排除以免递归。目标集合由 Flow 按配置 origin 实际解析，Main 只组合接口。DNS 目标变化须重新初始化，容器的真实启动和退出按绑定生命周期增减入口。

适配独占导流资源、接纳端点、原目标和连接发起实例的核验；仅将核验成功的连接移动为公共AdmittedConnection。端点与数据面都归原DriverSession受信资源，不接受中间层自报来源。原socket／连接生存期证据不足时拒绝接纳，不能把5元组或PID猜测包装成proof；当前实现从实际connect保管ProcessKey／TID／task birth／原socket cookie；ACTIVE_ESTABLISHED按原socket存储建立实时TCP关联，状态离开Established立即退役。接纳端按实际传输地址族用内核 conntrack 的 SO_ORIGINAL_DST 或 IP6T_SO_ORIGINAL_DST 原目标及真实 peer匹配该存活连接，领取后删除；SCM_RIGHTS携带的实际accepted FD再次核SO_COOKIE，不能用/proc、客户端头或最近connect补身份。缺失／碰撞／队列满均关闭该流，不发凭证。

该入口不解TLS、不存key、不解析HTTP或tool call，不把每条HTTP请求绑定成一个驱动hook。来源计划内未匹配受控route的连接由TLS出口明确拒绝；计划外原网络操作仍由原HIPS规则处理。网络Deny不能被导流绕过。关闭入口不停止已经移交Flow的实际连接，其排空由Flow负责；不关闭Root broker。

实际接口以[driver.rs](../../../crates/contracts/as-types/src/driver.rs)为准：错误使用项目ModuleError，返回Arc<dyn TlsIngressSession>；当前仅start／status／stop_accepting，无replace接口。固定资产as-tls-ingress／as_flow.bpf.o复用同一配置派生目录及root拥有、不可组／全局写的资产核验。父子通过私有SEQPACKET fd和真实SCM_CREDENTIALS双向核验，不建立开放本地代理。要求root、cgroupv2、BPF及nft(/usr/sbin/nft)和匹配内核netfilter模块；缺失启动失败。stop先关闭本计划标记、释放links并删除唯一会话table，等待真实helper退出；不关闭HIPS会话。初始计划发送及受信首包等待不设适配器截止；真实停止、输出关闭、helper退出或实际I/O错误结束该阶段。stop_accepting沿原完成watch等待同一helper的实际清理与Child.wait结果，不以经过的时间报告失败或假称清理完成。

TLS启动在同一ipc退役锁下保管既有Shared.work token，覆盖第一个await、已开始的资产阻塞任务和实际Runner退出；Quiesce／故障通过同一admission_stop停止新接纳。SDK shutdown等待该实际寿命，并报告首个TLS退出错误；这个结果不伪造成Native LSM失效，不新增Owner／任务tracker／第二watch。


### Linux受控入口私有实现

用户态源码位于[本模块native目录](../../../crates/backend/as-driver-adapter/native/README.md)。实际socket标记和建立事实只引用[内核连接实现](../../drivers/linux/flow-binding.md#当前受控入口实现)，不在适配内重建一套内核事实。以下实现属于原TlsIngressSession，不新增公开端口或状态Owner。

私有回复的唯一C声明为[as_tls_ingress.h](../../../crates/backend/as-driver-adapter/native/as_tls_ingress.h)：原生端序、112字节；kind为1就绪／2连接接纳／3已停止，status当前固定为0。binding只在连接接纳时携带原socket事实，其余记录为零；accepted_cookie只校验同包SCM_RIGHTS传入FD的真实SO_COOKIE，不是原socket的cookie。Rust receive按同一固定偏移读取，并核实际SCM_CREDENTIALS、长度和句柄数。这条私有报文不进入/dev/as_wait或as_protocol.h。

接纳只领取同一内核 TCP 建立记录和当前族的 conntrack 原目标；它不是从/proc或到达时间寻找相近进程。发送方与接收方通过私有继承SEQPACKET、SCM_CREDENTIALS核验实际父／子pid和root UID；真实accepted FD经SCM_RIGHTS交接，并以SO_COOKIE逐次核验。原socket cookie与accepted socket cookie不同，前者是原通道身份，后者只校验本次传输句柄。

不持久化nft配置，不读其他软件秘密或改系统默认策略；会话table名称和mark由真实安全随机材料生成。只删除本会话table，nft输入走固定/proc/self/fd/0而非shell。nft子进程不设自有完成期限；启动时沿同一已鉴权控制通道与原父pidfd接受真实停止／断开取消，取消后仍waitpid确认本次子进程实际退出。采用和删除只认实际结果，不从取消或非零返回反推表从未创建。已进入删表清理后不被同一次遗留Stop再次取消，原helper等待本会话清理结果；失败明确返回，入口不能换成直连。停止移除新增导流并join helper，已移交Flow的stream由Flow排空。没有在线replace_targets实现；新地址／主体采用需入口重建，实际解析出的地址范围之外不宣称已覆盖。

启动计划头为原生端序四个 u32（全部主体、进程数、地址数、最大连接数）和一个 u64（容器 cgroup ID，宿主计划为零），总计 24 字节。后续主体和端点结构保持原义。原私有控制通道同时传 cgroup v2 目录和 netns FD。原 loader 核对 FD 类型和 cgroup ID，在实际 netns 内设置监听及 nft 表。容器计划只选择原 actor cgroup ID 等于本入口绑定的请求。宿主计划不接纳其他任务网络域的请求。每次标记还核对原 socket 的 netns cookie。

启用的地址族各自使用 loopback 监听，IPv6 设置 `IPV6_V6ONLY`。同一会话的 nft `inet` 表按实际协议族和标记重定向 TCP 数据包，保留原 socket 对端。接纳从真实 peer 以及对应族的 `SO_ORIGINAL_DST` 或 `IP6T_SO_ORIGINAL_DST` 匹配内核活连接，再沿原 SCM_RIGHTS 与 cookie 验证交接。退出沿原会话关闭标记、摘除已接入口、删除表及等待实际 helper。容器命名空间、TLS 解析及原 HIPS 裁决保持原职责。

## 容器实例绑定

范围为 Linux 上 rootful Docker 的默认 runc 启动链和独立网络 sandbox。驱动适配的候选对象是完整 Docker ID、Docker 报告的 init 宿主 PID 和 sandbox 定位信息。候选数据只用于打开真实对象。已验证绑定包含 `ContainerInstance`（本地 Docker 的 `runtime="docker"`、`namespace=None`、完整 ID）、init 原生 `ProcessBinding`、活 pidfd、cgroup v2 目录引用和 netns 引用。FD 是适配内部资源，只通过现行鉴权通道移交给入口执行者。进程树和 Flow 消费现有公共类型。

按完整 ID 读取 Docker 实例，先固定 init pidfd，再通过对应 OCI runtime 的公开 `state` 接口核对 ID、运行状态和 PID。runc 的运行态检查使用保存的启动时间核对当前 init。持有 pidfd 时读取原生 cgroup 样本及 init 的网络命名空间，分别与固定 cgroup 对象及 Docker sandbox FD 核对。发布前再次核对运行时实例和 pidfd 存活。任何不一致或来源缺失都保留未绑定状态。适配不从容器名、PID 数值或目录名独立证明归属。

现有 `ContainerInstance` 标识 Docker 对象。重启时原 pidfd 退出即退役原绑定，重新验证同一对象的新 init、cgroup 和 netns；重建后必须用新完整 ID。运行时通知用于触发核对，不能代替对象验证。对业务事件只采用事件时点已有的实际成员证据，旧快照不得填补后来的缺失。

init 的 cgroup 和 netns 只证明该 init 在采样时的归属。启用容器范围导流前还要核实运行时分配给该对象的独占 cgroup 边界，或限定到已分别核验的 `ProcessKey`。共享网络域不能充当容器身份。普通 HIPS 事件和 TLS 接纳分别使用各自实际取得的原事件事实，两个入口共用同一份实例绑定。

默认运行时定位和 init 对象检查已通过[隔离 Docker 试验](../../evidence/container-dynamic-reconnect.md#docker-默认-runc-链的运行后绑定)。参考 [runc state](https://github.com/opencontainers/runc/blob/v1.3.6/state.go)、[runc init 存活检查](https://github.com/opencontainers/runc/blob/v1.3.6/libcontainer/container_linux.go#L825-L861)和 [Linux pidfd 内核接口](https://github.com/torvalds/linux/blob/v7.0/include/uapi/linux/pidfd.h)。

独占 cgroup 的来源是同一 runc 对象的 OCI bundle 中的 `linux.cgroupsPath`。只读取此字段，按 cgroupfs 绝对路径或 runc 的 systemd slice 规则解析。以原生 `ProcessBinding` 的 cgroup ID 核对打开的 cgroup v2 FD，不能用进程名、路径尾部或 SandboxID 代替。发布快照按真实 cgroup ID 索引，每项保存完整对象、init PID 与内核 birth、原生采样结束的 BOOTTIME、pidfd、cgroup FD 及 netns FD。原事件的 cgroup ID 和时间必须命中该有效绑定，成员归属只覆盖已证实的 cgroup 对象。普通事件、进程观察和 TLS 共用快照。Docker 查询、runc 和文件读取在适配的 I/O 工作线程运行。回调接收泵只读取已发布绑定和零等待的 pidfd 退出状态，保持可处理这些查询自己触发的原操作。

Docker HTTP查询与runc状态读取不设适配器截止。原发现任务沿同一admission_stop取消网络future；已经启动的runc子进程在取消或实际错误后仍由该任务等待真实退出，原TaskTracker寿命覆盖清理。真实peer、固定路径、进程／对象身份和文档大小契约继续核验，不能以取消后的空结果发布新绑定。

绑定发布由同一发现任务负责。消费者只在 `watch` 借用期间取得不可变 `Arc` 快照，退出检查及容器范围比较在借用释放后执行。发现异常时，实际 pidfd 检查和失效项释放也在发布锁之外。原操作相位锁只提交结果，随后通知等待者；设备退役先取出 FD 引用，再在 I/O 互斥锁之外释放。

## 容器网络域接线

驱动适配同时持有实例绑定与其入口寿命。导流使用绑定的 cgroup 目录 FD 作为 attach 范围，在核实的 netns 内创建监听和同一会话的 nft 表。标记前仍核原 socket 的 netns cookie。连接接纳沿现有 `AdmittedConnection` 和 `IngressFacts`，只有实际确认的容器成员才填 `ProcessInfo.workload`。cgroup ID 独立保留，未取得成员证据时 `workload` 为空。

容器上游连接由同一受信适配提供。Flow 按[受控出口契约](../flow/tls-egress.md#一次请求的固定处理顺序)核对原对端后，适配在该连接的真实 netns 创建 socket，核对 socket 所在网络域后将流移交 Flow。连接失败直接返回，禁止换到宿主网络重拨。

AllProtected 使用宿主入口及已绑定容器入口，Processes 选择精确的宿主进程实例，Workloads 选择完整容器对象。容器入口按 cgroup ID 和 init 的内核 birth 管理，避免旧计划跨实际进程寿命复用。每个入口复用同一 loader 与 BPF，启动计划携带 cgroup 与 netns 两个 FD。状态的 `accepting` 表示至少一个实际入口在接纳，`generation` 是最近成功采用的入口计划代。每条接纳连接一并移交只允许拨本条原对端的能力。私有网络域由专用线程固定netns创建socket，Flow沿原owner等待真实建连和TLS结果，不附加请求时限；实际错误、对端结束或显式取消各自收尾。

停收与最终退出分开确认。先关闭本实例标记、停止新接纳并撤导流，再按现有 Flow 停止顺序处理已移交的连接。已开始的上游连接操作由原所有者收尾，释放最后一个实例资源后才报告最终退出。绑定失效后禁止用旧引用给重启或重建后的实例接纳新连接。

### R12原件的适用范围

[R12独立复核](../../evidence/runtime-correlation-research.md#r12-同次-toolcall-native-eventkey-研究的当前复核)证明原ToolCall对象、原req/work及原EventKey的具体交接，不证明HTTP→ToolCall，也不证明当前正式源已取得工具字段。现有ToolExecutionFacts、NativeToolContext、NativeRequestReturn、Main消息、业务库和ActivityContext继续复用；原始源契约见[原运行时观察源](runtime-observation-technical.md)，不为尚无真实生产者的高级语义新增占位命令或DTO；已取得且来源与寿命明确的原始情报继续保留，不因当前缺业务消费者而删除。工具语义的正式采集仍按[缺口清单](../gaps.md)记录，不作为原生事实接纳条件。

## DNS查询快照解释

原Native受控／直接事件仍经唯一解码入口。UDP／53 Send的已读取快照由私有dns解释器用锁定domain库解析一次，随后沿原EventData交Main；不再读取目标内存、不保存IP到域名缓存、不新增执行或问询端口。字段、部分读取和解析状态只在[事件契约](../../contracts/events.md#udp-dns-send-查询快照)定义；匹配与能力边界只在[规则契约](../rules/technical.md)定义。它表示原hook读到的DNS查询，不保证后续发送字节未被调用者更改，不解释DoH、DoT或任意网站连接。
