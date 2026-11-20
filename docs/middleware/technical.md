# 中间层WS与业务契约

[产品](design.md)与[架构](architecture.md)分别维护；本页定义中间层接口。正式回环 WS 承载防护管理、问询闭环和聊天工作台。底层事实和命令类型直接使用[消息契约](../backend/main/messages/technical.md)、[现行protocol.rs](../../crates/backend/as-main/src/messages/protocol.rs)、[transport.rs](../../crates/middleware/as-middleware/src/transport.rs)及[流量公共契约](../contracts/flow.md)，不复制其字段表或另写兼容解码。

## 1. 连接与封套

使用成熟 RFC6455 实现，一条完整 WS 文本消息承载一个现行 JSON 封套。协议分片、Ping／Pong 和 Close 由 WS 库处理，应用层沿用 `protocol::MAX_FRAME_BYTES` 校验重组后的消息。version 取现行 `protocol::VERSION`，普通 `message` 内直接使用原 Down／Up。`stream`、`private`、`business` 和 `diagnostic` 的现有表示归中间层 [transport.rs](../../crates/middleware/as-middleware/src/transport.rs)，正式接入共用解析与编码，WS 目标只新增首次鉴权载荷。

| 载荷 | 字段 | 唯一含义／来源 |
|---|---|---|
| authenticate | token: 私有UTF-8字符串 | 本机授权初始化入口交付的客户端证明；不是LLM key或phantom，不接受客户端自报角色 |
| authenticated | access: Access, chat_content: bool | 服务端验证后、与当前底层真实授权相交的权限；复用已有类型与语义 |
| business（已有请求） | id: CommandId, query: BusinessQuery | 本连接内的一次业务查询 |
| business_reply（已有回复） | id: CommandId, result: BusinessResult | 原业务查询的真实回复 |

上述载荷均位于同一 `version` 封套内，每条消息只有一个载荷。出现第二个载荷字段即拒绝，显式 `null` 也占用字段。`authenticate` 和 `authenticated` 由 WS 首次鉴权使用。Unix 与 WS 共用一个普通封套解码入口，载荷从原输入借用，再解析一次为原类型。

BusinessQuery 由 [business.rs](../../crates/middleware/as-middleware/src/business.rs) 唯一声明。现有业务封套承载保护概览、聊天历史、活动和 Ask 查询。BusinessResult 沿用 Completed／Failed 和现行 FailureCode，数据结构见第 3 节。

### 首次鉴权与真实底层授权

连接顺序为 WS 握手、校验首条 `authenticate`、建立底层授权绑定、返回 `authenticated`，然后接纳普通消息。握手本身不建立可答复渠道。身份验证失败终止该连接，认证前的业务消息不送入 Main。重复认证不在原连接内替换身份。token 只在认证载荷传递，原私有输入处理负责保管和释放，诊断记录有限失败原因。

