# Agent 调用图谱技术约定

产品范围见[设计](design.md)，所有权和生命周期见[架构](architecture.md)。本页定义 Group 的来源边界、持续采集和本地只读原件分析入口。Agent内部行为链的正式查询归[因果审计](../audit/technical.md#原驱动事件为锚的行为链)，不在本模块重复实现。正式业务查询沿用[中间层业务契约](../technical.md)与[因果审计](../audit/technical.md)的 `FactReference`、`EvidenceRelation` 和 `ContextView`。

## 输入和来源域

原生运行报告提供同一 Agent 的运行和回调明细。已核验的 A2A 原件可以离线只读复算。正式 WS 查询持续采集写入业务库的原件。原问询闭环沿[现有契约](../technical.md#原问询闭环)交付。

`as-agent-graph <result.json> [<result.json> ...]` 读取本地采集侧已验明原件的报告，输出 JSON 到标准输出。每份报告的 `runtime.capture` 包含 `boot_id`、`pid`、`start_ticks`。起点沿原采集器的 Linux 进程启动 tick。三项组合限定来源域。报告中的 `runtime.artifact_binding` 必须为真。原件的 SHA 核验与只读复算归采集侧，命令检查报告内字段与引用一致性。

- `protocol_runs` 是采集报告中的运行集合。单场景权限报告用 `protocol` 承载同一运行结构。每次运行保留原 `run_id`、`session_id`、`session_key`、请求 ID、请求／回复行号、终态、终态回执和生命周期记录。生命周期 `ts` 为 Agent Gateway 的 Unix 毫秒值。每个工具的 `id`、`name`、`start_line`、`start_ts`、`arguments`、`results` 沿原生协议。工具作用域为原运行及原工具 ID。
- `records` 是已自持有的单回调记录。原 `sequence` 是报告内序号，`pid`、`tid`、`isolate`、`request`、`async_id_bits`、`begin_ns`、`end_ns` 均保留原值。`*_ns` 为回调采集器的单调纳秒。`binding` 包含采集侧已复核的 Frame 作用域、原工具／响应对象引用和原协议匹配位置。`response_timestamp` 是响应对象创建时的 Unix 毫秒值。
- `runId`、`sessionId`、`sessionKey` 从实际回调 Frame 自有值取得。连接工具时要求唯一原生 `runId`，并在该运行内校验唯一协议匹配、工具 ID、工具名称与开始行号。双方均取得的会话值须一致。原 `toolCall` 和 `assistantMessage` 属性属于同一上下文对象，响应内容数组中的实际工具引用须等于该 `toolCall` 引用。证明不足或值冲突的回调仍输出原记录和缺口。

## 输出结构

输出含 `sources` 和汇总 `coverage`。每个 `SourceView` 含 `source`（报告路径）、`capture`、`runs`、`callbacks` 及本来源 `coverage`。报告路径用于追溯，来源域取原启动身份。报告内重复运行、同一运行的重复工具 ID、重复回调序号或不同报告使用相同来源域属于输入错误，命令以非零状态退出。

| 结构 | 字段与含义 |
| --- | --- |
| `RunView` | `run_id`、`session_id`、`session_key`、`request_id`、`request_line`、`reply_line`、`terminal_line`、`terminal_state`、`terminal_receipt` 均来自原协议。`window` 是可比较的原生命周期毫秒区间，缺失为 `null`。`tools` 包含该运行的全部原工具调用。 |
| `ToolView` | `id`、`name`、`start_line`、`start_ts`、`arguments`、`results` 取原协议。`callback_sequences` 引用证实归属的原生请求。`run_timing`、`response_timing` 给出同来源的时序对照。 |
| `CallbackView` | `original` 保留原回调记录，`source_status` 保留采集报告的状态。`relation` 存在时给出已校验的 `run_id`、`tool_call_id`、`response_id` 与 `tool_start_line`。`gap` 的取值为 `incomplete_capture`、`missing_proof`、`ambiguous_proof`、`missing_run_identity` 或 `conflicting_evidence`。 |
| `Coverage` | `runs`、`tool_calls`、`linked_tool_calls`、`callbacks`、`linked_callbacks` 是实际输入计数。工具分母包含失败、没有回调和没有结果的已观察工具。 |
| `Timing` | `possible`、`excluded`、`unavailable` 分别存放符合顺序、时序排除、时间不足的原候选引用。运行引用为 `run_id`，响应观察引用为 `response_id`、`callback_sequence` 和用于本次比较的 `created_ms`。 |

原参数、原工具结果、终态回执和回调记录保留 JSON 原值，包括显式 `null`、空值、零和 `false`。可选原参数与终态回执的缺失属性省略。可选标识和时间缺失时输出 `null`。时序字段不足时给出 `unavailable`。输出保持报告顺序、报告内运行键顺序和原回调顺序。

## 委派回执候选

既有离线入口从同一采集源的原 `sessions_spawn` 工具结果读取结构化 `details`。只有原 `toolCallId` 与所在工具一致、结果行晚于原开始行且成功结果明确返回 `status=accepted`、`runId` 和 `childSessionKey` 时，输出一条 `delegation_candidates`。字段包括父运行、原工具引用、结果行号和回执中的子运行与会话。报告按原结果行排序。

`observed_child_run` 只在同一采集源实际存在运行 ID 和会话键同时相符的子运行时填写。启动接纳与子任务完成分别观察。两个会话可能属于同一 Agent，跨 Agent 确认关系仍需两端真实身份及交接证明。工具正文中的 JSON 文字和时间接近不参与此来源识别。[OpenClaw 会话工具](https://docs.openclaw.ai/session-tool)

旁路采到的子进程、stdout 端点及原字节用于核对传输事实。确认父工具与子会话的关系仍需原来源明确给出的交接引用，并核对它在两端各自的原身份域。Pi 并发实测的正式 TLS 结果缺少这份引用，传输观察保留为候选。[并发原件](../../evidence/runtime-correlation-research.md#并发委派与通用旁路的证据边界)

ACP 的连接内请求 ID 与原 `sessionId` 可核对子会话和原轮次终态。A2A 的消息、任务和上下文各沿其原标识核对。同一服务进程可以同时处理多个 A2A 任务，`ProcessKey` 只归属实际进程。跨到父模型工具或子模型交换仍需原生产者明确给出的交接引用。两种协议的 CLI 工具正文回显可用于与原流对照，正式关系保持候选。[ACP 与 A2A 并发实测](../../evidence/runtime-correlation-research.md#acp-与-a2a-的协议交接)

原生 OTel 实测已核对子任务的原 `contextId`、运行时 `gen_ai.conversation.id`、模型原 `gen_ai.response.id` 和 SQLite `ExchangeKey` 的交接。父工具跨度与子任务之间仍需核对实际传播的 trace 上下文、显式 span link 或其他原生交接引用。对照版本的 A2A 发送路径缺少该引用，确认边保持空。[原生 OTel 并发实测](../../evidence/runtime-correlation-research.md#原生-otel-的父工具和子模型边界)

### 传播 traceparent 的本地 A2A 原件

`as-agent-graph --a2a <capture.json> <business.sqlite>` 读取本地采集者已核验的同一宿主机原件。顶层 `parent` 和 `child` 为实际 `ProcessKey`，`spans` 各项含 `process`、`trace_id`、`span_id`、`parent_span_id`、`kind`、`attributes`，保留原标识和属性。`requests` 各项含 `url`、`server_address`、`server_port`、`client_port`、`path`、`status`、`traceparent` 以及原 JSON-RPC `request` 和 `reply`。采集侧核实双方进程寿命、原连接和原件摘要。该入口以只读事务查询现有 SQLite，并复用 `FactReference::Protocol`、`ProtocolRecordId`、`ExchangeKey` 与 `ProcessKey`。

确认父工具到子任务要求原 `traceparent` 精确定位父进程的 HTTP 客户端 span，该 span 的原祖先含唯一 `execute_tool` 和原 `gen_ai.tool.call.id`。子进程的 HTTP 服务端 span 以该客户端 span 为直接父引用，原连接端口、路径及实际 HTTP 状态一致。原 A2A 请求和成功回执按 RPC ID 核对，回执任务历史含原请求消息 ID，子 `invoke_agent` 使用该任务的 `contextId`。父工具 ID 在父进程的唯一原模型响应交换与后续原工具结果中都需存在。同一个客户端跨度反复声称为多个原请求时，输入整体冲突。

继续连到模型交换要求同一个子 `invoke_agent` 的后代模型 span 使用原会话 ID，且 `gen_ai.response.id` 在该 `ProcessKey` 的 SQLite 原上游响应中定位唯一 `ExchangeKey`。多个实际模型交换各自保留引用。输出的 `delegations` 保留原请求、任务和终态身份，以及核对过的工具、客户端及服务端 span 和原事实引用。`gap` 依次区分 `original_task_unavailable`、`task_terminal_unavailable`、`trace_handoff_unavailable`、`parent_tool_facts_unavailable`、`child_model_exchange_unavailable`。完整关系的 `gap` 为 null。未终结的回执保留已取得的任务和会话 ID；实际失败、取消或拒绝保留其原状态。缺模型证据时保留已证实的父工具到子任务及已核对的其他模型记录。正文、时序、裸 PID 与单独相同的 trace ID 都不形成确认边。适用范围为已核验的非流式请求及直接传播的 HTTP span 父引用。[原版并发验证](../../evidence/runtime-correlation-research.md#原版-a2a-端到端追踪交接)

## 持续采集的来源边界

模型请求的原追踪头以 [Flow 契约](../../contracts/flow.md#原始协议观察) 为唯一来源。Group 从已核验子进程的模型 span 找到实际 HTTP 客户端后代，并用该 span 的 `traceId`、`spanId` 和 HTTPS origin 查询 Flow 原请求。原响应 ID 再校验同一 `ExchangeKey` 的结果。采集输入不提供模型请求到交换的映射。仅凭进程、origin 和响应 ID 的匹配仍为候选。

采集范围使用配置模块定义的[Group来源端口](../../backend/configuration/technical.md#group-回环观察范围)，各来源可独立不启用，均仅覆盖宿主初始网络命名空间中两端均为 `127.0.0.1` 的 IPv4 字节及实际 I/O；该源范围不包含 Docker bridge、容器网络命名空间或非回环远端。修改范围需要重新初始化。A2A／OTLP来源接入明文HTTP/1.1非流式A2A请求／回执和直连OTLP/HTTP Protobuf。Gateway WebSocket来源见[公开Gateway旁路来源](#公开-gateway-旁路来源)。HTTP正文按唯一的`Content-Length`或单一`Transfer-Encoding: chunked`定界。分块报文包含终止块及完整尾字段区后才接纳。原始定界字节全部用于发送身份校验和入库，协议解析只消费逻辑正文，尾字段不参与归因。拒绝同时声明两种定界、重复定界字段及其他传输编码。

- 驱动仅提供原字节、连接和进程归属。IPv4 本地出站 TCP 包提供握手、原字节区间和关闭事实；发送观察使用`tcp_sendmsg_locked`，接收观察同时覆盖`tcp_recvmsg_locked`与`__tcp_read_sock`。后者只接纳原第四参数`noack: bool`为false且第五参数`copied_seq: u32 *`实际指向本socket的`tcp_sock.copied_seq`的消费调用，覆盖socket到pipe的splice读取；不把旁路游标、原包到达或peek冒充应用读取。三个fprobe共用原socket范围选择、cookie及同次调用的起止序号／正返回字节核对；成功区间沿同一Transfer记录输出原进程出生时刻、TGID和received方向。接收入口均在socket锁内，返回仍可能跨过释放socket锁的等待；只有序号推进与该次实际返回一致的区间可用。返回不一致、同段来源冲突或数据缺口均不能用于确认。全部入口注册成功才声明来源可用；初始化失败逆序回收本次已注册入口，正常退出全部摘除。驱动通过原鉴权会话上报，使用有限的原生普通事件槽。适配器直接向 Messages 运行时上的单个 Group 消费者交接，避开裁决及普通完成通知队列。停机先排空已接纳的驱动帧，再关闭发送端并等待消费者。
- [Moby TCP代理](https://github.com/moby/moby/blob/v28.5.2/cmd/docker-proxy/tcp_proxy_linux.go)复用双向`io.Copy`；[Go Linux TCP拷贝](https://github.com/golang/go/blob/go1.25.4/src/net/splice_linux.go)可采用splice。Linux的`tcp_splice_read`消费接收队列时不经过普通recvmsg读取入口；相关调用及socket锁约定见[内核TCP实现](https://github.com/torvalds/linux/blob/master/net/ipv4/tcp.c)。当前7.2.12+deb13-amd64的真实内核BTF保留`__tcp_read_sock(sk, desc, recv_actor, noack, copied_seq)`原签名，实际机器码中`tcp_splice_read`直接调用它：`tcp_read_sock`小包装已被内联，不能仅凭包装符号存在声明采集生效。只注册实际内层入口，不双挂包装或按旧入口回退；参数仍由匹配内核的类型与真实调用约定读取，不硬编码对象偏移或指令地址。这补齐同一回环来源的接收路径，不扩大网络命名空间、地址范围或业务关系权限。
- 原采集字段使用驱动来源实例、该来源内连续序号、回环有向端口、TCP 32 位序号、包标志及原负载。调用记录另带实际进程身份，后端以宿主 boot ID 组合现有 `ProcessKey`。消费者用完整握手与实际 TCP 字节区间连接收发记录。端口、序列、线程或时间单独都不能绑定原请求。指针不会传给业务层。
- 中间层只保留还未消费的连接重组、HTTP 边界和待配对区间。消费后释放。A2A 从当前采集源观察到的完整握手开始。直连 OTLP 还可从已经建立的 TCP socket 的首个应用发送开始。驱动在持有该 socket 的发送锁时核对 TCP 已建立、累计发送负载为零、发送和重传队列均为空，并报告该 socket 的内核 cookie 和首字节序号。后续仅消费来自同一个 cookie 的原包和实际发送区间。未观察到握手或首字节的连接缺原报文边界，停采期流量不能补录。来源实例/初始化代改变时结束全部在途连接。相同来源的向前观察序号缺口只证明采样通道缺记录，不证明每个socket都缺字节。Gateway已核完整握手、且从实际原包及收发回调绑定两端非零socket cookie的在途连接保留；每份后续原件仍须核实际socket实例、TCP字节及双方收发区间。尚未核完整握手/两端socket的连接及其他来源结束，从下一原边界恢复。已取得的未完成原片按实际来源与缺口落库，不能补造丢失字节或进程身份。原生单帧不是TCP负载上限：完整逻辑观察沿[原驱动封套分片](../../contracts/driver-communication/technical.md)在同回调复制、原子接纳后由适配器重组一次交接。真实分配/队列拒收仍产生序号缺口，不扩大队列掩盖。不补读 Agent 内存。请求／回执必须按同一 HTTP 连接内的消息边界配对，重复追踪头、重复声明同一客户端 span 的请求、同一来源中的冲突 span 均不给出交接证明。OTLP `process.pid` 属于 exporter 自报，直连批次的进程身份只取实际内核发送证据。
- SQLite 由现有业务日志所有者保存 A2A 请求／回执原字节、两端 `ProcessKey` 及真实连接身份，同时按原 trace/span 键索引已核发送方的 OTLP 批次。OTLP 与 Flow 允许晚于 A2A 回执入库。正式查询在 SQLite 读事务内用对应 trace 的原 span 和 Flow 原记录复算，缺原交接保留候选。查询不回放整个历史、不靠到达顺序绑定。

## 正式查询

适配器的 Group 队列独立记录实际拒收数。队列满、消费者已关闭分别计数；首次拒收及发送端退役时报告实际结果。最后一帧被适配器拒收也会计入。业务侧序号缺口只报告同来源实际收到的帧之间不连续的边界。内核队列尾部未交接的帧没有独立计数，以上诊断均不表示采集完整。

配置范围内已收到的 TCP 负载与实际收发区间，若无法归入已经接纳的连接，则分别按原驱动帧保存在业务 SQLite 的 `group_unframed`。每条记录携带宿主 boot ID、驱动来源及代、序号、源／目的端口、socket cookie 和 TCP 序号。负载记录引用原字节，收发区间记录保留实际进程身份和区间长度。此类记录标记 `http_boundary_unverified`，不生成 HTTP、span 或委派关系。停采期间未收到的字节不补写。

`BusinessQuery::GroupDelegations { before, limit }` 沿现有 Unix 和 WS 入口，要求 `chat_content`。返回 `group_delegations { records, next_before }`。两类记录共用按写入顺序分配的 SQLite 十进制排他行号 `id`，`limit` 为 1 至 64。`kind: a2a` 保留 `capture_id`、父子 `ProcessKey` 和原 `delegation`。A2A 原请求身份由驱动来源和真实连接内的 HTTP 请求起始位置确定。服务没有外部原件写入接口。

`kind: openclaw_tool_receipt` 是接纳回执候选。`actor` 为原模型请求连接的 `ProcessKey`，`receipt` 为该请求的 `FactReference::Protocol`。`call_index` 和 `output_index` 是原请求 `input` 的从零开始下标，分别定位唯一 `function_call` 与唯一 `function_call_output`。两项的非空字符串 `call_id` 必须逐字相等，调用名必须为 `sessions_spawn`，工具输出必须是结构化 JSON 字符串并报告 `status: accepted`、非空 `runId` 和 `childSessionKey`。响应协议仅使用无采集缺陷、具有上述原生工具结果引用的 OpenAI Responses `client_request` 完整正文。`tool_call_id`、`child_run_id`、`child_session_key` 均保留原值。后续请求重带相同内容时，其事实引用和下标仍属于当次请求，每行表示一次观察。

该回执的 `gateway` 复用下文公开 Gateway 原观察，`gaps` 按实际接纳交接、子终态和父公开回投动态计算。缺失与冲突分别保留，不能用模型自然语言、进程归属或时序补造。SQLite 只写派生索引与原件引用，原字节由业务日志保存一份。

## 原版 OpenClaw 运行记录的证据范围

离线命令 `as-agent-graph --openclaw-handoff <gateway.jsonl> <registry.json>` 消费同一个已验明 Gateway 实例的原接收帧和只读子运行登记快照。原工具起始及结果使用同一父运行、父会话、Agent 和工具调用 ID。结果的结构化接纳回执提供子运行和会话键。子执行终态来自登记中两键同时相符且请求方会话一致的原行。原父会话 `chat.final` 的 `announce:v1:<childSessionKey>:<runId>` 和登记的 `delivered` 同时成立才确认回投。结果引用原帧行号和登记行号。缺失或冲突的证据保留为未确认，命令只读复算，不为正式 WS 导入事实。

冻结版 2026.10.9 的 `subagent_runs` 以原运行和会话键保存登记。结构化载荷可提供明确的请求方／子 Agent 身份及子执行状态。登记中的 `requesterTurnRunId` 可能在已接纳的 steering 后更新，它只描述采样时的请求方轮次归属。可核实的委派边应使用原调用回执绑定父执行与子运行，同时核对两端 Agent 身份和原生命周期。子执行完成与结果投递分别保留。

会话工具动作的原审计路径绑定当前调用者的执行和运行身份，写库时将目标会话转换为不透明引用。该审计记录和可变登记各自保留原义。R68 原状态库的登记表为空，原件可用于核对来源形状，缺少跨 Agent 正例。[原件核查](../../evidence/runtime-correlation-research.md#group-原版委派来源核查)

## 时序规则

- 在同来源内，用 Gateway 的原生命周期开始和结束界定运行窗口。工具开始时点落入闭区间时保留候选，超出时可排除。缺开始、结束或工具时间，或者区间倒置，保留为时间不足。
- 已证实归属到运行的响应观察可以进入该运行各工具的响应对照。比较原响应对象创建时点与 Gateway 工具开始时点。创建发生在工具开始之后时可排除，先于或同毫秒时保留候选。每份原观察独立引用，同值响应 ID 不合并对象，也不重复计作已证实工具。
- 运行和响应对照仅比较同一来源内的 Unix 毫秒。单调纳秒作为原回调字段输出。没有时钟对应原件时保留各自时间。时序对照即使只剩一个候选也独立于确认关系。原引用证据成立时继续保留；若时序排除了该项，输出同时显示实际矛盾，便于复核。

## 使用和验证

先在各证据目录按原 `evidence.sha256` 与只读复算脚本核验，再运行：

```sh
cargo run -p as-middleware --bin as-agent-graph -- /path/to/r48/result.json /path/to/r49/result.json
```

R48／R49 的对应来源见[原生分析证据](../../evidence/runtime-correlation-research.md#r48-原版-agent-的运行会话和工具作用域)。检查五次运行、六次工具和七条原生请求的原件回放，并用并发与缺证据样本检查时序候选不会升级成确认关系。

## 公开 Gateway 旁路来源

原Group TCP来源使用显式配置的明文Gateway WS端口，不主动连接Gateway、不发订阅或查询、不读Agent私有内存/运行登记、不按进程名选择Agent。底层仍只给原TCP字节和实际收发进程。共用来源／公共协议解码归[中间层窄适配](../technical.md#nativegateway-共用来源与解析入口)，Session／聊天投影归[业务日志](../business-log/technical.md#session-与原生聊天业务模型)；本模块只定义下文委派／终态投影。原件沿同一业务库持久化，不增加导入接口或第二历史缓存。

配置GroupObservation的a2a_port、otlp_http_port和gateway_ws_port分别表示三种明确来源端口，均可不启用；启用的端口非零、互异且至少一个。不存在所选来源时保留缺口，不为无用来源填写占位端口。配置改变继续要求完整重新初始化，原格式删除而不兼容解析。

Gateway的握手、Upgrade、WS和引用接纳直接遵循[共用来源契约](../technical.md#nativegateway-共用来源与解析入口)，Group 不另写分帧或 JSON decoder。`agent_graph::gateway::project` 借同一 GatewayDocument，只输出本页定义的 GatewayFact；数据库 helper 保存已给定的投影，不从 SQL writer 内识别一般 Session／聊天。

原件、方向和帧位置的表示直接引用[共用来源与原引用](../technical.md#共用来源与原引用)，存储只用[业务日志原件 owner](../business-log/technical.md#gateway-原件存储)。TCP32位序号按当前已消费位置作有界序数换算，保留窗口小于2^31字节；不以时间换算或把序号回绕当重连。消费后释放在途字节，不缓存已完成消息。

### 任务与终态层次

现有Flow接纳回执继续按其原ProtocolRecordId和完整child_run_id/child_session_key定位该子任务。公开Gateway事实必须带可核的同一任务身份和源实例，缺失或冲突不向其他运行或旧会话对象顺延。

- 子终态：仅消费实际所见的该child本对象明确lifecycle结束/错误，或公开会话行精确lastRunId指向该child_run_id且明确status为终态的原投影。原status/phase/outcome分别保留；不能以父announce生命周期、idle、连接关闭或LLM回答结束代替。
- 父回投公开观察：原父会话的chat.final携带完整announce身份，且能沿原接纳回执/公开调用交接核对父会话、完整child两键；保存原帧与序列。这只表示父announce最终结果的公开发布/接收，不冒充registry.delivery耐久delivered、用户已读或child成功。
- 只有满足上述原引用的字段进入正式查询。私有registry/独立见证仍只作验收核对；自然语言和时序不产生确定关系。来源没有发布需要的信息，继续保留对应缺口。正式公开观察与离线registry共同判定的旧语义明确分开，不暗中升级或降低证明。

### Gateway原件、派生字段与查询引用

共用 `GatewayConnection`、`GatewayReference`、`UnverifiedGatewaySource` 的字段唯一见[中间层来源与引用](../technical.md#共用来源与原引用)。原连接、socket、wire、Gap 与未核片段表的表示唯一见[业务日志原件存储](../business-log/technical.md#gateway-原件存储)。本页仅定义 Group 的派生索引和关系判定。

`group_gateway_facts`只存可索引的派生类型/身份、原消息引用与已取得公开字段：

- `accepted_spawn`：服务端`agent`、`stream=tool`、`data.name=sessions_spawn`、`data.phase=result`、`data.isError=false`及`data.result.details.status=accepted`。父`payload.runId/sessionKey/agentId`、`data.toolCallId`、details的`runId/childSessionKey`均须为非空字符串。字段分别叫`parent_run_id/parent_session_key/parent_agent_id/tool_call_id/child_run_id/child_session_key`。原Gateway工具ID与Flow call_id各自保留，不剥后缀或混为同一ID。
- `child_terminal`：`lifecycle`来源取原`agent`事件的`payload.runId/sessionKey`、`data.phase=end/error`且`data.executionSettled=true`并未显式yielded=true；`session_projection`来源只取`event=sessions.changed`的`payload.session.key/lastRunId`和status为`done/failed/interrupted/killed/timeout`的完整会话行。前者保留phase及实际出现的status/outcome/aborted/stopReason原JSON值，后者保留原status；不同来源不改写成同义终态。sessionId/agentId仅在各自原对象实际提供时保存。自然语言、idle或无lastRunId的终态不派生。
- `parent_publication`：只取服务端`chat`、`payload.state=final`及非空runId/sessionKey，保留原agentId与seq；是否属于本任务，查询时按完整`announce:v1:<child_session_key>:<child_run_id>`逐字核对。final的`yielded`原值在存在时保存，不把yielded声明成任务完成。

每种事实的`event_sequence`是外层公开事件seq，`sequence`是payload.seq；都是原JSON非负整数，不用于跨连接排序。原缺失属性省略；终态扩展值采用原JSON值，显式null、空字符串、空数组、0和false不会被改写成缺失。完整正文始终可沿原帧引用复核。重复JSON对象键的消息不派生身份事实。

`openclaw_tool_receipt.gateway` 含 `accepted_spawns/child_terminals/parent_publications` 三组原观察；每项是 `{reference: GatewayReference,fact}`，原引用复用[共用定义](../technical.md#共用来源与原引用)，不扩展全局 FactReference 或增加外部导入。派生数组中的值仍是原观察，是否可组成当前关系须结合 gaps。多连接相同观察各保留 ref，不视为多个任务或冲突。

查询只在Flow actor与Gateway server完整ProcessKey相等的来源中查原身份。accepted_spawn按完整child两键核对父run/session/agent/tool；同一child run指向不同会话、父身份或工具为`gateway_receipt_conflict`。子终态要求child两键同时逐字相符；同run的不同session，或相同来源终态中双方已取得字段的原值矛盾，为`child_terminal_conflict`。缺字段不覆盖已有值；已出现的JSON null仍按Some(Value::Null)保留，与属性未出现分开。当前冲突比较只对双方非null的原字段判异，不借此改写、补全或覆盖任一原件；不以候选唯一确认。父publication还须与不冲突的同一accepted_spawn所给父会话/Agent相符；错误父身份或相同publication的实际yielded声明冲突保留`parent_publication_conflict`。全部相关原观察和引用仍返回，不顺延更早对象。生命周期和会话状态投影的不同原词汇不自动当作同一字段冲突。

`gaps`动态返回：缺accepted Gateway回执为`gateway_receipt_unobserved`；缺精确子终态为`child_terminal_unobserved`；缺父公开announce为`parent_publication_unobserved`，虽有announce但父回执身份未核实为`parent_publication_unverified`；实际冲突分别使用上述conflict值。这里没有registry耐久delivery/用户已读的完成门槛，也没有声称观察到它们。Gap原帧只记录实际来源缺口，不把没有任务身份的连接缺口冒充该子任务的终态。

这些解析条件不表示来源一定发布全部阶段。实际覆盖受现有客户端自身订阅、消息发布及真实字节观察边界约束；有UI在场的公开会话投影不能外推为没有客户端时默认推送。纯解析或原件复算不声明驱动保护效果。

### 采样通道与原socket生命周期

内核[sock_diag_save_cookie/sock_gen_cookie](https://github.com/torvalds/linux/blob/master/net/core/sock_diag.c)使用同一sk_cookie；[公开BPF helper约定](https://kernel.googlesource.com/pub/scm/linux/kernel/git/torvalds/linux/+/ea5751ccd665a2fd1b24f9af81f6167f0718c5f6/tools/include/uapi/linux/bpf.h)说明该身份在socket寿命内稳定。AS使用已经取得的NativeGroupTcp.socket_cookie，不加hook、不回读瞬时内存，不按PID、端口/时间相近替代该身份。来源/初始化代或socket身份改变不可接续旧上下文；未取得的socket身份不赋默认值。

Socket身份从实际data/transfer事实绑定一次；纯SYN/SYN-ACK或无完整socket的控制包不绑定身份。已核连接后无cookie的控制包不能推进该socket生命周期；无法归属的Gateway控制原件连同实际零长度载荷保留在group_unframed。普通不同cookie原件不能写进旧连接。观察到完整initial SYN且具有实际非零客户端socket cookie时，可将旧协议上下文按new_handshake_prior_terminal_unobserved退役后接纳新握手；这不宣称旧socket已经结束。同cookie的新SYN也不能沿用旧WS状态：socket身份不是一次TCP连接代，原TCP/协议边界仍独立核对。源序号向前缺口保持诊断，只有已核双socket与Upgrade的Gateway上下文可继续核实后续原件。实际TCP字节缺口、冲突及缺少收发证明仍阻止对应消息进入语义索引；不跳过缺字节，不从后续JSON猜边界，也不为此扩大队列、设置时间窗口或缓存已完成消息。

[Suricata原始流重组实现](https://github.com/OISF/suricata/blob/main/src/stream-tcp-reassemble.c)及[流检查约定](https://docs.suricata.io/en/suricata-8.0.3/devguide/internals/engines/stream/inspection_raw_data.html)按实际TCP流管理字节缺口；[Wireshark重组说明](https://www.wireshark.org/docs/wsug_html/)也要求缺失的实际段齐全。这里吸收流级连续性管理，不复制其可能使用的推定/越缺口策略；AS还核内核socket实例和原I/O归属。全局采样缺口与某条原流缺字节分开认定，不能以全局事件序号抹掉已经取得且能核对的其他连接。
