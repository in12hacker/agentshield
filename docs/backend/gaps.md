# 底层能力缺口与唯一归属

本页是当前能力清单及归属入口，不另定义协议字段。实现和验证分别以所属模块契约和绑定源码的证据为准。产品未实现、实现违反现行契约、明确不支持的边界分别记录；不能把三者统称为一个待补函数。安全日志仍归既有[安全日志模块](security-log/technical.md)。

## 当前剩余范围

普通HIPS、受控TLS／幻影key、原始事实采集和工具反馈分别按所属模块的现行契约及绑定产物的证据认定。原生组件不能代替stock验收；精确工具归因、执行帧调查和固定反馈不作为普通防护／凭据能力的附加前置。本页记录能力归属，不指定当前开发或验收任务。

R6已取得11份实际CAPI→Event交接和11份原返回，覆盖File、Spawn、Connect与直接TryWrite；这些原始事实能力不再列为未实现。R91未取得的EventBound和Deny原返回只保留该轮历史缺失，不反复挂为当前研发缺口，也不为补齐旧原件重新发业务。

独立功能缺口仍如实保留：通用“当前工具执行”可信发布及其到ChatPartRef的原交接未完成；公开消息候选不自动确认。排队Write／UDP的原请求条目／批次所有权与直接TryWrite分开。固定提示真正写出仍未证明，不因此阻断已授权HIPS／Phantom测试。[唯一来源边界](driver-adapter/runtime-observation-technical.md) · [历史和本轮证据](../evidence/runtime-correlation-research.md)

