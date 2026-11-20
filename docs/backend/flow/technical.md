# Flow 模块封装与内部状态

实现见[as-flow](../../../crates/backend/as-flow/src/lib.rs)。跨模块值与端口仅定义于[公共契约](../../contracts/flow.md)；配置、协议编解码及真实 TLS 写出分别见[配置](../configuration/technical.md#flow运行时配置)、[编解码](protocols.md)和[出口](tls-egress.md)。

## 职责与初始化

Flow 接收驱动适配移交的真实 AdmittedConnection，独占两端 TLS／HTTP、JSON／SSE 必要单次解析、原协议事实和单交换聊天投影。凭证模块提供授权与证书窄端口。Flow 不定位安装目录、不扫描进程、不做业务因果关联、不保存聊天历史。

Main 创建连接队列与唯一 ChatSender／ChatReceiver，向 FlowInit 注入 output、不可变设置／修订、实际入口和凭证端口。初始化准备实际 IPv4／IPv6 端点及 TLS 后，Main 才经驱动适配启用导向。配置开关不代表入口就绪；准备失败按真实错误清理。

公开接口只有初始化／停止、FlowStatusPort 和 FlowFeedbackPort。只读状态句柄不持历史，不允许停止驱动或修改配置。底层没有订阅、回放、快照或数据库端口。

## 内部职责

- lib.rs：实际生命周期、单调投递身份、当前状态与唯一消费输出。
- tls.rs／content.rs：成熟 TLS／HTTP 传输和有界 content decoding。
- protocol.rs／observe.rs：单协议单元及真实 partial 状态、原字段、结构化投影。
- feedback.rs／feedback_write.rs：已明确请求的窄写事务、撤权、实际写出与终态释放。

不同 HTTP stream 独立推进；不为每个容器、类别或 provider 新建线程。同一协议状态由其任务顺序持有。短锁不跨网络、解析、队列等待、数据库或人工等待。

DNS、连接和两腿 TLS 直接 await 原异步 I/O；Flow 不添加网络或排空计时器。显式生命周期取消沿原任务 owner 结束工作并实际等待，具体边界见[网络等待与显式取消](tls-egress.md#网络等待与显式取消)。

## 在途内存与消费

只保留尚未完成解析、实际 HTTP 或反馈写入需要的内存；结束即释放。Started／Delta／Finished 不进入历史环。队列入队占用增加，出队许可归还并把 Arc 移交消费者；SQL 事务和当前查询结果属于消费者正在处理的上下文，完成后释放。

保留单协议单元大小、当前解码状态大小、并发交换数、JSON 深度及单条输出／队列上限。删除整交换最坏倍数预约、独立 retained 池、回放 TTL 和订阅预算。拥有容量通过结构化字段计算，不为了计费生成 JSON，不把计费称实测 RSS。

Observer 在发布或插入新状态前核当前状态边界。真实输出错误向原 TLS／body 任务传播，不将观察器静默关闭后宣称完整。结束、Drop、取消及来源关闭必须产生其实际终态，Complete 不清除源质量缺口。原始字节不被重新编码 JSON 替代。

### 在途消息的所有权

单个 HTTP 方向的 `Observer` 按实际 profile 选择 `Codec`。同一方向创建一次不可变 `ProtocolSource`。每个 `ProtocolObservation` 保有自己的记录 ID、帧类型、原始字节、原生 ID 与质量，和 `ChatMessage` 共享来源引用。按原协议位置构造 `ChatPart` 的现有变体。原生 ID 的命名空间与 JSON Pointer 取自当前实际协议单元。请求、响应和不同采样点各自保留实际来源及授权范围。见[公共事件分型](../../contracts/events.md)与[Flow 事实契约](../../contracts/flow.md)。

`Codec` 依原消息出现顺序持有消息槽，每槽只有一份可变类型化内容。槽内的公共消息字段、按内容位置索引的 `ChatPart`、发布游标与原记录引用归同一所有者。私有工具状态保留尚需拼接的参数片段，已取得完整参数进入原 `ChatPart`；未取得工具名时仅保留待完成的实际值。JSON 正文由解析器借用，完成解析后移交原缓冲。SSE 按标准拼接的 `data` 与原协议帧各有自己的字节域。见[消息状态](../../../crates/backend/as-flow/src/message.rs)和[协议解码](../../../crates/backend/as-flow/src/protocol.rs)。

变更按内容位置记录。文本追加只保留新片段和原长度，其他替换在接纳前保管被替换的原部位。校验失败保留已发布的内容，回滚本帧内容变更并记录 Gap。首次与终态发布完整快照，流式中间更新只发已有的 `AppendText` 或 `PutPart`，不用整份旧消息计算增量。仅在队列接纳后推进实际发布游标。首帧已是终态时，`Started` 与 `Finished` 共享同一快照；两者之间中断仍由原状态保管待发终态。

显式结束与 Drop 消费同一组消息槽。已发布且仍在途的消息按真实失败或中断收尾，原内容所有权移交到最终快照。已正常结束的消息可在同一 HTTP 流中接纳 provider 实际给出的后续修订，传输收尾不重复发布。未实际发布 Started 的槽随当前任务释放。实时队列仍按每条事件的可达数据收费。

终态或来源缺口一旦交给现有 Flow 任务跟踪器，HTTP 调用方取消仅停止等待该任务。队列接纳、实际输出错误及停止由接管任务处理。同一 HTTP 方向只做一次终态移交。HTTP/2 连接内的 Hyper 子任务也归该跟踪器，显式停止取消原网络任务，再等它们及其已移交的收尾实际退出。来源关闭通知由同一跟踪器发布，接纳循环无需等待通知取得队列容量即可退出，停止时原消费者继续运行，清理事实沿同一队列完成移交后才关闭输出。

只在原 HTTP body 明确结束时完成解压与协议收尾。字节大小提示为零时仍可能有 trailer 或传输错误。已知传输中断不解析未完整的协议单元，已取得的非流式原字节以带 Gap 的原记录保留，已开始的消息按实际失败或中断收尾。

本地解码／解析失败也复用同一退役入口，移交已经取得的非流式正文和消息槽，不先清空正文。触发大小边界的本次已解码输入随前缀一起保留；记录质量含 Gap，完整长度和未核实字段保持缺失。原退役任务继续保管发布，HTTP 调用方停止等待不取消已移交原件；不会把失败前缀投影成完整聊天或改变原 HTTP 转发。

同一 Observer 仅在原协议记录等待输出队列接纳期间保管该原记录的一个 Arc 引用。接纳或明确失败后立即释放；调用方在背压等待中取消时，由同一退役任务接管尚未接纳的原记录，再发布原缺口。三种原单元发布入口共用此保管，不另建回放、队列或每帧后台任务，也不提前推进聊天投影游标。

SSE 同批输入按原顺序发布已完成的前序帧，随后报告该批实际解析错误。已完成分帧的 JSON 单元若解析失败或超出深度边界，仍保留该单元原始字节和 Gap，未核实的字段留空。当前方向停止投影，后续数据仍沿原 HTTP 传输语义转交。

同一次拒绝反馈写出的多个 `Applied` 共享一个不可变修改后 `EvidenceBody`，各自持有原拒绝和工具引用。原正文与修改后正文沿各自真实来源保留。线上 JSON、队列和数据库字段继续沿现有契约。见[观察与终态](../../../crates/backend/as-flow/src/observe.rs)和[实际反馈写出](../../../crates/backend/as-flow/src/feedback.rs)。

## 原数据保真

本地采集、发布、存档及查询保留实际取得的数据，不执行AS主动脱敏、遮蔽、关键词过滤或占位替换。不增加脱敏配置、匹配器、去密租约、元数据抑制Gap或为了内容遮蔽生成消息中断。原鉴权、采集范围和实际协议状态仍由现有接口负责；不借保真扩采集字段。

Observer只解析一次外层协议单元；Codec按已有profile和真实索引聚合。EvidenceBody直接保存HTTP content decoding后的原字节，offset属于该原单元；内层参数复用原serde_json解析值，原Text、Json、元数据和原生ID按实际类型传出，不重编码JSON冒充原始证据。Content decoding、半帧和实际未取得内容仍保原Missing／Gap，不补造数据。

RedactionLease、EvidenceSegment::Redacted和ChatPart::Redacted已退役，生产者、消费者和存储读取只保现行结构，不提供旧解码或兼容别名。AuthHeaderLease只承担已授权认证头替换，apply消费后不返回内容过滤材料；签发／轮换／撤销和幻影key仍按原凭证接口执行。

## 固定拒绝反馈

只在已提交业务事实给出明确桥后，经进程内 FlowFeedbackPort 准备；原规则反馈不依赖前端会话。Flow 校验传入原记录，准备完成不保留完整聊天／拒绝副本；在途条目仅保本次写入必需数据。无准备计划的普通请求不等待业务层。终态从 map 删除，历史归中间层数据库。

准备条目插入在途表后、首次等待 Prepared 输出之前，由私有 RAII guard 保管。等待期间调用取消、本次反馈取消、来源停止或输出错误，均移除同一 Arc 身份的条目并释放其容量；未成功发布 Prepared 时不补造反馈事实。只有 Prepared 实际入队、phase 转为就绪且原 TaskTracker 已接管本次反馈取消／退出监听后，准备 guard 才解除责任。之后写入者以原 WriteGuard 接管实际修改与写出，沿同一条目产生真实终态；旧 guard 不能删除复用同一拒绝键的新条目。

运行时 `RuleFeedbackPrepareInput.retention_guard` 是中间层原事实引用的 opaque `Send + Sync` 保管值，不序列化、不从历史 Prepared 重建。prepare 将其移动到同一 Transaction；验证失败或准备取消沿原局部值／PrepareGuard 释放，接纳后由最后一个 Transaction Arc 在真实 Submitted／Failed／取消／SourceEnd 收尾时释放。Flow 不解读 guard、不查询业务库，不增加另一个反馈状态机。原 Message、Notice、协议原件的校验后释放与这份数据库保留能力分开。

反馈端口为Send+Sync，不依赖调用与stop在同一runtime或OS线程。phase0入表在原feedback Mutex内复核接纳；Prepared发布成功后的就绪／supervisor交接，与stop封口复用同一把锁：锁内确认intake仍开放且条目仍为原Arc，随后phase1、原tracker注册及guard解除一并完成。stop在该锁内封闭接纳并取消原feedback_stop，释放锁后才等待任务。stop先完成封口时，尚未交接的准备返回Unavailable，不在stop回执后注册supervisor；已停止后才发起的新调用仍沿原Stopped边界失败。错误返回先释放锁，之后由guard清理，避免Drop重入同一锁。锁不跨await，不给外部未再poll的phase0 future持tracker token；phase0仍由原guard／stop清理，避免停止反等外部调用者。TaskTracker.close本身不禁止新spawn，不能将其当接纳栅栏。

后续 Responses 请求按原 previous_response_id，或同请求完整携回原 function_call 与其 function_call_output 核验目标；已有引用冲突不绕过，conversation 不能独立定位。原规则文字、原工具 ID／状态及原证据保持，修改证据单列。采用、本次反馈取消、真实写出及终态分别报告，不恢复 ID-only RPC 或 blind record_completion。

待修改的完整请求把单次解码所得 JSON、原记录引用及原生 ID 移交给反馈写入，同一次请求无需重新解析正文。已按原文提交的请求只保管当前未修改通知所需的原引用及完整调用选择摘要；同一次解析生成的选择信息在正常改写和未修改路径共享，不保留参数正文副本。精确定位到原工具结果时，待决反馈以实际不能修改的原因结束。

### 当前请求的私有选择信息

`RequestTarget` 只属于本次未完成 HTTP 请求：original 是本次 ProtocolRecordId；previous／conversation 保存本请求实际提供的原生标量及类型，references_valid 区分缺省／显式 null 与已提供但不合法的引用。calls 按实际 input 的零基下标保留 call_id、实际取得的 item ID 及调用字段 SHA256；outputs 仅保留本请求实际 function_call_output 的原 call_id。名称和完整参数摘要直接借用现有 Codec 的归一化 ChatPart 计算；不重复解析 arguments，不保留参数正文。摘要仅核协议对象选择，不是业务关系或内核因果证据。

Responses 的 Codec 只从 input 取消息；ChatCompletions／Anthropic 只从 messages 取消息。未知同名字段仍保留在原协议字节中，不代替正式协议根字段。内层工具参数复用 as_types::json::parse 的现有同次严格解析，包含流式参数片段；重复键不能先覆盖后参与确定匹配。未解析成完整参数时，不声称具备该选择依据，原协议字节仍原样发布。

只有同一真实认证域存在待决反馈时才生成选择信息；DecodedRequest 和 Observer 共用一个 Arc。正常改写与未修改／TooLate／容量失败使用同一选择器，结束即释放；无待决反馈的普通聊天不保管该信息。新增 item ID 堆字节及选择信息的 Vec 容量／标量堆字节计入既有在途与解码容量，不新增额度或时间窗。

## 实际错误与见证边界

普通响应不逐帧同步输出诊断，也不增加独立日志队列。已有Flow上下文的必要连接错误沿原Chat消费链提交含完整真实诊断的transport Gap，接纳后立即返回原错误，不再等一份重复stderr输出。无Flow上下文的初始化失败不制造Exchange／Gap，或原Gap消费者明确拒接纳时，诊断仍由公共异步stdio原语保管；原错误类别不改。背压让出运行时，不在Messages OS线程同步等待journald，不改变HTTP错误码或规则裁决。连接身份取原AdmittedConnection的channel／发起者实例，不用最近一个ExchangeKey指认HTTP/2连接错误属于单个响应；上游HTTP连接任务只使用其原独立交换。原ProtocolObservation、聊天状态及缺口继续由原消费者保管，进程最终flush只确认真实诊断写出结果。

provider 的 response.completed、聊天 Finished、队列接纳、SQL 提交、交回 Hyper、Agent-facing 实际写出／flush／HTTP 结束和 Agent 消费是不同边界。当前不提供 Agent-facing 写出／flush 或 Agent 消费见证；body EOF 只证明读到上游 EOF。连接正常返回和 Stop 成功都不能替代单响应写出回执，不据公共 dropped 计数、缺失记录或时间接近推定根因。

## 退出

Main 先停止外部入口接纳，保留原聊天消费者。Flow.stop 在原反馈接纳锁内封闭接纳，撤销反馈入口，显式取消原网络任务；等待入口任务退出，再关闭原 tracker 并实际等待全部网络任务及其已移交的 Observer／凭证／反馈收尾。原 publish 不把网络取消令牌当作队列关闭；已移交 tracker 的事实收尾继续等待原消费者。正在网络 future 中尚未完成移交的观察按真实 Drop 保留缺口，不补造队列接纳或落库。清理完成后关闭输出，Main 等消费者排空到真实 EOF，再完成模块退役。

stop 返回 Result<(), FlowError>，成功只表示上述实际等待完成。不存在排空计时器、按活动许可采样估算的已中断数量或“在期限内排空”回执。真实网络结果、对端关闭及显式生命周期取消分别保持各自事实；没有网络成功、写出或 Agent 消费的证据就不作这些声明。异常丢弃所有者只触发原取消与输出关闭，不能宣称执行了 stop 的实际等待。
