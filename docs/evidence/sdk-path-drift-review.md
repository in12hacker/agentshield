# 旁路采集偏向共享 SDK 的复盘

## 调查范围与结论

2026-11-12 按用户要求全文搜索当前工作树文档，包含被 Git 忽略的 `.codex/HANDOFF.md` 和交接归档。共 156 份 Markdown／规则文档；初次组合搜索 SDK、preload、Inspector、预加载、注入及采集门槛，命中 41 个文件、104 行。另检索共享运行时、执行库、标准上下文、traceparent、OTEP、零源码等相近说法。构建产物、`.runtime`、`.git` 对象和 node_modules 不作为项目文档扫描。

结果是需求与层级归纳发生错误，并有过时文字继续影响当前任务。用户没有授权恢复共享SDK产品接入。不能把原因归给用户表达，也不能只因历史文件在 archive 中就认为当前摘要不会再误用它。

原搜索清单及修正前两个关键段落位于 `/tmp/as-sdk-document-audit-20261112/{inventory.json,all-matches.txt,related-context-matches.txt,drift-before.json}`。初次统计绑定当时文档，不代表后续修改后的行数。

## 可核查的错误传播链

1. [旧预加载调研](../archive/runtime-preload-research.md)保存了2026-11-09“采用APM启动插桩”的提案，包含目标预加载与SDK传播机制。当时提案后来已经退出产品路线；归档首段此前只写“不是已交付”，没有醒目标明“已退役、不得作为当前指令”，仍残留“采用”“接入所需工作”等现在时。本地旧交接 `handoff-before-audit-context-design.md`（未随Git发布）也保留了目标启动 `--import` 的推荐。本轮两处都补了退役提示，原历史正文保留。
2. 提交 `73c1fd5dad396df3ded3b623841b03d1908af77e` 于2026-11-11明确记录退役注入及逐Agent适配。本地较早交接归档 `handoff-before-middleware-database.md`（未随Git发布）也写了“未知Agent且无需目标改动”“原预加载方案不再是产品路线”。因此不能把旧实验或旧提案当作新的授权。
3. 后续未提交的[运行时调研](runtime-correlation-research.md#跨语言来源与-http-请求交接核查)又出现“跨语言运行时的真实实现也需要发布方传递上下文”，将 OTel SDK／OTEP 某一传播机制扩大成全部跨语言采集的必要条件。同节又将“支持发布标准上下文的共享运行时或执行库”列为后续方向。两个原句均不在当前 HEAD `cfb1b171f` 的同文件中，属于后续工作稿新增的错误归纳。
4. `ToolExecutionFacts` 本来只承载已取得的工具语义，却被[驱动适配说明](../backend/driver-adapter/technical.md#原生请求持续采集与工具边界)与当前交接概括为所有原始来源的入口。于是原始采集被要求先提供ToolCall和完整应用对象布局，调查又回到SDK／应用内部对象。
5. 本聊天随后两次把共享SDK交接写入调查方向和子代理任务，没有先与纯旁路约束对照。这是执行偏移。另一次将“单回调取得当次原值”扩大成“最初采集点必须已有最终EventKey”的解释，也排除了尚可调查的机械身份交接。

## 必须分清的同名内容

- **OpenTelemetry eBPF Profiler**：本轮参考的纯进程外采样实现。同回调可采线程、栈及部分执行事实；V8／Python用户态符号化另有稍后读取目标内存的步骤，不能把该结果冒充原回调快照。[采样入口](https://github.com/open-telemetry/opentelemetry-ebpf-profiler/blob/21b48e5ff7cfa0efa0a6ed731b804037b4415fb5/support/ebpf/generic_probe.ebpf.c)
- **OpenTelemetry Instrumentation／SDK／Context传播**：部分实现要求预加载、Inspector或应用发布上下文。它们可作历史机制证据，不能替代当前Profiler旁路方向。SDK传播需要参与者，只证明该机制的前提，不证明原始系统观察也需要它。
- **AS文档中的 Native SDK**：指 AS 自己的驱动适配与guest诊断，未部署到目标Agent。[诊断说明](../../scripts/native-smoke/README.md)已补清含义；历史镜像、测试标记和原件名称保留。
- **模型厂商 SDK 源码参考**：Flow用其解释协议增量与终态，原[参考边界](../backend/flow/references.md#官方llm-sdk只复用增量和终态语义)已明确不引入模型客户端、重试或目标SDK接入。该类引用没有导致新接入要求，不应机械删除。

## 已纠正的当前边界

原始采集归底层；以尽早取得、与目标Agent私有架构无关的系统事实为主。中间层按明确规则组合现象、时间窗口、线程实例、实际请求／句柄／同步对象及操作顺序。调用栈保留为现场证据，不以固定栈形状或私有函数建立通用绑定。没有ToolCall不阻止原事实采集；原 `ToolExecutionFacts` 仍保留自己的准确含义，不用空工具ID伪造成功。

一次采全针对当次瞬时值；之后沿原对象有效寿命机械交接、分配EventKey是允许的。不得延迟回读目标或跨时刻补当次字段。明确模式命中与实际对象交接证明分别保留条件和证据。R37只证伪其实际执行的时间／路径候选方法，不是所有操作系统模式都无效的证明。

本轮同时修正 `AGENTS.md`、当前 `HANDOFF`、架构、驱动适配、Flow事实边界、审计模式及缺口说明。旧提案增加退役提示，错误的一般性结论和共享SDK后续方向从活动说明撤回。未建立第二套规则文件、评分框架、Agent模板、SDK入口或假采集器。

后续派发任务先对照当前交接中的用户约束；每个来源结论同时写清“实际取得什么、尚未取得什么、是否需要改目标、是否只适用于该实验”。研究不能仅凭项目名称从Profiler跳到Instrumentation；接口不适配原始事实时先核归属，不反过来修改产品需求以凑现有DTO。

本复盘完成的是文档与流程修正。通用持续采集和正式多条件业务匹配尚未交付，不能把文档审查或历史原件复算当成代码验收。
