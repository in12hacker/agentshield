# 流量公共数据与窄端口

本文定义当前底层公开事实、受限能力值及窄接口的含义；未实现目标扩展单列，不冒充已有Rust字段或端口。完整Rust字段／当前JSON表示唯一位于[flow.rs](../../crates/contracts/as-types/src/flow.rs)，中介入口接口位于[driver.rs](../../crates/contracts/as-types/src/driver.rs)。消费者引用这些声明，不重新定义DTO。设置及固定路径归配置所有者，在[配置契约](../backend/configuration/technical.md#flow运行时配置)维护，不归Flow私读文件。

原先混入当前接口的被动CaptureEvent、聊天正文修改和数据库伪schema保留在[归档草案](../archive/flow-design/flow-contracts-before-unified-audit.md)，未实现部分不是当前端口，也不构造空对象表示已经接入。

## 基础类型和数据约束

复用已有[ProcessInfo](../../crates/contracts/as-types/src/dispatch_event.rs)、[ProcessKey／EventKey](../../crates/contracts/as-types/src/process_tree.rs)、[Workload／ContainerInstance](../../crates/contracts/as-types/src/workload.rs)、[ConfigVersion](../../crates/contracts/as-types/src/configuration.rs)。公共层不导入任何功能模块实现；必要私有字节、TLS验证能力和有界接收端依赖成熟基础库，不能成为配置定位、调度或业务会话服务。

- InstanceId为128位来源代。Sequence／Revision在所属代内单调前进，不能复用旧键；ns为宿主CLOCK_BOOTTIME纳秒，utc_ms为实际采集的Unix毫秒。
- 新增JSON u64用十进制字符串编码，包含消息／凭证修订、游标、入口计数和时间。`NativeIdValue::JsonNumber(String)`取锁定Serde Number的任意精度表示，不先转浮点；它不是逐字节数字token。该库会将`-0`表示为`0`、`1e400`表示为`1e+400`，原始数字写法仍在同一协议原字节中，不从该派生表示重造原文或声称词法一致。
- 原JSON值解释归唯一`as_types::json::parse`；对象即使含数值库内部同名字段也仍是对象，不得按字段名称把它改成数值。合法任意精度数值不按f64范围裁掉，实际空值类型保留；重复解码键仍明确拒绝。复用锁定Serde的既有嵌套校验，不另设业务JSON深度或轮询门槛，不在业务查询中重写SSE／JSON解析器。
- None/null只表示未取得该值。真实空字符串、空数组、数组空项、0和false保留，不作缺失哨兵。
- 私有材料没有普通Debug／Serialize。已采集字符串／JSON／字节按既有鉴权和资源范围原样输出，不主动脱敏或改写；原始协议字节不等于解析后重新编码的JSON。
- 正文、原生ID及协议记录只交已认证且具chat_content权限的实际会话。采集到的内容、原生ID和协议记录保持真实值，不因看似敏感而遮蔽；实际未取得仍明确缺失。

## 来源事实

ActorFacts只含核实过的连接发起实例、已取得ProcessInfo、调用TID／task birth、cgroup及采集时间。缺失不能补成中介PID、Host或推测的容器身份。cgroup／namespace数字单独不构成ContainerInstance。

ChannelRef标识来源代内的真实原socket，kind=Socket，object_id是原socket cookie；对象生存期与来源代共同限定身份。裸FD、网络四元组或TLS指针不替代它。当前没有TlsSession来源。

IngressFacts携同一真实发起者、原socket、原目标IP／端口、采用计划代和配置内容标识；AdmittedConnection 将该事实、接纳的异步字节流和同一来源的 UpstreamConnect 能力一起移动给 Flow。UpstreamConnect 无调用方指定地址参数，仅为此次接纳的原 TCP 对端在原网络域创建独立上游流。它没有中间层反序列化入口；客户端不能用请求头自报PID或来源IP创建接纳能力。

网络连接、TLS 协商与原请求读写沿原异步 I/O 自然等待，不设置 AS 自有计时截断；实际错误和显式取消保持各自来源。FlowModule.stop 停止接纳并等待所持任务真实收敛，返回 Result<()>，不生成估算的 drain／中断计数；Main 只组合原停止与完成接口。

connection_initiator只说明谁建立连接，不说明每个HTTP请求的当前写线程。共享连接不自动提供消息生成者／工具调用身份；底层不由token、时间或进程名反推来源。

## 原始协议观察

ProtocolObservation沿ExchangeKey／ProtocolRecordId定位一次真实HTTP JSON正文或SSE事件。ProtocolSource保留实际发起者、channel、方向、profile、origin、route标签、配置修订、接收时间与凭证使用引用。中介原始记录没有BPF明文样本，不能伪造采样CaptureId。同一交换方向的原记录与聊天投影共享不可变的 ProtocolSource，线上字段值仍按原结构序列化。

受控 TLS 为同一 HTTP 交换分别构造 `client_request` 和 `upstream_response` 来源。两侧复用同一次接纳的连接发起者和通道、路由、配置修订及实际凭证使用引用和作用域。观测时间和只属于原请求的头字段各自保留。

`ProtocolSource.request_traceparent: Option<String>` 保留实际模型请求中唯一且符合 W3C Trace Context 00 格式的原头值。仅在 Flow 已接纳、授权原连接后，从将实际提交给上游的头读取，归入 `ClientRequest` 的原 `ExchangeKey`。缺失、重复、非法、被逐跳清理移除时为空；响应方向为空。它只记录交换中的原协议值，采集方仍需证明对应追踪 span 的来源。Flow 不读取其他追踪头，也不从该值产生任何业务关系。[W3C Trace Context](https://www.w3.org/TR/trace-context/)

EvidenceBody的Bytes保持TLS明文经HTTP content decoding后的原字节；offset属于该协议单元的解压后字节域。只有真实未取得内容使用Missing并保留实际位置／范围，片段不重叠，不凭空补字节。不能把HTTP帧、TLS分片或重新格式化JSON当作同一个原始协议单元。

NativeProtocolId保留协议明示的namespace、真实JSON Pointer、原类型和值，只提取指定profile和方向的已知路径。工具input、arguments和schema的同名业务字段不是provider ID。不同来源／端点／账户上下文的相同文本不证明同一调用；call_id、EventKey、DecisionId不得互填。

`ProtocolObservation.request_session: Option<SessionObservation>` 保存配置选定的原应用会话字段。仅在 `client_request` 读取。 `SessionObservation { field: SessionField, values: Vec<SessionValue>, issue: Option<SessionCaptureIssue> }` 保留来源位置与实际值。SessionValue 的 Header 变体保存原字节，Json 变体复用 `NativeIdValue`。重复头保留各值并标记 MultipleValues；未取得标记 Missing；JSON 值类型不支持标记 UnsupportedValue；正文尚未成功解析标记 UnparsedBody。字符串空值原样保留。Flow 在现有正文单次解析中读 JsonPointer，Header 只读实际原请求。该结构与 provider 的 Conversation ID 各有自己的来源和作用域。

## 聊天投影与真实修改回执

当前ChatMessage只表达同一exchange中的已观察消息，不表达正文修改回执。消息包含真实来源、原记录引用、RequestContext／ResponseOutput、角色、已取得候选／model、内容部分、状态、质量、provider_status／finish_reason／usage及时间。完整字段以同一Rust声明为准；submitted_parts／ChatEditReceipt仍为未启用设计，已从当前规范移出。

- 请求携带的历史标RequestContext，不因重复文本合并为刚发生的新消息。跨请求分组／工具与系统操作关联归中间层。
- ToolCall与ToolResult引用同一已取得原生ID、路径和类型。模型提出工具调用或应用报告结果，都不是某个syscall已执行的证明。
- ChatPart区分Plain、Refusal、VisibleReasoning、工具、媒体引用、Opaque。未知媒体先提供已取得的安全元数据，不自动取URL或构造隐藏思维。
- 工具字段未齐先保留原始记录／partial；缺name或content不造完整工具部分。参数未拼齐时None，真实空name和明确JSON null保留。
- state表示这条消息的观测状态：Streaming为仍在进行；Complete为已取得且可发布的正常协议完成；Interrupted为provider明确incomplete／cancelled或传输未取得正常协议完成；Failed只表示明确provider或解析失败。不因内容匹配或遮蔽生成中断；Interrupted不等于provider失败。provider_status保留provider实际终态，finish_reason只保存协议给出的结束原因，实际原因与缺口分别由原字段／Gap保留。Responses incomplete／failed不得变为Complete，也不得冒充finish_reason。可空error:null不构成错误。
- Complete只说明provider协议已结束，不能清除Gap／Truncated或冒称全会话完整。出处引用超预算必须标Truncated，质量延续到终态。

Started只发一次，Delta沿同一id和base_revision更新，Finished替换该消息的终态快照。done中的完整文本不能再次追加。未提供的字段保持None，累计usage按provider语义覆盖，不猜值或重复加和。

真实HTTP body在EOF前被丢弃时保留body_dropped_before_eof Gap，并与显式finish共用已有Streaming消息收尾：已有解析失败标记为Failed，否则为Interrupted，同ID发布一次Finished后交消费队列；处理结束后释放该消息在途状态。已取得的Complete不覆盖、不重复终态；丢弃路径不解析未完整的压缩／SSE尾部，也不凭EOF、静默时长或active_exchanges=0补Complete。

已知 HTTP 传输中断使用 `transport_interrupted`。非流式正文已取得的解码后字节沿 `HttpJsonBody` 原样发布，记录质量含 Gap，完整长度和未核实的原生 ID 均不填。HTTP 已确认结束、但尚未完成本地协议处理时若调用被取消，使用 `body_finalization_cancelled`。已移交的终态发布任务继续保管原消息并报告实际结果。

## 模型响应与工具的事实交接

- **模型响应中的调用。** `ChatMessage.id.exchange` 标识真实 HTTP 交换，`ChatMessage` 的 `ResponseOutput` 及 `observed_parts` 表达模型响应包含的内容。`ChatPart::ToolCall` 已携带局部 `id`、原生 `native_id`、`name` 和 `arguments`。消费者用现有 `ChatPartRef { message, part_id }` 引用该调用。原协议以 `ProtocolRecordId` 引用，原生 ID 的 JSON Pointer 作用域是包含它的原记录。`ChatMessage.record_refs` 是消息级记录列表，不能单凭 JSON Pointer 确定是哪一帧 SSE。消费需要指明原帧时，直接引用核实过的 `ProtocolRecordId`。以上沿用当前字段。
- **应用实际调用工具。** 可信旁路来源证明原工具到原生操作的交接后，保留该次原生作用域、响应和工具原值。中间层再按权威来源定义的标识作用域与实际协议记录组合，定位既有 `ExchangeKey`、`ProtocolRecordId` 和 `ChatPartRef`。具有明确发行者及唯一作用域的原响应引用可作为组合依据；裸值相等或库内只有一个候选本身不构成证明。
- **实际系统操作和结果。** 原操作沿现有 `EventData` 和 `EventKey` 提供，执行结果沿原 `OperationResult`。一次应用调用可以对应多个原生请求，逐个保留各自交接证据。应用的工具结果沿现有 `ChatPart::ToolResult`、原生引用及原记录提供，保持与系统执行结果各自的含义和出处。

唯一正式采集入口是 `MainMessage::RequestHandoff`。Submission的公开ToolCall／Assistant同对象pair只证明当前现场持有这些消息对象，普通ALS也能保存上一次工具；候选唯一、reads_consistent和CAPI→Event原交接都不证明“当前正在执行该工具”。原Submission／EventBound／ChildCreated全部保留，生产不据此自动生成工具执行事实。`ToolExecutionFacts { event: EventKey, context: NativeToolContext }`保留原查询shape及origin引用consumer，供既有明确受控的软件验证；当前工具执行的可信发布／交接仍未实现，没有独立Main工具消息、直接writer或自动candidate升级入口。这不是原始采集字段齐全门槛。

`NativeToolContext` 复用 `ProcessKey` 标识采集到的工具提交进程。tool_call_id、tool_name 保存实际运行时原值；response_id、run_id、session_id、session_key、agent_id、provider、api、model 为实际取得的可选字符串。arguments 复用 `ChatContent` 保留实际文本或 JSON。None 表示未获取，空值保持。复合运行时 ID 按原值保存。字段均来自已核验的同次上下文，内存地址等瞬时核验值随原上下文消费释放。

`NativeToolContext`保留取得的公开消息值；其类型本身不授予当前工具执行权威，也不是任意运行时快照。确已取得同义消息值时复用此结构；不得把任意 ALS／ContextVar 中名称相似的对象直接填入它。尚未取得原工具及原请求交接时，不生成该查询事实，也不以空工具 ID 或固定默认字段冒充成功；这不阻止原事件、线程、请求或现场栈等已取得事实沿各自准确契约采集和消费。禁止将原始内存片段套进坐标为HTTP正文的 `EvidenceBody`。原始执行字段归[事件契约](events.md)，业务模式组合归[审计契约](../middleware/audit/technical.md#原证据定位与组合)，不在Flow新增Agent识别器。

Main 沿原有消息队列转交RequestHandoff，入队回执只表示接纳。原 Chat、Ask 和完成仍走原入口。原生原件单份写入业务库，实际EventBound及原request事实引用进入已有活动查询；不再接受独立ToolExecution消息或重复工具列正文。直接完成需要进入业务日志时，原生原件先沿同队列接纳，再交原完成，消费端按实际已存交接判断其业务归属。中间层取得有权威作用域的跨侧引用后，才定位现有 `ChatPartRef` 并建立 `EvidenceRelation`。HTTP 元数据和 SDK 对象是否存在属于具体采集源的事实，底层接纳原生段不以它们为前提。[中间层关联契约](../middleware/technical.md#3-业务查询与结构) · [原生来源证据](../evidence/runtime-correlation-research.md#r47-冻结原版单回调可行性通过)

## 事件消费与状态端口

`FlowInit.output` 接收主模块创建的 `ChatSender`；每个 Flow 实例只有一个 `ChatReceiver`。`ChatEnvelope` 保留来源代及单调 sequence，承载 Observation、CredentialUse、RuleFeedback、Started／Delta／Finished、ExchangeFailed、Gap 和 Status。该 sequence 是投递身份，不承诺内存回放或数据库提交。

`ChatSender::send` 按结构化值的实际拥有容量取得队列字节许可；容量计算不生成 JSON，不声称等于 RSS。单条超过队列上限立即失败；容量暂满时等待消费释放，接收端关闭明确失败。`ChatReceiver::recv` 出队即释放队列许可，把原 Arc 所有权移交消费者；消费者完成后释放。队列不是历史缓存。

`FlowStatusPort::status` 只返回当前实际状态和计数。底层不提供 subscribe／after／snapshot／exchange_context／history，不保留聊天环、查询快照、完成事件或未来存档端口。中间层数据库消费原事件并保存聊天和业务关系；对外订阅、历史及 Ask 查询归中间层。

Flow.stop 停接纳、收敛实际协议与反馈任务后释放输出发送端；主模块等待消费者排空及真实 EOF。保留 status／feedback 句柄不会让已经停止的输出队列永远不结束。队列关闭与协议正文失败分别报告，不能静默切成关闭采集继续运行。

## 固定规则拒绝反馈目标契约

沿用 EventKey、MainNotice／RuleHit、ChatPartRef、ChatMessage、ProtocolObservation 和原规则文本；没有新的安全裁决或工具关联规则。

`RuleFeedbackRequest { denial, call }` 是中间层明确选择的引用值，不是可反序列化的底层写入能力。`RuleFeedbackFact { denial, call, state }` 保留真实进度：Prepared 只表示在途计划接纳；Applied 的 original 和 modified_body 分别保留原记录及原样拟发送正文，同一次写出的 Applied 共享不可变修改后正文；Submitted 只由真实写出者确认完整写出；Failed 保留真实原因。Submitted 不代表 provider 接受或 Agent 任务成功。

`FlowFeedbackPort::prepare(RuleFeedbackPrepareInput)` 是异步进程内接口。Input 包含原拒绝 notice、call、原 message、原 observations、本次未完成反馈的 `cancelled` 令牌；不实现反序列化。它执行原规则已配置的反馈，不依赖前端连接、ManageService 或 chat_content 授权。规则修改与聊天查询仍各走原权限；客户端不能自报拒绝、文本、工具关系或固定 ID 来调用该端口。

Flow 在准备期间核对真实拒绝、原规则固定文本、ToolCall、协议记录及实际凭证授权的来源域。原 MainNotice 的 Denied 是拒绝依据，不额外要求 NativeRequestReturn。校验完成即释放完整 notice／message／observation，仅保管本次尚未完成写事务所需的原引用、认证域指纹、固定文本和取消令牌。反馈事务没有计时期限或过期状态，沿原 I/O、显式取消及本次 Flow／消费源的真实寿命收尾。条目数量受既有消息容量约束，不增加聊天缓存或盲目保留所有拒绝；真实终态立即删除，历史与终态去重由中间层业务库承担。Flow不提供完成登记或历史回填旁路。

现有在途相同绑定只能对应原事务；冲突明确失败，不改绑或重试到其他请求。中间层从已提交历史防止重复发起；数据库重开不能恢复在途取消令牌或原操作能力。

### 当前支持范围与写入

当前反馈只支持 OpenAiResponses。协议目标采用两种明确锚定方式：请求实际 previous_response_id 引用原 response；或同一 input 完整携回原 function_call 并提交对应 function_call_output。后者核原生 call_id 的类型和值、原工具名及完整 Codec 规范化参数，并核同一真实认证域；可选 item ID 只在两侧实际取得时核对冲突，不作为新增必备字段；仅凭 ID、相似参数或候选数量不成立。请求已有 previous／conversation 引用冲突时明确不匹配，不能换另一方式绕过。conversation 只核一致性，不能单独证明来源。认证范围指纹来自同一次真实凭证授权及实际标准认证域，不能用作 provider 账户身份或业务因果桥。缺完整域、记录或桥时明确不可用；Chat Completions／Anthropic 采集不因此变为精确反馈支持。

原工具名与完整 Codec 规范化参数的 SHA256 选择摘要（call_id 与可选 item ID 按原类型和值独立核验）只在该反馈在途上下文中保管；有待决反馈时，请求选择信息在既有单次 JSON 解析及 Codec 参数解码中生成，共用于实际修改和未修改／过迟收尾，不保存正文副本或聊天缓存。这些字段只核协议上报目标，不生产或替代 EventKey→ChatPartRef 内核执行关系。

计划在原请求真正写出前定位唯一原 ToolResult。只在 Agent→LLM 的工具执行完成上报（ToolResult）原内容后附加规则配置的固定原文，保留原 ID、role、错误状态和原结果；不修改 syscall 返回值或 Agent 内部工具执行结果；不新增 system／developer 消息，不伪造工具返回。原证据与修改后证据分别保留实际字节。请求没有及时计划时不延迟普通聊天等待业务关联，不追改或重放已经开始提交的请求。

实际失败码以唯一 Rust 枚举为准，区分来源关闭、无目标、多目标、过迟、类型不支持、容量、本次反馈取消和传输错误。反馈失败不改变已生效的 HIPS 拒绝。部分写出后不能混发原文、补 Submitted 或重放；真实取消／停止沿原 HTTP 任务收敛。

## 受控TLS出口与凭证窄端口

RouteId、SecretRef、BindingId、PhantomTokenId与CaRef是不同newtype，不能因宽度相同互换。中间层不能构造实际流、TLS签名句柄或认证头租约。

VerifiedUpstream只能从真正完成的TLS连接和正常根信任核验产生，同时校验证书链／实际origin主机；没有DTO布尔构造。它只供同一已验证origin的一次实际请求授权，不赋予业务关联或任意目标权限。

CredentialUseRequest借用实际IngressFacts、method／编码path、route、唯一AuthSlot、私有phantom与VerifiedUpstream。CredentialPort按当前绑定、秘密与撤销状态授权；锁内不读盘、不等网络。授权前已撤销必拒绝，已授予并在途请求按真实结果收尾，不能声称撤回已发送字节。

AuthHeaderLease含实际授权的BindingId、SecretRef及该秘密修订；这些是AS引用，不是猜出的provider账户ID。它只由实际发送者消费，单次替换指定认证字段并设置sensitive／never-index；解析器、驱动、前端没有读真实key接口。apply仅完成该认证头替换，不返回脱敏材料；内容采集保持原值。CredentialUseFact的Prepared／AppliedToRequest／Submitted／Failed只由真实执行阶段生成，不把HTTP提交称模型认证或理解成功。

CertificatePort仅提供受允许origin的叶证书链／不透明签名句柄及公有CA。CredentialAdminPort提供ensure_ca、import_secret、replace_secret、issue_phantom、revoke_phantom；Main首次连接鉴权及ManageService保护私有载荷，真实写盘由凭证模块完成，不进入普通配置／日志／广播。

ensure_ca只在未创建过CA的受控新存储生成，已有材料复用、损坏失败；没有在线轮换或关闭上游验证入口。导入／替换输入使用PrivateBytes并受MAX_SECRET_BYTES约束。签发phantom只在成功创建时返回一次值，后续列表不回读；失败／丢失答复不自动循环签发。SecretWriteReceipt只证明实际持久操作，修订用精确十进制codec。

## 归一化契约在实现中的执行边界

provider只对已知协议路径执行归一化。非流式按真实数组项，流式按实际逻辑index／item／block；局部PartId与原生ID各有含义，不能用缺省0互代。原ID不做占位替换，也不按文本外观猜原值。

Text／JsonArguments按同一provider解析路径取得实际字段；跨SSE参数沿原索引拼齐后使用既有JSON解析值，不做脱敏、关键词过滤或按转义语法遮蔽。原文、结构化结果及查询值分别保持实际字节／类型，不重序列化冒充原始协议。

来源、投递、保留缺口分别报告。exchange级缺口沿同一Envelope的真实来源标签过滤，全局来源状态不夹带正文。字节计费、序列与短临界区的实现见所属模块；预算转换先检查表示范围，不截断，不新增业务规则或测试门槛。

## 原消息顺序

协议生产者保留原请求与响应中的消息顺序及唯一消息 ID，Started／Delta／Finished 属于同一逻辑消息。中间层数据库保存原顺序及修订，按该来源查询，不能按 PID、时间或全局最新消息补顺序。底层没有交换历史查询接口。
