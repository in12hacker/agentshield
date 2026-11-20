# 因果审计与Ask窗口技术模型

聊天工作台从原 ChatMessageId 或 ChatPartRef 打开现有 ActivityContext。读取原协议引用及直接相连的同交换、原生工具引用、问询完成和已有 tool_execution 关系。工具引用沿原作用域和实际 ID 重新核对候选，冲突按既有 ContextGap 报告。没有底层执行桥时，工具项显示 NoExplicitRelation，消息历史与原协议记录仍可读。返回值继续使用 ContextView。[业务契约](../technical.md#llm-聊天工作台)

活动模块统一判定 `SameExchange` 和 `QuestionCompletion`。前者要求消息显式引用的原协议记录与消息具有同一 ExchangeKey 和 ProtocolSource，原生工具结果也只用该范围内的记录证明自己的引用。后者要求同一原 DecisionId 和逐值相同的 EventData；不一致沿原 ContextGap 报告。SQLite 入库提供原事实并持久化判定结果，联查从已存原事实复核同一判定。现行引用、关系证据和存储结构保持原定义。

本页定义中间层业务结构；实现使用同一结构和业务日志数据库，运行验收范围另见证据。底层事实直接引用既有结构；消息身份、字段值、None／空值及时间单位不重新定义。产品含义见[设计](design.md)，模块边界见[架构](architecture.md)。

## 复用的实体与关系

Agent根实例和实际actor均复用ProcessKey，但字段含义不同：前者是明确绑定的审计主体，后者是原操作真实执行者。EventKey、DecisionId分别引用原事件和实际Ask，不互换。ChatPartRef指原消息内ToolCall位置；模型原生call_id保留自己的命名空间及原值，不当全局ID。

FactReference、SourceRecord、EvidenceRelation、ContextGap、ContextView继续由[中间层契约](../technical.md#3-业务查询与结构)唯一维护。原始确证关系包含same_exchange、native_tool_reference、question_completion、tool_execution；新增原request引用及补充时序关系由末节分别定义，命令相等不单独构成确定执行关系。

确定边的条件分别是：同一实际ExchangeKey；同一明确协议域内的原生引用；原Ask和原完成的同一DecisionId；以及原执行来源明确给出的ToolCall→EventKey。每条关系必须携原证据引用。两个节点存在、仅剩一个候选、PID／命令／时间相同都不是桥。

`ToolExecution` 的协议绑定在唯一 `activity::validate_execution_link` 中核验；登记、查询、Ask 和固定反馈复用该函数。有原 `ToolExecutionFacts` 时，提交进程的完整 `ProcessKey` 必须等于所选响应原 `ProtocolSource.actor.process`，工具名称及双方实际取得的参数类型和值须一致；参数缺失不补造，也不把两个缺失值当作参数相等证明。`tool_call_id` 必须等于所选 `ChatPartRef` 的字符串型 `NativeProtocolId`。Responses 的公开复合形式只允许按同一原协议记录、同一调用对象路径的 `ToolCall /call_id` 与 `ProviderEvent /id` 拼成完整 `call_id|item_id` 核验；不切掉后缀、不从响应内另一个调用借 item、不以同 response_id 代替调用身份。若取得 response_id，还须按原 profile 的响应命名空间核验。身份所需原件缺失为 NotProvided，实际矛盾为 ConflictingEvidence。该核验不生成执行来源，不把共享消息候选升级为当前执行，也不要求驱动原事件与提交进程同 PID（子进程交接仍由原执行来源证明）。

系统结果复用OperationResult；Agent上报的工具结果复用原ChatPart／ProtocolObservation。它们可以内容矛盾，必须保留两份原事实及各自出处，不能合并成一个“成功”布尔值。

### 原证据定位与组合

业务层用既有 `FactReference` 定位原记录，再进行模式条件比较或组合已证明的交接关系。进程寿命和进程状态继续由进程树维护。线程索引使用实际进程寿命、TID 和已取得的线程出生时间，连接使用包含来源实例与对象代的 `ChannelRef`，请求使用其原 `ExchangeKey`。这些引用各自定位原事实，实际对象交接证明与多条件模式命中保留各自依据；现行 `EvidenceRelation` 仍按上节原契约使用。[进程树职责](../../backend/process-tree/technical.md)

句柄和内存引用由采集方在其原对象寿命内核验。Linux 文件描述符是进程描述符表的槽位，关闭后下次打开可使用同一数字。PID／TID 由各 PID 命名空间循环分配，存活线程也可先后处理多个调用。瞬时记录在同一有效回调采齐，瞬时值的生命周期限于该次上下文，消费完成即释放。采集窗口可用于定位取证原件，确定关系依据原对象的实际交接。R47 的原 `uv_fs_t` 到 EventKey 就是一条已验证的原生交接。持久层继续保留原事实及经确认的 `ChatPartRef`、`EventKey` 和 `EvidenceRelation.evidence`。[来源采集约束](../../evidence/runtime-correlation-research.md#单一回调点的采集约束与流程) · [交接契约](../../contracts/flow.md#模型响应与工具的事实交接) · [Linux FD 分配](https://man7.org/linux/man-pages/man2/open.2.html#DESCRIPTION) · [Linux PID 分配](https://github.com/torvalds/linux/blob/v7.0/kernel/pid.c#L238-L253)

### 通用操作系统模式

用户明确以尽早取得、与目标Agent私有架构无关的系统事实进行通用匹配。业务层可以组合现象、事件顺序、时间窗口、线程实例和已采到的请求／句柄／同步对象，识别提交、等待、唤醒、执行与完成模式。调用栈可以保留为现场证据，不能将固定函数名、栈形状或Agent私有对象作为通用绑定条件。SDK接入不是前提。

每条模式只比较其明确使用的字段，保留原引用、条件和实际命中值。时间单位、时钟域、窗口边界及对象有效寿命必须明确；窗口值来自模式定义或真实生命周期，不随失败任意扩大。发送信号、唤醒发生、线程实际运行和操作实际完成各自取真实事件，不能合并成一个成功状态。缺失字段与冲突值保留原义，不用零值、空串或最近候选补造。

`BusinessQuery::ToolAttribution` 以业务库已保存的原操作 `EventKey` 为入口。调用方明确提供回看窗口、身份条件和参数字段映射。查询从同一 SQLite 读事务按消息保存顺序分页发现模型工具调用。身份条件支持连接发起进程等于实际操作进程、连接发起进程等于原物理父进程，或连接发起线程等于实际操作线程。进程比较使用同一宿主启动 UUID、TGID 和进程出生时间。线程条件额外比较 TID 与线程出生时间。物理父进程条件使用原事件携带的父进程字段。模型侧身份仍只表示连接发起者。宿主启动 UUID 由 Main 在受信原生入口附加，历史记录缺失该事实时返回 `not_provided`，禁止回填。

时间窗口以已完成响应投影的最后观测 BOOTTIME 纳秒为起点，以原事件的采样时间为终点。窗口由查询显式给定且包含边界。按实际工具参数 JSON Pointer 比较原执行参数、文件路径或网络对端。文本可用空指针选择全文。路径和参数按字节比较，网络按原 IP 类型及端口整数比较。每项返回原工具位置和消息修订、时间与条件比较结果。匹配项保留实际比较的原事件目标值。缺参数或无法取得的目标字段报告缺失，所有符合条件的调用保留独立引用。分页游标仅定位已保存的消息行。匹配结果为业务模式命中，确认归因仍使用原 `EvidenceRelation::ToolExecution`，匹配查询不写库或参与裁决和自动反馈。

`ActivityContext` 可携同一 `AttributionRequest`，此时anchor必须是原EventKey的Event引用。原view与候选在同一SQL事务读取并分别返回；缺可选来源只增加原NotProvided缺口，不遮蔽已证明关系。候选没有写入ToolExecution的能力，行为图消费原引用。

`AskContext` 可携 `attribution` 参数，其字段为 `pattern`、`after` 和 `limit`，直接复用归因查询条件。中间层在读取原 Ask 的同一 SQLite 事务内执行匹配。`AskAuditContext.attribution` 返回该页结果，`candidate_windows` 引用目标条件相等的候选所属交换，全部窗口共享下文定义的20条总预算；已有确定window的同一交换复用该窗口，不重复列入candidate_windows。正文只在既有 `view.source_records` 出现一次。原 `trigger_call` 与 `window` 保持确认关系语义。候选上下文使用本次事务的原消息修订，已确认交换保留原冻结修订；若候选所需修订未在已有窗口取得，候选元数据保留，以原ContextGap::NotProvided说明，不覆盖窗口或输出两个同ID正文。原缺失与歧义保留，原问询期限与答复持续由 Main 管理。

字段定义如下，所有时间均为同一宿主启动范围内的 BOOTTIME 纳秒，u64 对外使用十进制字符串：

- `AttributionRequest`：`pattern: AttributionPattern`，`after: Option<ChatMessageId>`，`limit: u16`。limit为1至64，按消息分页；一条消息的全部工具项保留。
- `AttributionPattern`：`actor: SameProcess | PhysicalParent | SameThread`，`lookback_ns: u64`，`target: TargetPattern`。窗口为 `[event_time.saturating_sub(lookback_ns), event_time]`。0只比较同一时间值。
- `TargetPattern`：`ExecArgument { pointer, index }`比较原执行argv零基项；`Executable { pointer }`比较原执行目标；`FilePath { pointer }`比较原主路径；`NetworkPeer { address_pointer, port_pointer }`比较原对端。所有pointer为RFC6901 JSON Pointer；空pointer选择整个原值。只对适用事件类型执行，不做路径规范化或shell解释。
- `ObservedTarget`：`Bytes { value: Vec<u8> }`或`NetworkPeer { address: IpAddr, port: u16 }`，值来自原事件；未获取完整目标为None。
- `AttributionCandidate`：`call: ChatPartRef`、`message_revision: u64`、`response_observed_ns: u64`、`target_comparison`。比较值为Equal、Different、EventTargetUnavailable、ArgumentUnavailable或SourceIncomplete。原消息存在质量缺口时使用SourceIncomplete。
- `ToolAttribution`：`event: EventKey`、原`pattern`、`required_connection_process: ProcessKey`、`required_connection_thread: Option<ThreadIdentity>`、`event_boottime_ns: u64`、`event_target: Option<ObservedTarget>`、`candidates: Vec<AttributionCandidate>`和`next_after: Option<ChatMessageId>`。required_connection_process／thread是本次比较所需的身份条件，取原事件及actor选择；零候选也保留条件，不声称已观察到匹配连接。ThreadIdentity仅含原`tid: u32`与`birth_boottime_ns: u64`；非SameThread条件时未选择线程限定，字段为None。所有身份条件必须实际取得，缺失返回NotProvided。

单回调一次采全针对本次瞬时值。之后沿原对象有效寿命传递身份、绑定 EventKey 或组合业务关系是允许的；不能为了在同点拿到最终业务关系而改为目标注入或Agent代码适配。原始采集不要求先有ToolCall或HTTP对象身份。

## 新增业务输出

AskAuditContext为只读业务值，字段如下：

- decision_id: DecisionId。来自已实际接纳的原Up::Ask。
- event: EventKey。由该Ask携带的原EventData取得。
- agent: Option<ProcessKey>。已取得明确审计主体绑定时填写；未取得为None，原因在view.gaps。不能以当前父PID或程序名称补值。
- trigger_call: Option<ChatPartRef>。仅当原事件有唯一明确tool_execution时填写；多候选冲突不择一。
- window: Option<AskMessageWindow>。该原调用对应的确定模型上下文窗口；None表示未取得，不能返回空窗口冒充有上下文。
- view: ContextView。保留复用的原事实引用及确定关系，不重写源字段。
- command_checks: Vec<CommandCheck>。独立的内容核对结果，不参与触发调用选择。
- attribution: Option<ToolAttribution>。请求携带匹配条件时返回本次查询结果；未请求或所需来源未获取为None。后者以view.gaps的NotProvided说明，原Ask仍可读取；参数错误及真实SQL失败沿原业务错误返回。
- candidate_windows: Vec<AskMessageWindow>。本页目标比较为Equal且未由window提供的候选交换，共享确认窗口之后剩余的条数预算；原来源候选窗口沿下文同一规则，无匹配时为空。候选窗口不填入trigger_call或window。
缺口仅放view.gaps，不在外层复制一份；沿用ContextGap的原引用与原因。

AskMessageWindow字段：exchange: ExchangeKey；message_refs: Vec<ChatMessageId>；earlier_observed_messages: u32；source_gaps: Vec<ObservationGap>。message_refs按下节原顺序，长度不超过固定产品值20；原ChatMessage只在view.source_records承载一次，保留原revision／role／origin／record_refs及quality，窗口不再复制正文；零条只用于源明确提供空上下文，不能表示未获取。earlier_observed_messages只计已观察且因20条上限省略的前项，不声称源缺失部分的数量。source_gaps只携该交换实际源Gap，不复制业务关系缺口，不增加假“全会话完整”标志。

CommandCheck字段：call: ChatPartRef；operation: EventKey；argument_pointer: String（原工具参数中的RFC6901位置）；exec_argument_index: u32（原目标argv的零基索引）；outcome: EqualBytes／DifferentBytes／NotComparable；evidence: Vec<FactReference>。指针与索引必须来自显式核对说明，不按工具名猜字段。比较UTF-8原字符串与原argv对应项字节，不trim、不shell split、不拼接argv；已有明确映射但原字段不是字符串、或原参数／argv未取得时为NotComparable，并给出原ContextGap。EqualBytes只说明这两段原数据相等。

当前没有用于任意未知工具自动生成argument_pointer／exec_argument_index的事实来源。未配置明确映射时只保留ContextGap，不产生该项CommandCheck，不建立工具规则池或Agent名称识别器。

## 20条窗口算法

1. 必须有真实Ask并通过当前会话的聊天内容授权。未授权沿既有FailureCode::Unauthorized失败，不把权限错误伪装成源缺失。
2. 从原EventKey定位唯一明确trigger_call，并取其ChatMessageId.exchange。没有桥时返回NoExplicitRelation；Ask展示和原答复仍继续。
3. 从中间层业务数据库读取该交换已保存的原请求／响应消息顺序及修订，来源唯一见[业务日志](../business-log/technical.md)。不能在中间层按HashMap次序、墙钟、PID或跨服务sequence重排。
4. 原请求上下文在前，本次响应消息在后；每个原逻辑消息占一项。同ID的Delta／revision只采用本次查询固定的原修订，重复的原协议消息不按内容去重。取尾部至多20项，冻结为这次Ask的只读窗口。
5. 跨交换重放历史不再次插入本窗口；其他会话更晚产生的消息不替换窗口。消息缺失、淘汰或源有Gap时保留实际已取得消息及质量，同时报告缺口，不用当前最新消息补齐。

活动视图可以在Ask发出后独立补送上下文；原DecisionId、规则版本、剩余期限、可选动作均不得更改。窗口发布后新的聊天不滚动替换它；其消息引用与原revision保持。更新不能创造ask_closed或执行结果。

## 接口、所有权与实现缺口

现行 `BusinessQuery::AskContext { decision_id, attribution }` 通过 `business::start_query` 接入 Unix 和正式 WS，沿现行业务封套及 ContentAccess 输出 AskAuditContext。来源是业务库内实际登记的 Ask 和 Main 提交的受信交接，客户端不另传 event、agent、tool_call 或 text 建立关系。原引用尚未提交时保持缺口，已知多条冲突交接时保持冲突；只有唯一可核的原调用才能冻结同一交换的窗口。

Ask 的原事实、活动视图及窗口读出或冻结使用同一次 SQLite 事务。工具项和消息联查在遇到 ToolExecution 时核对其目标 EventKey 的全部直接交接，让不同查询锚点报告相同冲突。竞争检查止于该事件的直接交接。

中间层不保留聊天内存历史；查询按短 SQL 事务读取已提交原值，当前处理持有的 Arc 在消费完成后释放。Question 承载实际 Up::Ask，不能由完成事件的 DecisionId 伪造。真实OperationResult及原EventData无论是否已有Question或关联都沿原结果入口保存；Observed仍不制造执行结果，也不把业务库改成安全日志回放。

通用工具执行桥和 Agent 根实例业务绑定仍缺真实生产来源；没有桥时 Ask 查询给出 NoExplicitRelation，不用其他交换凑窗口。数据库与审计接口的实际本地测试及真实验收分别认定；不能以接口存在推定任意未知 Agent 的端到端图谱已完成。

## 原驱动事件为锚的行为链

Agent内部的正式行为/因果链复用 `BusinessQuery::ActivityContext`，以 `FactReference::Event(EventKey)` 定位原hook事实。返回的原 `ContextView.relations` 承载已有且已核的协议、工具原对象交接和Ask/完成关系；可选 `attribution` 直接使用原 `AttributionRequest`／`ToolAttribution`，不重建关系DTO。查询不接收客户端提供的确认边，也不按一个候选自动绑定。

原事件已有唯一确认的 `ToolExecution` 时，活动详情可随 `source_records` 返回该 HTTP 交换的最后一条原请求用户消息。查询在同一读事务内，以工具消息首次入库为截止，按消息首次入库的原角色和顺序定位该消息，再读取截止时的原修订。按 [Flow 来源契约](../../contracts/flow.md#原始协议观察) 核对两侧观察。用户投影须引用其首次入库前保存的唯一完整原 HTTP 请求正文。若所选消息或来源缺失、质量异常或冲突，沿 `ContextGap` 标明原消息或正文引用，该条预览缺失。用户投影保持完整原值和原引用，包括空文本。前端用已核实投影显示原请求文字，并保留确认工具链的独立核验。该消息属于原客户端请求上下文，其中可能携带历史。原话的展示不增加因果边，也不推断用户意图。Ask 冻结窗口沿现有契约。

节点引用继续使用 `FactReference`、`ChatPartRef`、`EventKey`。原对象交接证据成立时显示已确认边；多条件匹配仅显示候选及实际比较依据。候选原消息通过已有消息或Activity查询读取，不在因果视图另建正文快照。原hook的source_instance／sequence_domain／sequence可标识同来源的记录分配顺序，不能跨来源排序成因果，也不能冒充系统调用完成顺序。原对象地址只在原采集记录的上下文内解释，不当历史对象缓存的key。

此查询与现有Ask上下文共用同一数据库所有者、活动判定和候选算法。来源未取得时保留原view和原ContextGap；参数错误和真实SQL错误仍显式返回。已证明的线立即使用，缺证据的连接保持缺口。精确持续采集与真实任务边界的未实现范围不阻断已有原件的离线行为链验证。

此视图归因果审计模块；Group仅引用它作为某个Agent节点的内部明细。Agent间边必须来自真实委派/调用/响应证据，工具关系、线程唤醒和进程父子关系都不能替代。

## 调用方消费边界

Ask和活动的内部查询仍在同一数据库所有者核完整原件及关系。对外默认回复中的重载Protocol／NativeRequest原件用`SourceRecord::Deferred(FactReference)`明确引用；这表示原件仍在库内且可按需读取，不是缺失、截断或删除。消息窗口与已核关系保持原结构；前端只核本次CommandId、Ask／Event身份及引用，不另行重算原协议归因。候选与已确认关系仍按后端的原kind分别展示。

Ask的聊天窗口总预算为最近最多20条：已确认窗口先保留其既有冻结引用，候选窗口共享余下条数，按同快照取得的原消息观测时点选择，逐交换保留原引用及原顺序。候选交换不各自额外塞20条正文；图谱所需的原ToolCall消息仍独立保留。被省略的窗口正文仍可从已有ChatMessages历史查询取得，原SQL内容不改写。

`BusinessQuery::OriginalRecord { reference, body_offset, body_limit }`复用既有内容授权和Activity证据读槽，仅读取指定Protocol／NativeObservation原件，不再次扩展整个关系图。body_offset为原协议单位中的绝对字节偏移；body_limit为请求页长度，当前单页至多传输上限的1/8，前端默认65536字节。协议页返回`SourceRecord::ProtocolPage { observation, next_offset }`，复用ProtocolObservation元数据与EvidenceBody原segment绝对offset，明确该页范围与下一偏移；不会把未请求的页标为Missing或伪造Gap。NativeObservation读取仅本observation的实际原行（矛盾原件仍保留），不追整个request寿命。前端分页替换同一当前读取结果，不攒第二份回放缓存；未取得或冲突仍按原事实报告。

完整原件与页读取共用业务日志的同一`read_protocol_range`／`read_evidence_range`所有者。分页先读取原segment位置和长度元数据，再用SQL substr只把所选页字节移交Rust；不先构造整个BLOB Vec后再切片复制。页字节从SQLite读成自持有Vec是离开数据库事务并跨异步发送所需的边界复制；原元数据移动构造、Arc只增引用，不建立另一可写快照。Protocol记录按原身份不可变，分页不回读目标进程内存；晚到回复仍由现CommandId、当前selection及原证据读槽核对，不能覆盖已切换的对象或Ask冻结修订。

本次去除的是单次关系查询携带整个原协议／native证据图造成的膨胀，不提高传输上限。单条结构化ChatMessage或其他无法按原字节段分页的值本身超过现行帧上限时，仍报告实际输出错误，原SQL记录不删；不能把这项查询收敛声明为所有任意大小载荷均已支持。原操作的Allow／Deny及真实回执始终独立于这些可选证据读取。

Dashboard活动与Ask使用同一显式候选条件表单；后台接口和窗口由本模块独占，不在界面重做关联。
两处行为链共用同一原消息展开组件：工具候选或原上报结果节点可展开其实际ChatMessage，由现有OriginalChatMessage展示原文字、工具参数、上报结果和原引用，并跳转到该原交换的聊天记录。组件只借用当前查询返回的原对象，不生成意图说明、不另存聊天正文；缺失或不合法的原消息保留明确状态与原件。跳转仍核当前授权、当前证据读槽和实际返回的消息／窗口引用，旧回执不得替换新选择。

同一工具结果在后续HTTP历史中被重复上报时，行为链仅合并其重复正文展示：必须是相同后端关系kind和同一原ToolCall PartRef、相同类型的原结果native ID、已取得且相同的process／auth／profile／origin／route／config／方向采集域，以及相同消息origin／role／state／quality和完整ChatContent。结果ID或采集域缺失时保持独立；不同调用、来源或内容绝不因文字相同合并。组内每份原PartRef、Message、revision与原记录引用仍可展开，展示借用当前原对象，不改事实、SQL或关系强度，不产生新缓存。该折叠只表示多个已观察的历史上报具有相同内容，不确认某次系统操作属于该调用。
普通原事实阅读不带归因条件；候选比较才传入原AttributionRequest，结果沿同一证据读槽消费。
独立tool_attribution与组合查询共用原ToolAttribution结构解码，界面比较使用组合查询，不增加重复读槽。
完整窗口按后端原引用显示各角色与parts，缺件和冲突单列；只读候选不参与原规则裁决或固定反馈。
来源仍未实现的范围见[中间层缺口](../gaps.md#仍需原始来源的缺口)。

## 前端消费参数与在途状态

界面候选表单提交现有AttributionRequest，初次比较after=None，limit明确沿当前查询页大小（界面初始8条消息，可选16；这是分页大小，不是匹配条件或候选个数限制）；下一页只取该实际ToolAttribution.next_after，不按候选数量推导。lookback_ns来自显式秒输入的十进制换算，最多九位小数，整体结果必须在u64域；不经过JavaScript浮点数。actor与target各使用原枚举，JSON Pointer按RFC6901校验，WholeValue显式映射为""。

EvidenceRead.attributionRequest在原pending/result外保管本次AttributionRequest或None，作为回复关联和下一页依据；它只属于当前读取寿命。Activity／Ask接受非空归因回复时同时核对原事件／原询问和所提交pattern；没有请求归因时只接收空归因。独立tool_attribution回复按同一结构解码，不在无匹配CommandId时创建旁路状态。源事实均保留接收原对象，不复制同义DTO。

完整窗口按原message_refs机械读取当前view内原ChatMessage。同ID原消息不一致时显示冲突，缺记录显示缺失，不择最后值或最新revision。候选窗口须能在同页归因结果或原tool_reference_candidate／tool_container_candidate操作边找到同原ExchangeKey的候选；target_comparison各实际结果独立显示，不以Equal作为阅读门槛。此条件只验证窗口来源，不建立ToolExecution。消息展示复用现行完整ChatMessage组件，窗口元数据、源缺口和原引用独立可读。

## 先连接已证原件与补充时序

工具部分证据沿原 `ContextView` 和 `ToolAttribution.candidates` 立即提供，不以通用执行发布者或整链齐全作为阅读门槛。`tool_reference_candidate` 使用原 `EvidenceRelation`：from为原ToolCall的Part，to为原Event或原ToolResult的Part，evidence列明原Part、协议及已取得的native交接。它的唯一含义是这些原件满足已声明的候选联系；不确认当前工具执行或跨HTTP会话，不进入ToolExecution、trigger_call、规则裁决或固定反馈。原native对象／线程／child交接的确证关系保持各自原kind。

`tool_container_candidate`同样复用EvidenceRelation，但明确表示另一种已授权候选模式：原事件与原模型来源均记录同一完整ContainerInstance（runtime、namespace、id），来源ProcessKey与事件host_boot_id一致，模型ToolCall的原观测时点不晚于事件。查询在原业务库按这些原值读取最近最多20条含ToolCall响应，不要求shell只能有一层物理父进程，也不按裸cgroup、名称或最近唯一项绑定。source的原ProcessKey／容器字段、事件时点和原Part均保留供查看；Host或任一容器字段未取得时不补造此关系。此模式不依赖未取得的provider call_id，原PartRef已能定位已观察工具项；缺call_id仍不能链接工具上报结果。该模式只供候选展示和候选聊天窗口，不证明祖先链、当前工具执行或触发固定拒绝文本；准确Native／进程交接存在时仍独立保留。前端Activity与Ask共用同一候选组件，分别标明本模式，工具原call_id的后续上报结果继续由原Tool引用逻辑链接。指向当前原Event的实际候选边用虚线展示，确认边与同原操作回执保持原种类；不任选一个候选替代多项，也不绘制没有后端关系的连接。

候选来源优先沿已取得的原工具／响应标识和record_refs；语义投影未取得时，仍可沿EventBound到Submission.site.process，或原Event携带的完整boot／PID／birth及原父PID／birth确定真实进程范围，再读取该时点之前同范围最近最多20条含ToolCall的已观测模型响应消息。先筛原response_output／upstream_response及ToolCall，再取20条；普通非工具消息不占此额度，所选消息内的原ToolCall部件全部保留，20不是工具部件个数。此范围只给候选，不表示该进程的某一次工具执行；无默认回看秒数、不按名字猜字段、不强选一个。该原来源范围候选与显式AttributionRequest的比较条件分别保留，不将范围候选宣称为已满足某项参数／线程比较。

跨HTTP上报结果按原native标识、真实ProcessKey及实际auth/profile/origin/route/config域索引读取，双方原record_refs和Part分别保留；缺会话只显示候选。原 `ChatPart::ToolResult.content` 直接显示为“工具上报结果”，不要求NativeReturn、poll/log或内核全桥，也不升级为任意物理副作用保证。重复历史是独立原观察，不是新执行。

候选窗口复用 `candidate_windows`，以原Ask携带的Event BOOTTIME为时间截止，从对应已知来源交换读取该时点的原修订，并按上文所有候选窗口共用的总20条预算选择，以当前SQL读取快照保管引用；候选范围明示，不伪造会话。该时间未取得时保留实际读取及明确缺口，不补造Ask时间。后来取得的工具上报结果可通过当前source_records读取，不修改原触发窗口。只按原引用和native索引读取相关原件，不扫描整个ProcessKey聊天历史，不建立第二缓存或正文DTO。

业务关联复用ContextView的原source_records、relations和gaps以及现SQLite/ActivityContext/WS，不建第二套事实、历史缓存或关联服务。Driver/Main只继续供原值，业务层组合；当前Tool原协议关系及QuestionCompletion复用原实现，不重新开发。不等EventBound、HTTP或整链齐全才展示已有部分。

关系结构改用中性名称EvidenceRelation，仍为kind/from/to/evidence，不保留旧类型别名；证据级别由唯一kind决定。原same_exchange/native_tool_reference/question_completion/tool_execution各自原条件不变。新native_request_reference仅表示某原NativeRequestHandoff.request明示引用其原Submission observation ID，不表示ToolCall执行因果。新same_thread_return_sequence是补充模式命中，不是原对象交接证明，不进入ToolExecution、Ask.trigger_call、HIPS裁决或固定反馈授权。

native_call_scope只表达EventBound.enclosing_call实得外层API：from=event(EventBound.event)，to=native_observation(enclosing_call)，evidence引用该EventBound原observation。它不是实际队列条目／工具对操作的所有权，不进入ToolExecution或固定反馈；Write／UDP可在本次入口刷出先前请求，不能以当前scope冒充请求归属。原工具的已证native交接仍按[唯一原生来源](../../backend/driver-adapter/runtime-observation-technical.md)及原ToolExecution契约核验。

FactReference新增native_observation(EventKey)，仅定位原raw观察，不能套LSM event含义；原SourceRecord::RequestHandoff继续Arc共享原结构。该引用可沿既有ActivityContext查询。原始记录无request仍可单独读取；request引用原件缺失时保持原引用并报告NotProvided，不拒绝本条原件。相同observation的相同原值可核，冲突原值保Conflict，不覆盖也不选一。

native_request_reference边直接来自本条原request字段：from=native_observation(本observation)，to=native_observation(原request引用)，evidence至少包含from。原Submission到达时继续核phase=Submission和引用身份；缺Submission仍保留明示引用及缺口，不按地址/时间补原件。读取同一已明示request寿命的原观察仍使用现native_request_handoffs索引；保持每份原值及入库顺序，不强迫各阶段齐全。

same_thread_return_sequence补充模式只组合已得原件：从原EventKey定位已得NativeRequestReturn，核原操作是当前已采libuv FileOpen语义，原WorkLeave/Done的operation也明确相符，再核同一原驱动source、真实TGID/TID/thread birth和BOOTTIME；取返回之后该线程实际已保留的第一WorkLeave观察（同最早时点的多条全部保留），其request必须是来源明示值。再沿原request和work对象交接读取Done；原Done.result与原NativeReturn.return_value相同时记录模式命中，缺值/冲突分别报告，不能以一个候选当确认。该模式没有虚构秒数阈值，表达的是“下一份已保留WorkLeave”，不声称采样完整或真实下一调用；源缺样、缺Submission和其他可能原执行来源均保持原缺口。

原Return或原EventData本身不能唯一核对时，该模式不命中，相异原件继续保留；新增模式不向Event写入会改变原ToolExecution／Ask／反馈判断的冲突gap。原raw观察或其request/work/result值有明确引用且冲突时，在对应native_observation报告，不为未取得的raw构造引用。

模式from为原event，to为WorkLeave的native_observation，evidence引用原event、该WorkLeave及同request/work的Done；原NativeReturn也通过同event下的原SourceRecord保留，不复制其字段或原数据正文。原事件、return、WorkLeave、Done在内部ContextView同一次读事务内核对；对外重载原件按上述Deferred引用单独读取，展示不得将模式kind改称ToolExecution。缺NativeReturn等所需原件时该模式不命中，原事件及已有关系照常读出；不是采集、入库、Ask或业务通过门槛。

原始observation冲突与补充模式不同：原件若被正式native工具交接消费，其晚到矛盾由业务日志唯一owner沿显式request／bound_event传播到对应Event，原事实保留且确认失效；具体存储和重核入口仅见[业务日志](../business-log/technical.md#原生请求交接原件)。不能用先到正例继续授权反馈。
