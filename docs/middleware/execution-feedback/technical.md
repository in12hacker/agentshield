# 中间层执行反馈技术契约

当前消费者实现位于[as-middleware](../../../crates/middleware/as-middleware/src/execution_feedback.rs)；它没有生产通用工具执行关联，正式服务从现有真实完成消费者发起反馈，原问询和执行结果独立转交。产品与职责分别见[设计](design.md)、[架构](architecture.md)。本文只定义中间层消费步骤，不重定义底层字段。

## 输入与输出

输入是原 MainNotice、消息入口唯一投影的 OperationResult 引用，以及同库按其 EventKey 读取的已提交活动视图。SQL写入、执行反馈和前端分发共享该投影的Arc，不在消费者内再次投影或复制RuleHit文本。EventData、RuleHit、OperationReport、EventKey均复用[Main消息](../../backend/main/messages/technical.md)；工具与关系引用复用[中间层契约](../technical.md)。固定文本只取[原RuleHit目标扩展](../../backend/rules/technical.md#固定拒绝提示)。业务日志已有原完成和交接，反馈直接读取并校验，不重新写入查询视图或完成回执。

输出是向显式传入的后端 FlowFeedbackPort 发起的进程内调用，复用原 notice、ChatPartRef、ChatMessage、ProtocolObservation、本次取消令牌。规则反馈不由前端会话授权，不因前端断开而撤销；规则编辑和内容查询仍走原权限。参数的唯一结构见[Flow公共契约](../../contracts/flow.md#固定规则拒绝反馈目标契约)。WS 客户端只提交用户选择，不能登记这些事实或提供执行关系。缺少明确工具来源时返回原 ContextGap，不调用 Flow 写入。

## 确定性路由

1. 核原OperationReport确有实际Denied；不要求额外取得 NativeRequestReturn；Failed、仅有Deny提案或缺原回执不足以声称已拒绝。终止是否成功继续取原TerminationReport，不影响已确认的Denied事实。
2. 取原命中deny_message。None是没有取得，空串是没有文本可发送；两者不生成默认提示。字符串原样转交底层的原命中上下文，中间层不另带一个可编辑text参数。
3. 活动视图须提供同一原事件到原ToolCall的明确tool_execution关系，且有原始执行上下文证据。按[审计唯一绑定校验](../audit/technical.md)核原提交进程、完整调用 ID、名称、已取得参数和响应引用；同响应内的其他工具不是原执行目标。只有命令相同／父子进程／时间接近不成立，原生ToolCall与ToolResult ID相同也只证明协议引用，不证明内核操作属于该调用。
4. 根据该原ToolCall的ChatPartRef提出请求；Flow自行定位后续实际工具结果字段。不得把客户端提供的JSON路径、ChatRole::User或模型文本作为权限／关联证明。
5. 业务日志在原反馈 claim 事务内检查该事件是否已有相互冲突的交接，并用已提交记录防止重复准备。同一工具调用若已有另一拒绝事件领取反馈，返回 AlreadyClaimed，两个原事件仍分别保留。首次接纳只表示 Flow 保管在途计划，终态历史不留在 Flow。实际改写和提交取原 RuleFeedback 事实，沿原 ChatEnvelope 报告。缺口或 Failed 保持原结果。

## 正式消费与寿命

Main 将真实 Flow 的窄反馈端口随唯一 ChatReceiver 交给 MessageModule.consume_chat；Media 只安装一次。MainMessage::Completed 沿原队列保管原 notice，Media只投影一次并将同一完成结果交给业务库、反馈与分发；固定拒绝文本即使 log=false 也保留反馈所需的真实完成。已提交后复用 execute_feedback 和原业务库查询，不建立另一个拒绝票据缓存。普通完成同样保存真实OperationResult；没有固定文本或明确工具关系只限制反馈，不限制原结果入库。

反馈端口的准备、SQL 去重和原完成转交使用同一在途输入。反馈事务不接收 deadline，也没有 Expired 状态；原 I/O 自然等待，消费源显式取消或 Flow 真实退役沿同一所有者收尾。容量不足／来源缺失等反馈结果不改变原拒绝，也不阻止原完成转交。数据库实际失败仍报告失败。

## 线程与边界

模块是现有中间层业务处理中的一个窄消费者，没有自己的安全规则池、线程池、日志库或进程树。来源接收、WS写出和反馈命令等待独立推进，不阻塞Ask展示／答复。公共接口通过显式参数传入真实结果、已核关系及消息发送端；不查找Rules／Flow内部对象或Main全局状态。

普通前端／IM管理规则时仍使用同一PolicyDocument；不能把任意“提示词”命令作为正文改写入口。反馈写入由原规则和真实拒绝驱动，传入已保存的原拒绝、消息、协议证据和原工具引用；Flow 校验完释放完整输入，只持本次写事务所需数据；模型不能通过工具内容自授权。

## 必须如实暴露的缺口

RuleHit 固定文本、Driver 拒绝回传标记和 Flow 窄写入端口已实现。正式 as-service 已从原完成消费者调用现有执行反馈接口；是否能发送仍取决于已提交的可信 `ToolExecution`。通用工具语义正式生产来源尚未接通，当前不能把候选改成已确认发送。Flow 写入范围为 Responses 的明确原响应引用，或同请求完整携回原调用与对应结果；两者均保留原真实工具关系和认证域核验。[精确因果来源](../../backend/gaps.md#精确因果来源) · [执行反馈实现](../../../crates/middleware/as-middleware/src/execution_feedback.rs)

普通 Responses 还允许把原 function_call 与 function_call_output 一起携回同一 input，不使用 previous_response_id；这是[官方正常用法](https://developers.openai.com/api/docs/guides/function-calling)。Flow 按同请求完整原调用与结果核验这类协议锚定；previous_response_id 不是协议或内核因果的必要条件。R91 原请求已取得 ToolCall／ToolResultReference 各2个、PreviousResponse 为0；这些数量只证明字段存在，不能证明内核工具关系或逐条对应。该协议目标核验与工具语义来源分开，不能只凭 call_id 改写。

业务来源、明确关系及真实反馈阶段只在同一[业务日志库](../business-log/technical.md)保存；数据库不持有效取消令牌／原操作能力，不从历史重建授权或自动重放写命令。

`ActivityContext` 查询原事件时，若原完成及唯一 `ToolExecution` 仍可核实，则按同一原拒绝 `EventKey` 和工具 `ChatPartRef` 读取已提交的最新反馈事实。在现有 `source_records` 中以 `kind: rule_feedback` 返回原 [`RuleFeedbackFact`](../../contracts/flow.md#固定规则拒绝反馈目标契约)。原执行结果、反馈阶段和已确认关系保持各自语义。只有已经提交的 Flow 事实进入视图；仅有反馈 claim、无明确归属或查询遇到归属冲突时，该项不出现。查询复用 `chat_content` 授权。