配置模块提供 `middleware.websocket` 的只读快照，字段及接线见[配置契约](../backend/configuration/technical.md#middlewareaccess)。部署者为客户端生成并交付独立的 32 字节随机值，以 64 个十六进制字符作为 token，将其 UTF-8 字节的 SHA-256 摘要配置为 32 字节数组。首次认证只传入原 token，Main 对摘要执行常量时间比较。服务只在显式指定的回环地址监听。

中间层用 tokio-tungstenite 处理握手和首条应用消息，不增加鉴权等待截止。Media封接纳时取消尚未建立会话的原认证future并等待其任务退出；已经登记的会话沿原授权和输出所有权收尾。MainMessages.connect_websocket 在原授权锁内核实当前配置的监听地址、实际握手 Origin 和凭证摘要，登记独立可撤销 Connection。Unix 管理继续由 connect 从实际 OS peer 验证身份，两者共用 Main 的授权登记及一次性问询。

`authenticated` 返回该连接实际取得的 `Access` 和 `chat_content`。ReadOnly 不能答复，声明问询可用及 Answer 都要求 ManageRules。聊天内容授权独立于原 Ask。授权撤销或 WS 断开立即释放本连接的底层能力；重连重新鉴权。浏览器 Origin 检查归接入层，应用身份仍由认证材料验证。回环监听使用 WS，浏览器请求的 Origin 必须与配置相同，原生客户端可省略 Origin。[Main 授权](../backend/main/messages/technical.md#主模块代码接口) · [RFC6455](https://www.rfc-editor.org/rfc/rfc6455) · [OWASP WS 鉴权](https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html)

读取和修改按现行Access区分，聊天正文另检查chat_content；内容scope只收窄。private须ManageService，先核权再解析秘密，零化缓冲，输入／专属回复不进通用日志或广播。底层没有取真实key的接口，中间层也不提供。

直接提交原 Down 命令时沿用客户端 CommandId。Main 为每次提交返回专属回执接收端，接入层只向原连接写出；不同连接可以使用同一 CommandId。单个连接的同时在途命令由客户端使用不同 ID。组合查询的内部命令才分配内部 ID。Driver／Event／Process／Message／Decision 等事实 ID 保持原值。

### 防护管理

正式回环 WS 与 Unix 共用一个命令接纳实现。管理请求使用现行 `{version, message: Down}` 与 `{version, private: PrivateRequest}`。回复使用现行 `Up::Reply` 或 `private_reply`，保留原 CommandId、持久化回执、实际采用值及失败 facts。规则、配置和控制均由 Main 的现有授权与执行入口处理，凭证沿原 `credential_admin`。业务查询和流订阅仍使用各自现行载荷。

| 入口 | 正式 WS 操作 | Main 权限 |
| --- | --- | --- |
| `Down::Rules` | `get`、`get_rule`，以及 `edit`、`update`、`reload`。普通组和 Remembered 共用 `RuleChange`、`RuleVersion` 与原规则文件。 | 读取 ReadOnly，修改和重载 ManageRules |
| `Down::Configuration` | `get`、`update`、`reload`。沿原版本检查、固定来源和保存／采用回执。 | 读取 ReadOnly，修改和重载 ManageService |
| `Down::Control` | `get_status`、`set_runtime_feature`、`resume_runtime_feature`、`request_stop`。临时覆盖只在实际运行态生效；停止接纳仍不等于实际退出。 | 状态 ReadOnly，临时操作及停止 ManageService |
| `private` | `ensure_ca`、`public_ca`、`import_secret`、`replace_secret`、`issue_phantom`、`revoke_phantom`。 | ManageService |

WS 与 Unix 均将现行 `Down` 原命令交给 Main 的同一权限与执行入口，不在传输层建立第二份功能白名单。进程刷新和 `RequestStop` 复用原接口；停止回执的写出见证只在原连接实际完整写出后确认，宿主保管该写出或会话终止的真实结果，仍须等待当前实例真实退出。退役工作可与回执写出并行。实际不支持的能力由所属模块明确返回。所有业务 ID、版本、配置和规则字段引用下节的唯一声明。

私有请求在取得真实凭证管理能力后才解析命令和值。私有回执只发原连接，编码和发送时继续核权。成功签发的 phantom 值只在该次专属回执返回。应用拥有的秘密和专属回执缓冲使用零化内存；共享的输入缓冲及内部协议缓冲由 WS 库持有。正式宿主不启用输出 WS 载荷的库 Trace。普通输出、业务库、诊断和广播不保存私有载荷。`chat_content` 只控制聊天及相关查询，管理权限仍由 Access 决定。

版本冲突使用原失败码。普通 WS 回执从编码到实际写出均核对原会话仍有访问权；因配置变更撤权时停止写出。已接纳的实际写入沿 Main 正常收尾。Main 回执端关闭时，仍在线的原连接沿 `reply_output_unavailable` 和原 CommandId 得到未确认结果。原连接已断开时，重新鉴权后通过原查询读回实际保存和运行状态。断线不自动重放写命令，也不从断线推定回滚。配置和规则持久化结果与实际采用结果分别以原回执为准。

## 2. 直接复用的底层操作

| 业务 | 唯一调用／数据出处 |
|---|---|
| 规则及记住组管理 | RuleCommand、PolicyDocument、RuleVersion、RuleWriteReceipt；[规则契约](../backend/rules/technical.md) |
| 配置及实际采用 | ConfigCommand、ConfigSnapshot、ConfigVersion和实际保存／采用facts；[配置契约](../backend/configuration/technical.md) |
| 临时开关和驱动状态 | ControlCommand、DriverStatus；[驱动通信](../contracts/driver-communication/technical.md) |
| 进程详情、父子／祖先／子树、命中和刷新 | ProcessCommand及ProcessQuery／查询结果；[进程树契约](../backend/process-tree/technical.md) |
| 流量真实状态与原聊天事件 | FlowCommand::Status、MessageModule.consume_chat／原ChatReceiver及ChatEnvelope；对外订阅和历史归中间层；[公共契约](../contracts/flow.md) |
| 问询及实际完成 | Down::Answer／ReviewAvailability、Up::Ask／OperationResult；[消息契约](../backend/main/messages/technical.md) |
| 凭证及公有CA | 现行private载荷和CredentialAdminPort／CertificatePort；[凭证契约](../backend/credentials/technical.md) |

所有字段含义与校验归上述出处。中间层不编译／匹配安全规则、不查询Docker或procfs补身份、不杀进程、不写底层配置／规则／秘密；业务数据库仅由业务日志模块按配置固定路径写入、不解TLS或重新解析provider消息。当前公共 JSON codec 和脚本传输已归中间层；底层只保原值及授权端口，不复制私有结构。

普通原生检查结果仅表示AS在该检查点的允许／拒绝，不能改称文件写入、网络发送或进程退出成功。driver_status.running也不能替代某次结果。OperationResult携原EventData、实际RuleHit及原OperationReport；直接Driver结果decision_id为null，不捏造问询ID，仍可按原EventKey定位。

## 3. 业务查询与结构

### 运行审计

WS 与 Unix 的 `Down::Processes { id, command: Query { query } }` 复用 [ProcessQuery 与 QueryReply](../backend/process-tree/technical.md)。ReadOnly 可读取进程树状态、原 ProcessKey 的节点、父子及祖先、分页列表、容器归属和命中计数。容器筛选只接受原 Workload 实例。缺失归属原样保留；裸 PID 只用于现行 ResolvePid 提示。Get 沿原有按需补取，显式 `ProcessCommand::Refresh` 在两种传输中均由Main检查ManageService权限，不另设WS功能白名单。

`Down::SecurityLog { id, command: SecurityLogQuery }` 复用原安全日志分页结构，`Up::Reply` 的 `security_log` 返回规则日志和写关闭审计。完整记录要求 ManageService，命令接纳、实际查询及敏感回执写出均核原会话授权。业务日志 SQLite 继续拥有聊天、Ask、关系及反馈事实，安全日志由底层日志模块读取并保持原值。[安全日志分页](../backend/security-log/technical.md#只读分页)

### LLM 聊天工作台

正式 WS 使用现有 `stream` Subscribe／Unsubscribe 和 `Up::Chat`，沿 SQLite 的原提交顺序实时读取。续读使用最后一条完整收到的 `ChatCursor`。客户端可先订阅再打开历史，以原消息 ID 和 revision 对照重合记录。授权来自同一连接的 `chat_content`，订阅建立、取数和内容写出均沿现有检查。

| BusinessQuery | BusinessData | 约束 |
| --- | --- | --- |
| `chat_sessions { before: Option<ChatSessionRef>, limit: u16 }` | `chat_sessions { sessions: Vec<ChatSession>, next_before: Option<ChatSessionRef> }` | 模型选定字段和原生真实 Session 分别保留来源；结构唯一见[业务日志](business-log/technical.md#session-与原生聊天业务模型)。 |
| `chat_exchanges { session: Option<ChatSessionRef>, after: Option<ExchangeKey>, limit: u16 }` | `chat_exchanges { exchanges: Vec<ExchangeKey>, next_after: Option<ExchangeKey> }` | 只返回真实模型 HTTP 交换。原生 Session 没有已证 HTTP 成员时返回空页，不构造交换或改查其他 Session。 |
| `chat_messages { scope: ChatMessageScope, after: Option<ChatMessageCursor>, limit: u16 }` | `chat_messages { messages: Vec<ChatMessageEntry>, next_after: Option<ChatMessageCursor> }` | 按明确来源读取原消息／观察；原生顺序与固定读取截止唯一见[业务日志](business-log/technical.md#原生对话顺序与分页变更)，不是统一套 SQL 首次提交序。 |
| `activity_context { anchor: FactReference, attribution: Option<AttributionRequest> }` | `activity_context { view: ContextView, attribution: Option<ToolAttribution> }` | 按原记录和确定关系读取一跳证据。需要候选时anchor必须为Event；同一SQL事务复用现有模式查询，候选不写成确认边。 |
| `ask_context { decision_id: DecisionId, attribution: Option<AttributionRequest> }` | `ask_context { context: AskAuditContext }` | 读取原 Ask 的已提交事实，可组合候选及其原消息窗口；查询不裁决或改变原操作。结构归[审计契约](audit/technical.md)。 |
| `tool_attribution { event: EventKey, pattern: AttributionPattern, after: Option<ChatMessageId>, limit: u16 }` | `tool_attribution { view: ToolAttribution }` | 按显式条件比较原工具与原事件，候选无确认关系写入权限；字段归[审计契约](audit/technical.md#通用操作系统模式)。 |
| `group_delegations { before, limit }` | `group_delegations { records, next_before }` | 分页读取已观察的 A2A 委派及 OpenClaw 工具接纳、子终态和父公开回投原证据，要求 `chat_content`，详见 [Group](agent-graph/technical.md#正式查询)。 |
| `protection_overview` | `protection_overview { driver: CommandResult, flow: CommandResult }` | ReadOnly 即可查询。组合原驱动和 Flow 状态，逐项保留真实结果及失败。 |

历史分页的 limit 为 1 至 64。after／before 保留库内有效的相应来源引用，消息 cursor 另核同一 scope 与原分页依据；不同来源的游标不互换。原生续页沿首屏的 `read_through` 固定读取已提交证据，late window／成员或位置后补在重新读取首页时进入新截止；不建立变化回执或追踪框架。正常 next 只在尚有下一页时返回当前页末项。历史请求沿既有业务封套、SQLite 和原件重建入口；查询事务结束后释放临时视图。响应超过现行单帧限制时，沿原请求 ID 返回 `output_too_large`，保留库内原值。模型实时订阅仍逐条读取原 ChatEnvelope；NativeGateway 不制造 Flow Envelope／ChatCursor，原生聊天从同库历史查询读取。

Dashboard 选择模型字段分组后浏览原交换，选择原生 Session 后直接读其原生聊天；未分组模型消息继续由全部原交换入口读取。`OriginalChatMessage`、`ChatContentValue` 和既有部件显示共用内容渲染；原生分支只展示其有来源的字段，不生成假的模型 source、状态、时间、空部件或 HTTP ID。前端仅严格解码业务类型、提交原选择和展示，不重新解析 Gateway 或做跨方向归组。ProcessKey 只表示来源进程寿命；provider Conversation 独立保留。

保护概览复用原 `Control::GetStatus`、`Flow::Status` 及各自回执，外层请求沿原 CommandId 关联。管理和内容分别沿原权限核验。已建立的 WS 聊天订阅与 Unix 共用停机尾部游标，宿主在封接纳后并行推进源退役和原 writer。

BusinessQuery的保护概览、活动查询与Ask上下文各使用其明确字段，不新增通用SQL／表达式查询／任意shell／任意文件接口。

保护概览保留驱动和 Flow 的原 CommandResult。客户端也可分别发出原状态查询，规则／配置详情沿[防护管理](#防护管理)调用原接口。

activity_context接收一个FactReference，返回ContextView。以下新增类型只描述引用和业务关系，不是新的事实模型：

| 类型 | 字段／变体 | 含义 |
|---|---|---|
| FactReference | event(EventKey)、native_observation(EventKey)、process(ProcessKey)、protocol(ProtocolRecordId)、message(ChatMessageId)、part(ChatPartRef) | 直接使用已有完整标识；ChatPartRef由Flow目标公共契约唯一声明，只指原消息内的ChatPart位置，不是全局工具ID |
| EvidenceRelation | kind、from:FactReference、to:FactReference、evidence:非空FactReference列表 | 关系含义由kind定义，保留明确引用或已定义时序模式及原证据，不承载概率 |
| ContextGap | at:FactReference、reason | reason 还区分 agent_binding_unavailable、command_mapping_unavailable、command_input_unavailable；原 not_provided、not_retained、source_unavailable、no_explicit_relation、conflicting_evidence，分别来自实际缺供给、保留到期、来源失效、无显式桥或标识冲突 |
| ContextView | anchor:FactReference、source_records、relations、gaps | source_records只承载原底层DTO／只读查询结果；不改名复制进程、消息或规则字段，不用空数组声称视图完整 |

FactReference另允许question(DecisionId)和completion(EventKey)：前者只指实际收到的Up::Ask，后者只指实际Up::OperationResult；不能因某完成带decision_id就制造一条未出现的问询。各变体以kind唯一标识，值使用原类型。SourceRecord使用共享不可变值封装原问询、操作结果、协议观察、消息、原生请求交接、原请求返回、private futex选择、原生工具执行事实及已提交反馈阶段；变体及原类型直接对照[context.rs](../../crates/middleware/as-middleware/src/context.rs)。原生请求交接、原请求返回和同步对象事实不自动形成工具因果边，反馈也不替代原操作结果。原进程查询事实承载尚未接入，普通进程查询仍走原 Down::Processes。native_observation只定位原NativeRequestHandoff，不冒充LSM事件；无request也可按原observation直接读取。EvidenceRelation.kind含same_exchange、native_tool_reference、question_completion、tool_execution，以及native_request_reference、native_call_scope和same_thread_return_sequence；后三者的条件和证据级别唯一见[审计契约](audit/technical.md#先连接已证原件与补充时序)。进程树自己的父／创建关系沿原查询结果传递。

新增业务关系只允许：同一ExchangeKey内的请求／响应关系；明确作用域内原生ToolCall与ToolResultReference的引用；原Ask与同DecisionId实际结果；以及实际执行上下文桥明确供给后才可产生的目标tool_execution。底层已给出的进程实例／父关系原样展示。每条新增关系须列出原证据。来源没有完整身份、会话域或唯一引用时不建立关系，不由PID、父子PID、命令相同、相同文本、SNI或时间接近补足。

`SourceRecord::RequestHandoff`只共享原[`NativeRequestHandoff`](../../crates/contracts/as-types/src/runtime_observation.rs)。原始观察不要求ToolCall、HTTP或完整request字段即可入库；只有原`EventBound.event`可把其明确request寿命的已保存前后观察带入同一活动读事务。observation/request身份不会冒充LSM事件；缺Submission、缺工具和未关联记录均保持原件，不合成Question、ToolExecution或成功。存储与索引只由[业务日志](business-log/technical.md#原生请求交接原件)维护。

`tool_execution` 只接纳可信执行来源明确提交的 `Part(ChatPartRef) → Event(EventKey)`。活动模块统一校验方向、非空证据引用、同一原事件和可回溯到原协议记录的终态响应 ToolCall。Ask 窗口和固定拒绝反馈复用该校验。同一事件收到不同工具引用时保留交接原件并返回 `conflicting_evidence`，查询省略该事件的已确认执行关系。没有显式交接时返回 `no_explicit_relation`。证据引用可读仅证明原事实存在，生产者仍须证明实际交接，原协议与原事件并存不能产生关系。

中间层仅接纳Main转交的原RequestHandoff，保留Submission公开消息候选及实际EventBound／ChildCreated／原request事实关系。公开对象同一性和CAPI交接不证明当前工具执行，生产不自动生成ToolExecution。既有ToolExecutionFacts／SourceRecord::ToolExecution查询shape及origin consumer保留给明确受控软件验证，当前工具执行可信发布者及HTTP权威交接仍未实现。原件无完整Tool／HTTP身份也照常采集、入库和读取，不影响HIPS。缺失与冲突按原ContextGap表达。[事实与组合契约](../contracts/flow.md#模型响应与工具的事实交接)

消息的RequestContext、ResponseOutput、角色、状态、quality、原生ID及记录引用原样保留。请求里的历史不当作新发生消息，没有明确会话／轮次边界不称任务原始请求。ProcessInfo不能替代ProcessKey，源instance不能替代boot_id；当前进程信息与事件发生时信息分开读取，不能修写历史。风险仅展示原规则命中、动作和依据，未命中不称安全。

ChatRole::User只是协议角色，不是AS管理授权。模型文本、工具返回或应用自称用户已同意都不能产生答复能力或放行决定。FileTarget.access_mask沿现行包含位语义，包含READ不能改称只读；目录名、进程名或缺失hash也不构成可信判断。

## 4. 问询和订阅

### 原问询闭环

1. 已鉴权客户端用原 `Down::ReviewAvailability { available: true }` 声明可答复。接入模块把声明交给对应 Main Connection，Main 检查 ManageRules 并登记规则模块的真实渠道。认证完成本身不声明用户能够答复。
2. 原执行者接受规则期限和超时动作后，规则模块产生原 Prompt。问询协调选择一个真实 ready 连接，Main 生成 `Up::Ask`，中间层原样转交并保存。默认动作独立于可选动作，前端以原操作完成回执确认实际结果。字段和选择来源见[Main 消息](../backend/main/messages/technical.md#主模块代码接口)。
3. 客户端以原 `Down::Answer { decision_id, decision }` 答复。Main 验证同一连接、同一未结束问询、原允许选择及期限，再交原一次性答复能力。Answer 没有 CommandId，成功接纳不会产生管理命令 Applied 或新的完成 DTO；输入失败沿现行 diagnostic 报告。
4. 驱动在原执行点继续或拒绝，实际完成沿原 MainNotice 投影为 `Up::OperationResult`。中间层保持原 EventKey、可选 DecisionId、命中与 OperationReport。检查点 Allow 仍只表示允许原调用继续；真实文件内容由工具操作及独立验收确认。
5. 当前连接释放、撤权或停止 review 时，Main 释放绑定的 Prompt，由原规则超时动作收尾。同一问询的其他客户端答复、迟到答复及重放均由原接口拒绝。初始无有效渠道时底层只走快速规则，跳过慢查询和询问池。

原 Ask 的业务登记调用唯一 BusinessLog::record_up；SQL 工作接纳后由数据库所有者保管，展示与答复不等待提交。原完成由现有 record_completion／record_up 和媒体消费者接纳；内部完成只在消息入口投影一次，SQL、执行反馈与分发共用同一不可变结果，正式宿主按唯一消费链组合。原 Connection、Prompt 和 StopReplyWrite 等运行时能力只留在其所有者；数据库和 WS 只承载原序列化值。

### 输出、并发与关闭

原 Ask 优先转发，业务上下文缺失以 ContextGap 说明。弹窗选择沿规则模块原 Prompt，经 Main 投影为现行 `choices` 数组。WS 发送答复只说明送出；允许／拒绝、终止和失败取原 OperationResult。

同一原询问只有一个有效答复能力。选择实际已鉴权且声明可答复的会话后固定绑定；断开／撤权释放其底层会话能力，按原规则收尾，不交另一客户端接管同次Ask。其他展示端只接收授权范围内的真实结果。无有效渠道不在中间层模拟一个在线用户。

问询路由读取原 Main Connection 的实时可答复状态。传输保留对原状态的只读弱引用，释放或撤权后即不可用。媒体按登记顺序将同一原 Prompt 交候选槽位，槽位在短临界区内复核可用性并接纳；拒收时将原 Prompt 归还给媒体继续尝试。接纳后该 Prompt 归该槽位，后续关闭沿原规则收尾。已绑定的答复能力由 Main 独占。Unix 为读回已接纳结果而保留连接期间，撤权后的连接同样不能参与新的问询路由。

聊天、工具、关系和交互业务数据只保存于[业务日志数据库](business-log/technical.md)；已消费事件不留内存历史。实时输出从原消费链取得，历史读取已提交结构化数据，不调用底层回放／快照。底层安全日志通过[运行审计](#运行审计)读取。

底层读取、业务处理和每个 WS 写出独立推进，reader 继续接收 Answer。队列出队即移交所有权，消息处理完成后释放原引用。短锁只保护连接／路由状态，锁内不等待网络、SQL 或用户。一个客户端的普通输出背压不撤销其 review 身份，也不阻塞其他连接的答复处理。原生事件处理、原规则期限和消息完成继续由底层拥有。

writer 在应用消息边界优先发送 Ask 和执行结果。已开始的 WS 消息保持原发送上下文，真实断线或撤权结束发送。收到客户端 Close 先释放 Main 授权和原 Prompt，再在没有未完成业务写出的连接上等待 WS 库真实写出 Close。Close写出与应答均不设自有截止，真实I/O错误、对端结束或显式停止解除等待。控制结果入队、完整写出和客户端实际接收分别认定，断线后不自动重发原 Answer。

### 聊天订阅停机收尾

宿主封新请求并撤下问询，保留已登记 Unix／WS 会话的聊天内容授权。已有 `StopReplyWrite` 只在 `Accepted` 整帧本地写出后消费，不设回执写出宽限；连接结束或显式停止取消时保留未确认结果。宿主并行推进原订阅 writer 与底层退役。Flow 和唯一入库消费者退出后，宿主读取 SQLite 最后提交的 `ChatCursor`，writer 用原 `read_next_chat` 写到该游标；已接纳命令先将真实结果移交原控制队列，控制队列与已接纳SQL读取收尾后才确认排空。游标只在整帧写出后推进，正常结束时 Unix 发送 EOF，WS 发送 Close 1000。

原连接权限逐帧检查，真实撤权立即生效。最终游标输出不设额外截止，宿主持有并等待原会话和WS任务真实完成；后续显式终止信号可取消原网络等待，再等待同一任务退出，不中止SQL或底层资源owner。退役期间仍有活动订阅，writer 确认最终游标前发生 Close、EOF、撤权、写入失败或显式取消时，报告 `chat_drain_incomplete` 并保留 SQLite 原记录。writer 确认最终游标后的 WS Close 握手有自己的传输结果。客户端接收以实际记录核对。完整停机复用[停止契约](../backend/main/messages/technical.md#停止请求与真实退出)。

## 5. 交付顺序与来源缺口

as-service 组合同一 Media、唯一业务库、一个 Unix 管理入口和显式启用的回环 WS。首次鉴权后建立原连接授权；客户端声明问询可用性后才参与原 Ask 路由。命令、聊天订阅和业务查询共用现有接纳及权限接口，不按连接重复初始化或消费源。

原始事实保存、确定关系校验、候选查询及页面消费各有独立完成边界。后端现有 ToolAttribution、ActivityContext 可选归因和 Ask 候选窗口，不代表 Dashboard 已完成对应业务；实际差异及下一批建议统一维护于[中间层缺口](gaps.md)。通用工具交接归[底层缺口](../backend/gaps.md#精确因果来源)，固定反馈和 Group 来源分别归其模块契约。参数尚未确定的「记住」交互沿[路线图](../roadmap.md)。

原问询、聊天、管理、Session、原交换导航及各轮执行范围统一引用[真实联调证据](../evidence/middleware-websocket.md)；Open、Execute、Connect 和 Send 的原版运行范围见[原操作现场证据](../evidence/execution-site-capture.md#原版openclaw)。历史轮次、冻结产物和失败反例留在证据文档，本页只维护当前接口与组合关系。

WS依据[RFC6455](https://www.rfc-editor.org/rfc/rfc6455)，连接授权与敏感消息分发参考[OWASP WebSocket Security](https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html)。普通AS协议不改为旧REST／旧项目接口，不递增迭代版本或提供兼容路径。

## 规则固定拒绝反馈的消费

中间层执行反馈是独立业务子模块，不归Ask答复处理；直接拒绝与Ask实际拒绝共用它。消费步骤唯一见[执行反馈技术](execution-feedback/technical.md)。RuleHit固定文本由规则模块提供，反馈请求及实际阶段直接使用[Flow目标公共契约](../contracts/flow.md#固定规则拒绝反馈目标契约)，不在本页复制字段、不提供动态生成文本／任意正文写命令。

RuleFeedbackFact 沿原 ChatEnvelope 分发。活动详情按已确认的原拒绝和工具调用读取已提交阶段，沿原证据视图返回。prepared 和 applied 分别表示准备和实际改写，submitted 表示上游发送已确认。正式完成消费者已经调用反馈接口，真实工具执行来源及stock自动追加正例仍未完成，不能把消费入口接通称为提示已发送。[反馈契约](execution-feedback/technical.md#必须如实暴露的缺口)

## Agent因果链与Ask窗口

发起者、工具请求、命令数据、实际actor、系统回执及Agent工具结果分别保留。命令比较只是内容核对，不形成EvidenceRelation。新增AskAuditContext／AskMessageWindow／CommandCheck唯一见[技术模型](audit/technical.md)；窗口从中间层数据库取得原消息顺序与修订，不从 Flow 查询历史。通用工具执行桥未实现，缺桥时返回明确 Gap，不能推定任意 Agent 的完整因果链。

对外WS JSON由本接入层一次组装；同进程底层接口交类型化原值／Arc，不反序列化自己刚编码的JSON。Unix JSONL 和正式 WS 共用现行数据封套及 Main 授权，帧分隔由各自传输处理。

### 应用会话目录

目录与聊天的业务类型、原标识作用域、成员证明、修订和存储只在[业务日志技术契约](business-log/technical.md#session-与原生聊天业务模型)定义。本页只定义对外查询和共用 Gateway 来源。目录 reference 是原件观察引用，不是 AS 生成的应用 Session ID；ModelHTTP 和 NativeGateway 的分组域各自核验，跨来源没有自动合并。

NativeGateway 目录按业务日志定义的已核 Gateway 发行域和原 Session ID 延续会话，capture 只限定原件与 RPC 连接。每个原聊天观察保留自己的实际连接来源；创建请求首问的正文仍读原请求，接纳后的 Session／运行仍引用原成功回执。前端不从代表性锚点连接推定所有消息同源，不重新解释方法角色或进行跨连接合并。

所有 Session／聊天查询均经 `business::start_query` 取得本连接真实 `ContentAccess`；读前、结果返回前和实际敏感写出时复核同一授权。Gateway 的进程／socket 证明、原应用认证结果与 AS 客户端授权各有独立含义。缺 Session 不阻止已有未分组模型聊天、原 Ask、答复及普通 HIPS；这些业务不等待会话解析。

### NativeGateway 共用来源与解析入口

本节是公开 Gateway 共用来源、原件引用与解码窄接口的唯一出处。原始采集字段仍引用 [NativeGroupTcp／原驱动通道](../contracts/driver-communication/technical.md)，TCP 与 I/O 接纳规则沿[既有持续采集](agent-graph/technical.md#持续采集的来源边界)。端口显式配置、完整 TCP 握手、双向实际 send／recv 及各端非零 socket 身份核验后，原 HTTP GET／101 Upgrade 配对；`Sec-WebSocket-Accept` 核对实际请求 Key。101 不是 Agent 身份、应用登录成功或任意内容管理权。

WS 复用锁定 tungstenite 的帧头、遮罩、分片与 UTF-8 实现；纯内存输入禁止写出，不连接或答复目标。只有核验后的完整原消息进入 JSON 解析。未知扩展、缺字节、冲突、未核收发归属等仍由原 owner 保存实际原件和缺口，不扫未核片段中的 Session，不将 HTTP 请求当作 WS。公共 JSON codec 是 `as_types::json::parse`；重复对象键不产生业务身份事实。

Session 的应用身份与聊天归属只消费服务端实际声明及其真实原来源，客户端进程身份不是应用 Session 的身份组成。这里的 `GatewayConnection` 表示既有双端核验来源；Session 经此入口继承的是该来源能力，不代表所有服务端自有观测都必须证明对端进程。Group 的跨 Agent 关系仍沿原双端证据判定，不能为了 Session 伪造 client、cookie、SYN 或 Upgrade。

`NativeGroupTcp::Transfer` 只提供本次实际 actor、socket cookie、方向、端口及成功字节区间，不携带 payload、地址或 socket netns；字节由受上述范围限制的 Packet 另行提供。不能单凭 Transfer 恢复一条 WS 消息，也不能由 `sk/msg` 在原回调中可访问推定字段已经导出。容器／远端的服务端自有消息接入所缺真实源契约见[既有待决项](../archive/unresolved-module-contracts.md#session-服务器自有消息的真实来源)；不把未取得事实填默认值后套成此已核连接。

#### 共用来源与原引用

- `GatewayConnection` 复用原握手类型：`capture: [u8;32]` 是该已核 TCP 连接原身份；`source: [u8;16]`、`generation: u64`、`first_syn_ordinal: u64` 分别为驱动来源、初始化代和首 SYN 来源序号。`client_port/server_port: u16` 是实际 IPv4 回环端口，范围 1..65535 且互异；`client/server: ProcessKey` 为双方实际进程寿命。`client_socket_cookie/server_socket_cookie: u64` 是各端原内核非零 socket cookie，分别核验，不混为同值；数据包／send 取发送端 cookie，recv 取接收端 cookie。u64 覆盖原类型范围，JSON 为十进制字符串。连接不表示应用会话或 Agent；capture 的来源算法沿原采集 owner，不由查询方指定。
- `GatewayReference { id: u64, capture: [u8;32], from_client: bool, frames: Vec<GatewayFrameReference> }` 直接复用原 Group 查询的原件引用，并供两个业务共用。`id` 是本业务库正常原消息正行号，范围 1..i64::MAX、JSON 十进制字符串；`capture` 与 `from_client` 是该原消息真实连接及方向，true 为 client→server。`GatewayFrameReference { offset: u64, length: u64 }` 按原帧顺序列出本方向流偏移和 wire 字节长度，均为十进制字符串；列表非空，length 为正。offset 从 SYN 后首应用字节起计，包含 Upgrade 字节，不是 TCP32 位序号、时间或消息序号。正常帧位置不重叠；Gap 原片不套此正常消息约束。
- `GatewayFieldReference { message: GatewayReference, pointer: String }` 指一个原消息内的实际 JSON 字段／对象。pointer 为 RFC6901 JSON Pointer，`""` 表示根；数组位置从 0 起。它不指重编码 JSON 的字节偏移。引用只指已存原件，不携第二份正文。
- `GatewayDocument` 是私有调用期值：借用上述已核 connection／reference 与唯一一次解码得到的 JSON 树。不序列化入库、不派生载荷 Debug、不跨消息缓存、不构造第二份 source DTO。共用窄入口 `decode_message(&Message)` 在事务前解码原完整 WS 消息，返回 JSON 树或 `GatewayDecodeIssue`；事务分配原引用后，以显式 connection、reference 和该树组合借用文档。后者仅有 `non_text` 或 `invalid_json`，重复对象键属于 invalid_json；原件始终保留。读取原件时复用同一入口，不另写查询专用 decoder。

`UnverifiedGatewaySource` 仍只含 `source/generation/first_syn_ordinal/client_port/server_port` 五项原来源字段，类型与上述对应字段相同。它没有已核 capture、ProcessKey、Session 或 Agent。原字段值、范围及未核原片只由原采集和业务原件 owner 保存；不能从邻近连接补齐。

#### 窄调用与边界

`decode_message → business_log::sessions::project_gateway / agent_graph::gateway::project` 是一个原文档、两个纯业务投影；函数名描述所属私有落点，不新增可写外部端口。前者返回[Session／聊天结构化投影](business-log/technical.md#session-与原生聊天业务模型)，后者只返回[原 Group 事实](agent-graph/technical.md#gateway原件派生字段与查询引用)。两个投影不再次解析 JSON，也不查询对方实现。业务日志 facade 在事务前完成一次 JSON 解码，唯一 SQL 工作保管原 WS Message 和该树，在事务分配原引用后借树完成纯投影；晚到配对所需的已存引用由 SQL 显式读出、作为参数交纯业务函数。

原 wire、Upgrade 和事实引用沿既有业务库 owner 保存一次；原消息 JSON 树在该次消费／读事务结束后释放。原件含认证材料时仍保持原值和既定访问范围；会话查询仅返回本契约定义的会话／聊天字段及原引用，不扩展为握手、`connect` 认证参数或任意原帧下载接口。模型、终端诊断和普通广播不输出 Key、Token 或 HTTP 认证头。独立浏览器见证只作核对，不存在导入为生产来源的入口。
