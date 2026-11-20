# 中间层与主模块消息契约

公共引用：[MainContext公共上下文](../../../contracts/main-context.md)、[模块唯一职责](../../../architecture.md)、[产品设计](design.md)、[模块架构](architecture.md)。公共定义以链接目标为准，本页不另定义一份。

消息封装归主模块，拥有中间层端口和驱动端口；下行字段与原生分段buffer见[驱动契约](../../../contracts/driver-communication/technical.md)。中间层协议采用现行version=2的结构化DTO，不兼容旧命令名，也不把二进制驱动帧直接传给中间层。

## 下行

每条管理命令有原CommandId。请求封套不能自报身份、角色或authenticated，也不能指定底层写入路径／原生句柄。已获ManageService的主体可在ConfigDocument内修改middleware.peer/access，这是修改持久授权策略，不是为当前请求自授权限；本条命令仍按修改前的实际会话授权，旧连接随后重新鉴权。

- `configuration.get`：读取ConfigSnapshot和只读ConfigPaths。
- `configuration.update { expected_version:ConfigVersion, document:ConfigDocument }`：底层配置模块实际保存和校验；需要ManageService。
- `configuration.reload { expected_version:ConfigVersion }`：读取同一固定源；不改变自保根或配置源。
- `rules.get / rules.get_rule{rule_id} / rules.edit{expected_version,change} / rules.update{expected_version,document} / rules.reload{expected_version}`：普通及记住规则模块唯一读写入口；字段与修改语义只在[规则管理契约](../../rules/technical.md#81-单条与组管理的唯一数据结构)定义。Get／GetRule要求ReadOnly，其余要求ManageRules。单条读取回复Rule，Edit保存复用RulesSaved及真实驱动采用确认。
- `control.set_runtime_feature{feature,enabled}`：暂时明确开启／关闭指定普通防护。
- `control.resume_runtime_feature{feature}`：移除覆盖，采用当前持久默认；不等同于强制开启。
- `control.get_status`：主模块读取实际驱动状态。
- `answer{decision_id,decision}`及`review_availability{available}`：原问询答复与真实渠道可用性。
- `control.request_stop`：ManageService请求当前宿主一次性接管停止；只按[停止请求契约](#停止请求与真实退出)返回Accepted，不称已退出。

feature为process／file／network普通防护；不包含停用自保或修改自保位置。临时值不写盘，手动恢复或完整重新初始化后清除；普通重载、状态查询、重连不清除。

## 上行

- `reply{id,result}`：原命令最终结果；只有真实保存和必需采用完成才整体成功。失败携带saved_version／applied_version等真实事实，不把已发送或已保存冒充全部生效。
- `driver_status{snapshot:DriverStatus}`：原驱动上报／查询结果，字段只按[DriverStatus唯一定义](../../../contracts/driver-communication/technical.md#4-运行时与持久状态分开)，不在中间层DTO另复制一套；只有驱动是这些实际值的来源。
- 底层通过内部DriverUnavailable通知实际媒体消费者来源失效；当前Up未声明独立transport_unavailable DTO，不伪造一条已支持消息。历史快照不能冒充当前值。
- `ask`与`operation_result`：保留原请求身份及真实执行结果；不发送ask_closed或自行决定界面关闭。

主动状态与命令回执都携带相关驱动revision，回执可说明原请求在某版本成功，但不能覆盖更新的状态。中间层首次连接主动查询真实状态，不由本地期望值补全。

## 所有修改在底层

配置模块新增update并原子保存固定config/as.yaml；规则模块写固定config/rules.yaml。自保根只由配置模块内部定位和缓存，消息封装只借用其ConfigSnapshot／ConfigPaths，Update正文不能覆盖；中间层不获维护票据、写FD或任意WriteFile能力。

主模块消息封装先认证连接，再按read_only／manage_rules／manage_service授权。身份来自本机真实凭据／受保护端点，同连接复用；修改授权策略时撤销旧权限并重新鉴权。普通规则Allow不提供任何服务管理权。

驱动回调端口只在[DriverCallbacks](../../../../crates/contracts/as-types/src/driver.rs)定义：controlled与events是同会话的两条有界职责入口，不是新驱动连接或业务总线。原受控操作独立交Fast，辅助原件在Messages消费；Native答复和原操作完成仍由唯一接收泵关联。辅助入库／日志等待只保管其自身原件，不能使ControlPriority受控读取与现有统一Ask入口停止派发。NativeFact只将原RequestHandoff／RequestReturn／FutexSelection三个运行时tag移到MessageModule.consume_native_fact窄接口，仍复用原结构体和业务日志record_*；完成／状态保留MainMessage原通道。事实SQL等待不经过控制notices。Main不在该端口重建询问、规则、原操作或第二份事实缓存。退出和两任务真实join顺序以[主模块](../technical.md#取消与退出实现)为准。

底层给每个驱动请求分配session内request_id，并关联上层CommandId；大镜像分段仍是同一逻辑请求。驱动答复由唯一接收泵关联后唤醒，不在等待者占住的队列中自锁。操作已开始后，由实际任务保管到保存／采用／退出真实完成，不因中间层断开而遗弃。

普通日志不主动全量上报，查询只调用实际数据提供者。ProcessQuery::Get异步调用进程树单次缺失补取接口；其它树查询仍只访问缓存。补取复用ReadOnly查询授权，不新增逐次确认，显式刷新仍按既有ManageService权限。进程树详情／父子／祖先／子树／命中和分页结构见[进程树接口](../../process-tree/technical.md#7-对外接口和查询结构)，已由Down::Processes接入，不能回空成功占位。普通日志尚未明确的DTO先[存档](../../../archive/unresolved-module-contracts.md#日志与查询数据)。LLM当前实时端口见本页Flow章节；没有存档实现时不返回假空历史。

## 主模块代码接口

[MainMessages](../../../../crates/backend/as-main/src/messages/management.rs)由MainAssembly组合真实配置／规则所有者、调度句柄和窄DriverPort。connect通过操作系统peer_cred执行首次鉴权；submit接受现行Down，管理请求返回原CommandReply的接收端，wire仍只编码唯一Up。Answer／ReviewAvailability直接交原能力接口，不制造管理Applied；运行时包装定义见[停止请求契约](#停止请求与真实退出)。

connect 验证真实 UnixStream 的 OS peer。connect_websocket 在同一接纳锁内核验当前配置的监听地址、实际 Origin 和首条认证 token，复用 Connection 的撤权、review 和单活动问询。Connection 为传输路由提供对实际可答复状态的只读弱引用，原授权和渠道生命周期保持唯一。[首次鉴权与连接接入](../../../middleware/technical.md#首次鉴权与真实底层授权)

授权层级为ReadOnly、ManageRules、ManageService；ReadOnly仅查询，问询渠道及答案需要ManageRules。管理任务进入队列并取得持久修改锁后再次检查原授权。临时控制／状态使用driver_urgent独立接纳容量，不被持久修改占满普通消息容量所阻塞。授权变更与连接登记由短临界区协调；撤销会立即撤掉旧渠道可用性，传输对象释放也会撤权，排队令牌不延长连接寿命。

`MainMessages::question(connection, prompt)` 接收规则模块的原 `Prompt`，向实际 ready 会话生成现行 `Up::Ask`。`Prompt::choices()` 是[规则模块](../../rules/technical.md#4-条件和动作的范围)提供的唯一选择集合。`PendingQuestion` 仅保存 `(Authorization, Prompt)`，Main 从原 Prompt 投影 `Up::Ask.choices`，答复时读取同一集合。错误 ID、其他会话、未提供的选择和已关闭／过期答复均拒绝。答复消费原一次性能力，执行结果仍由原执行者提供。传输所有者须在收发失败时释放连接。MainThreads只登记一份MainMessages，重复取得返回同一实例；close与接纳共用短临界区并关闭有限命令tracker。已接纳命令在实际驱动会话关闭前排空，长活消息消费者在原生生产者结束后另行join。

配置／规则已经保存而必要驱动采用缺失时，回复Failed(Unavailable)并附真实保存回执；reload附读取快照。驱动端口缺失返回Unavailable，配置里的true不能变成实际状态。停止请求未绑定真实宿主、监听者掉线或一次性端口已耗尽时返回Unavailable，不回复Accepted或推定已停。reload_all 和驱动升级尚未接入。原安全日志分页由 ManageService 通过 Main 转交日志所有者。[查询契约](../../security-log/technical.md#只读分页)Flow只暴露真实状态，聊天历史归中间层业务库。

## 停止请求与真实退出

本节是停止请求及运行时回执能力的唯一字段／接口文档，源码位于[protocol.rs](../../../../crates/backend/as-main/src/messages/protocol.rs)、[messages](../../../../crates/backend/as-main/src/messages/mod.rs)和[runtime.rs](../../../../crates/backend/as-main/src/runtime.rs)。仅使用当前RequestStop接口，不保留旧停止命令别名或解析路径。

- `ControlCommand::RequestStop`没有PID、unit、路径、shell或客户端期限；唯一目标是当前Main实例的宿主。它使用ManageService、原urgent容量及队后复查，最终接纳与授权采用／关闭共用原authentication短临界区，不跨await。
- wire回执为`CommandResult::Accepted { data: ReplyData::MainStopRequested }`，沿原Up::Reply和原CommandId关联。Accepted只证明真实宿主oneshot已接纳；Completed仍只用于要求的效果实际完成，不新增MainExited广播／停止状态缓存。
- `MainThreads::bind_stop_listener(&mut self) -> Result<oneshot::Receiver<MainStopRequest>, ModuleError>`仅一次，且须在MainAssembly组合MainMessages前调用。未绑定例子不冒充有停止能力；重复绑定或在组合后绑定Conflict。
- MainMessages::new借原MainThreads取得执行端口、进程树和停止端口，并经其原management_entry只构造一次；MainAssembly::messages调用同一入口，重复调用返回同一Arc。第一次组合封口未绑定状态，随后不能补绑停止监听者；不留脱离MainThreads.close／drain的独立MainMessages入口。
- `as_main::runtime::MainStopRequest { reply_written: oneshot::Receiver<()> }`仅进程内，不serde；该接收端观察的是本次Accepted完整本地写出，不是前端已读或进程退出。
- `as_main::runtime::StopReplyWrite`字段私有、不可Clone／serde；`written(self) -> ()`只由原writer在Accepted整帧写成功后消费。入队、编码、未写完、EOF、取消或错误都只drop能力；观察者已结束不把实际Written改成另一种物理结果。
- `as_main::messages::CommandReply { message: Up, stop_reply_write: Option<StopReplyWrite> }`只包装同一原回执的运行时能力，不是第二个可序列化DTO；不Deref、不隐式转换或保留旧Up接收端接口。普通命令能力为None。
- `MainMessages::submit`正式返回`Result<Option<oneshot::Receiver<CommandReply>>, ModuleError>`。transport把message与能力作为同一当前输出工作移动，Arc只可装原Up，不能clone／广播停止写出能力。宿主接纳后即使原回执等待者被取消，停止请求仍有效；原commands tracker不等待宿主退役或socket写出。

宿主收请求后停止新socket接纳，调用原MainMessages.close封管理授权和Ask，继续poll原session写Accepted。回执尝试只由reply_written真实确认、session结束或显式终止信号收敛，不设置固定宽限或额外网络截止；这些结果分别记录为written、unconfirmed、session_ended或cancelled，不推定进程已退出。随后由原MainThreads退役并等待宿主数据库等原资源；原订阅在真实清理期间继续推进。敏感半帧遇撤权仍按安全边界关闭，不能插入另一JSON保证回执到达。

正式 as-service 的 Unix／WS 订阅由宿主调用 `shutdown_draining_content`。该入口沿相同退役实现封管理和释放问询，保留原内容权限至订阅 writer 排空。配置策略变更仍撤销原授权。宿主保管连接和最后 SQLite 游标，见[聊天订阅停机收尾](../../../middleware/technical.md#聊天订阅停机收尾)。

客户端收到Accepted也不能称服务已经退出。未取得回执时只保留未确认，不重发或从socket消失推退出；父Child.wait／原实例pidfd或服务管理器的本次结果才是实际退出证据。宿主必须保留shutdown和自身资源close的真实失败，不以收到请求、发出信号或旧实例日志跳过检查。

现行DTO见[protocol.rs](../../../../crates/backend/as-main/src/messages/protocol.rs)。二进制纯codec在[共享driver_wire.rs](../../../../crates/contracts/as-types/src/driver_wire.rs)；[DriverPort](../../../../crates/backend/as-main/src/messages/driver.rs)保管同一真实会话与候选／采用代租约，不编译平台镜像或重复打开配置。原生I/O由[底层驱动适配](../../driver-adapter/technical.md)封装，没有旧解码或第二条初始化路径。

所选连接失效与全局渠道数分开：问询分配给某个已鉴权连接后，该连接Drop／撤销／停止接收问询都会释放其原Prompt，按原规则超时动作收尾。另一个仍在线的连接不能接管这次答复；它仍可接收后续合法问询。此语义不依赖所有渠道同时离线。

完成通知MainNotice包含真实decision_id（无对应原生等待身份时为None）和实际Result。log=false不关闭已进入底层的完成通知。protocol.operation_result将Controlled及实际Driver直接结果投影为Receipt或Failed，沿同一Up::OperationResult保留原EventData和实际RuleHit；直接结果不伪造decision_id，消费者以EventData的原EventKey定位该事实。无命中时hit为None；proposal是可选的OperationProposal{decision,reason}，源先完成而未选提案时保留None。Observation不是原操作完成，不生成该结果。原生Driver直接结果按实际log选择或原规则派生的report_denial提供；固定拒绝反馈不因log=false丢失其必要完成事实，未收到通知也不能推定没有操作。DriverStatus也使用纯协议投影。正式中间层已消费这些出口，WS 问询回执沿[原问询闭环](../../../middleware/technical.md#原问询闭环)传递。远程 CS 接入另有归属。

原操作Receipt新增真实TerminationReport，复用[原操作终止契约](../../../drivers/linux/operations.md#终止与释放)及as-types::operation，不在中间层复制枚举或重新杀进程。proposal保留提出的决定，requested和单项信号结果只取原执行者；信号接纳不叫进程已退出。

## Flow 输出及内容授权

Down::Flow 只提供真实 Status；没有 Snapshot、ExchangeContext、History 或 ID-only Prepare 命令。Main 仅组合 FlowStatusPort、凭证授权端口及 MessageModule的唯一聊天消费端口，不保留聊天／完成缓存。

ContentAccess 只绑定原真实 Connection 的当前 chat_content；中间层查询业务库前后均核权限。规则固定反馈是后端进程内执行能力，不绑定前端会话。Main 不再保留无人消费的反馈端口转交；Flow 的现有窄端口与当前生产接线缺口见[公共契约](../../../contracts/flow.md#固定规则拒绝反馈目标契约)及[执行反馈](../../../middleware/execution-feedback/technical.md#必须如实暴露的缺口)。外部 JSON 不能登记拒绝、因果桥、原消息或反馈写入能力。

Main将Flow原ChatReceiver交给同一消息模块的consume_chat，接口只在[消费端口](../technical.md#消息与聊天消费端口)定义；不再包装成MainMessage或另一条聊天队列。中间层保存原ChatEnvelope，前端从已提交数据库单条读取；入队、SQL提交、socket写出和前端接收是不同事实。Ask 原 Prompt／期限与聊天存储及查询独立，不等上下文或 SQL 才展示，不增加权限确认。

唯一正式原件入口为 `MainMessage::RequestHandoff`；原Submission及实际EventBound／ChildCreated作为raw、公开消息候选和CAPI交接事实保存，不自动升级为当前工具执行。ToolExecutionFacts查询shape保留，但生产可信执行语义发布者未实现。没有独立ToolExecution消息或直接正文writer；原始观察无完整工具语义也照常消费。字段、来源和发布顺序以[Flow 公共契约](../../../contracts/flow.md#模型响应与工具的事实交接)为准。当前没有对外提交交接的命令。

对外 JSON codec、脚本实时订阅和私有载荷封装属于[中间层接入](../../../middleware/smoke/technical.md)。as-service 同时组合本机 Unix 管理和显式启用的回环 WS 问询。聊天历史及关系查询只走中间层业务数据库，底层不再提供内存回放恢复。

## 凭证私有管理入口

复用现有已鉴权连接及ManageService，通过公共CredentialAdminPort调用所属模块：credentials.ensure_ca、credentials.import_secret、credentials.replace_secret、credentials.issue_phantom、credentials.revoke_phantom；只读导出公有CA调用CertificatePort.public_ca。参数／回执类型只在公共契约维护，不在本页复制。

导入真实值和创建时返回phantom使用专属私有载荷，禁止通用请求日志／Debug／普通事件广播；普通配置只保存非秘密引用。中间层不指定写入路径、不读取真实key或CA私钥，参数不能变成任意读文件／shell命令。成功回执来自真实模块持久提交，配置保存和网络应用分开；MainMessages::credential_admin仅对ManageService返回保留原连接授权的私有端口；每次调用重核授权，返回phantom时再次核权。私有载荷不进入Down／Up通用codec，不实现普通取密或任意路径命令。

Flow变更需要重建时回复实际保存及ReinitializeMain；CredentialSettings变更由同一Main入口立即交凭证所有者采用。未配置Flow时查询返回Unavailable，不生成空消息。聊天持久查询由中间层业务日志执行，不在 Main 增设存储入口。

普通管理任务保持有限命令容量及原 Messages 调度／tracker，控制与人工答复使用原有职责。私有输入受 MAX_SECRET_BYTES 约束，超限明确失败。

配置保存／Reload只要已经发布了新快照，就先收敛原会话授权，再采用凭证或其他模块；无关模块采用失败不能跳过撤权。失败仍附实际保存／读取facts，不以旧grant保留管理能力。完成／失效通知与普通日志的分流只在Main现有消息职责内进行，不承诺已实现远程可靠传输或持久日志。

中间层[防护管理](../../../middleware/technical.md#防护管理)在本机 Unix 和正式回环 WS 共用原命令接纳，普通 Down／Up 保持原格式。stream/private 为互斥专属载荷，秘密沿私有端口。WS 的独立会话继续复用原问询协议与原授权。

OperationResult的公开值统一由protocol.rs同名struct定义，Up::OperationResult只封装它，JSON仍为原字段。中间层 codec.decode_operation_result 只在实际来源鉴权后解码当前完整封套；EventData按原header.kind选择对应payload并共享现行操作／decimal校验。数据反序列化不产生OriginalOperation能力，也没有外部登记完成／拒绝的下行命令。

`Up::Ask.event` 与 `OperationResult.event` 使用 `Arc<EventData>`，分别共享原 `Prompt` 和 `MainNotice` 的事件。进程内投影只复制引用；跨进程发送仍在现有媒体边界序列化原字段。答复修改原操作的决定状态，不能改写已发布的事件事实。

`Up::Ask.timeout: Decision` 来自原操作已接受的 `ReviewBinding.timeout`，沿原 Prompt 一次传递。与 `choices` 分开，完整保留 Allow 或 Deny 及其终止范围。`remaining_ms` 为创建原 Ask 时剩余时间的毫秒快照，实际等待仍以原操作的期限为准。是否已按默认动作执行取原完成回执。
