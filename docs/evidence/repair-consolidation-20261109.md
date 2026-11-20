# 2026-11-06至09日修复收敛审计

本页为原批次证据；已退役的 `scripted_service.rs` 链接固定到保留它的历史提交，当前服务入口见 [as-service](../../crates/middleware/as-middleware/src/bin/as-service.rs)。

历史限定：原审计及 2026-11-13 复查保留各自的源码、计划和结果；正文中的待修、执行顺序、去密及“当前”不能替代后续已收敛的现行契约或成为新门槛。先完整运行正常真实业务，再按其实际结果迭代 AS 自身；历史局部通过不覆盖新代码。

范围仅为独立重构分支，从7e021f131起点至已提交333eb2f。未提交的控制器网络helper及其活动文档已撤回，原补丁与775a构建记录保留为暂停方案，不当作已交付能力。本审服务当前重构决策，不执行新的模型／内核场景，不以补测试或提交数量计进度。

后续现行分支复核见[2026-11-13 复查](#2026-11-13-复查)。以上范围仅指原报告。

## 结论

需要按功能整体重构，而不是撤销所有已验证修复。最主要的三个问题是：维护事件没有统一在驱动规则入口分流；业务事实消费仍依赖前端发送；正常完成、拒接纳补偿和反馈准备没有完整统一各自的在途生命周期。原生错误折叠又削弱了定位能力。

已修掉的采集预算、缓存误用及原Ask完成丢弃有真实证据，应保留其正确目标。问题在于后续层新增独立SQL职责时，仍继承了旧“直接转发给socket”的等待关系；只修局部满队列会把丢弃变为等待，不能独自消除等待环。

## 这几天的主要演化

- **11月6日，7e021f131至8bea67f42**：独立起点、三类规则工厂、确定的顺序／容器范围、鉴权管理、原操作保管和串行人工问询。规则编号、原期限、真实身份及实际结果是基线，不能因简化删除。
- **11月7日，9b144d992／14f88e8e2／4216177e5／7755bc503**：配置唯一定位与MainContext、Native及自保、进程树／事实补全、取消与实际退出收敛；d79b4d3b9等接入原actor／parent终止。此阶段的真实资源依赖应保留，但要重新审查维护流量是否进入自己的保护管线。
- **11月8日，2f0b6b4e2／5dce23f23至3d1ba9dd3**：受控TLS／phantom、脚本中间层和多次输出背压修复。eedfddc8d修writer未获调度的误判；1374bb6d7在旧订阅模式先预留输出；3d1ba9dd3让Peer通知等待输出并加sending交接。它们各自解决了当时的真实局部问题，也留下前端进度决定上游消费的关系。
- **11月9日，375836399／a1fca61ec／1731d9329**：固定文本反馈；为解决observer_capacity暂时预约整交换额度；随后按用户边界把聊天／关系持久化归中间层，删除底层ring、snapshot、盲拒绝缓存和整交换预约，迁移JSON及目录。这是必要收敛，不应恢复旧预算／回放补丁。
- **11月9日，333eb2f**：删除AS自身自保写的允许通知源，并让六个正常完成出口等待原有队列接纳。R17真实验证两项有效，但不是整条调度链已经收敛：补偿出口与前端依赖仍需整体处理。

## 一、维护流量：统一规则入口，停止按hook追加例外

**已发生。** R16大量AS自身SQLite WAL写通过as-self-protection/backend-write制造普通通知及日志，主通知／日志大量拒收。333删除这一允许观察源后，R17实际自身写类上行与旧允许日志均0，外部写仍真实EPERM。R17又以原EventKey证明4个AS消息线程自身TCP443 connect进入ToUser，约30秒unbound_wait超时；另16个OC文件打开同类超时。具体代际错误的完整用户态触发链尚未证明。

**根问题。** 保护对象与维护主体的入口规则没有一起规划。Flow的BPF排再次导流，不代表普通HIPS池已排自身维护；加文件静默放行，再单独给connect/send加不同hook特判，会继续分裂策略。

**重构落点。** 优先规则模块的同一PolicyDocument→DriverPolicyImage，按真实源executable前缀在进程／文件／网络三个Driver池各建静默Allow，使用已有Prefix，不新增DSL、第二份规则缓存或按PID／名称临时豁免。让编号和来源版本完整包含这些规则，真正保存／下发／读回；不能暗插镜像却保持旧document/digest。

现有能力已核：Process Execute、File Open、Network Connect／Send；Driver只支持Exact／Prefix，不支持Glob／Regex。Process Create只是观察，不能声称已用规则保护。File组1为配置自保保留，维护规则最早2；Process／Network最早1；有占用时在原文档保序合并，不能偷偷抢先。真实根借ConfigPaths.self_protection_root一次派生结果。

两个语义必须保持清楚：源executable路径在根内，与执行目标在根内是两条不同规则；路径字节前缀也不等于已鉴权controller或同一mount／inode对象证明。现有String规则不能无损表示非UTF-8根，此为明确接口缺口，不用lossy转换。外部修改自保仍先经过实际对象guard，普通Allow不能绕过。

**已清理暂停项。** 未提交的controller_network_maintenance helper、两个hook调用及其活动文档已从工作区撤回，775a5949模块只作对照，不交付。原差异完整保存于 /tmp/as-paused-775a-before-rule-consolidation-20261109.patch；没有撤销333已证明的自身写静默及真实完成保管。应在统一规则／维护主体设计中明确哪些是可编辑策略、哪些是控制通道不可重入约束，不能把路径Allow伪称身份鉴权，也不能让可删除的普通规则无声替代核心安全保证。

依据：[规则编译](../../crates/backend/as-policy-engine/src/compile.rs)、[Driver投影](../../crates/backend/as-policy-engine/src/driver_image.rs)、[Core](../../drivers/linux/as_core.c)、[配置唯一根](../backend/configuration/technical.md)。细证据：/tmp/as-maintenance-rules-feasibility.md。

## 二、消费边界：真相与前端输出不能共用完成屏障

**源码已证明的两处机制：**

1. Media仅在sending=None取下一notice。Completed和Chat均join(SQL,Peer)，所以SQL已提交但Peer输出等待时，它也不再消费下一条事实。
2. [transport](../../crates/middleware/as-middleware/src/transport.rs)把真实Ask入库future放进与socket回复相同的pending；pending首次poll受control.reserve输出许可控制。输出满时SQL可能尚未开始，EOF会丢未poll工作。已开始SQL仍由数据库tracker保管，不能混同两种阶段。

Peer的events容量16与逐帧socket写出有真实边界。对端不读时，依赖可以沿Peer→Media→Main notices→原完成send等待传到串行Main，后续尚未Bound操作可能耗尽原窗口。**这是源码可达关系，不是R17已唯一证明的现场队列因果。** Fast reader和控制写并非都在Messages，不能编造那个直接闭环。

**应作为一项完整接入重构。** SQL原事实的开始、提交和继续消费独立于可选前端进度；UI仍不等SQL才显示Ask。前端复用现有有界输出并明确接纳／交付／缺口／关闭状态，不能拿bool或队列接纳冒称送达。消息线程自己的同步维护I/O也不得进入自身ToUser。

不通过扩大队列、恢复try_send丢原完成、加转发worker／通用bus／RAM历史或延长deadline解决。保留原控制／Ask与来源事件的不同输出职责；不是看到两条队列就删除一条。

依据：[Main完成](../../crates/backend/as-main/src/dispatch.rs)、Media、[Peer](../../crates/middleware/as-middleware/src/transport.rs)。细证据：/tmp/as-recent-middleware-audit.md、/tmp/as-r17-backpressure-analysis.md、/tmp/as-r17-own-network-loop-analysis.md。

## 三、原操作与反馈：统一所有出口的生命周期

### 原操作正常完成与补偿仍有分叉

333只收拢正常ctx的六个queue_completion出口。当前[native_events](../../crates/backend/as-main/src/native_events.rs)拒接纳／reply失联／旧规则代际缺失的三个补偿出口，仍用driver_notice→try_send；满时补偿完成仍可丢。adapter关键callback接纳失败会实际cancel原请求，但只有聚合拒收记录。

应把“原Original的接纳、拒接纳、取消、实际结果收集、可靠交给消费方”收进同一有界生命周期，而非再逐处加await。补偿spawn目前也不受fast pending同一预算约束，不能假称改三个调用就完成工程收口。必须保持实际结果优先，不把发cancel当已取消、不把发送停止当已退出。

正常完成的原ctx保管、已有bounded pending并发及先drain processing后停止messages应保留；回滚成丢弃会掩盖问题。

### 反馈准备也有同类半完成区间

Flow prepare在pending map插入后，先await输出Prepared，之后才启动deadline／撤权监护。若调用future在该await中被取消，当前缺少统一Drop／回滚保管，phase0条目可留到Flow.stop。此为静态生命周期缺口，尚无证据表明它造成R17失败；也不能因此恢复盲缓存所有Denied。

应在Flow单次写事务的明确接纳／提交／终态范围内统一保管，首个await前就确定取消和期限责任；不造全局Owner服务，不重放已经写出的请求。

依据：[Native补偿](../../crates/backend/as-main/src/native_events.rs)、[适配接纳](../../crates/backend/as-driver-adapter/src/lib.rs)、[反馈](../../crates/backend/as-flow/src/feedback.rs)。

## 四、错误没有完整沿原链传递

wire.result读到原native errno后，经通用ModuleError折叠；例如-110在上层成为Unavailable。operation只保留折叠结果。f68c3f72f增加内核timeout stage日志，使R17能离线追踪，但没有补全原错误数据链。

应在所属原操作结果契约保留实际native错误及已取得阶段／原EventKey，公开概括错误另作展示。没有采集到的阶段仍缺失，不从errno、PID或时间反推。该改动用于准确定位，不把增加日志当执行问题已解决，也不一项项为不同错误名加特殊恢复。

依据：[wire结果解码](../../crates/backend/as-driver-adapter/src/wire.rs)、[原操作结果](../../crates/backend/as-driver-adapter/src/operation.rs)。

## 已验证且应保留的收敛

- 单物理SQLite owner、结构化原协议／parts／IDs／关系，完整u64和None／空值语义、真实commit回执、取消后仍观察已开始写入、真实close。这些没有证据支持推翻表结构或增加连接池。
- 唯一Flow消费者、必要协议解析与去密、原始字节和结构化投影分开；同进程typed Arc，不重复JSON计费。不要恢复订阅回放或底层聊天库。
- 原进程实例／出生标识、进程树初始与最新数据、缺失才有界补取；配置唯一定位及只读MainContext；鉴权／撤权和自保真实对象／exec生命周期；原操作与真实终止／退出结果。
- 控制与来源／普通日志不同队列、必要协议半帧和在途ctx。这些不是“缓存腐败”。进程树、配置、规则及有界证书缓存是有职责的真实状态；不能宣称全项目无缓存。

## 已退役，不得再当当前方案

1731已删除Flow ring／snapshot／exchange_context、blind record_completion、ID-only外部prepare和全交换最坏倍数预约。1374的旧forward_chat许可链、226的订阅字节补丁以及a1的整交换预约，只保留历史问题与旧通过范围，不回迁。旧R15通过不覆盖R16／R17；测试数和commit数都不表示功能进度。

## 建议执行顺序与验收口径

1. 先完成一份三池维护规则的真实数据／优先级／来源设计，清理尚未交付的hook特判方案；不先加第N个豁免。
2. 将Media和Ask持久化的两个前端进度依赖一起收拢；保留可靠原完成，明确消费／持久化／展示各自边界。
3. 完整统一Original的正常／补偿生命周期，再收Flow反馈单次事务的取消区间；补真实错误数据链，删过时包装与不再需要的状态。
4. 每项按所属模块一次接入，适用本地检查后由原OpenClaw聊天按真实业务验收。规则不按测试PID／探针名缩窄，不新增“全部统计必须0”等门槛。原R16／R17失败原件保留，未证明的因果仍标未证明。

本次审计未新增产品能力，未运行CI或实例；已有未提交修改仅记录其状态，不冒称重构已完成。

## 2026-11-13 复查

按 UTC 10 月 30 日至 11 月 13 日枚举当前分支的 132 个第一父链提交。11 月 6 日的 `7e021f131` 是独立重构边界。更早的主线合并只用于追溯，现行生产调用链的复核以该边界之后仍在使用的实现为准。沿配置采用、驱动队列、Main 取消与完成、进程树补全、Flow 原反馈及中间层持久化复核先前发现。已确认的原事件共享、Flow 准备保护和原生错误链继续沿原所有者工作。此次是风险导向复查。

- **实际配置采用与回执不一致。** `1b7170fae` 之后，保存待重启的 Group 端口会改变后续规则热替换使用的配置镜像。内核只在初始化改变实际观察端口，因此镜像摘要能反映未实际采用的设置。同值重载又会因本次差异为空而丢失待重启状态。配置所有者统一固定初始化范围及比较基线，Main 复用保存／重载后的采用流程，内核拒绝热替换变更 Group 端口。已发布后发生的凭证采用失败沿真实配置回执返回。
- **适配器尾部拒收此前静默。** 旧 Group 入口丢弃 `try_send` 的结果，只能依赖之后收到的序号识别中间缺口。适配器在原发送端计数满队列及消费者关闭的实际拒收，首次和退役时报告。中间层接到 EOF 时显式提交已经取得的未定界原件。内核末尾未交接的损失仍缺独立计数。观测拒收与原执法通路分别处理。
- **活动详情旧补丁会掩盖原证据问题。** 最近一条原请求消息缺证据时曾顺延，真实请求和响应又因独有字段不同被前端等值比较过滤。修订由业务层定位原对象并报告缺口。相同原件分别验证了回放、展示和严格解码。见[活动详情复核](middleware-websocket.md#活动详情原话的离线复核)。

仍有一条源码可达的等待关系。现行普通控制回执在提交 SQLite 后仍等待 Peer 队列，`Media::run` 在等待期间不会消费后续控制通知。独立聊天消费及底层安全日志继续推进。此前真实验收未证明这一分支导致现场超时，且现行契约明确要求必要控制结果保留原有界接纳。需要用正式慢读对照同时核实原操作、后续业务提交和控制送达，才能决定分离何处。

在 `1ba1d8871` 的隔离副本复现了保存后的驱动镜像摘要变化。同一回归在当前候选通过。209 项 Rust 工作区测试、严格 Clippy、格式和改动入口 LSP 通过。最终 aarch64 `7.0.0-1019-nvidia` 隔离客体核实保存新端口、同值重载仍待初始化、规则重载继续使用原驱动配置摘要。两条并发 A2A 委派均完成，正式 WS 分页、SQLite 和原报文字节逐项一致。客体退出 0，模块实际卸载。适配器满队列和关闭的拒收由定向回归覆盖。

首轮并发运行保留原版 kagent 会话库的 SQLite 517 错误、夹具屏障失败及未完成的关联。复验只在隔离夹具的原会话库连接串采用其锁定 SQLite 驱动支持的 `_txlock=immediate`。两次 AS 服务和内核模块的字节一致。[SQLite 隔离](https://www.sqlite.org/isolation.html) · [锁定驱动实现](https://raw.githubusercontent.com/glebarez/go-sqlite/v1.21.2/sqlite.go)

原件和候选位于 `/tmp/as-repair-audit-20261113.oh5RGrW3`。见 `baseline-repro.log`、`workspace-test-final.log`、`artifacts.sha256`、`guest-original/report.json`、`guest-original-r2/report.json` 和 `a2a-ws-sqlite-verified-r2.json`。本轮没有新的原版 OpenClaw 验收。