进程创建／退出和现行操作现场采集已经接通。线程创建／退出、其他唤醒／接收或语言运行时入口只有在已核实来源及明确消费用途后才接入，不把一份可采API列表当作全部必须开发的产品功能。[回调内主动采集与边界](../drivers/linux/hooks.md#回调内主动事实采集)

原Ask、候选查询、窗口和前端消费是不同层次的完成状态。中间层已有接口而页面未完整消费的差异，由[中间层业务缺口](../middleware/gaps.md)唯一维护，不在底层重做规则、聊天库或关联算法。

### 已完成能力的定位

FileOpen、Execute、Connect、Send的原请求返回和等待交接，private futex事实及ActivityContext已接通；现场字段、进程树、容器实例、规则和日志的结构继续以各所属模块契约为准。[原请求返回](../contracts/events.md#原生系统调用请求返回) · [原件与执行范围](../evidence/execution-site-capture.md) · [同步对象复验](../evidence/runtime-correlation-research.md#private-futex-正式采集和业务库复验)

正式WS、聊天历史、应用Session、规则／配置／临时控制／凭证、原问询和原交换导航已有实现及原版OpenClaw有限验收；后续开发复用同一接口。[现行中间层契约](../middleware/technical.md) · [最新Session与整包证据](../evidence/middleware-websocket.md#原应用-session-的正式查询链)

### 明确边界与待定范围

- 文件写入使用新取得写访问时的Open Ask和已配置目录直接子项的写关闭审计；已打开FD的逐次Write Ask已移出交付范围。写后扫描未接入，不因已有关闭事实自动扩为扫描产品。[文件问询与关闭](../drivers/linux/hooks.md#写打开与写关闭)
- 已有共享映射经mprotect新增写权限，本期只保留自保即时检查。[原范围记录](../archive/unresolved-module-contracts.md#已有共享映射升级的普通问询)
- 安全审计库与业务库的共享总预算（出厂1GiB）及滚动保留已由[配置](configuration/technical.md#日志共享保留预算)和两个原数据库所有者实现；事务提交、物理回收与维护延期分别报告。按时间保留期限及导出未在现行端口开放，仍列[待明确事项](../archive/unresolved-module-contracts.md#日志与查询数据)，不把已实现的容量保留重复列为缺口。
- 回答时「记住」的产品参数、正式安装升级签名和双架构发行沿[路线图](../roadmap.md)，MCP暂缓。

具体历史修复及验收边界保留于下文和原证据，不把已完成项重新列作待开发。

## 原操作结果与通知

**R18回执与R19背压撤权旁路已修，R20有限目标真实验收PASS；不覆盖下文未实现能力。** R18的五个OpenClaw会话均已结束，但普通文件Direct拒绝回执未被消费端观察到，见[原验收证据](../evidence/maintenance-rules-integration.md#原聊天r18真实验收)。聚合拒收数不能证明某个具体丢失点。

- 内核驱动负责真实裁决、原生回执和驱动端队列；[驱动原操作](../drivers/linux/operations.md)维护原对象与等待生命周期。
- [驱动适配](driver-adapter/technical.md)负责唯一接收泵、原生答复关联及上报接纳；不能为等待业务消费者而阻塞同泵的答复接收。
- [主模块](main/technical.md)负责在原有有界调度内保管正常／拒接纳／取消结果，并可靠交给现有消息消费者；Direct完成不能作为可丢遥测。普通日志与实际完成分别处理。
- 中间层负责保存真实业务事实与向客户端发送。R19已证明普通输出Full关闭整个Peer会撤销问讯可用性，并使后续原Ask规则走NoInteraction Allow，旧策略不成立，见[真实反例](../evidence/backend-gap-normalization.md#r19真实失败与修正方向)。新[中间层接入](../middleware/smoke/technical.md)把聊天独立入同一SQL，按原事件索引单条读取；控制结果保留原有界可靠路径，普通聊天背压不能撤问讯身份。实际EOF／授权撤销与内容慢读分开，真正无前端的原用户语义不改。Ask的SQL由原tracker接管，客户端输出／EOF不取消它；接纳、提交、socket写出、前端接收分别认定，不在底层新增聊天库或事件缓存。R20已用同一未重建回答身份在主工作流负载后取得真实Ask及无人答复Timeout／Deny，并核原回执和SQL；结论与保留的辅助来源中断见[本轮验收](../evidence/control-chat-delivery.md#原聊天r20真实验收)。

## 原生失败与反馈事务

**实现已修正。** 原生操作实际负错误码通过原ExecutionOutcome::Failed保留；它与真正通道失效分开。原操作结果定义归[公共原操作契约](../drivers/linux/operations.md)，解码归适配，Main只转交，中间层只保存与展示。实际通道失效仍由驱动会话的真实失败供给，不从单次操作失败推定。

[Flow](flow/technical.md)的prepare在首await前由私有准备guard保管精确条目；取消、错误和停止均释放自身条目。发布后的正式监护交接与stop封口共用原feedback短锁，防止跨runtime在stop回执后注册新任务。复用原任务跟踪，无外部等待future长期持token，不另建反馈Owner或取消缓存。这属于Flow一次事务接纳／移交／释放，不归凭证、规则或Main。

采用项目锁定的[TaskTracker 0.7.18](https://docs.rs/tokio-util/0.7.18/tokio_util/task/task_tracker/struct.TaskTracker.html#method.close)及[Tokio优雅退出机制](https://tokio.rs/tokio/topics/shutdown)：close不阻止新任务加入，模块自己的接纳封口必须先于最终等待；不以调用close冒充实际退役。

## 记住规则与系统规则展示

**手工管理已实现；回答时记住的业务链尚未实现。** 普通／Remembered规则已有同文件CRUD、版本检查及发布，见[唯一规则模型](rules/technical.md#81-单条与组管理的唯一数据结构)。底层不从一次Answer猜测条件或扩大范围；用户选择、草稿预览与范围确认属于中间层／前端，确认后的完整规则复用底层Edit。原操作答案与持久规则保存是两个真实结果。

**驱动直决计数已交付。** 修正后的真实 OpenClaw IPv4 验收通过。按原进程实例和规则版本计入静默允许与有日志拒绝，无命中默认放行不计入。两个 AS 实例实际有序退出，模块实际卸载。首轮失败与第二轮完整证据分别保留。见[进程树统计](process-tree/technical.md#8-命中归集修复和资源)和[联调证据](../evidence/driver-direct-hit-counts.md)。近期问询计数和「记住」联动仍列[路线图](../roadmap.md#进程树命中计数与重复问询记住选项)。

文件自保保留配置快照、驱动实际状态和日志命中这三项现有来源。单独枚举固定自保规则暂无独立业务需求，从本次交付项移除。

## 精确因果来源

R12的已证前半与当前生产实现分开认定：原ToolCall对象与assistant内容中的对象引用相同，沿原req/work提交、worker syscall和AS原scope连续到EventKey。2026-11-14在原封存字节及新分析副本独立复算PASS（1 binding、0 error）。当前工具事实接口和原native_tool_executions表已能保存这些工具字段，不建第二份工具库/Owner；独有的原生请求阶段原件归现业务日志native_request_handoffs表；公开共享格式Submission及实际CAPI原交接已接通，但只证明现场持有消息对象和CAPI事实，不证明当前工具执行；生产工具执行可信发布者仍未实现；独立Main工具消息和直写writer均已移除；`bind_tool_execution`仅测试调用，正式服务已从原完成消费者调用执行反馈，通用覆盖及到原 ChatPartRef 的绑定仍未完成验收。剩余是生产采集与接线，不是重复实现原scope或再从时间/名称猜关系。[唯一复核记录](../evidence/runtime-correlation-research.md#r12-同次-toolcall-native-eventkey-研究的当前复核)

R12研究采集器的固定UV_OPEN/PC/CB/LR和对象偏移不能照搬生产。当前准确ELF的公开postmortem参数已在原封存字节上复核resource／AsyncContextFrame读取；这只证明运行时对象定位。toolCall／assistantMessage是应用存入ALS的值，不是V8提供的工具语义；不能将识别这些承载字段称为跨Agent通用能力。实际getter的存在也不等于已实现自动提取或正式采集。缺失元数据或语义如实记录，不注入、不加SDK、不按Agent私有布局补绑。[本轮边界复核](../evidence/runtime-correlation-research.md#真实工具桥生产接入复核2026-11-14)

R47 从原版 OpenClaw 的单次回调核实了工具和响应对象到原 EventKey。R63 证伪了工具侧诊断组合作为模型交换的唯一身份。R64 的 Chat Completions 原回调未观察到逐请求 HTTP 交接。R66 用本地 Responses HTTPS 夹具和原版 OpenClaw 验证了同路径、同工具 ID 的两次测试调用及一次同期心跳调用，三条回调均未取得原 HTTP 请求身份。Anthropic Messages 已做冻结源码核查，原版单回调实测为 NOT_RUN。[R47 证据](../evidence/runtime-correlation-research.md#r47-冻结原版单回调可行性通过) · [R66 证据](../evidence/runtime-correlation-research.md#r66-原版-openclaw-responses-http-对照)

原生段沿正式RequestHandoff的Submission及实际EventBound／ChildCreated交接供给；`ToolExecutionFacts`仅保留为查询返回，不另设Main消息或直接库writer。瞬时上下文在有一致性依据的回调中一次采全，消费后释放。中间层按实际发行者和作用域核实响应引用，再绑定既有 ChatPartRef。原生ToolCall→EventKey供给不以HTTP对象或模型交换身份为接纳条件；不要求Agent接入共享SDK。[单回调采集约束](../evidence/runtime-correlation-research.md#单一回调点的采集约束与流程) · [事实交接](../contracts/flow.md#模型响应与工具的事实交接)

补核冻结版模型流、工具 hooks 和外部 harness 事件，所查公开字段没有补齐逐请求 HTTP 引用。运行、会话、工具 ID 仍只保留各自来源。[模型与工具事件补核](../evidence/runtime-correlation-research.md#模型与工具事件的交接补核)

## 聊天公开可读性

**R23 已完成 AS 实际采集、发布、入库及已执行查询的有限保真验收。** 原字节、Text／Json、ID 和元数据按实际值与类型传递，实际未取得仍为 Missing／None。职责和接口见[Flow 保真](flow/technical.md#原数据保真)，已执行范围见[验收记录](../evidence/local-fidelity-integration.md#原openclaw边界)。

R23 的 UI 原文样例在 OpenClaw 侧改写后才进入 AS，该样例仍为 FAIL，缺原锚点的查询为 NOT_RUN。R22 文件 Gate 有限 PASS、聊天公开字段完整性 FAIL 和[隔离反例](../evidence/flow-redaction-readability.md)继续保留。未来业务侧脱敏只列入[路线图](../roadmap.md)。

## 底层安全日志

当前写关闭来源已在本轮Native/Main候选中替换为file_release，日志模块只消费同一队列；配置、公共记录及前端同步。实际记录、目录改名／卸载、三类保护回归及冷恢复退出见[替换联调](../evidence/write-close-native-replacement-20261115.md)。该候选未重新跑原版OpenClaw，不把下列历史R65当新来源已通过的证明。

正式回环 WS 的 ManageService 只读分页、原规则日志对账和 ReadOnly 拒绝已通过原版 OpenClaw R60。R65 复用同一分页验证了写关闭审计的原通知、WS 和 SQLite 一致性。[安全日志契约](security-log/technical.md) · [R60 验收证据](../evidence/middleware-websocket.md#原版-openclaw-r60) · [R65 验收证据](../evidence/file-wait-extension.md#r65-原版-openclaw)

## 操作与容器覆盖

**文件问询的当前内核可实现范围已通过 R29 真实 OpenClaw 验收。** TruncatePath、TruncateOpenFile 与 MapSharedWritable 复用同一原 File gate。R29 验证路径截断允许、FD 截断拒绝和首次共享可写映射允许的原对象、实际效果与日志。隔离 guest 另验证 `O_TRUNC`，真实 OpenClaw 未执行该场景。原五项和历史验证边界保留在[R22 证据](../evidence/file-gate-integration.md#r22原openclaw有限真实验收)，本轮范围见[R29 证据](../evidence/file-wait-extension.md#真实-openclaw)。

具名创建的已实现范围和真实交接见[驱动契约](../drivers/linux/hooks.md#具名普通文件创建的同次请求交接)。

**原缺口第 1 项「容器动态接入与精确实例绑定」已通过真实验收。** 默认 rootful Docker 私有网络域覆盖 AS 和目标的两种启动顺序、精确容器事件与规则、私有 IPv4 TLS、同 ID 重启、同名重建以及旧凭证隔离。R33／R34 保留实际模型证据，最终代码候选 `92e47a2d` 的 R35／R36 保留新旧实例、旧凭证拒绝和同一 AS 正常运行的原件。见[真实验收证据](../evidence/container-dynamic-reconnect.md#r36-同一-as-正常运行对照)和[容器接入边界](flow/container-egress-design.md)。

正式 WS 的正向 Docker 归属已通过原版 OpenClaw R61。原 ContainerInstance、ProcessKey、正反容器筛选和规则命中与独立 Docker 及持久原件相符。[R61 证据](../evidence/middleware-websocket.md#原版-openclaw-r61-正向-docker-归属)

宿主网络命名空间的 IPv6 受控 TLS 已通过隔离验证。当前 GLM 上游无法通过 IPv6 访问，真实 GLM／OpenClaw IPv6 按用户确认只做理论验收，真实运行保留 NOT_RUN。R30／R31 的重连与信任结果仍保留，强断重连未进入自动接入。见[双栈实测](../evidence/ipv6-tls-ingress.md#r26-真实-openclaw-验收)和[受控出口](flow/tls-egress.md)。

## 控制与部署边界

**授权停止闭环已完成，R21有限真实验收PASS。** Main的RequestStop经ManageService及原预算把一次请求交真实宿主，Accepted只证明接纳；原宿主唯一调用既有shutdown，外部父进程实际wait才证明退出。未绑定／端口不可用不回Accepted，不让消息任务等待包含自身的drain。唯一字段和runtime写出cap见[Main消息契约](main/messages/technical.md)。两个真实实例的一次请求、实际写出及外部wait分别见[停止验收](../evidence/main-stop-integration.md#验收边界)。配置／规则重载仍归原模块，主模块只组合结果。

DKMS 入口按目标发布号选择构建树，并复用目标 Kbuild 的 BTF 生成。当前 aarch64 内核的新构建产物已通过 R25 真实 OpenClaw 集成验收，未启动目标的正向构建仍需目标 `vmlinux`。正式安装、升级、签名加载及双架构分发归 [AS 整体分发](../roadmap.md#as-整体分发)，在 AS 整体开发完成后补齐。当前驱动实测见 [R25 证据](../evidence/target-kernel-build.md#r25-真实-openclaw-集成验收)。

## 封装规则与验收

现已核实的运行时缺口均有现存模块归属。Main的global只适用于无专属业务所有者、通过明确参数调用的小型通用函数；不承载可变全局状态、规则／路径缓存、跨模块查找、协议解析或部署执行。独立生命周期、持久状态或专用I/O说明它应属于对应模块，不能为了文件少而塞入global。

按模块先修产品／架构／技术契约和唯一数据结构，再重构生产者至消费及销毁链，清理旧路径。基础脚本覆盖明确模拟范围，冻结候选交原OpenClaw聊天验收；反馈回到原模块整体调整。当前目标内的缺口未闭合不报交付；明确排除的MCP、Windows及首版mTLS／ECH／HTTP3等不自动扩成当前任务。
