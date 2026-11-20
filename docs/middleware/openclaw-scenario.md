# Docker OpenClaw场景与底层缺口

本页定义自有Docker测试环境中AS现有接入链路；OpenClaw和模型是第三方工作流背景，只迭代AS自身及测试环境，不改Agent源码或承诺加速Agent。已支持的地址族和流量范围统一见[受控 TLS 出口](../backend/flow/tls-egress.md)，容器覆盖以[底层能力清单](../backend/gaps.md)为准。Docker 内实际信任配置和真实 OpenClaw 使用效果由指定聊天验收。

## 一次实际使用应该经过什么

1. Docker中的OpenClaw使用phantom凭证发起模型请求。主机侧要取得真实ProcessKey、原cgroup及经运行时核实的ContainerInstance；不能将容器内PID1、容器名或NAT后的IP当作完整身份。
2. 请求进入AS明确接入的受控出口，客户端信任对应TLS证书。底层TLS中介验证实际上游目标；凭证所有者经CredentialPort按原主体、目标和绑定范围授予一次性认证头租约，真实发送者注入对应请求。真实key不回到OpenClaw、Docker环境、中间层或日志。
3. 同一出口产生请求／响应的原始协议观察，经过HTTP与SSE解码，将消息和provider原生ID推给中间层。ClientRequest与UpstreamRequest用同一真实ExchangeKey区分两侧；认证替换的真实结果另有CredentialUseFact，不伪装成聊天内容修改。
4. 模型返回文本及tool call。底层保留实际响应、tool id、名称、参数、原字段路径和来源片段，不能把模型的调用建议标为已经执行。
5. OpenClaw调用本地工具／外部工具后，在后续模型请求中提交tool result。底层保留其中实际引用ID与原始内容；若发生现有受控系统操作，另有真实EventKey、ProcessKey和操作结果。中间层基于这些事实构建会话／工具／系统操作关系，原记录不可被关联结果覆盖。

本场景的 phantom 和聊天采集共用现有受控 TLS 出口，明文只交原 provider 解析器。独立被动 TLS 采集尚未实现，首期 WS 消费现有出口事实。

HTTP2同一连接可并发多个请求，凭证使用必须绑定每个实际exchange，不能保存全局“当前key”。目标校验包含该请求的实际authority／origin与TLS对端；重定向或连接复用不能沿用前一次授权。容器内OpenClaw／Node实际信任库也必须核实，宿主机信任CA不证明容器客户端已信任。这些是凭证／传输职责，不交中间层会话关联来补授权。

## 底层应该提供的事实

结构统一依赖[流量公共契约](../contracts/flow.md)，不在本页复制字段：

- 主体事实：原ProcessKey、已核Workload／ContainerInstance、实际cgroup／namespace事实与观察时刻；未知仍None。既有进程树只填缺失，不覆盖源事实。
- 传输事实：来源代、ChannelRef、ExchangeKey、方向／中介两侧、真实原始记录范围和缺口。连接、请求、聊天消息、系统操作的ID分域。
- 协议事实：原生request／response／message／tool-call／tool-result引用ID的类型、值和路径，以及实际可见的消息／工具参数。保留原始字节，真实缺失有标记；不只给摘要／重新序列化JSON。
- 执行事实：原操作结果沿已有链路输出，不用模型tool result或已提交signal取代真实内核结果。ProtocolObservation只证明线上内容。
- 凭证事实：真实凭证执行者给出绑定标识、范围拒绝／实际注入／请求提交状态，不输出秘密或phantom原值，也不把HTTP200泛化成任意凭证验证成功。

## 哪些关联可以直接保留

同一HTTP exchange中的请求与响应、同一provider item的增量／最终值、协议显式给出的tool call与tool result引用都是可提取的原生关系。底层保留字段和出处，跨请求的实际关系计算仍放中间层；不会另建ConversationGraph、ToolExecutionGraph或置信度算法。

同一 PID、父子关系、时间和参数都不足以证明某条工具调用造成某条系统操作。已读 OpenClaw 的 write 直接使用文件接口，exec 经 shell／supervisor，不同调用可以携带相同命令。缺少显式 ToolCall 到 EventKey 的来源桥时，中间层返回原 ContextGap。明确来源冲突保留冲突，命令比较仅返回 EqualBytes、DifferentBytes 或 NotComparable，不添加时间推定关系。唯一结构见[因果审计模型](audit/technical.md)。

精确工具执行归因仍缺通用生产来源。底层已保留实际取得的目标 argv、线程寿命及文件原对象事实，单个冻结 OpenClaw 样本的调查不能证明通用接入。来源边界见[运行时证据](../evidence/runtime-correlation-research.md)，中间层通过现有 ContextGap 提供实际缺失原因。

## 支持范围与待接入项

默认 rootful Docker 私有 IPv4 出口和实例归属已通过原版 OpenClaw 验收。受控出口观察 HTTP/1 和 HTTP/2 正文，新连接须在 AS 接入后建立，原客户端须信任 AS CA。被动 TLS 采集尚未实现。[容器实例绑定](../backend/driver-adapter/technical.md#容器实例绑定) · [受控出口](../backend/flow/tls-egress.md#支持范围与成熟依据)

聊天、Ask 和确定关系沿现有中间层业务日志入库。精确执行关联仍缺原 HTTP 交换、原工具和原操作的可信交接。工具生命周期事件提供原工具事实，现有命令对照只用于报告内容一致性。[业务日志](business-log/technical.md) · [来源缺口](../backend/gaps.md#精确因果来源)

## 文件问询与审计验证

通过正式 WS 首次鉴权和原问询协议核对同一次 File.Open 的等待、答复、原回执及实际文件效果。已启用写关闭审计的目录还核对内核通知、原安全日志与分页结果。R65 已在原版 OpenClaw 验证 Allow 后的真实写关闭及 Deny 时无对应写关闭。写后扫描未接入。[问询验证](../evidence/middleware-websocket.md) · [R65 写关闭原件](../evidence/file-wait-extension.md#r65-原版-openclaw)

文档对齐、源码实现和真实场景效果分别报告。已授权的正常OpenClaw／模型流程复用当前唯一执行入口，先完整实跑至真实终态，再按原件分别报告保护效果、幻影凭证／模型交互、脚本观察与实际退役；可选归因取证不作为前置，不新增测试矩阵。未执行、未取得和实际失败分别保留，不以缺字段补0或提前销毁仍在执行的原请求。
