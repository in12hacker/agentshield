# 工具执行来源与运行时关联证据

## 当前约束与结论

用户现已明确正式方案采用纯进程外观察，面向不同实现语言的 Agent，用户维护通用行为规则。逐 Agent 的私有函数适配和进程内观察器已退出正式计划。此前 R40／R41 的 Inspector 接入仅获准用于隔离验证，原结论及证据继续保留。此前Node预加载调研及机制原型已[归档](../archive/runtime-preload-research.md)。

当前主线是尽早采集与目标私有代码无关的系统事实，由中间层按明确的操作系统行为模式组合。时间窗口、线程实例、实际请求／句柄／同步对象及提交、等待、唤醒、执行、完成的真实信息均可作为模式条件。调用栈只作现场证据，不依赖固定栈形状或应用架构。原始采集不以ToolCall、共享SDK、完整V8对象布局或同点已有EventKey为前提；后续机械身份交接与延迟回读目标内存必须区分。唯一业务边界见[通用模式约定](../middleware/audit/technical.md#通用操作系统模式)。

R47 已在冻结原版 OpenClaw 上证明单回调纯旁路采集可行。两次真实调用分别在一个回调内复制原工具／响应对象及原生文件请求身份，沿真实原生交接关联到各自 EventKey，允许、拒绝和同期独立操作通过。原件与适用范围见下文 R47。跨语言持续采集仍未实现；原 HTTP 对象身份未取得，不是独立发布原生事实的前提。R43／R44 的局部证据与失败结论保持。

用户所指的“同步等待”按调用方等待本次执行结果理解。原版 OpenClaw 的 `prepareToolCallEntry` 同时保存原 `assistantMessage` 和 `toolCall`，`prepareToolCallExecution` 在该上下文内调用工具，`emitToolResultMessage` 用原 `finalized.toolCall.id` 回填结果。调用关系有具体代码依据。继续取证的对象是这份结果归属及其到原生请求的交接，不以工作线程被挂起推定整个 Agent 只有一个在途工具。原文件身份及真实并发结果见下文 R37，外部引用链见 R43。

该调查方向不限于 Node。[Rust 的 await 定义](https://doc.rust-lang.org/reference/expressions/await-expr.html)明确保存待决 Future 状态并在再次 poll 时恢复，[Python Future 定义](https://docs.python.org/3/library/asyncio-task.html#awaitables)明确协程等待原 Future 完成。这些语言机制确认结果依赖有具体承载，未规定通用的 ToolCall 字段或进程外读取 ABI。运行时等待对象、应用工具身份与模型协议记录的交接仍分别取证，不把一种运行时的对象布局当作跨语言结构。

## 真实工具桥生产接入复核（2026-11-14）

本轮目标是正式真实工具执行→既有ChatPartRef→原固定反馈写出，不再将来源生产整体称为延期。当前工具桥未完成；没有新增Agent注入、SDK、私有函数探点、版本表或推定绑定。继续复用NativeRequestHandoff／ToolExecutionFacts／NativeToolContext与既有SQL、Activity和Flow反馈，不为尚未取得的事实新增同义DTO。

R12同次原件仍证明原ToolCall和assistant.content成员为同一对象，沿原req／work、线程实例和AS原scope到EventKey。原件见`/tmp/as-gap2-r12-same-run-proof/HANDOFF.md`；它使用的固定PC／CB／LR及承载字段解释不具备通用生产结论。已存回调字节上的元数据复算`/tmp/as-v8-helper-research-20261112/replay_metadata.json`只证明6条resource／Frame定位，不证明完整Map解释、语义生产或GLM来源。

冻结`r66-live/frozen/tool-execution-context-BwYF5HLu.mjs`及`agent-core-jCWPqNtz.mjs`明确由应用代码构造toolCall／assistantMessage并存入ALS。这不是V8公共工具协议；Node的[AsyncLocalStorage契约](https://nodejs.org/api/async_context.html#asynclocalstoragerunstore-callback-args)允许应用自行选择store，公开[OpenClaw实现](https://github.com/openclaw/openclaw/blob/main/packages/agent-core/src/tool-execution-context.ts)也把它标为内部工具上下文。运行时元数据的准确性不能替代应用语义或发行者证明。R66受控发行者的组合正例仍有效，不能据此对真实GLM仅凭responseId相等自动绑定。

R95原件的静态和SQL复核：sealed Node SHA256为c8c5721679d8d1a09e9987e345d5f9a64899bb85f7a384783ba8a93e6e62348a；两个真实文件Ask的用户栈返回PC0x18e97ec／0x18e0498分别归uv__fs_work内联的uv__fs_open／worker，实际open64调用在0x18e97e8。现成metadata reader重算五探点和result88／work336均有值；已有原Open引用的work差值也为336。不能从目标Open缺失推定另走私有API或整体安装错误。实际安装命令字节未留存，静态重算不冒充原命令回执。

R95保留32081条原始阶段，而source序号和拒收计数显示存在损失；目标Open与EventBound均没有原件。Main原observe_native用try_send在暂满时丢弃已经从DriverEvent队列取出的值，是独立可达且有原计数的断点，不能进一步断言每个目标记录恰在此处丢失。当前删除该旁路，三类已取出事实改用原driver_notice等待同一消息队列接纳，暂满保管当前原值，关闭明确失败，不加队列。适配读泵及内核原子分配／排队的损失仍可能发生；EventBound未生成还是后续丢失尚未区分，不据缺失修改SP比较。

本轮独立只读复核报告`/tmp/as-r95-native-gap-review-20261114.json`。同一消息槽饱和时的真实native_events路径测试已执行通过，两个原件等待消费并在关闭接纳后排空，未把该软件测试写成guest或真实GLM验收。其他检查及最终验收状态见当前HANDOFF；固定文本真实写出仍未证明。

## 公开共享格式授权后的窄读取复核

用户已明确允许公开共享消息格式解析，不做逐Agent分支。这不授权固定内存位置、私有函数选择、Agent注入、SDK或把公开标签推定为实际网络来源。唯一读取边界见[原生请求源](../backend/driver-adapter/runtime-observation-technical.md#公开共享消息格式的读取边界)。

对R12原bank五条记录的纯离线投影复核，仅seq2取得匹配：从33个Frame live ALS值出发，只枚举store自己的data字段及其直接对象，按公开消息字段识别后，再核助手content成员与原Tool为同一对象。选择时不使用holder字段名。该条1268个逻辑读，未读的Function／Set／Undefined或accessor分别保留，不把无关对象解释失败当成全部采集失败。原件脚本与结果位于`/tmp/as-pi-format-v8-read-review-20261114/read-public-shape.py`及`public-shape-result.json`，输入bank SHA256 ed6c6528912400ea70d2b774604f5db290380f0663360fa7019849d474b9fee1。此脚本仍复用原固定映像decoder，只证明窄投影在原字节上成立，不证明自动布局或生产采集已经实现。

同目录minimal-metadata.json保留实际ELF符号和值，actual-getters.json保留16个实际具名runtime getter字节，fetched-sources.json保留公开V8／Node源码来源及哈希。大部分布局已有v8dbg；尚须自动核定的collection.table、OrderedHashMap／NameDictionary类型、JSArray.length及PropertyArray长度来自实际getter数据流，不复制默认值。未知指令／冲突如实缺失。

GLM发行作用域仍未补齐：Pi公共类型允许自定义provider／api字符串，responseId是provider-specific标识；公开格式成功解析不是该标签与Flow鉴权来源的原始交接。原生工具段可以独立接纳，HTTP绑定不足仍保留候选，不授权固定反馈。

本轮对既有原件追加只读核查：R12 seq2原Frame的另一live holder已取得traceId／spanId／traceFlags，trace ID与两次HTTP原头相同，native span与两侧HTTP span不同；seq1（未取得工具同对象正例）也持同native span。已存原日志、诊断JSON及显式字段没有这三span的parent／link路径，不能把共享trace补成共同祖先、专有工具边或发行域。R66受控发行器ordinal／字节／作用域证明保持其离线有效范围，R95真实Flow侧auth／credential／origin也保持原值；它们不能填造缺失的native跨侧交接。范围、原槽、原头行号及哈希见`/tmp/as-h-existing-evidence-20261115/report.md`和`span-check/report.md`；未运行Agent／VM或晚读目标。

## 本轮统一读取器的组件运行

版本限定：下面 R3／R4 的 15 秒／50 秒是当时测试器的附加检查，原失败判定与实际原件保留，不能据此推定网络／采集因果或移作当前准入条件。R5／R6 的组件通过也不替代 stock Agent／真实 GLM 正常业务的完整运行。

本节只记录自有公开消息格式Node工作负载，未运行Agent／GLM，不计N／H／F完成。两轮均使用当前AS服务、真实原生驱动、已鉴权Unix渠道及原MvpSuite；原件不回写通过。

- R3：三类控制及同进程同参双调用效果已执行；Node发送stopped后测试端未关stdin，wait15秒失败，AS随后强退、未确认卸载。独立无AS Node复现同一stdin问题，关原stdin后exit0。原导出不含完整在途WAL，主文件计数不代表最终持久化。原件`/tmp/as-native-n-live-component-20261115/run-r3`。
- R4：只修正测试端stdin关闭，未延长期限。File四项、Execute／Connect／Send各两项共10个实际问询／效果／回执，最终正常库questions／operation_reports／native_request_returns各10；Node exit0，AS最终STOPPED且模块卸载。服务退休先超过原50秒检查，因此整体FAIL。安装诊断表明Node元数据准备约60.2秒，两个fork子实例约57.6／59秒；ready在停机后发生，inspect实际Stopped，未安装。native_request_handoffs／投影／native_tool_executions均0，不能称生产桥正例。原件`run-r4/{summary,original-sql-counts}.json`及extracted/evidence/service.log。
- 性能定位：一次读取ELF只把完整File准备从4916降到4175毫秒；相位计时为image29、十探点重复扫至1238、UV遍历结束1343、V8结束4093毫秒。实际主耗时为重复符号扫描，非DWARF。最终同次符号索引的完整File比较4202→511毫秒，machine／10探点／531 UV／499 runtime及原gaps逐值相等。证据`/tmp/as-runtime-symbol-index-20261115/equivalence.json`，不据主机速度推定guest安装成功。
- 保真收敛：已取得PROPERTY先保存，字段名只限制后续递归，字典已读非data及未解码details保留独立kind；尚未取得的accessor值不追加读取。原R12自持字节25次容量重算与解析保留seq2同对象正例，其他四条无匹配；容量与OOM守卫通过，未增容。范围是封存字节组件，不是新生产N。证据`/tmp/as-shared-capture-20261115/root-fidelity-results.json`。

R5统一快照的原生组件运行通过：installed真实回执8667毫秒，92条handoff／11份投影35074字节；10次原操作及正常AS／Node／guest退出和模块卸载通过。同参数A/B沿不同req／work／真实worker／EventKey闭合。原件`/tmp/as-native-n-live-component-20261115/run-r5`、`canonical-native-audit.json`。这是Unix原生组件，未运行stock Agent／GLM。

**R5的8条native_tool_executions行是旧自动升级逻辑生成的记录，不认定为通用Tool执行证明。** 后续语义审核确认：公开ToolCall／AssistantMessage只定义消息值，没有“任意ALS store就是当前工具执行发布者”的契约。普通store可保留上一工具，原req／child／syscall真实交接不补这层语义；唯一候选不能升证明。原库与报告原样保留，当前代码撤自动升级及旧直写入口，保原投影、同对象候选和原生交接。R12具体OpenClaw执行run的有限原件保持其既有证明范围，不泛化为通用生产来源。

R5两个Send实际栈落uv__try_write／Node DoTryWrite，未经过已装write入口。已按真实API及libuv公开契约设计TryWrite／TryWrite2入口、无假req／callback，API返回结束本次调用，不进入uv_req哈希；排队Write／UDP仍独立保留缺口。R6使用12探点／144字节计划完成事实源验收：140条原观察，26份投影47870字节，13个公开候选，11次原问询／原返回／实际效果通过；同参A/B保持不同req／work／worker／EventKey，直接TryWrite两次都有原调用到Event的交接。实际安装回执8987毫秒，AS／Node／guest／QEMU退出0、模块卸载、无强制清理。maintenance复用已完成原工具的同一公共对象及相同参数，原候选仍完整且reads_consistent=true，但没有新dispatch；Tool记录／反馈／反馈绑定均0。该实际反例证伪了旧自动升级，当前不追加固定提示。

最终原件`/tmp/as-native-n-live-component-20261115/run-r6`，report SHA256 `039332f9d72871f816cf6e324f40a99face1254d3a78bf0a52aeb16e8ce49463`；候选`candidate-r6.json`绑定完整源码和产物。service SHA256 `2f066591beeff41f2f70aa0ff7abdfc5900e354edab70c2891a07b1a7ad4bbbf`，KO `a03da62b4ffd097ddf75884d210103a5472872a9b5f00353c0d8cccb63dbf2f4`，image `94ec76b12a4eb53962024dcb9b430c02ea1b95dc8a2d75a7abead2fdca4f3e5c`，aarch64／7.0.0-1019-nvidia。事实源组件通过不增加N／H／F：三者仍各0/3，未运行stock Agent／GLM；原OpenClaw验收、精确执行发布及真实固定文本仍分项未完成。

## 执行发布者的剩余原字段调查

本节是当时待调查来源的研究记录，未实施内容不作为当前真实正常业务的前置，也不授权继续开展执行帧新方向。

本轮只读核现有R12／R66自持字节、冻结源码及公开接口，没有运行目标或回读瞬时内存。R12 seq2可解143个JSFunction，R66每callback可解2个；已保存可解释own字段没有取得ToolDefinition.execute→实际函数对象引用。SP／FP只是原数值，原调用帧、实际receiver／argc／execute实参及async交接未被bank保存；unsupported／cap继续为unknown，不据此断言原现场不可获取。报告`/tmp/as-execute-source-evidence-20261115/report.md`及其binding.json。

公开Tool只定义schema；AgentTool.execute四参、扩展ToolDefinition.execute五参，wrapper可能成为另一函数。冻结dispatcher真实传prepared.toolCall.id与prepared.args，后者可能经过prepareArguments／validation；另有internal-preparer路径。不能用原ToolCall.arguments冒实际实参，也不能假设所有调用经过同一公共方法。执行函数身份、实际调用与当前模型消息是不同交接，任何单个函数名、消息形状或地址相似都不补齐它们。

成熟实现提供了有限取证参考：[llnode的Frame读取](https://github.com/nodejs/llnode/blob/main/src/llv8-inl.h)按运行时元数据读取函数、receiver及参数槽；[常量读取](https://github.com/nodejs/llnode/blob/main/src/constants.cc)区分有符号宽度。[OTel V8采样器](https://github.com/open-telemetry/opentelemetry-ebpf-profiler/blob/main/support/ebpf/v8_tracer.ebpf.c)取得frame marker、Function／SFI／Code等用于展开和符号化。这些能力不等于已提供Tool执行语义，AS也未接入其执行帧／实参读取。2026-11-15获取的原源码及SHA256见`/tmp/as-vm-frame-source-review-20261115/sources.json`；只作参考，不安装插件或复制其版本后备／采样阈值。

真实ELF已有部分frame槽常量，但当前u32原位模式不能直接当有符号位移；未来若增加读取须先按其真实声明定字段语义和共同有效期。此处记录待调查来源，不新建生产DTO／缓存／匹配器，不要求普通采集、HIPS或本轮事实源先满足完整执行帧；没有原件证明时仍保候选。N／H／F各0/3，准确归因完成日期未确认。

## 文件过拟合的生产及验收复核

版本限定：本节“五探点／文件专属”指 R6 扩展前的源码核查。后续原始入口扩展及其有限结果已在上方 R5／R6 分别记录；两者不互相回写，也不把原始类别覆盖升级成工具语义来源。下面的后续研究设想不成为当前开发目标或正常运行门槛。

2026-11-15按用户要求逐项核源码与原件，三类覆盖分别认定：

- 当前metadata.rs五探点及as_runtime.c原入口只实现uv_fs_open／work模型；R12公共格式离线复核也来自文件原bank。没有进程／网络正式工具来源，不能仅修改测试类别名称声称覆盖。
- scripts/mvp-smoke/frontend.py原questions仅File.Open，退役SQL核对把operation硬编码为file_open；已有独立Execute／Connect／Send场景没有收进默认正式前端入口。当前已将原Ask／回执共用逻辑收敛，并把四种操作各Allow／Deny纳入同一入口；实际结果另以本轮guest报告认定。
- 历史stock OpenClaw R67的8项确已执行：file／execute／connect／send各2项。原件`/tmp/as-openclaw-evidence-20261112/r67-live/result.json`，使用responses.r67.test本地provider和fixture-model；进程／网络由stock exec工具执行原生目标程序，不是web_fetch外网或真实GLM，不补作通用归因桥。
- R95真实GLM observation-cases.json只有file-allow／file-deny，ToolExecution和反馈事实均0。其文件PASS不覆盖当前进程／网络工具归因或固定提示写出。
- 独立原生WS六项原件`/tmp/as-ws-exec-network.MaUl7ens/result.json`和`/tmp/as-tool-handoff.TKXfoWwW/process-network/result.json`的实际Execute／Connect／Send效果与等待有效，属于自有guest原生探针，不是stock Agent。

过拟合原因是将首个文件研究点、默认前端场景和文件专属SQL断言持续复用，却没有按实际native调用方式核其他类别。后续来源设计改用当前线程TLS／Isolate／Frame这一公共根，再分别核文件work、spawn child、connect同步提交和发送队列／批次；静态核对不充当真实采集。实际getter和调用路径原件见`/tmp/as-node-generic-entry-review-20261115/{elf-getters,report,sources}.json`。刚加入但未提交的File.Open专属fs_type过滤已整体撤回，没有留旧分支或兼容路径。

新收到R14三组有限研究报告`/tmp/as-gap2-r14-research-handoff/handoff.md`，SHA256 9c4c38063e51f6d25538b90467bf2a747dfc6eadfe9468249d20693f834c193e。它使用真实GLM、一个exec工具执行三组操作，含合作式研究观察／修改，且不是AS EventKey或未改Agent纯旁路验收。原件可以指导逐边核验，不能移植观察器或据此划掉当前三类生产来源缺口。

## 原专项量化点与历史排期

本节N／H／F保留为此前专项的未完成功能口径。当前用户已收敛本轮为真实GLM＋AS幻影key的文件／进程／网络验收及脚本修正；不得用本节历史研究排期阻挡普通HIPS／Phantom执行，不再开展执行帧新方向。组件通过不等当前任务交付。

2026-11-15按用户要求采用三类独立完成点：文件、进程、网络分别记完成或未完成，不按测试数、代码行数、阶段平均数或静态可行性换算归因百分比。网络项须分列Connect及Send的实际系统调用路径。当前生产精确归因完成0/3，真实GLM固定提示写出完成0/3；历史有限研究和当前自有guest控制通过不计入这两个分子。

1. **N：原工具到原事件，当前0/3。** 正式生产者在同一有效回调取得公开工具原值，沿实际请求／队列／线程或child task实例到原EventKey，由正式消费者入库；每类有真实Allow和Deny原调用及对应效果，且能区分同进程同参数并发调用。提供原Tool引用、原对象交接、ProcessKey／线程寿命、EventKey、实际回执与原件定位。接口存在、空结果、候选、仅父子PID／时序或有修改的研究采集均不满足本项；缺失的可选现场字段及NativeReturn仍如实记录，不因此拒收已有事实或阻止普通HIPS。
2. **H：原事件到ChatPartRef，当前0/3。** 在N的真实事实基础上，核实际发行作用域与原跨侧引用，定位既有消息／part及原协议记录；同ID跨交换、同参并发和对象退役复用不能错绑。只保留候选的分支不算完成。provider别名、AS侧唯一候选或管理员配置声明不能替代实际交接；不新增alias字段冒充来源。
3. **F：固定规则原文实际写出，当前0/3。** 原OpenClaw聊天在同一冻结候选上以真实GLM幻影key验证三类拒绝；按H的已确认引用，在实际工具完成上行追加规则原文一次，原结果／ID／错误状态保留，原件证明真实写出及Submitted，正常退役。Prepared、软件TLS、规则文本已入库或模型最终口述拒绝均不代替写出。另保留来源不足时不改写的实际结果。允许与拒绝仍各按原操作效果认定。

每类划完成须同时有实现入口与消费、绑定源码／产物的真实验证、交叉审核及提交推送。对照JSON保留原始计数、缺失、候选与确认引用，不把NOT_RUN写0次成功。完成点只约束本次已授权交付，不成为原始研究接纳、普通HIPS启动或用户试用的新门槛。

执行排期采用UTC：

- **11月15日原01:45提交点未达，调整为01:55：** 提交推送本轮三类控制与发送外层取证修正。审核发现仅以非负原返回值不能证明字节／消息单位，已增加逐值断言并在最终原流程复跑；原01:45安排继续保留，不回写为按时完成。这是独立C完成点，不充作N／H／F完成。
- **01:45—04:45：** N的生产采集执行窗口，共用当前线程TLS／Frame和公开格式读取，再分别接文件work、spawn child和网络请求交接。按家族交付实际有证据的生产代码候选及原件，候选关系继续独立标记；某类来源未成立就保持未完成，不能以空壳／文件正例代表它。
- **03:45前检查点：** H的实际发行域／跨侧引用逐项给出可用原件、生产点或具体缺失边；该段目前尚未证实，没有依据承诺完整精确归因的最终日期。未取得交接不进入推定绑定，N及原始事实采集继续推进。
- **H路径证成并有冻结候选后：** 才安排H实现和原OpenClaw的F正式批次；确认候选与原验收owner后记录具体窗口，不先编造尚无依赖依据的交付时刻。

窗口是工作安排，完成只由上述结果认定；到点未满足仍标未完成并更新真实缺边，不反复写“快完成”或用其他层PASS替代。

截至11月15日04:45，C完成点已于01:52:38随e9b7d1e8f提交推送；16次真实原操作及逐值返回通过，范围为原生WS控制，不能替代stock Agent归因。03:45的H检查点和04:45的N窗口未完成，按逾期记录。N新代码已接Submission统一投影、实际child交接、SQL单BLOB及原候选引用；组件检查通过，LIVE尚未执行，因此N／H／F仍各0/3。H仍缺实际发行作用域及跨侧引用，未授权使用provider别名推定；最终归因日期未确认。后续实际运行原件和审核结果在此追加，不回写期限或将组件通过改算N。

下一实际检查点为11月15日05:15 UTC：核第二冻结快照的原生组件guest结果、原件与源码／产物绑定，并完成公开读取、临界区及SQL冲突复核。该组件使用明确的公共格式工作负载，不是stock Agent／GLM；即使通过也不自动增加N／H／F。若失败保留原失败与具体边，不能将正在构建写为已执行。

## 单一回调点的采集约束与流程

用户明确要求在回调的同一个有效时点一次拿全精确关联所需的瞬时上下文。R44 的 Ask 期间延迟深读、随后回读原内存的方式只保留为失败调查，不再作为后续采集方案。原对象之后移动、复用或销毁，不会反向改变此前已正确采集的历史值；R44 暴露的是跨多次读取没有证明取得同一时点的完整上下文，不能据此否定瞬时采集。

1. 先读真实生产者与消费者，列全本次关联所需的身份、调用作用域、原请求引用及生命周期信息，明确每项来源和单位。一次拿全指完整的必要上下文，不扩大为全进程内存采集，不在来源未明时新增公共 DTO。
2. 找到这些字段共同有效的单一回调点，核对原程序在此处的所有权、交接和并发约束。在同一回调内完成必要字段及原值复制，发布一条自持有记录；只记录地址、回调后再沿地址读内容不算完成采集。同点拿不全即保留明确缺失，不用别的时刻补成完整关联。
3. 回调后的解析、关联、入库与核验只处理已采集记录，复用原事件／在途上下文和消费队列，消费完成即释放。必要进程树补空保持原有唯一所有者，但不能把当前进程树值冒充采集点的瞬时调用身份。
4. 验证关注采集点来源、记录完整性、实际消费和原操作效果。原对象随后变化可以作为生命周期事实，不能要求它一直不变；也不能把多次不一致的原内存读取拼成成功。观察器消耗原 Ask 期限属于实际失败，不增加期限、容量、回退或测试专用放行来掩盖。

[Linux uprobe](https://docs.kernel.org/trace/uprobetracer.html)支持在指定执行点读取寄存器和用户内存，因此瞬时值可以在回调时采集。一个回调不等于整个目标进程的原子快照，共享对象是否同时有效仍须由该点的真实代码和运行证据确定。R47 已验证冻结样本的单点采集；通用产品来源仍未实现。

一次采全约束的是本次原始值，不要求采集时已生成后续 EventKey 或最终业务关系。沿原对象有效寿命传递已经取得的身份属于机械交接，不等于跨时刻补读。原始事实也不必预先带ToolCall；旧工具专属结构不能限制其他系统观察。

## 成熟实现核查

本轮市场样本采用 [JetBrains 2026 调查](https://blog.jetbrains.com/research/2026/08/ai-coding-agent-adoption-2026/)中的主流编码 Agent，并补充现有 OpenClaw 和 Python 自主 Agent。运行方式确有差异，[Claude Code 原生版](https://bun.sh/blog/bun-joins-anthropic)与 [OpenCode](https://github.com/anomalyco/opencode/blob/dev/CHANGELOG.md)使用 Bun，[Codex](https://github.com/openai/codex/blob/main/codex-rs/Cargo.toml)核心为 Rust，[Gemini CLI](https://github.com/google-gemini/gemini-cli/blob/main/package.json)声明 Node 运行环境，[Hermes](https://github.com/NousResearch/hermes-agent/blob/main/pyproject.toml)为 Python。该核查是调查样本，不能作为 AS 已支持名单；取证方案继续围绕系统接口和原始协议。

[Linux uprobe文档](https://docs.kernel.org/trace/uprobetracer.html)定义的是在已知可执行对象的偏移处读取寄存器、参数、内存及返回值。无需目标源码修改，但要求找到真实对象和正确ABI。它不为未知JIT函数或私有业务对象提供统一的tool_call接口。

[Tetragon hook说明](https://tetragon.io/docs/concepts/tracing-policy/hooks/)支持LSM、tracepoint、kprobe、uprobe及已有USDT。它可以基于真实内核对象丰富进程／文件／网络事件；不能把其process exec_id当成LLM工具ID。安全裁决继续使用已复制到内核的真实对象，用户指针入口快照不能取代最终LSM事实。

AgentSight官方仓库现重定向到eunomia-bpf/agentsight，本轮固定提交c1574366122a90c451bca6a84713e28813ab95e2。其[HTTP关联设计](https://github.com/eunomia-bpf/agentsight/blob/c1574366122a90c451bca6a84713e28813ab95e2/ext/analysis/docs/http-correlation.md)用真实进程出生标识、TLS对象及HTTP/2 stream或HTTP/1 FIFO配对协议请求响应；明确把缺来源的旧配对标成inferred。其[多信号关联代码](https://github.com/eunomia-bpf/agentsight/blob/c1574366122a90c451bca6a84713e28813ab95e2/script/multi_signal_correlator.py)还使用时间窗口、进程亲缘及参数相似度计算关联分数。AS可参考真实对象和协议分流，不能采用这些推定生成精确工具执行关系。源码SHA与原件位于`/tmp/as-agent-independent-research-20261109`。

OBI的[Node支持](https://github.com/open-telemetry/opentelemetry-ebpf-instrumentation/blob/3d083233bc9dda6856a5ee3cc1b64002ebae61e6/pkg/internal/nodejs/fdextractor.js)通过运行时加载 JS 取得异步上下文，属于历史隔离授权所研究的机制，不是当前正式路线。它与纯进程外采样的 OpenTelemetry eBPF Profiler 是不同项目／路径，不能因为同属OpenTelemetry就互换接入方式。

[Falco 的规则字段](https://falco.org/docs/reference/rules/supported-fields/)围绕实际系统事件、进程、文件描述符和容器身份组织，支持共享行为规则。[OpenTelemetry 的上下文传播](https://opentelemetry.io/docs/concepts/context-propagation/)则依赖实际携带跨边界的上下文标识。规则可判定已取得字段是否匹配，不能据字段匹配产生一条未观察到的工具身份交接。

CamFlow 的[原论文](https://camflow.org/publications/ccs-2018.pdf)说明，全系统信息流图对进程与共享状态采用保守依赖假设，并可能产生误报。该机制可作系统对象关系研究，不能据此保证同一进程内每个工具调用与系统操作的唯一映射，也不作为本项目更换内核的依据。

## 按语言运行时适配与布局元数据

2026-11-12 核查结论：按运行时共用采集实现有成熟先例，可以减少逐 Agent 维护。它不能仅凭语言名称保证精确 ToolCall 归因。用户要求保持跨语言方向，不固定 Node 发行版、不建立逐 Agent 私有适配；本轮按运行时调查支持范围，尚未宣称正式持续采集通过。

[OpenTelemetry eBPF Profiler](https://github.com/open-telemetry/opentelemetry-ebpf-profiler/tree/21b48e5ff7cfa0efa0a6ed731b804037b4415fb5/interpreter)将 Node/V8、CPython、Go、JVM 等读取器分开，共用进程与采样链。其证明范围是运行时识别、堆栈及符号解析。AS 可借鉴该边界，不能把 profiler 的语言支持表直接变成 AS 的工具归因支持表。

- **Node/V8**：[元数据读取代码](https://github.com/open-telemetry/opentelemetry-ebpf-profiler/blob/21b48e5ff7cfa0efa0a6ed731b804037b4415fb5/interpreter/nodev8/v8.go)的 `readIntrospectionData` 从实际 ELF 读取 `v8dbg_*` 字段偏移、类型及标签；[llnode](https://github.com/nodejs/llnode/blob/de1f01d70a5c58111dd873d340f898023e4e8fe6/src/llv8-constants.cc)使用同类元数据读对象。两者也有缺符号时的回退或布局假设，不能直接移入禁止猜测的精确采集链。Node 的 AsyncLocalStorage 存的是应用传入的任意值，[getStore](https://github.com/nodejs/node/blob/v24.x/lib/internal/async_local_storage/async_context_frame.js)按存储实例读取；运行时本身不规定 `toolCall` 或 `assistantMessage` 字段。R47/R66 能读取这些字段的事实仍只覆盖原验证应用。
- **CPython**：[3.14 的 `_Py_DebugOffsets`](https://github.com/python/cpython/blob/3.14/Include/internal/pycore_debug_offsets.h)给出目标构建的线程、帧、局部变量槽及常见对象字段偏移，包含版本、cookie 和 free-threaded 标志。它为布局发现提供实际来源；读取器仍须理解元数据格式、核实并发一致性。不能推定所有 Python 实现或所有旧发行版都提供相同字段。只研究其读取能力；[远程执行机制](https://docs.python.org/3.14/howto/remote_debugging.html)会写目标并执行代码，不属于本项目旁路方案。
- **Go**：[`.gopclntab`](https://github.com/open-telemetry/opentelemetry-ebpf-profiler/blob/21b48e5ff7cfa0efa0a6ed731b804037b4415fb5/doc/gopclntab.md)可为剥离符号的程序提供函数、代码位置与栈信息，格式由目标头部识别。它没有统一的 ToolCall 字段，goroutine 或栈身份也不能独立证明工具归属。运行时布局和业务对象来源仍须分别核实。
- **Rust/C/C++ 原生程序**：可以复用 ELF、调用约定及实际存在的 DWARF／展开信息；展开信息不等于对象成员布局。Rust 默认类型布局[允许随编译改变](https://doc.rust-lang.org/reference/type-layout.html)，不能维护一张“Rust 固定偏移表”读所有 Future 或工具对象。缺真实布局及来源时不发布精确工具事实。
- **Bun/JavaScriptCore**：与 Node 同属 JavaScript/TypeScript 使用场景，但 [Bun 使用 JavaScriptCore](https://bun.sh/docs/runtime)，不能套用 V8 读取器。本轮没有取得可复用的完整工具上下文旁路实现，保留待验证，不记为已支持。

实际 Agent 样本确认了运行时差异：[nanobot 的 Python 构建声明](https://github.com/HKUDS/nanobot/blob/9f7ae6fb7b60ae1efb23688716f8a64807914970/pyproject.toml)、[Goose 的 Rust 工作区](https://github.com/block/goose/blob/8c5d9f77e04a32bd51e9c8eea21bc240a560e424/Cargo.toml)、[PicoClaw 的 Go 模块](https://github.com/sipeed/picoclaw/blob/bbf6893ca7afad27f1d00a0f5a45982a549c6ed6/go.mod)、[OpenCode 的 Bun 入口](https://github.com/anomalyco/opencode/blob/101776024cc69bafb3f9dc90027ab4d1eab40a2a/package.json)。这些只是研究样本，不是市场覆盖率或 AS 支持承诺。支持说明应按运行时、架构、必要元数据及实际验证能力记录；不按品牌匹配，不要求用户填写内存偏移。识别目标版本用于正确解码，不等于将产品固定在单个 Node 版本。

本地实际 ELF 复核：`/tmp/as-oc-r47/root/usr/local/bin/node`，SHA256 `c8c5721679d8d1a09e9987e345d5f9a64899bb85f7a384783ba8a93e6e62348a`。`readelf -Ws` 按名称去重得到 480 个 `v8dbg_` 和 16 个 `nodedbg_` 元数据符号，存在 `uv_fs_open`；`readelf --debug-dump=info` 未找到 FSReqBase、AsyncWrap、IsolateData 类型名。符号存在不能证明全部布局可读取。该次 ELF 静态检查结果及固定提交源码位于 `/tmp/as-runtime-metadata-research-20261112`。后续隔离进程的元数据实读与原 OpenClaw 聊天复核见下节，不能将静态检查计为持续采集验收。

扩展继续沿现有模块边界：驱动适配识别真实映像及布局，驱动在已证明的回调点一次采集必要上下文，Main 沿唯一 `MainMessage::RequestHandoff` 转交Submission与实际原交接，业务库以origin外键提供 `ToolExecutionFacts` 查询返回，中间层组合模型协议关系。公共结构以[事实契约](../contracts/flow.md#模型响应与工具的事实交接)为准，不为每种语言复制 DTO、队列或数据库。映像元数据与瞬时工具对象分别处理；后者只属于原在途上下文，消费完成释放，不能在后续回调重新读取或补字段。具体探点、缺失布局来源与同点一致性未证明前，不增加悬空的通用插件框架或假生产者。

### 持续采集接入前的源码与实读复核

2026-11-12 用户要求先将 Node/V8 持续采集接入正式链路，同时核实 Bun 与 CPython 的入口；禁止固定 Node 版本和逐 Agent 适配。复核发现原实验不能直接替换为通用生产者。已有消费接口保持，正式持续采集仍未实现。

**Node 的可收敛部分。** [ReqWrap::Dispatch](https://github.com/nodejs/node/blob/v24.22.1/src/req_wrap-inl.h)在调用 libuv 前通过 `Dispatched()` 设置 `req_.data = this`。在已经确认属于此 Node 包装器的原请求上，可沿实际 `req.data` 获取包装对象，删除实验的 `req - 104`。不能把任意 libuv 使用者自行设置的 data 指针当作 Node 对象。[BaseObject 调试偏移](https://github.com/nodejs/node/blob/v24.22.1/src/node_postmortem_metadata.cc)可定位 persistent handle；[AsyncWrap 的帧槽号](https://github.com/nodejs/node/blob/v24.22.1/src/async_wrap.h)等于 BaseObject 的内部字段数。原实验读取 async ID、provider 及 Realm／Environment／IsolateData 的全部固定偏移并非都必需，不能因这些字段缺 DWARF 就断言整段请求交接无法读取。

**静态符号与运行时值的区别已实测。** 16 个 `nodedbg_*` 位于该 ELF 的 `.bss`，由 `GenDebugSymbols()` 启动时赋值；`v8dbg_*` 的已核值位于 `.data`。在新建的隔离空闲 Node 进程中，以 ELF 实际符号地址读取目标内存，取得 BaseObject persistent 偏移 8、内部字段数 2、ReqWrap 内部字段数 3；JSObject 内部字段起点为 24。数值只证明该 SHA256 映像的本次结果，不能写成生产默认偏移。记录 `/tmp/as-runtime-metadata-research-20261112/node-live-metadata.json`；目标未加载 Agent，读取后子进程实际退出，退出码 -15。该实验没有采工具上下文或挂载驱动，不能计作正式采集或 OpenClaw 验收。

**剩余布局与语义分别缺失。** 该 ELF 的现有调试元数据没有覆盖原采集所需的全部 JSCollection／OrderedHashMap 布局；JS 对象头还受实际类型、压缩指针和填充影响，不能由普通 JSObject 常量无条件反推。V8 的[debug helper](https://github.com/v8/v8/blob/main/tools/debug_helper/README.md)是依赖 V8 构建的调试器辅助库，当前调查未证明它能直接为该任意发行映像提供所需完整布局。另一方面，R48 的 `read_context` 要求应用对象的 `toolCall`／`assistantMessage`，再核其中同一工具对象的引用。Node ALS 只负责传递应用值，不规定这两个字段的含义；改为可配置字段名仍是应用适配，不能当作运行时通用语义。

**替代公共点的实际边界。** `v8::Object::SetInternalField` 的参数包含 JS 对象、槽号和帧，但在已核 FSReqPromise 新建路径上，该调用发生在 AsyncWrap 初始化中，随后 ReqWrap 才继续构造并在 Dispatch 设置原请求；不能在此宣称已经取得原 `uv_req_t` 交接。该公共函数也用于资源复用或清空，不能将一次新建路径的顺序概括为所有调用。`GetInternalField` 常用路径内联，SlowGetInternalField 不是必经点；`AsyncWrap::context_frame/set_context_frame` 内联，GetAsyncContextFrame 是按需调试入口。这些公共点尚未证明能给出可持续追踪的原资源身份；“当时没有uv_req_t”本身不能否定后续机械交接。JS地址可移动、Local handle有短有效期，必须核实真实寿命，不能延迟回读补字段。源码复核原件位于 `/tmp/as-node-capture-review-20261112`。

**Bun／CPython 是源码候选，未实测。** Bun Linux 的文件异步 Open 由 `AsyncFSTask` 分配 `Job` 后交工作池，JS 半段 thread-affine，工作线程由原 task 返回所属 Job。候选是在实际任务已分配、尚未交工作池时复制当次 ALS／参数及原任务引用；Linux 的 Open 别名不是可套用 Node ABI 的 libuv 请求。CPython 普通 FileIO／os.open 在调用前释放线程状态；应研究具体 open 路径脱离前的回调，而不是在 LSM 中延迟回读 Python 帧。3.14 的 `_Py_DebugOffsets`／asyncio 调试元数据覆盖线程、帧和部分任务字段，但没有直接给出所需的 `PyThreadState.context`／Task 上下文偏移。`asyncio.to_thread` 明确复制 context，普通 run_in_executor 没有这项保证；free-threaded 构建不能借用 GIL 的对象一致性结论。这两条路径均未定义成已实现探点，没有发布新公共数据结构。

Bun 固定源码提交为 `43850629ce62b457ececc95689ad54abae3f6d47`：[文件调度](https://github.com/oven-sh/bun/blob/43850629ce62b457ececc95689ad54abae3f6d47/src/runtime/node/node_fs.rs#L2220)、[Job 交接](https://github.com/oven-sh/bun/blob/43850629ce62b457ececc95689ad54abae3f6d47/src/jsc/job.rs#L342)、[ALS 帧](https://github.com/oven-sh/bun/blob/43850629ce62b457ececc95689ad54abae3f6d47/src/js/node/async_hooks.ts#L352)。CPython 3.14 固定提交为 `69ab05e86317491a283353038ade7c165a32c208`：[FileIO](https://github.com/python/cpython/blob/69ab05e86317491a283353038ade7c165a32c208/Modules/_io/fileio.c#L392)、[脱离线程状态](https://github.com/python/cpython/blob/69ab05e86317491a283353038ade7c165a32c208/Python/pystate.c#L2278)、[调试字段](https://github.com/python/cpython/blob/69ab05e86317491a283353038ade7c165a32c208/Include/internal/pycore_debug_offsets.h#L69)、[to_thread](https://github.com/python/cpython/blob/69ab05e86317491a283353038ade7c165a32c208/Lib/asyncio/threads.py#L12)。源码与只读研究记录位于 `/tmp/as-bun-cpython-entry-review.sDpimBFL`。固定引用只用于复核本轮证据，不是产品版本白名单。

**原 OpenClaw 聊天独立复核。** 直接从 R48／R66 原运行镜像核对实际 Node 和观察器，均为同一 Node v24.22.1 与 R48 模块；6 条及 3 条原回调匹配固定 PC、callback 和调用位置。观察器没有读取上述元数据符号，仍按指定 PID／路径、最多 32 条样本及退役时 `/proc` 导出工作。原真实效果与离线重放保持有效，脱离固定布局和应用私有上下文均为 NOT_PROVEN，正式持续采集为 NOT_RUN。独立报告 `/tmp/as-node-capture-independent.YwagTb9z/result.json`，SHA256 `3de53e5e620c1438943c54ad97d258219027cef581a6dde2a0d95c685929386e`；复算脚本同目录 `check.py`。本轮没有新生产候选，也没有把前提核验记为正式验收。

### 扩充 Agent 样本与实际安装形态

2026-11-12 根据用户点名补查 DSH（DeepSeek Harness）和 Pi Coding Agent，并扩充到下列 20 个有官方来源支撑的样本。核对执行入口、构建声明和必要打包脚本，不以仓库语言占比、npm 分发或构建工具名称判定运行时。Pi 横跨两种安装形态，只算一个项目；OpenHands 条目指实际执行侧 SDK／服务，不将客户端语言计入。以下是研究样本，不是 AS 已支持清单，也不是市场份额。

- **Node/V8 执行入口**：OpenClaw、Gemini CLI，以及 [DSH CLI](https://github.com/deepseek-ai/deepseek-harness/blob/e8f7b93a344964de3dca1ace9e72b6e4d6de87cb/apps/cli/package.json)、[Qwen Code](https://github.com/QwenLM/qwen-code/blob/cbb06f945a9b8fdb584dc8ef2115595d0077d82b/package.json)、[Kimi Code CLI](https://github.com/MoonshotAI/kimi-code/blob/410814349d4677fc2844ad79069b0ad4a751303a/apps/kimi-code/package.json)。前两者入口见 [OpenClaw](https://github.com/openclaw/openclaw/blob/main/package.json) 与 [Gemini](https://github.com/google-gemini/gemini-cli/blob/main/package.json)。DSH 的 CLI 是 TypeScript 构建出的 `lib/bin.js`，实际源码入口使用 Node。Kimi 当前公开执行代码为 TypeScript；独立包由[Node SEA 构建](https://github.com/MoonshotAI/kimi-code/blob/410814349d4677fc2844ad79069b0ad4a751303a/apps/kimi-code/scripts/native/02-sea-blob.mjs)，不因无需安装 Node 就判成 Rust/Go。旧 Python kimi-cli 已归档，不能据旧项目给新版本分类。
- **Pi 的两种运行形态**：[Pi Coding Agent](https://github.com/earendil-works/pi/blob/a40abecaaf39e5a11e6c4850ee5cd8ecda4958cc/packages/coding-agent/package.json)为 TypeScript，npm CLI 的入口是 Node 脚本；同一包的 `build:binary` 明确使用 `bun build --compile`。原 `badlogic/pi-mono` 地址已重定向至 `earendil-works/pi`。同一个品牌不能固定归入 V8；应识别当前执行映像。
- **Bun/JavaScriptCore 执行形态**：OpenCode、oh-my-pi、Cline CLI、Claude Code 独立包。依据分别为前述 OpenCode 入口、[oh-my-pi 的 Bun 引擎声明与 CLI](https://github.com/can1357/oh-my-pi/blob/4ad629ea51d96c18bbdfcc2252fd13b301f57f9f/packages/coding-agent/package.json)、[Cline CLI 的 Bun.build compile](https://github.com/cline/cline/blob/50e77bc1e8f7ab023b7986821e4b5f82ff51b8c4/apps/cli/script/compile-binary.ts)、[Bun 官方对 Claude Code 分发方式的说明](https://bun.sh/blog/bun-joins-anthropic)。Cline 此项仅指已核的 CLI 形态，不能据此替其 VS Code 扩展或其他历史形态作相同判断。oh-my-pi 与 Pi 分开记录。
- **Python 执行侧**：[Aider](https://github.com/Aider-AI/aider/blob/main/pyproject.toml)、前述 nanobot、[Hermes Agent](https://github.com/NousResearch/hermes-agent/blob/eaac327fac2c2db0b77ee143989d49b29a6f36cb/pyproject.toml)、[mini-SWE-agent](https://github.com/SWE-agent/mini-swe-agent/blob/04d809ceab9df28f9adaed044884180159172930/pyproject.toml)、[OpenHands 执行 SDK](https://github.com/OpenHands/software-agent-sdk/blob/9f7d5cb11b1256c1878f5584a657401feca9ad16/openhands-sdk/pyproject.toml)。这些声明证明 Python 执行代码，不保证每个实际安装都具备相同解释器构建、调试元数据或工具上下文。
- **Rust 原生执行侧**：[Codex CLI](https://github.com/openai/codex/blob/main/codex-rs/Cargo.toml)、前述 Goose、[ZeroClaw](https://github.com/zeroclaw-labs/zeroclaw/blob/6c78ba59bce6a3d5079cf0c0310488770e264ad4/Cargo.toml)。语言相同不提供统一 Future、执行器或工具对象 ABI。
- **Go 执行侧**：前述 PicoClaw、[Crush](https://github.com/charmbracelet/crush/blob/3e67f10e3cd9290b48ca71b54829a17b2bc8ec88/go.mod)。Go 工具链元数据可作为共享读取来源，但仍须确认实际请求上的原工具引用。

另外检查了 GitHub Copilot CLI 的[官方分发说明](https://github.com/github/copilot-cli/blob/16e78633fb9b246201dd1adfd89a0bd3811358dd/README.md)。本轮取得的分发仓库不足以判定其执行核心语言，因此未纳入上述 20 个已分类样本；npm 安装命令不作为 Node 引擎证据。

本轮新增固定提交的清单、入口文本与摘要存于 `/tmp/as-runtime-metadata-research-20261112/agent-samples`，未安装或运行这些 Agent。工程判断：Node/V8、Bun/JavaScriptCore 和 Python 都有多个真实样本，应共同进入运行时研究范围；Go 与 Rust 原生路径继续独立核实。适配按实际映像、元数据和调用约定选择，不新增逐 Agent 品牌分派。运行时读取器共享底层事实接口；采集到对象布局不自动证明 ToolCall 归因。

### 当前采集技术与跨系统边界

当前 AS 后端和中间层为 Rust。Linux 生产保护使用 C 编写的 BPF-LSM、原生支撑模块和 libbpf 加载器；原进程生命周期取自内核 tracepoint。LLM 协议事实由 Flow 的受控 TLS 出口采集，使用 rustls／hyper，不是通过运行时堆栈重建聊天。R47/R66 工具上下文实验使用自有原生 uprobe 观察器，在原回调复制用户内存；该实验持续采集器尚未接入正式产品。OTel Profiler 与 llnode 当前仅作为调研依据，没有作为 AS 运行依赖。

跨系统扩展须分别处理操作系统采集与语言运行时读取。当前驱动适配器 crate 仅在 Linux 编译；Linux 实现的 `LinuxDriverAdapter::connect_owned` 对非 Linux 平台参数返回 Unsupported。不能把 Rust 可跨平台编译等同于 AS 已支持其他系统。Windows 有原生[文件 minifilter](https://learn.microsoft.com/en-us/windows-hardware/drivers/ifs/about-file-system-filter-drivers)与[WFP 网络过滤](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/windows-filtering-platform-architecture-overview)，macOS 提供 [Endpoint Security 和 Network Extension](https://developer.apple.com/system-extensions/)；它们是未来平台执法入口的研究依据，不证明可复现 Linux 的同点工具内存采集。公共事件、规则及业务消费可沿现有接口复用，系统探针、映像格式、原生等待、权限与进程身份必须在各系统核验。当前不扩开发范围、不创建平台空实现；现有 Windows 通信预留与已实现 Linux 的区别见[驱动通信](../contracts/driver-communication/technical.md#1-机制选择与依据)。

## 能增加哪些真实hook事实

- 进程：继续以实际fork／exec／exit及线程创建退出观察维护强实例。可补真实发起线程与子任务的创建关系；只能证明创建，不把其所有未来操作说成某个工具调用。
- 文件：在现有LSM原操作补真实file对象、设备／inode、权限和结果；FD复制／传递可记录实际对象传递。路径相同、打开时间相近或读过某文件均不能升级成工具因果。
- 网络：通过真实socket生命周期、connect／accept及成功收发保留socket cookie、netns和字节边界。HTTP/2按真实stream，HTTP/1按完整协议顺序对应请求响应；连接建立者不能替代每次实际发送者。
- 管道／Unix socket：可以观察实际端点、内核缓冲对象及已完成的收发。仅观测write参数不足以证明实际写入长度，也不能假定一个write等于一条消息；边界须依据实际成功结果与已取得协议。本轮不恢复已暂缓的MCP解析／控制。
- 语言运行库：对确有ABI／符号证据的libuv、TLS库或已有USDT，可以补原生请求对象到系统操作的机械传递。库类型识别、符号、生命周期或来源缺一项就不产生该关系；不从进程名称认定Agent种类，不把不存在的工具ID填入事件。

这些是按实际缺口逐项接入的方向，不是本轮已新增的hook。普通HIPS动作继续只按现行规则和原操作执行；不为关联增加新的Allow规则、暂停策略或失败回退。

## 扩大线程、请求和内存取证

检查范围包含当前 AS 尚未采集的系统事实。本机 7.0.0-1019-nvidia 匹配头文件已核实含 syscall、task、sched、signal、workqueue 和 io_uring tracepoint 定义，运行时附着和完整来源仍需实测。

- 同步系统调用可由原 `sys_enter` 的 `pt_regs`、系统调用号和六项参数，到同一调用的退出结果及其中实际 LSM 原事件。当前 AS 的 TID／线程出生事实已有实现，完整系统调用进入／退出的关联尚未接入。[Linux 原定义](https://github.com/torvalds/linux/blob/v7.0/include/trace/events/syscalls.h)
- 内核异步请求可检查 io_uring 的原 `ctx`、`req`、`user_data`、opcode、flags 和内嵌 work。`submit_req`、`queue_async_work` 与完成点提供真实请求对象；`user_data` 是应用提交的值，不能默认等于 ToolCall ID，也不能在 SQPOLL 情况下把轮询线程当成写入 SQE 的应用线程。[Linux 原定义](https://github.com/torvalds/linux/blob/v7.0/include/trace/events/io_uring.h)
- 内核 workqueue 的入队和执行点携带同一 `work_struct`。这证明对应内核工作项的移交，用户态工作队列需另找实际对象。[Linux 原定义](https://github.com/torvalds/linux/blob/v7.0/include/trace/events/workqueue.h)
- `sched_waking` 明确处于实际唤醒上下文，`sched_wakeup` 不保证这一点。futex 和信号记录用于核对等待与唤醒；它们自身不声明哪个工具请求被执行。[调度原定义](https://github.com/torvalds/linux/blob/v7.0/include/trace/events/sched.h)、[futex 唤醒语义](https://man7.org/linux/man-pages/man2/futex_wake.2const.html)
- 用户线程的原寄存器、用户栈及实际参数指针纳入取证。后续瞬时上下文采集遵守上文单一回调约束。`process_vm_readv` 支持进程外读取但不保证原子快照，下文的活体深读只作为调查原件，不作为正式来源的回调后补全路径。[系统接口](https://man7.org/linux/man-pages/man2/process_vm_readv.2.html)

可复用的跨语言基础包括 [OpenTelemetry eBPF Profiler](https://github.com/open-telemetry/opentelemetry-ebpf-profiler)，其已有进程外 native／Rust／Go 及 V8、Python 等用户栈展开；[py-spy](https://github.com/benfred/py-spy)还支持读取 Python 局部变量。该能力证明可以扩大进程外取证，未直接提供工具调用的业务身份，也未在 AS 中接入。优先核对通用内核接口、公开调试元数据和运行库对象交接，不因现有 EventData 缺字段就认定真实来源不存在。

本机另完成一次独立系统接口验证。只对本次创建的 `cat` 子进程，在它阻塞于下一次 `read` 时读取 `/proc/<pid>/syscall` 的实际参数、用户 SP／PC，通过该原缓冲区地址用 `process_vm_readv` 读回上一轮已经回显的 32 字节测试内容，并用两端持有的 FD 核对同一 pipe inode，子进程正常退出 0。这里的字节是缓冲区保留的上一轮值，不能当作尚未完成的当前读取结果。没有向目标加载代码。原件 `/tmp/as-kernel-origin-readproof.json`，SHA256 `7d53f520f2bb3c99b3ec6f2a23a7cfa28d1b6a293aa233ed055b0e9f0837cd8e`；这只验证进程外原参数和相关内存可读，不是 Agent 或 ToolCall 关联验收。

Node 24.22.1 的[原生请求代码](https://github.com/nodejs/node/blob/v24.22.1/src/async_wrap.cc#L556)在创建／重置异步资源时保存 `context_frame`，其后才进入可选的 JS async hook。该顺序提供了检查真实上下文交接时点的源码线索，尚未证明在单一回调内一次拿全所需字段。该字段属于运行库结构，不能混作所有语言都具有的内核字段。

对前述已核 SHA256 的 Node 二进制进行 ELF 符号读取，实际存在 `nodedbg_offset_BaseObject__persistent_handle___v8_Persistent_v8_Object`、`nodedbg_const_BaseObject__kInternalFieldCount__int` 以及 `v8dbg_class_JSObject__internal_fields__uintptr_t` 等公开调试元数据。Node 的 [BaseObject 源码](https://github.com/nodejs/node/blob/v24.22.1/src/base_object.h#L176)说明这些偏移用于外部调试。R47 的固定版对象解码已通过；由目标元数据自动取得通用布局尚未实现，不能把该固定布局当成跨版本或跨语言能力。

## 为什么纯系统对象仍不足以唯一指向工具

同一进程可同时持有两次不同工具调用，对同一文件提交相同read操作。系统hook可明确给出两个原生请求和两次真实读取；如果程序没有在该边界携带工具身份，把两个业务ID互换不会改变这些可见系统字段。因此从现有字段中选一个工具ID是额外推定，增加相同层面的时间／PID／路径字段不解决歧义。

应用现有协议若已经携带请求ID，AS可以在该协议的明确作用域内保留真实引用；这只证明协议关系。只有原始生产者还给出该请求到具体系统操作的传递事实，才能进一步形成既定tool_execution。不能为了回传固定文案，把“同一进程发生过拒绝”改写成“这个工具已被拒绝”。

## 明确执行交接的来源核查

[OpenTelemetry Context](https://opentelemetry.io/docs/specs/otel/context/)把标识附到实际执行单元，并在执行边界显式或由插桩传递。独立采集的内核操作无法取得未传递的应用调用标识。[OpenClaw 工具策略 hook](https://docs.openclaw.ai/plugins/hooks/tool-policy)依赖目标运行时的进程内插件，文档列出的工具生命周期标识和执行参数也未给出 AS 原内核事件。已核的 [Gateway 事实与命令链](../backend/flow/tool-call-evidence.md)可以证明原工具生命周期与实际命令输入，仍缺异步分发或子进程提交处的明确交接。

后续若发现满足无需目标接入的真实来源，首先核对两次参数相同且并发的工具调用是否仍能分别提供各自原操作，以及同一执行器内无关操作是否保持无关系。Ask 需在原请求等待期间取得关系，事后来源只支持事后审计。确定边仍复用[中间层契约](../middleware/technical.md#3-业务查询与结构)，不因待核来源增加接口或字段。

## R37 同步问询窗口反例

原 OpenClaw 聊天使用冻结 AS `92e47a2d` 与同一原版 Gateway 实测。单会话对照保留一次原 `read` 的开始和结果、四条有实际用户允许回执的同路径 Ask，等待期间可见候选数为 0 或 1。双会话对照取得两个原 `read` 工具 ID，原参数完全相同，Gateway 原时间区间重叠 1379 毫秒。原事件序号 238544 对应的 Ask 尚未回答时，同一观察采样已有两个候选，随后该 Ask 收到实际用户允许回执。双会话等待期间分别观察到 0、1、2 个候选。

本轮观察的是工具事件和 Ask 在同一观察器内的到达，Gateway 的原时间只用于核对两次工具调用彼此重叠。0 表示当时未见候选，1 只表示当时见到一项。实测反驳了「同步 Ask 使时间窗口内的工具唯一」，未建立工具到内核事件的关系，也未写入产品关联。原结果 `/tmp/as-openclaw-evidence-20261110/r37/result.json` SHA256 `1373d0fb140c4e53dd745fc97a59e4d3cf44656ea8d8d7ab68b9585f9a5e7f12`，七份引用原件哈希已核对。

## R37 原线程与文件请求交接

- R37 原 Ask 和答复（`as-events.jsonl` 第 554、712 行）均为同一 `File.Open` 原事件 238544，Gateway TGID 549，发起 TID 2093，线程出生单调时刻 340853172320 ns。以原线程出生标识核对，单会话和并发会话的同路径 Ask 都重复使用了同两个非主线程 2090、2093。Gateway 工具开始和结果帧携带原 `toolCallId`，没有线程或文件请求标识。原等待代码用每个请求自己的等待队列，不把工具 ID 附给该线程。
- 从 R37 运行镜像内的 Docker 存档核对 rootfs 层与原记录一致，九份相关文件与读取的解包原件 SHA256 全部相同。OpenClaw 2026.10.9 的 `agent-core-jCWPqNtz.mjs` 在 `prepareToolCallEntry` 把原 `toolCall` 放进 `AsyncLocalStorage`，并以原 ID 构造 `toolResult`。`tools-BZlysk8b.mjs` 的本地 `read` 通过按文件键排队的 Promise 调用 `ops.readFile(path)`，`@openclaw/fs-safe` 0.26.0 再调用 `fs.promises.open(path, flags)`。工具逻辑等待 Promise，原生请求实参不含 `toolCallId`。
- 用同版 Node 24.22.1 在独立进程加载上述未改的 `read` 和执行上下文模块。两个不同的夹具调用 ID 并发读取同一路径，原读结果一致。`async_hooks.init` 观察到各自 5 个不同 `FSREQPROMISE` 资源，回调内能同时读取该资源的 `asyncId` 和上下文里的原调用 ID。独立的无上下文读取只见空调用 ID。这证明 JS 侧存在可观测的交接，夹具没有启动或更改 Gateway，也未记录驱动原事件。
- [Node OpenFileHandle](https://github.com/nodejs/node/blob/v24.22.1/src/node_file.cc#L2258)把实际 `FSReqBase` 提交给 `uv_fs_open`；[libuv](https://github.com/nodejs/node/blob/v24.22.1/deps/uv/src/unix/fs.c#L1891)在工作队列路径使用同一 `uv_fs_t`，另有 `io_uring` 分支。源代码可以解释原生执行请求的传递，但 R37 未采集请求对象到实际 `EventKey` 的交接。`asyncId` 不是 TID，原线程等待也不携带该 ID。当前仍缺无需改动目标环境的观测来源，能在原 Ask 等待期间证明这段交接；业务层只持久化有该原始证据的关系。

## 独立夹具的原生请求传递

逐字节核对了 R37 stock 镜像中的 Node 24.22.1 可执行文件，SHA256 `c8c5721679d8d1a09e9987e345d5f9a64899bb85f7a384783ba8a93e6e62348a`。独立进程用同一二进制和原版 `read` bundle，提交两个不同调用的同路径读取及一次无关读取。观察器在 `OpenFileHandle` 的同一调用栈内取得 `FSREQPROMISE` 的 `asyncId` 与实际提交的 `uv_fs_t`，在 libuv 工作者中核对同一个请求的打开调用和原完成回调。夹具的 JS `async_hooks` 按 `asyncId` 核对调用身份。连接使用请求对象与其生存期，未按时间、文件名或线程推定。

两次工具调用的请求对象实际复用了同一个地址，分别属于 `asyncId` 133、471，中间有原完成回调。无关读取 `asyncId` 45 无工具 ID，还与第二个工具复用了同一工作线程。原日志 `/tmp/as-node-native-request-proof.axwSXG/gdb-r3.log` SHA256 `35a1aef1a650c1adb8b36a00bca34119bd79a0f1c04c506294f02e2b9ea8698e`。该独立夹具只证明到原生打开调用，未加载 AS 驱动，也未采到原版 Gateway 内的运行时调用身份。

R37 三个实际 Gateway 工具 ID 都能在已采集的模型流原 `tool_calls[].id` 中找到相同值。此次原帧的值相同，模型协议 ID、Gateway 运行时 ID、Node `asyncId` 和 AS `EventKey` 仍各自分域保存，其他调用如有重写须取实际映射。[ID 重写边界](../backend/flow/tool-call-evidence.md#结论四openclaw已有额外的工具生命周期事实)

## R38 原版 OpenClaw 原生请求到驱动事件

原 OpenClaw 聊天用冻结 `92e47a2d` 和原版 Gateway 完成两次参数完全相同的实际 `read`，保留两个不同工具 ID 的原开始和结果。只读观测在驱动 `HELD` 时记录了原请求，事后可按同一 `uv_fs_t`、其内嵌工作对象、实际系统调用参数指针、原文件门及 `as_original` 核对到三个原 `EventKey`。原序号 244053 和 244387 来自同一原生请求、同一次系统调用的两次驱动门；原序号 247309 来自另一个请求，其原生完成没有捕获。一次原生请求可对应多个原事件。

六份回答前快照均未包含各自的完整原生链。观测端报告 tracefs 丢事件，完整链在答复后才读到。事后原生段保留为 PASS，Ask 等待期供给为 NOT_RUN。无关读取实际完成并有独立请求，但同期负对照为 NOT_RUN。原版 Gateway 的工具 ID 与原生请求之间没有取得同源标识，完整链为 NOT_RUN，关联写入为 0。

原结果 `/tmp/as-openclaw-evidence-20261110/r38-tracefs/result.json` SHA256 `d6bc1a8a74216a76ee819cc32a51fc7a7fd492dcee673cae8362bf6c2ffa98ad`，九份有摘要的原件哈希已核对。首次 eBPF 探针未附着，未发 stock 请求，保留 `/tmp/as-openclaw-evidence-20261110/r38/result.json`。结论只适用于本轮原版 Node/libuv 与驱动；生产接口仍需有等待期的可靠来源和原工具身份，不以事后记录支持当前 Ask 上下文。

## 对当前AS的收敛

### R40 隔离 Inspector 原来源验证

原版 Gateway 的第二项实际 `read` 在 Ask 回答前取得完整链。原 ToolCall `call_4c872dbb525a43a888ec7292`、response ID `20261111090502c96ba7be069b429f` 和主 isolate 的 FSREQPROMISE `asyncId=2516338` 来自同一原执行上下文。原生探针核对 FSReqBase 内嵌的 `uv_fs_t`、工作对象、系统调用和 `as_original`，对应原 EventKey `054f3140c9566e1d3960aae7433a60ca/1/49534`。回答前快照包含该链，后续原操作结果、日志、原生完成及真实析构均有原件。

第一项同路径调用有不同原 ToolCall 和 `asyncId=2130205`，其完整链在回答后才读到。原生对象地址在真实析构后被第二项复用，按各自资源 ID 和生存期区分。同期无工具上下文读取实际记录 `tool_call_id=null`，事后可核对独立请求和原 EventKey，但回答前供给仍缺。R40 整体等待期目标为 NOT_RUN，两个分项结果分别保留。

该轮未报告 tracefs 丢记录，读取原始流超过五十万条，只保留其中 39 条与目标对象相关的原件。晚消费的具体原因仍由隔离观测诊断确认。原观察器 stop、注销、FD 关闭、Inspector 关闭和自有资源退役通过，未写产品关联。

原结果 `/tmp/as-openclaw-evidence-20261111/r40/result.json` SHA256 `01a6b836fb6dbee281dbf59e2722b33ecb06ada778c1522ff4bcdf36280cda38`。十二份引用原件已核对摘要，只读重算与结果一致。

### R41 等待期完整源链验证

新的独立原版 Gateway 上，两项相同路径的原 `read` 都在 Ask 回答前取得完整源链，整体 PASS。原调用 `call_b5f52b085e924a0a9638a72b`、`call_d7885eec711e48908205758c` 分别对应原 asyncId 2129200、2496984，以及原 EventKey `8d3cd2163626134483e830d7942e105f/1/42540`、`8d3cd2163626134483e830d7942e105f/1/52435`。同一 `uv_fs_t` 地址 0x30fe9438 的第一次真实析构在第二次分配前，分别保留源身份和生存期。

第一项仍在 HELD 时，无工具上下文的只读对照创建原 asyncId 2130041，原 `tool_call_id`／`tool_name`／`response_id` 均为空，独立关联到 EventKey 序号 42622。该原 Open 完成及析构已在回答第一项之前被消费，整体 readFile Promise 当时仍为 launched、最终为 completed，两种完成分别记录。两次工具的原结果和 stock final、三个原操作回执及三条 SQLite 规则日志均按原字段核对。

该轮观察器线程 join、hook 注销、私有文件关闭、Inspector loopback 关闭、探针移除通过。底层读取 1191 条 trace 记录，保留 35 行目标对象原件，各 CPU 无丢失及剩余。两个 AS 均通过实际 RequestStop 退出 0，stock／guest／QEMU 退出 0，Native 已卸载。R40 的初始失败和等待期未取得结果继续保留。

原结果 `/tmp/as-openclaw-evidence-20261111/r41/result.json` SHA256 `4bc2bbb6a1947bf5ac910d89327b28410b3c594eee6ea861084155cbfe839d87`。结果引用的十二份原件及 `evidence.sha256` 的十八项已校验，只读重算逐值一致。该结果证明获准的隔离 Inspector 观察及原生对象采集可以在等待期间提供真实交接。生产常驻接入和中间层关系写入仍未实现，也未据此声明其他运行时或其他操作类型已支持。

### R43 纯进程外线程和内存调查

原版 Gateway 在两个真实 Ask 等待期均取得用户 syscall 参数、SP／PC、FP／LR、用户栈、内核栈及本次 `as_original` 的等待队列。两次实际执行 worker 都是 TID 178，四帧用户栈相同、inode 相同、libuv loop 相同；原请求地址分别为 0x13050138、0x12952bd8，原 asyncId 为 27260、37212，原 EventKey 序号为 41729、50175。线程和栈确认了实际执行位置，区分请求使用各自原对象及生存期。

每份定向活体读取为 246 字节。实际引用链包括 `uv_fs_t.data`／内嵌 work、FSReqBase、持久句柄、V8 Resource 对象和 Resource 内部的上下文引用；两项取得不同的上下文 Map 地址。内核栈的等待位置、真实 wait entry、TASK_INTERRUPTIBLE 与原请求核对一致。原 filename 指针在 work 与 syscall x1 之间一致，proc 所见参数前后与原寄存器一致。对应 inode 的用户 FD 当时尚未出现在 fd 表。

同期独立 C 程序的原 argv 路径指针、syscall 参数、原 file／as_original 和 EventKey 序号 41997 能贯通，在第一项仍等待时实际完成。该对照不被赋予工具身份。目标 TracerPid 和待决信号字段实际为零，未启用 Inspector、加载进程内观察代码或暂停线程。原请求、SQLite 和所有自有资源退出通过。

工具 ID 在原 stock 工具事件中存在，但从上下文 Map 到保管本次 ToolCall 的业务对象这一段尚未取得，`toolcall_to_native_origin=UNESTABLISHED`。原件 `/tmp/as-openclaw-evidence-20261111/r43/result.json` SHA256 `bbc4c14b5cdd9b550d697259443930a0383bf695ad04d1b0c4f274b30366c3ff`，十六项总清单和只读重算已核。该结果证明能够扩大到纯外部的线程、寄存器、等待对象和相关内存；运行库对象解码的通用性仍须与内核通用字段分别认定。

### R44 原待决结果对象的纯外部读取

冻结 AS 仍为 `2279afe6dc5693a8db05c4959971f8710ca48eac`，同版原始 OpenClaw 未加载 Inspector 或进程内观察器。两个实际文件 Ask 的 EventKey 序号为 46413、55784。首次读取取得 FSReqPromise 的 resolver 属性、待决 Promise 状态、实际反应项以及与资源槽相同的上下文 Map 引用。随后对原读取字节复查，两项的 Resource 对象 Map 指针均已改变。工具 ID 和原响应域未稳定取得，完整对象链的结论为 `NOT_ESTABLISHED`，不能把首次读取的片段拼成通过的一致快照，也不能由未读到字段认定应用没有保存它。

两项 guest 读取实际为 6049／5982 字节，耗时约 18.347／16.002 秒；主机快照调用均超过 12 秒并失败。第一项答复确认晚于原 Ask 期限，实际按原规则 Timeout／Deny 结束，原工具报告拒绝；第二项实际允许。原回执、SQLite、原生完成及析构分别核对，trace 未报丢失，自有 AS、Gateway 与 QEMU 均正常退役。本轮失败属于外部观察的一致性和及时交付，未据此归因于 AS 主队列丢记录，也未更改产品期限或增加放行规则。

同版独立夹具取得原工具对象、原响应对象及响应数组内同一对象的引用，并拒绝“ID 文本相同但对象不同”的对照。这只验证该冻结布局的解码能力，未替代真实样本的失败结果。报告还修正了属性键用词：[Node 源码](https://github.com/nodejs/node/blob/v24.22.1/src/node_file-inl.h#L164)使用 `promise_string()`，同版夹具实读为 V8 String `promise`，不是 Symbol；原采样标签保留，不篡改证据。原 ToolCall 对象到 EventKey 与原 HTTP ExchangeKey 的交接分别认定，本轮后者为 `NOT_OBSERVED`，同值协议记录不能补造对象边。

原结果 `/tmp/as-openclaw-evidence-20261111/r44/result.json` SHA256 `1600d5e02563212760f1dae9f75cfa4a094230e0ae53c4baabda121b7cf2b400`。十二份引用原件及十八项总清单已核对摘要，`verify-waiter.py` 只读重算逐值一致。当前未新增生产来源接口、逐 Agent 适配或关系写入。

### R45／R46 单回调记录与明确的解码缺口

R45 的独立夹具暴露属性布局误读，未提交真实 OpenClaw 请求，隔离栈已正常退役。冻结 Node 所含 [V8 属性定义](https://github.com/nodejs/node/blob/v24.22.1/deps/v8/src/objects/property-details.h)中，ConstnessField 为位 1，LocationField 为位 5，不能用常量性判断字段存放位置。R46 按该定义修正，独立夹具从同一回调的自持有记录重算得到两个原工具／响应对象关系，ID 相同的对象副本未被误认，无上下文操作保持无关系。原失败结果 `/tmp/as-openclaw-evidence-20261111/r45/result.json` SHA256 `c36ba9a1840889bb9f9ff947008ea165eed8617c524b93163f4c1c9a41f9788a` 保留，不回写历史证据。

R46 真实 stock 两项 Ask 分别按用户选择允许／拒绝，同期独立 C 操作正常完成，原回执、SQL、原生完成及隔离退役均已核实。原规则期限为 20 秒，通知到答案回执分别约 430／238 毫秒。单回调已选样本的复制与复核耗时约 9.364／8.555 毫秒，采集 6788／6433 字节，读取 fault 均为 0，回调和原生 trace 均未报告丢失。该时长从筛选后开始、发布计数前结束，不代表整个 uprobe 陷入和所有筛选的总开销。

两项都取得同一上下文值中 `toolCall` 和 `assistantMessage` 的原属性槽及对象引用，但真实 ToolCall 的 Map 字典位为 1。采集器当时只实现快速属性解码，未取得字典中的工具 ID，后续响应字段也未尝试读取。每项 `skipped=5` 和完整缺失原因均保留；本轮完整来源为 `NOT_ESTABLISHED`，HTTP ExchangeKey 交接为 `NOT_OBSERVED`，没有把部分字段升级成完整关系。此处证明的是该采集器的具体布局缺口，不能以此断言瞬时采集不可行。

原结果 `/tmp/as-openclaw-evidence-20261111/r46/result.json` SHA256 `951cfbfa8b676805db54b3c15b3d64a797cb91f79aeca44d3ace6b4391d58ef1`。十四份引用原件已核对摘要，`verify-callback.py` 只读重算一致；`dictionary-boundary.json` SHA256 `09f6c4b1d7e16ceaea742e9bd478cd5b0fe12d4364fa486115b4618fbe94d510` 从原回调记录定位了断点。采集代码只用于隔离调查，未进入产品。

### R47 冻结原版单回调可行性通过

原版 OpenClaw 与冻结 AS `2279afe6dc5693a8db05c4959971f8710ca48eac` 在同一隔离 VM 内运行，实际内核 `7.0.0-1019-nvidia`，Node `v24.22.1`。观察模块只通过 Linux uprobe 在原 Node 线程进入 `uv_fs_open` 的同一次回调内复制所需字段，未加载进程内观察器或修改目标源码。回调后的解码只读取自持有二进制原件。产物、进程实例和 guest boot_id 已逐项绑定；这不是宿主保护或其他运行时的验收。

补齐冻结 V8 NameDictionary 后，同一路径的两个真实调用分别取得上下文的原属性槽、工具对象、响应对象及响应 content 内的同一工具引用。原 ToolCall `call_b2cb6c43ac7f460fa6776300` 与 `call_cbac996dd19b4dc6ac196120` 沿各自原 `uv_fs_t`、asyncId、内嵌 work、syscall 参数指针和驱动原对象，分别关联到 `e13cc0c78acb4aef845e033739c24c63/1/42684` 与 `e13cc0c78acb4aef845e033739c24c63/1/52232`。采集时原 Promise 均待决，`has_handler=false`、`reaction_or_result=0`，尚未挂接 JS await 的反应项；来源来自该请求已保留的上下文，不制造后来的反应项。采集完成均早于原工作队列提交；时间只核对先后及耗时，不用于选择调用身份。

两份记录分别复制 7864／7868 字节，已选样本的复制与同回调复核为 12.105／13.664 毫秒，读取 fault 均为 0，已选回调和原生 trace 均无报告丢失。其余 Map 条目仍保留 `skipped=4／3` 的实际状态，不宣称解码了整个进程或 Map 的全部业务字段。时长不包含整个 uprobe 陷入和前置筛选，不构成全负载性能结论。

第一项原工具实际读到文件内容，第二项实际收到 EPERM。原规则仍为 20 秒，收到 Ask 到答案转发回执分别约 480／270 毫秒，原回执和 SQLite 与各自事件／规则／选择一致。同期独立 C 操作在第一项仍等待时正常完成，EventKey 序号为 42806，未被赋予工具身份。观察模块、AS、OpenClaw 和 QEMU 均实际退役。独立夹具还保留同 ID 对象副本不匹配、无上下文不建立关系的原件。

该冻结样本验证了“单回调采集 → 原工具／响应对象 → 原生请求 → EventKey”。模型流 SSE 中实际工具 ID／响应 ID 的值另行核对，HTTP ExchangeKey 的对象交接为 `NOT_OBSERVED`。R47 未发布生产交接。当前中间层消费入口和待接的生产来源见[底层缺口](../backend/gaps.md#精确因果来源)。跨语言／跨版本通用产品可行性为 `NOT_VERIFIED`，本次固定 Node／V8 布局用于调查。

原结果 `/tmp/as-openclaw-evidence-20261111/r47/result.json` SHA256 `179176d1df3578dcbaf5074f79cef104e3bb1a63ea493d6c0dd034492af04665`，观察模块 SHA256 `36d1e29d9811c5e7773ff7bc02f8d39ecea82e9cd22586820d73a218a9842ee3`。十四份引用原件、十九项总清单及其中十五项脚本／源码／模块清单已核对摘要，三个原应用文件与已核镜像源文件一致。`verify-callback.py` 只读重算逐值一致，关键源代码与实际引用另经主聊天复核。原 R45／R46 失败记录保持不变。

### R48 原版 Agent 的运行、会话和工具作用域

本轮以原版 OpenClaw 自身的对象和 Gateway 协议为分析范围，复用冻结 Node 24.22.1 的单回调采集。在三个会话中完成四次真实运行和五次工具调用。同一会话连续运行两次，第二次实际执行 `read` 和 `write`，写入内容经原文件核对。两个独立会话按原 lifecycle 时间重叠 20.440 秒，权限对照从原 `read` 结果返回 `EACCES`。

六份 `uv_fs_open` 自持有记录均取得模型响应及其 `content` 中实际引用的工具对象，原参数也与对应 Gateway 开始帧值一致。其中同一次 `write` 有三个不同的原生请求，访问模式依次为只读、读写、只读，三者均持有同一原工具调用。六份记录各有自己的 `asyncId`，两组重复出现的原生请求地址分别对应两个和三个不同 `asyncId`。采集时的 Promise 均为待决且 `has_handler=false`。原权限失败发生在 `access` 检查，Gateway 保存了失败结果，本轮未取得它的 `open` 回调记录。

六份记录补齐了 R47 漏采的原值。`sessionId`、`sessionKey`、`runId` 和 `agentId` 与对应 Gateway 作用域值一致。`inputId`、`transcriptInputId`、`idempotencyKey`、`lifecycleGeneration`、`databaseAgentId`、`traceId`、`spanId`、`parentSpanId`、`traceFlags` 和 `hasUnobservedAsyncToolResults` 也均有原值。`operationalRunInstance` 取得原引用，内部字段未取得。原参数、响应停止原因、时间戳和用量保留在对应工具／响应记录中。六份记录读取 fault 均为 0，已选记录无丢失，其他未解码 Frame 项保留实际状态。

同一会话的两次运行保留相同会话身份，运行和诊断标识发生变化。第二次运行的四份回调共享 `runId` 及当时的 `traceId`、`spanId`、`parentSpanId`，其中有两个不同的模型响应 ID 和工具 ID。六份记录的 `inputId` 与 `transcriptInputId` 值相同，`idempotencyKey` 与 `runId` 值不同。工具和响应的直接关系来自原对象引用。运行、会话和诊断值各自保留原 Frame 作用域，Gateway 对照只核实其原生值。此处只记录这版 OpenClaw 的实际样本。

历史原生 Gateway 帧另做了只读对照。R24、R25、R26、R28、R29、R37、R39、R41、R43、R46、R47 的 58 条工具帧涵盖 26 次调用（23 次 `read`、3 次 `exec`）。同一运行的工具开始和结果帧都携带相同原 `runId` 和 `toolCallId`，每条工具帧的 `sessionKey` 与其会话回复一致。26 次工具开始的 `seq` 都为 8，符合冻结源码的按运行计数。R29 有一项 `exec` 的退出码为 1，同时工具结果的 `isError=false`，两个原字段各自保留。该对照只用 Agent 原生帧。

原结果 `/tmp/as-openclaw-evidence-20261111/r48/result.json` SHA256 `ba718ff66a681561b8b85bab546363087d071aaee7f8db6573c16a6afb7302a1`。八份引用原件、十一项证据清单和十三项脚本清单已核对摘要，`verify-agent.py` 只读重算逐值一致，隔离栈实际退役。

### R49 权限失败前的原生 access 记录

对 R48 漏掉的权限失败阶段做了单独的真实复验。原版 OpenClaw 在 `uv_fs_access` 的单回调里保留一份自持有记录，原路径对应权限为 `000` 的合成文件，请求访问掩码为 4。通过同一 Frame 中的原对象引用，核到 `read` 工具与响应 `content` 所持的是同一个工具对象。其原参数与 Gateway 开始帧一致，同一工具的结果帧实际返回 `EACCES`。回调记录的是访问请求提交点，失败结果来自工具自己的结果帧。Node 的 `Access` 会把原路径、掩码和 `FSReqBase` 交给 `uv_fs_access`。见 [Node Access 实现](https://github.com/nodejs/node/blob/v24.22.1/src/node_file.cc#L917)和 [libuv access 实现](https://github.com/nodejs/node/blob/v24.22.1/deps/uv/src/unix/fs.c#L1679)。

这份失败记录与 R48 六份 `open` 记录共有上述十四个带原值的作用域字段，以及 `operationalRunInstance` 的原引用。R49 同次回调沿该引用进一步取得 `instanceId` 和 `runId`，其中 `runId` 与该次 Gateway 原运行值一致。七份回调均未取得 `requestId` 或 `turnId`，Gateway 的原请求 `id` 和终态回执 `turnId` 分别随协议帧保留。R49 复制 9704 字节，读取 fault 为 0，`skipped=4`，已选记录无丢失。原结果 `/tmp/as-openclaw-evidence-20261111/r49/result.json` SHA256 `d2aea7d8f738c9d4786b34ab3f7afee4120b564b566d9b9efc243cd5b9d3787b`，八份引用原件、十三项证据清单及十六项脚本清单已核对，`verify-failure.py` 只读重算逐值一致。两轮原生作用域对照见同目录 `native-scope-comparison.json`，已从两份结果逐字重算。观察器与隔离栈均实际退役。

### 原生节点交叉核查与线程对照

R48／R49 的五次运行均可通过原 Gateway 请求／回复的同一个 `id`，取得回复里的实际 `runId`。五份生命周期终态回执显式携带同一 `runId` 和对应 `sessionId`。五个样本的回执 `turnId` 都与 `runId` 同值，Gateway 请求 `id` 则各自不同。冻结版生成代码允许 `turnId` 使用原终态值，缺失时回退到 `runId`，因此保留原字段及其来源。两次 `read` 的工具结果报错，所属运行均完成最终回复，终态回执的 `successfulToolNames` 为空。运行终态和单个工具结果各有实际出处。

七份原回调与 Gateway 值核对共有 63 个字段对照。七次所见原参数中的 `path` 都与原生请求路径一致；响应 provider/model 与该运行终态回执的实际路由值一致。一次 `write` 的三份记录具有同一个工具 ID、响应 ID 和同一组用量值，各有不同的 `asyncId`。按原生打开次数统计该响应的用量会把同一组值计三次。对象归属取同回调中响应 content 持有的原工具引用；`traceId`／`spanId` 共用情况见 R48。

R47 已有的连接来源记录显示，同一冻结 OpenClaw 进程的六次模型连接由 TID 168 建立。两次工具提交及 libuv 完成回调也在 TID 168，文件操作的 libuv worker 分别为 177 和 180。连接来源只有建连现场，模型 HTTP 实际写出现场与用户态栈须另用对应回调原件核对。

另用两种语言的现成 Agent 框架做独立线程对照。框架通过真实本机 HTTP 连接收发 Chat Completions，响应由本机确定性模型夹具给出。工具的第一个动作是打开专用文件，随后打开第二个文件。`strace` 在进程外记录成功的 HTTP `POST` 写出和两次 `openat`，回包与后续请求的实际 `tool_call_id` 及工具返回值也已核对。共十次完整调用，每个配置跑两次。

- Python Pydantic AI 1.0.0 的同步工具两次均在线程池中执行，两个文件操作的 TID 都与实际 HTTP 写线程不同。异步工具在该夹具内直接读取文件，两次均与实际 HTTP 写线程同 TID。对应版本的 `FunctionSchema.call` 对协程直接等待，对同步函数调用 `run_in_executor`。见 [Pydantic AI 同步与异步执行](https://ai.pydantic.dev/dependencies/#asynchronous-vs-synchronous-dependencies)。
- Rust Rig 0.24.0 用 Rust 1.97.1 编译为 AArch64 ELF。Tokio 单线程内联读文件时，两次 HTTP 写出与工具首个、后续文件操作均同 TID。单线程运行但把第二个读取交给 `spawn_blocking` 时，工具首个动作仍同 TID，第二个读取在另一线程。相同的 Tokio 多线程内联配置，两次实际 HTTP 写出和工具首个文件操作一次同 TID、一次不同。见 [Rig 0.24.0](https://crates.io/crates/rig-core/0.24.0)和 [Tokio spawn_blocking](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)。

Python 依赖首次装配遇到 OpenTelemetry 新版移除了旧框架使用的导入，固定相容依赖后复跑，首次失败原件保留。原文件操作、HTTP 帧、固定依赖及哈希见 `/tmp/as-python-agent-thread.W4ZhjG/` 的 `python-observations.json`、`rust-observations.json` 和 `evidence.sha256`，四十三项原件及编译后二进制摘要均已核对。对照观测的是 HTTP 实际写出和工具首个可见文件动作，逻辑提交点仍以各 Agent 的原始现场为准。

### R50 原版 OpenClaw 用户态栈对照

本次唯一的原版 `read` 收到工具结果和最终响应。通过同一个原 `uv_fs_t`、内嵌 work、异步 ID 和路径，核实工具在 TID 167 提交，在 libuv worker TID 178 执行。提交栈可见 `uv_fs_open`、Node `OpenFileHandle`、V8 微任务和 libuv 事件循环，帧指针记录达到 24 帧上限，两个返回地址在保留的文件映射外。执行栈可见 `uv__fs_work → worker → start_thread → thread_start`。两段已解析的可见栈没有共同函数或 ELF 地址。

R50 的模型网络记录为 `NOT_OBSERVED`。有限请求筛选器在真实场景中未选到 TLS 或 socket 写入，计数 `tls_selected=0`、`socket_matches=0`，采集器未报丢失。回环预检中观察到的首次 `SSL_write=-1` 仅是写入尝试。原 OpenClaw 实际模型写线程、用户栈及其与工具栈是否重合仍无法判定，R47 的建连线程重合保持原证据边界。原结果 `/tmp/as-openclaw-evidence-20261111/r50/result.json` SHA256 `6f90085f63831e79e259ff1d1104ae7266315ad842dec66e85d446c75b637235`，只读重算逐字节一致，二十二项证据、三项运行二进制及十八项脚本清单均已校验，隔离栈实际退役。

### 跨语言来源与 HTTP 请求交接核查

Flow 已在收到实际 HTTP 请求后为该次 `exchange` 分配 `ExchangeKey`，请求、响应和聊天投影均沿用该键。同一入口支持 HTTP/1.1 和 HTTP/2。锁定的 Hyper 1.10.1 在交付 `Request<Incoming>` 前持有 h2 的接收流，该版本的公开 `Request` 没有交付流 ID。即使取得连接与流的原身份，仍需见到工具执行端对原 HTTP 请求的实际交接。原生执行事实和 Flow 原协议分别交给中间层，由中间层在证据成立后组合原 ChatPartRef。见 [Flow 请求入口](../../crates/backend/as-flow/src/tls.rs)、[Hyper 接收流交付](https://github.com/hyperium/hyper/blob/v1.10.1/src/proto/h2/server.rs)和[交接契约](../contracts/flow.md#模型响应与工具的事实交接)。

只读重算 R47 的十四份原件后核对实际值。两份记录的原工具／响应对象仍可精确关联各自 `EventKey`。冻结的 `openai-completions-transport.ts` 在 `request.asResponse()` 取得原 HTTP 响应，把它交给接受回调，再把响应流与单独创建的输出对象传入 `processCompletionsStream`。解析器从 `chunk.id` 填写输出响应 ID，再从工具 delta 填写工具 ID。执行上下文持有原 `assistantMessage` 与 `toolCall`，采集的响应字段含 `api`、`provider`、`model`、`responseId`、`role`、`turnId`。所读交接代码没有随输出携带原 `Response`，本轮原件也未采到原 HTTP 请求、连接内请求身份或受信交接引用。GLM 的对话补全协议分别定义任务 `id` 和请求 `request_id`。R47 核对的是前者和工具 ID，SQLite 中各命中一个 `upstream_response` exchange，序号为 1、4。两条原件保持 `http_exchange_object_handoff=NOT_OBSERVED`。结果和固定运行镜像见上节 R47 原件。见 [GLM 对话补全协议](https://docs.bigmodel.cn/api-reference/模型-api/对话补全)。

核对了 OpenTelemetry 的两个独立机制。W3C `traceparent` 由参与者传递 HTTP span 上下文，trace ID 可覆盖多次请求。GenAI 约定的工具 span 可带工具调用 ID，模型调用到工具执行的显式传播在上游仍有开放提案。冻结 OpenClaw 的 `recordToolExecutionStarted` 使用事件的诊断父上下文创建 span，并附加可选工具 ID，返回 span 上下文供诊断层使用。所读工具发布路径没有把 HTTP 原请求引用和内核原操作交接一起提供给外部读取者。启用现有导出器能取得应用声明的诊断 span，该字段集合还不足以在原 Ask 等待时建立这条因果关系。见 [W3C Trace Context](https://www.w3.org/TR/trace-context/#traceparent-header)、[GenAI 工具 span](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/model/gen-ai/spans.yaml)、[上游因果传播提案](https://github.com/open-telemetry/semantic-conventions-genai/issues/309)和[OpenClaw 导出字段](https://docs.openclaw.ai/gateway/opentelemetry/spans-and-events)。

此前核查的 Go／Python OTel SDK 通过 `context.Context`／`ContextVar` 传播已发布的 span；OTEP 4947 的线程上下文桥也有发布方参与前提。这些结论只适用于该 SDK／trace 传播机制，不能扩大为“所有跨语言旁路观察都需要SDK”。冻结 Node 工具上下文是另一个实际样本。纯系统采样可以独立取得已有线程、栈及原请求事实，不因没有应用trace或ToolCall而停止。相同参数不足以唯一确认原 HTTP 请求，但这也不否定按额外真实系统事实定义多条件模式。见 [Go SDK](https://github.com/open-telemetry/opentelemetry-go/blob/main/trace/context.go)、[Python SDK](https://github.com/open-telemetry/opentelemetry-python/blob/main/opentelemetry-api/src/opentelemetry/context/contextvars_context.py)和 [OTEP 4947](https://github.com/open-telemetry/opentelemetry-specification/blob/main/oteps/profiles/4947-thread-ctx.md)。

本段历史调查中的完整工具／响应交接仍缺来源，但不能把它设为全部原始采集的条件。已取得事实沿现有事件和消费链保留，中间层复用 `ExchangeKey`、`ChatPartRef`、原事件引用及业务库进行组合。此前在此处列出的“共享运行时或执行库发布标准上下文”方向已撤销，不能据此要求目标Agent接入SDK。当前方向以本页开头的纯旁路约束和[业务模式契约](../middleware/audit/technical.md#通用操作系统模式)为准。

本轮交付复核逐值核对 R47 与 R52 原件。R47 的两次真实调用仍为原生工具对象至各自 EventKey 的正证据，原 HTTP 对象交接仍未观察。R52 的原 Ask、真实文件读取和协议原值通过复算，后续 HTTP 的工具引用与该请求内的工具历史一致，跨来源因果桥未执行。中间层已接入现行交接消费，真实来源仍待核验。[R52 验收](backend-final-review-20261111.md#验证) · [中间层交付记录](middleware-business-store.md#工具执行交接消费)

以本机锁定的 Rust 1.89 离线复跑两个现有测试。同一 TLS 连接上的两个并发 HTTP/2 请求各自完成，并分别保留请求上下文和响应投影。业务库的 Ask 用例在没有显式关系时返回 `NotProvided`，在供给显式关系后才冻结对应交换的最近 20 条。两项通过。它们覆盖已有 HTTP 分配与关系消费，R47 的原对象交接范围以上节为准。见 [Flow 并发测试](../../crates/backend/as-flow/src/tests.rs)和[业务库 Ask 用例](../../crates/middleware/as-middleware/src/database/tests.rs)。

### R63 原 HTTP 交接离线复核

按原字节复算 R62 的 6 次交换及 1235 帧 SSE。两次原工具调用来自交换 1、4，后续请求中的实际工具调用与结果引用分别属于交换 3、6。R62 的 `ProtocolObservation` 和 `ProtocolSource` 未保存原 HTTP 头、`traceparent` 或诊断 `callId`，因此现有原件不能回读这些值。

冻结 OpenClaw 源码 `8b18b5578f66eab1501cba4cef2310d88bfb9a41` 中，模型诊断为每次调用建立子上下文并传递 `callId` 与可传播的 `traceparent`。Chat Completions 传输在 session affinity 分支用 `sessionId` 填充 `x-client-request-id`。工具端收到原 `assistantMessage` 与 `toolCall`，其诊断子上下文由工具侧 `ctx.trace` 创建。R48 回调 2–5 的原值显示，同一个 `traceId/spanId/parentSpanId` 组合覆盖两个不同原响应和两个不同工具调用，证伪了已采工具侧诊断组合作为单次模型交换的唯一身份。

R50 模型写出及原 HTTP 对象交接均为 NOT_OBSERVED。已有协议 ID 与诊断父上下文保留各自原事实，原请求引用的后续核查见下节。本轮未发起新的 OpenClaw 请求。原件 `/tmp/as-openclaw-evidence-20261112/r63-offline/result.json` SHA256 `9562b7e53b0eccf249ce26454afdbd9b34922b100e101db25ead8e777a8b1bde`，清单与只读复算已独立核对。

### R64 冻结运行版的 HTTP 请求身份核查

从 R47 的原启动盘提取并逐字节核对六个实际运行的 JS 模块。已检查的 Chat Completions 成功路径把 SDK 原请求与 HTTP `Response` 保留在传输层。响应状态和头部交给接收回调，终态向工具链交付解析后的 assistant output。工具上下文构建只接收原 `assistantMessage`、原 `toolCall` 和异步结果标记，`assistantMessage.responseId` 由模型分片 `chunk.id` 填入。该路径没有显式把 HTTP 请求、响应对象或逐请求身份交给工具执行上下文。

R47 两次同路径的原版 `read` 单回调均核实原响应与原工具对象，沿各自原生文件请求核到 EventKey 42684（Allowed）和 52232（Denied）。另从原 SQLite 逐字节复算交换 1、4 的 HTTP 请求和 SSE。这些协议值有自己的原件，原 HTTP 交接仍为 NOT_OBSERVED。补核 R48／R49 原二进制的 7 条单回调，在原工具上下文和 assistant message 中完整枚举已采的自有字符串属性，未见 `requestId`、`_request_id`、`request_id`、`headers`、`request`、`httpRequest` 或 `httpResponse`。此结论限已检查的路径及这些对象，其他来源仍需实际证据。

原 OpenClaw 聊天的 R64 证据为 `/tmp/as-openclaw-evidence-20261112/r64-offline/result.json`，SHA256 `fee18252c3540d99e4fb3f5560b95f71c988edd32607bc8db5b94694a9360367`。启动盘、运行模块、原回调和原 SQL 已独立复算。R48／R49 补核摘要见 `/tmp/as-http-identity-review.Bfsr8yMY/source-and-callback-review.json`，SHA256 `dde060d823f3c84c2471105ed23c7ce944617cba8436ff74cc6a725140baf4c8`。本轮使用原实机记录，新增 OpenClaw 请求为 NOT_RUN。

对 AS 已支持的另外两种 HTTP profile 核查同一冻结源码 `8b18b5578f66eab1501cba4cef2310d88bfb9a41`。OpenAI Responses HTTP 的 `openai-responses-client.ts` 将 `response` 交给接受回调，SSE 的 `response.created` 将模型的 `event.response.id` 填入输出。Anthropic Messages 的 `anthropic-transport-stream.ts` 分开处理 HTTP 响应元数据与事件流，`anthropic-stream-reducer.ts` 从 `message_start.message.id` 填入输出。两者的 `providerReplay` 保存继续请求的 provider／会话作用域和压缩检查点。所查成功路径均未找到把逐请求 HTTP 身份交给工具上下文的来源。这是源码核查，两种 profile 的单回调实测均为 NOT_RUN。

### 模型与工具事件的交接补核

同一冻结源码的 `attempt.model-diagnostic-lifecycle.ts` 在模型端为一次逻辑调用生成 `callId`，HTTP 接受回调可因重试收到多个响应。该版本的 `upstreamRequestIdHash` 由错误分支生成。成功的模型流经 `finalizeTransportStream` 交付 `AssistantMessage`，`EventStream.result()` 保管该值。流式提前执行工具时，`agent-stream-response.ts` 将已提交的消息片段交给工具批次。上述交付点的字段没有携带原 SDK HTTP 请求引用。

`hook-types.ts` 的 `before_tool_call` 事件和上下文提供 `runId`、`toolCallId` 及相应工具信息，没有模型侧 `callId` 或逐请求 HTTP 引用。外部 harness 的 `native-hook-relay-codec.ts` 归一化 `turnId`、`toolUseId` 等执行端字段，仍缺原 HTTP 请求交接。OpenTelemetry 的精确模型调用到工具执行链接仍见上游提案。这些字段各自可保留为原事实，跨侧关系仍需核实引用作用域。本次仅核对冻结源码、公开字段和 R63／R64 既有证据。[OpenClaw 模型与工具钩子](https://docs.openclaw.ai/plugins/hooks/reference) · [OpenClaw 模型观测单位](https://docs.openclaw.ai/gateway/opentelemetry/model-calls-and-metrics) · [GenAI 因果链接提案](https://github.com/open-telemetry/semantic-conventions-genai/issues/309)

独立审核补核了应用内事件。冻结版 `turn_end` 同时携带助手消息和工具结果，`tool_execution_end` 从同一助手对象生成 `assistantTurnId`，值为 provider 的响应 ID 或本地 turn ID。该应用事件在工具结束时交付，字段没有原 HTTP 请求引用或 EventKey。独立审核者重算 R47、R63、R64，结果均与冻结原件逐字节一致。

### R66 原版 OpenClaw Responses HTTP 对照

原版 OpenClaw 使用隔离 guest 内的本地 Responses HTTPS 夹具。两次独立会话对同一路径执行 `read`，夹具复用工具 ID，分别提供实际的逐 HTTP 请求 ID。八次真实 HTTP 交换的请求与 SSE 在 AS 原库中各唯一匹配。两次工具响应分别位于交换 2 和 6，属于各自的原协议事实。

工具到内核的原生交接为 PASS。两次测试会话分别关联 Allow 原事件 44703 和 Deny 原事件 55779。同期原版 OpenClaw 心跳也发起同路径、同工具 ID 的调用，其回调记录的原运行和会话与两次测试不同，单列 EventKey 44434。三个单回调都核实原工具对象、原助手对象和实际原生请求。三个回调未采到逐 HTTP 请求身份，完整 HTTP→ToolCall→EventKey 交接为 NOT_OBSERVED。真实外部模型与生产正向验收为 NOT_RUN。观察器、OpenClaw、AS、夹具及 guest 正常退役。

原件 `/tmp/as-openclaw-evidence-20261112/r66-live/result.json` SHA256 `f6257659bbab4f2a35da0493910b5e06220f2938c76d14af587d0529c3c27a22`。已独立复算最终结果并核对 33 份证据、21 项脚本及 8 份冻结运行模块。首轮观察器路径不匹配与后续 HTTP 请求目标解析失败分别保留在 `r66` 与 `r66-fixed`。恢复验证用原 HTTP 数据复核夹具解析，使用新会话及新测试文件。

### 原生事实供给与业务绑定的根因修正

对比 R41、R47 原件和提交 `cfb1b171f`，以前通过的是原工具到原生请求及 EventKey 的交接，隔离采集尚未作为正式服务来源接入。之后工作树新增的入口要求底层先给出 Flow 派生的 ChatPartRef，又把原 HTTP 请求身份进入工具回调设为所有事实发布的前提。这样把原生事实与业务组合绑到一起，挡住了已能表达的原生段。

现行代码以 ToolExecutionFacts 独立接纳原生段，中间层保存原字段后再处理协议绑定。R66 夹具的发行代码为每次原请求唯一生成响应 ID，原服务请求／SSE 字节已与 Flow ExchangeKey 核对。冻结 OpenClaw 将该原响应 ID 搬入助手对象，单回调核实该助手对象持有原 ToolCall。该受控发行者范围内可用明确响应引用完成组合，SDK 请求对象仍保持原实验的 NOT_OBSERVED。

当前 Rust 消费链在复制的 R66 原库中重放心跳 44434／交换 1、Allow 44703／交换 2、Deny 55779／交换 6。三个原生事实在无 HTTP 绑定时可查，重复投递和重开保真通过。三条受控绑定通过；轮换错误交换在绑定事务中返回 Conflict 并回滚，随后正确绑定可成功。输入及取值依据在 `/tmp/as-r66-replay-input.ftO3cHwy`，当前代码重放在 `/tmp/as-native-facts.yl1WqkA4/replay` 的 `delivery-correct-result.json` 与 `delivery-wrong-result.json`。

独立交叉审核及 188 项工作区回归通过。缺少原生事实作为来源缺口处理，冲突原生事实逐条保存，两个独立有效的竞争声明均保留，查询和反馈领取拒绝择一。现有三个 provider 的响应标识按其真实命名空间校验。

冻结候选 `/tmp/as-native-facts.yl1WqkA4/delivery` 的 165 项源码及 20 项产物已核对，镜像 SHA256 `8f0fee03c9cc62b35898b06dd939d537a0cc44e540cd7957f93083eafadce541`。真实隔离内核的正式 WS 九项冒烟通过，服务、guest、QEMU 退出 0，Native 卸载。报告 `delivery-run/report.json` SHA256 `c12d79ef10f1677bd2276e0a0036777233b5e75431e93ea128823756714f9b58`。本轮原生来源验收是冻结 OpenClaw 原件的离线重放，通用持续采集生产者仍待接入。

原 OpenClaw 聊天独立从 R66 原库副本重新编译并复现重放。无绑定副本重开后有三条原生事实和零条 ToolExecution 协议关系。全部六种跨交换错绑均返回 Conflict，错误关系和数据库写失败计数均未增加，随后三条正确绑定成功。正式 Activity 查询与 SQLite 原行逐值相同。独立报告 `/tmp/as-r66-independent.Mt48lS4j/result.json` SHA256 `2b2d3b6aa72ead0ff16e0c772ec1abb1eac08c0cec4969deb3dba553a468e2c6`，26 项证据及只读复算已核。该验收从 MainMessage 接纳开始，持续采集及自动发布仍为 NOT_RUN。

### 正式候选查询与 R67 原件复算

本轮在原 `AskContext` 接入可选匹配条件。同一 SQLite 事务读取原 Ask、实际候选及各匹配交换最近20条消息，正文只进入原 `source_records`，查询不创建确认关系。交叉审核修正两项：身份来源缺失不再遮蔽原 Ask；已有确认窗口的同交换复用原冻结修订，其他消息继续 Streaming 更新不会制造假冲突。定向回归覆盖真实 Delta 后的再次查询，原冻结消息逐值不变。

最终候选 `/tmp/as-tool-attribution.oYwjHEYA/final-delivery`，镜像 SHA256 `aa02a86f56d0d461a412af45976e4e6c4cff0b895e09d0cf415b45d226325eda`。167项源码／20项产物清单 SHA256 `3099894ecbcf6f9a355c3c02681894ce228492d747e67c54946d4ac4cc2085c1`。196项工作区回归、严格Clippy、LSP、交叉审核通过；最终同产物隔离内核和正式WS十项通过，报告 `/tmp/as-tool-attribution.oYwjHEYA/final-delivery-run/report.json`，SHA256 `930c66d1278a5f37206ae5a660826ef2d2d28ad7e7caee87e08078139c62d9bf`。其中原工具响应→Open Ask→候选查询→候选原消息窗口→Allow→实际打开与原回执完成，消息未重复或改写。服务、guest及QEMU正常退出，Native卸载。原OpenClaw聊天的R68真实复验已通过，详情如下。


正式 `ToolAttribution` 已经通过同一候选的隔离内核和 WS 验证。既有 Responses 夹具发出真实工具调用，执行方进入原文件 Open 问询，查询返回原工具位置、消息修订及目标比较结果，允许后文件实际打开。195 项 Rust 测试、严格 Clippy 和 LSP 通过。隔离流程 10 项通过，服务正常退出且模块卸载。候选镜像 SHA256 `33ba5215ec39046ba035912a119e1b2fa959badf05df47bedc6949659ac07650`，报告 `/tmp/as-tool-attribution.oYwjHEYA/delivery-run/report.json`，SHA256 `381acd36dd70313ef931e3dc1e7290187690084ac1801a326cbca518afc85332`。

R67 原版 OpenClaw 的两条文件原件在当前 Rust 查询上复算。使用原运行已记录的 guest Hello 启动 UUID，仅在重放副本中提供该启动范围。原库、原 WS 帧和原报告保持原样。原事件 40857 和 46689 的同进程与目标条件分别匹配原交换 1 和 4。同线程条件均无候选。结果 `/tmp/as-tool-attribution.oYwjHEYA/r67-replay.log`。R37 的重叠同路径调用和 R66 的同工具 ID 反例继续约束查询保留多候选。当前正式生产者尚未持续提供 R47／R66 实验中的原生请求交接，候选结果不产生确认关系。

### 原版 OpenClaw R68：正式候选与 Ask 窗口

原验收聊天在本机隔离客体和回环 Responses 端点，使用上述最终冻结候选完成两条新会话。两次原工具均读取同一个既有文件。Allow 的 EventKey 序号43261，在原等待期间按同进程、300秒BOOTTIME回看及原path参数匹配到交换1的一个候选；Deny 的48967按同一条件和每页1条读取，保留交换1、4的两个同路径候选。没有选择最近或唯一候选建立确认关系。

三个候选窗口各有4条原消息；原修订、正文、参数与SQLite及原聊天帧逐值一致，一条正文在同一回复只出现一次。原连接TID161，文件操作TID172/171，SameThread查询均为空且原线程出生时间保留。省略归因及参数字段缺失均保留原Ask，trigger_call与确定window保持为空，库内ToolExecution关系为0。

11组正式WS查询都在原答复前完成；查询前后独立proc syscall相同。Allow原openat返回FD199，Deny返回EPERM；实际工具效果、原回执及安全日志通过。127条原聊天投递对账，末尾无交换的source_closed原样入库，候选窗口无源Gap。服务、OpenClaw、guest、QEMU正常退役，当前Native模块卸载。原聊天另独立重编17项数据库回归，包含20条上限、缺失线程来源以及同exchange复用冻结窗口；最后一项在该原版现场未执行，因为没有生成已确认关系。外部模型和宿主防护未执行。

原报告 `/tmp/as-openclaw-evidence-20261112/r68/result.json`，SHA256 `081ac98a1944fa48e67cacd7346ed2c4838bc0ccf7f0d81b1588c38233d79c19`。40份证据和20项脚本摘要已独立核对；原聊天只读复算通过，入口 `r68/recheck.sh`。本轮交付范围为正式多条件候选与Ask原消息上下文，未宣称通用原请求因果交接已经取得。

### 现有原件的行为链接入（离线）

按用户本轮要求，只使用已有原件，不重跑OpenClaw。正式 `ActivityContext` 以驱动 `EventKey` 为锚，同一事务返回原 `ContextView` 与独立 `ToolAttribution`。它属于单个Agent内部的因果审计；Group的Agent间委派关系仍需自己的真实来源，两者不合并。

离线工具复用R66原native事实/绑定来源及R66、R68原消息记录，按当前唯一schema新建回放库，未修改原库或回填缺失字段。R66输入来源摘要以及两轮全部evidence/scripts摘要均重新核对。已有 `record_chat`、`record_up`、`record_tool_execution_facts`、`bind_tool_execution` 和正式 `activity_context` 共同完成接线，不新建事实模型。 这是当时回放使用的接口；当前独立工具消息/直写writer已退役，软件回归也通过原Submission＋实际交接和origin引用供给，不保留生产旧入口。

- R66的原hook 44434、44703、55779各取得一条已证明的ToolExecution、一条QuestionCompletion和三条SameExchange关系；原对象交接及协议作用域沿既有验证。
- R68的原hook 43261、48967保留各自QuestionCompletion，分别保留一个、两个候选，不产生ToolExecution。SameThread仍无候选，保留真实线程差异。
- 原来源内的EventKey序号只表达驱动记录分配顺序，输出为44434→44703→55779及43261→48967，不声明跨来源先后或把记录顺序升级为因果。原文件/行号同时保留。
- 两个新回放库实际关闭重开后，图内原事实、已确认关系及候选逐值一致，写入错误计数为0；瞬时数值仅属于原回调记录，回放不会重新取得原对象。

结果 `/tmp/as-callback-facts.ExwNZJ71/offline-graphs-final/graphs.json`，SHA256 `7bd489d4c19dc9a805693a9fbeffb3857ef20d9e333d87c5eb1a4252a9782279`。回放源码 `/tmp/as-callback-facts.ExwNZJ71/replay/src/main.rs`，运行日志 `replay-final.log`。这是3条已证交接和2个候选视图的离线接入验证，不是新的持续采集或跨Agent调用验收。

本轮查询接线通过中间层45项回归、严格Clippy、LSP及交叉审核。最终源码/产物冻结在 `/tmp/as-callback-facts.ExwNZJ71/final-delivery`，168源码/20产物清单SHA256 `7b895fe0b2701c3d6bf038f946784ce04a1665c945391943be6bd8bb72484d36`，镜像SHA256 `81a2608b6630053380a646ffc976d3a938131d1bcd542b7f49b478f8445b7f2e`。该最终业务查询使用原件离线验证，未新增OpenClaw运行；驱动采集的先前同轮guest验证范围见[现场采集证据](execution-site-capture.md#公共线程事实与bpf用户栈)。

### 已退役的隔离试验来源代码

该试验代码已从活动源码移除，原文件保存在提交 `3867c6e70dd7b1ac38c007e43d738483251ffd82` 的 `scripts/native-smoke/observe-tool-requests.mjs`，SHA256 `45d6ad9eb8a08324d5c59d615cb6cf0962c78b577ae9b2115b50da7735a8cd63`。它校验原执行上下文模块摘要，在 Node `FSREQPROMISE` 创建时读取原异步上下文；每个 isolate 只允许一个观察者，停止时取消 hook、关闭私有证据文件并释放注册。以下结构和验证只描述该历史试验。

私有试验记录中的 `namespace_pid` 为目标 Node 的 `process.pid`，`node_thread_id` 为 Node isolate 的 worker 标识，两者均不替代驱动身份。`async_id`、`trigger_async_id` 是该 isolate 原始异步资源 ID 的十进制字符串。`tool_call_id`、`tool_name`、`response_id`、`assistant_turn_id` 直接取原执行上下文，缺失为 null；`sequence` 只计该观察者完整写出的记录。记录不包含原文件路径、工具参数或另造的 EventKey。

同版 Node 和原 `read` bundle 的独立验证取得 13 条资源记录，两个同路径夹具调用保留各自 ID，无关读取的 ID 为空；重复注册被拒绝，停止后无活动 hook 或注册引用。结果 `/tmp/as-inspector-source-proof.CyPtvH2n/result.json` SHA256 `046da3b6d20a4858c9ce48ddeb8f3b2390c00b95e39a79f0b34d2c4b5994840d`。该试验没有原版 Gateway 的驱动事件。成熟运行时接入的依据见 [OpenTelemetry Node 实现](https://github.com/open-telemetry/opentelemetry-ebpf-instrumentation/blob/main/devdocs/runtimes/nodejs.md)，AS 的完整链仍须自己的实测。

[公共事件](../contracts/events.md)、[进程树](../backend/process-tree/technical.md)和[Flow事实](../contracts/flow.md)各归原模块。Main继续组合接口；中间层仅从真实来源建立关系。缺工具桥时固定反馈不准备，不另建模糊概率边、最新版trace或通用关联Owner。

此前 Flow 采集预算修复的原件见[修复证据](capture-admission.md)。来源接入须逐项明确原始字段、生产者和现有消费者；尚未明确的产品接口保留在[缺口存档](../archive/unresolved-module-contracts.md)。

## 原文件打开返回与 Group 回执候选

2026-11-12 首轮隔离复验中，文件问询、答复及实际效果均通过，但内部 `do_sys_openat2` 探点没有为两次原 `openat` 提供返回观察。改用原生 syscall 包装函数的入口和退出，并在同一在途调用中记录实际接纳的 File.Open 等待。最终候选在 aarch64 7.0.0-1019-nvidia 中通过原生构建、WS、SQLite 及退役复验。EventKey 序号 130 的等待编号为 3，原返回描述符为 3。序号 146 的等待编号为 4，原返回为 -1，对应原操作的 EPERM。两条记录均使用原 `DecisionId`。工作区 200 项测试和严格 Clippy 通过。

驱动 FIFO、主模块消息和业务库继续使用原消费路径。等待及返回作为[原事实](../contracts/events.md#原生系统调用请求返回)独立查询。旧的事件和工具关联保持原证据。用户空间同步对象、io_uring 原请求及其他调用族仍各需真实来源。

最终候选位于 `/tmp/as-open-handoff.HimGA5Eb/final-delivery`。168 源码／20 产物清单 SHA256 为 `3e7de88baa9d00216c991703b37f8d3dbfa0f7931ae13096081227aa4de38ca8`。真实客体报告位于同目录父级的 `final-guest/report.json`，SHA256 为 `0bb9a220c4d9e45ed80ad0568a2851eaa9c6d727ebdb49d53ad11f6c7a16c0b9`。原内部探点的失败记录保留于 `/tmp/as-open-handoff.ctMPLIQK/guest`。

Group 的现有离线分析接入了[结构化委派回执候选](../middleware/agent-graph/technical.md#委派回执候选)。同一采集源内的子运行和会话均与接纳回执相符时，输出原运行引用。夹具覆盖匹配、会话冲突、错误工具 ID 和正文 JSON。R48／R49 原件重新校验摘要并回放，保留 5 个运行、6 次工具调用和 7 个已关联回调。两份原件没有委派候选，跨 Agent 的真实确认边仍待来源。[OpenClaw 会话工具](https://docs.openclaw.ai/session-tool)

## 应用 futex 队列交接可观测性

2026-11-12 在隔离 aarch64 7.0.0-1019-nvidia 中，用 tracefs 的 BTF fprobe 同步读取 `futex_do_wait` 与 `futex_wake_mark` 的原 `q` 和 `q->task->pid`。实际等待方为线程 80 和 81，线程 79 对独立的空 futex 唤醒返回 0，然后对两个原等待对象各唤醒一次。等待回调与选择回调的队列条目地址和线程在各自生命周期内完全相同，两个对象各自独立。两次原等待实际返回 0。首次夹具用追加模式打开 tracefs 失败，改用直接写入后成功。[Linux fprobe 的 BTF 参数](https://docs.kernel.org/trace/fprobetrace.html) · [Linux futex 原队列与选择](https://github.com/torvalds/linux/blob/v7.0/kernel/futex/waitwake.c)

这份证据确认所测 private futex 的实际等待条目与唤醒选择交接。原地址仅在单次隔离取证范围内核对。采集报告位于 `/tmp/as-request-families.lUkaQ15B/futex/result.json`，SHA256 `5f3d83700a872e2eddef409c6723c8180795177b75946b95cc6d372b1a4909bb`，同目录保留取证程序和原 trace。正式接入见下一节。

## Private futex 正式采集和业务库复验

2026-11-12 在同版 aarch64 7.0.0-1019-nvidia 客体使用冻结的正式驱动及 as-service。四次等待均实际返回 0。服务初始化前就进入等待的线程 100／101 在初始化后被线程 99 选中，EventKey 序号 18／19 的实际选择均保留为空等待证据的候选。初始化后线程 117 选择线程 118／119 的两个原条目，EventKey 序号 57／58 分别保留独立等待编号 1／2。空 futex 唤醒实际返回 0。两类观察均通过 SQLite 与现有鉴权 WS `ActivityContext` 对账，线程出生纳秒与原活线程样本一致，没有产生工具或 Agent 边。

基础客体十项通过，两个客体均实际停止服务、卸载模块并退出。范围为原生 WAIT_PRIVATE／WAKE_PRIVATE，真实恢复调度、其他同步实现与工具归属仍使用独立来源。源码与产物候选位于 `/tmp/as-futex-delivery.sCauDi8d/final-delivery`，170 项源码及 20 项产物清单 SHA256 为 `2cbda86dec879a18b26f09b11bd6c88cec76aa8fbc896831c10928057aa66402`。

专项报告为候选父目录的 `final-futex-guest/report.json`，SHA256 `8ae0dd75f624ec0eccdea328cf44ac19cec0fb7b3954ca064b15364a38b506d8`。基础报告为 `final-base-guest/report.json`，SHA256 `cf21e994417cc20628179227631d507ecbdfef9c20df11365037eaecf75d4e90`。两个镜像的正式产物与冻结清单逐项相符，专项镜像复用当前测试辅助代码并替换测试入口。工作区 200 项测试、严格 Clippy、LSP 与格式检查通过。

## Group 原版委派来源核查

从 R68 已封存的 OpenClaw 2026.10.9 运行镜像读取实际发布的会话工具、登记 codec 和审计 writer。登记的 SQL 行保留原 `run_id`、`child_session_key`、`requester_session_key`。原生会话工具在登记时可供给 `childAgentId`、`requesterAgentId` 和 `requesterTurnRunId`，执行和投递状态分别保存在结构化载荷。发布代码的 `adoptSubagentRunForRequesterTurnInRuns` 会给已接纳的 steering 更新请求方轮次，证实该字段可变。

会话工具动作经 `recordAdmittedSessionDecision` 使用实际调用方执行令牌，写入时保留原运行／执行身份并将目标会话投影为不透明引用。采集按源保留这些值。可变登记和该引用均需与原始调用回执及子运行原证据核对。R68 原状态库的 `subagent_runs` 表实际为 0 行，`execution_decision_facts` 未建立。R48／R49 原件回放仍保留 7 条已关联回调和 0 条委派候选。来源核查报告位于 `/tmp/as-futex-delivery.sCauDi8d/group-stock/r68-group-source.json`，SHA256 为 `4d0c1002e3e71f1b8be86f40a12418e6819897bb1cc1fab0386b62407104153a`。下一份正例需在同一授权采集范围取得明确的两端 Agent 身份、原委派接纳和实际子运行终态。

## 跨 Agent 调用的观测来源

- **原编排事件。** 本机 Codex CLI 0.167.2 生成的 App Server 类型包含 `collabAgentToolCall`，其中 `id` 定位原调用，`senderThreadId` 与 `receiverThreadIds` 定位逻辑会话，`status` 表示该次协作工具调用的状态。子 Agent 任务的终态须看子会话。App Server 的 stdio、Unix socket 和 TCP WebSocket 是客户端到服务的传输方式。Python Google ADK 的 `AgentTool` 可在同进程调用目标 Agent 的 `Runner.run_async`。两例中的会话或 Agent 标识均来自编排层。[Codex App Server](https://learn.chatgpt.com/docs/app-server) · [Google ADK 源码](https://github.com/google/adk-python/blob/main/src/google/adk/tools/agent_tool.py)
- **模型聊天。** 模型给出的结构化工具调用及 Agent 回送的工具结果可由原协议的调用引用配对。例如 Claude 用 `tool_use.id` 和 `tool_result.tool_use_id`。若原结果明确携带目标身份及任务回执，可再与实际子运行核对。仅观察到另一次模型请求、提示文本或工具调用意图时，缺少委派接纳与子运行证据。该入口可以复用现有 Flow 和 `ChatPartRef`，来源范围限于已采集的模型流量。[Claude 工具结果协议](https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls) · [现有因果审计](../middleware/audit/technical.md#复用的实体与关系)
- **本地命令与管道。** Pi 的 subagent 示例逐次 `spawn` 独立 `pi` 进程，使用 `--mode json -p`，从 stdout 管道消费 JSON 事件并保留进程退出结果。驱动可见原启动与实际进程身份。管道对象可证明传输范围，逻辑任务及状态使用原结构化记录。持续复用子进程时，每次任务由协议自身的身份区分。[Pi 示例源码](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/examples/extensions/subagent/index.ts)
- **文件消息。** Claude Code Agent Teams 把消息写到接收方的 JSON inbox。成功写入表示发送方已经投递，接收方读取后还要逐条校验。原 `openat`／`read`／`write` 可以定位文件访问，确认投递、接收和任务完成须分别核对原消息和运行回执。[Claude Code Agent Teams](https://code.claude.com/docs/en/agent-teams)
- **远程调用。** A2A 协议提供原消息、任务及任务终态。TCP 连接只证明传输端点，任务关系使用原协议标识及回执。[A2A 规范](https://a2a-protocol.org/latest/specification/#311-send-message)

上述条目来自发布源码、协议和本机生成类型核查。Group 的接入判定以[现有技术约定](../middleware/agent-graph/technical.md)为准。对已有 TLS 流量，结构化工具结果是可直接复用的候选来源。原编排、进程、IPC 或文件来源只在取得真实两端身份和同一次交接时补充确认关系。

## Pi 跨 Agent 委派实测

2026-11-12 在 aarch64 `7.0.0-1019-nvidia` 客体运行官方 Pi 1.5.2 和随包委派扩展，沿正式 AS Native、受控 TLS、幻影凭证及业务库完成一次真实委派。上游使用确定性 HTTPS 夹具。试验用原执行问询在子进程连接前按真实 ProcessKey 添加凭证授权；只读复算使用实际 syscall 记录和应用原输出。服务停止、Native 卸载、客体及 QEMU 退出均通过。[Pi 委派源码](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/examples/extensions/subagent/index.ts)

- 父会话的原 `tool_execution_start` 和 `tool_execution_end` 保留同一个 `call_pi_91f094f23e6b48a2a79c`。实际子进程 171 完成 `execve`，输出独立的会话记录、`agent_start` 和 `agent_end`，父进程 127 实际 `wait4` 回收退出码 0。独立的接纳回执未出现，实际启动由子会话原件证明。
- 父进程创建的 Unix `socketpair` 原对象 999／1000 贯通子 stdout 和父读取。按 trace 重组子端实际写出的 11,301 字节，父端成功读取总量相同。子 `message_end` 的完整 assistant 消息与父原工具结果 `details.results[].messages` 相等，其中的原 `responseId` 与 AS 库中 Exchange 2 的上游响应相同。
- 正式库以原 ProcessKey 将 Exchange 1／3 归父、Exchange 2 归子。Exchange 3 的原工具结果携带同一 `tool_call_id`，已产生该交换内的 `native_tool_reference`。父发往模型的结果正文没有子会话或子响应标识。实验原件足以证明本次委派；正式 TLS 和进程事实缺少父工具到子会话的原交接，只保留 Group 候选。正式接入仍需可共用的结构化交接来源。

复算摘要位于 `/tmp/as-pi-group-20261112/original-r4/verified.json`，SHA256 `8d12b819861e82ec1a2a715668f397a99fc21d0bbf84b1b29df4e2b2a2753e8e`。同目录保留 TLS 上游原件、SQLite、两端 stdout 及 syscall trace。工作区以外的试验文件、二进制和原件清单分别见同一试验根目录的 `r4-inputs.sha256` 和 `r4-evidence.sha256`。

### 并发委派与通用旁路的证据边界

同版本、同正式采集链在隔离客体执行两个同时在途的原工具调用。两次委派参数和模型结果正文相同，`tool_execution_end` 的顺序与开始顺序相反。两个子进程及四个 TLS Exchange 各自保留原 ProcessKey。两个子会话均有原 `agent_start`、`agent_end`，父进程分别实际回收退出码 0。服务停止、Native 卸载和 QEMU 退出均通过。

通用 `strace` 复算不解析应用载荷。每条传输沿父 `socketpair`、实际子进程的 `dup3` 到 stdout、原端点读写和 `wait4` 核对。两条 stdout 均重组 11,301 字节，父端各自读完。父在两次 `socketpair` 中获得过相同的对端 FD 数字 24，实际对象分别为 2566 和 2596。独立子会话身份可在各自的原输出内观察。取证只覆盖本次已跟踪的进程及其原端点。[Linux socketpair](https://man7.org/linux/man-pages/man2/socketpair.2.html) · [strace 原参数与数据记录](https://man7.org/linux/man-pages/man1/strace.1.html)

对照 Pi 的原生结构化工具结束记录可逐条核对父 `toolCallId`、结果内的子消息 `responseId`、实际子 stdout 消息及 TLS 记录。两条子流、子启动参数和子 TLS 请求都未携父工具 ID，父的 TLS 工具结果也未携子会话或子响应 ID。通用传输及 TLS 事实在允许的身份约束下保留四条候选配对，只有读懂 Pi 原记录的字段归属才能在本样本确认两条边。正式 Group 继续等待可共用的结构化交接，未采用按字段名扫任意 JSON、按先后或按文本连接的做法。

复算摘要位于 `/tmp/as-pi-group-20261112/r5/original/verified.json`，SHA256 `8605b93e8a555e81051703c78bbfad44de40937baaac5cc5944de6e85555f993`。同一试验目录保留通用原件复算、Pi 样本核对及 `evidence.sha256`、`inputs.sha256`。

## ACP 与 A2A 的协议交接

- **ACP。** OpenCode 1.18.35 在原标准输入输出上用 ACP SDK 的 `ndJsonStream` 和 `AgentSideConnection`。客户端 `session/new` 的 JSON-RPC 回执返回子 `sessionId`，`session/prompt` 引用该 ID，`session/update` 保留该会话身份，最终答复以同一连接和原 RPC ID 归到请求。ACP 的 `toolCallId` 属于该 Agent 会话中被报告的工具。外层模型工具调用仍需要明确交接。[OpenCode 入口](https://github.com/anomalyco/opencode/blob/v1.18.35/packages/opencode/src/cli/cmd/acp.ts) · [ACP 会话](https://agentclientprotocol.com/protocol/v1/session-setup) · [ACP 单轮与终态](https://agentclientprotocol.com/protocol/v1/prompt-turn) · [ACP 工具 ID](https://agentclientprotocol.com/protocol/v1/tool-calls)
- **A2A。** Python ADK `RemoteA2aAgent` 为发送的 Message 新建 `messageId`，按原上下文传 `contextId` 或可继续的 `taskId`。A2A Python SDK 的 JSON-RPC 传输另建 RPC ID。返回的 `Task.id` 标识任务，`contextId` 标识可含多个任务的会话上下文。上述身份可以在原协议域内关联；外层模型工具 ID 只有在实际生产者明确传递时才可用于跨域确认。本轮核查了该实现与协议。[ADK 发送实现](https://github.com/google/adk-python/blob/main/src/google/adk/a2a/agent/_remote_a2a_agent.py#L1890-L1952) · [A2A Python JSON-RPC 实现](https://github.com/a2aproject/a2a-python/blob/main/src/a2a/client/transports/jsonrpc.py#L64-L99) · [A2A 规范](https://a2a-protocol.org/latest/specification/)

### 原版 OpenCode ACP 并发实测

2026-11-12 在隔离 aarch64 `7.0.0-1019-nvidia` 客体，原版 Pi 1.5.2 通过两次参数相同的内建 `bash` 工具调用，并发启动 acpx 0.22.0 与 OpenCode 1.18.35。上游使用确定性 HTTPS 夹具，模型请求走现有 AS TLS 与凭证链。按原进程归属和实际 stdio `socketpair` 重组 ACP JSON-RPC，核对两个子进程 219／224 的执行回执和原连接。两条连接分别返回不同 `sessionId`，均复用本连接中的 `session/new` ID `1` 和 `session/prompt` ID `2`。两次都收到带各自会话 ID 的原输出和 `end_turn` 回执，输出正文相同。原 Pi 也完成两次工具回送和续轮，外部实际退出码为 0。

用 acpx 正常选项 `--format json --json-strict` 时，父模型的两份工具结果正文包含该客户端输出的 ACP 记录。对照各自原 socket 流和实际客户端 stdout，逐条核对两组原请求、子 `sessionId` 和结果，能复算出本样本的两条实验配对。外层原工具 ID 由 AS 首个 TLS 交换和父工具回送共同核对，在两条原 ACP 请求及回执中均未出现。ACP 原流可确认协议内部关系，CLI 输出的工具正文不足以成为通用的源级交接。正式 Group 保留候选，未接入按工具正文识别的路径。

实际经过受控 TLS 的上游请求为六次，其中两次是 OpenCode 标题生成。首轮试验包装器固定预期四次，因而记录失败并跳过 SQLite 快照导出。补验分别保留主任务与标题生成，停止服务后从原 SQLite 按六个原 `responseId` 逐条核对 `ExchangeKey` 和完整 `ProcessKey`。两次原主任务分别归实际 OpenCode 子进程 219／235，父首次请求及续轮归原父进程。两条 ACP 都再次收到 `end_turn`，父工具回显与实际 ACP 客户端 stdout 逐条一致。补验客体退出 0、Native 卸载及 QEMU 正常退出。

补验摘要位于 `/tmp/as-acp-group-20261112/r6/original/verified.json`，SHA256 `e67c6cc6a5feccfe4fe3ad8becedaff313f46b82676c996ae18e65ea52a17ca4`。同一试验目录保留 `evidence.sha256`、`inputs.sha256`、SQLite 和两条原 ACP 流。首轮失败报告及复算仍保存在同根目录的 `r5`。

### 原版 Docker Agent A2A 并发实测

2026-11-12 在同内核的隔离客体运行原版 Pi 1.5.2、官方 A2A CLI 0.4.0 和 Docker Agent 1.160.0。Agent Card 的原接口为 A2A 1.0 JSON-RPC。Pi 通过两次参数相同的原 `bash` 工具并发调用同一个 A2A 服务。两个主任务同时进入确定性 HTTPS 模型夹具，模型输出正文相同；模型请求走现有 AS TLS、幻影凭证和 SQLite。`tcpdump` 只读捕获回环 A2A 原流，内核丢包计数为 0。[Docker Agent A2A 接口](https://docs.docker.com/ai/docker-agent/features/a2a/) · [官方 A2A CLI](https://github.com/a2aproject/a2a-cli/tree/v0.4.0)

- 两条原连接各有 `SendMessage`，各自的原 JSON-RPC `id` 在回执中逐值相符。两条原 `messageId` 分别在各自任务历史中出现，两个 `Task.id` 和 `contextId` 均不同。任务的 `contextId` 与返回元数据 `adk_session_id` 相同。两条原任务终态均为 `TASK_STATE_COMPLETED`，从各自原 artifact 取得实际结果。
- 两次原 CLI 输出与实际 A2A 回执完整相符。父端 `tool_call_id` 及其回送的工具正文在原 TLS 记录中可核对，实验回放可逐条对上两次 CLI 的任务结果。两条 A2A 原请求及回执均未传父工具 ID。CLI JSON 正文回显仍属于实验对照来源，正式 Group 保留候选。
- 从原 SQLite 按四个 `responseId` 核对各自 `ExchangeKey`、完整 `ProcessKey` 和父工具引用。两个子模型交换归同一 Docker Agent 进程，子模型请求均未携 A2A `Task.id` 或 `contextId`。同一服务进程及相同结果正文无法补出任务到模型交换的精确交接。原 Pi 和 Docker Agent 退出 0，AS 停止完成、Native 实际卸载及客体与 QEMU 正常退出。

复算摘要位于 `/tmp/as-a2a-group-20261112/r2/original/verified.json`，SHA256 `515ffe853f27e149251dee5531c7fd5d703fa63f558e95330e90e2358c6b18ec`。同一试验目录保留原 PCAP、上游原件、SQLite、stdout 和 `evidence.sha256`、`inputs.sha256`。

### 原生 OTel 的父工具和子模型边界

2026-11-12 在隔离 aarch64 `7.0.0-1019-nvidia` 客体，父、子均运行原版 Docker Agent 1.160.0，启用自带的 `--otel`。父进程以原生 A2A toolset 并发委派两个同内容任务给同一个子进程。只配置本次回环工具的原生权限，官方 Collector 0.165.0 接收 Agent 原生 OTLP。两条任务均进入受控 HTTPS 模型夹具的并发屏障，原结果正文相同。两端 Agent、客户端、Collector、AS 与 QEMU 正常退出，Native 实际卸载；回环捕获 84 包，内核丢包 0。[Docker Agent 原生 OTel 初始化](https://github.com/docker/docker-agent/blob/v1.160.0/cmd/root/otel.go) · [原生 A2A 工具](https://github.com/docker/docker-agent/blob/v1.160.0/pkg/tools/a2a/a2a.go)

- 父原 `runtime.tool.call`／`runtime.tool.handler` 的 `gen_ai.tool.call.id` 分别与 AS SQLite 中父模型的两个原工具调用及各自工具回送一致。每个工具归自己的父 `runtime.session`，用原 `gen_ai.conversation.id` 对到父 A2A 会话。
- 两条子 `SendStreamingMessage` 的原 RPC ID 各自关联原任务、`contextId` 和 `TASK_STATE_COMPLETED`。两个 `contextId` 分别与同一子 `ProcessKey` 下的原 `gen_ai.conversation.id` 相符。Docker Agent 以该 A2A 上下文 ID 作为原运行会话 ID；两个子 `chat` span 的 `gen_ai.response.id` 分别在原 SQLite 对到 Exchange 4／5。模型上游收到的原 `traceparent` 指向对应 `chat` span 的 HTTP 客户端后代 span。整个子任务到模型交换可逐条核对，无需依赖文本或时间。[A2A 会话绑定源码](https://github.com/docker/docker-agent/blob/v1.160.0/pkg/a2a/adapter.go) · [模型响应 ID 采集](https://github.com/docker/docker-agent/blob/v1.160.0/pkg/telemetry/genai/stream.go) · [W3C Trace Context](https://www.w3.org/TR/trace-context/)
- 两条父发往子的原 A2A 请求均缺 `traceparent`、`tracestate`、`baggage` 及父工具 ID；父子 trace 独立，原 span 的链接、属性和事件也未提供两端交接。对照该发布版，A2A 工具使用共享 `ClientForAllowPrivateIPs`，这条出站链没有包进模型 HTTP 客户端使用的 OTel 传输。接收端有原 W3C 提取能力。本样本保留四个父工具与子任务候选配对，确认跨 Agent 边为零。[A2A 客户端调用](https://github.com/docker/docker-agent/blob/v1.160.0/pkg/tools/a2a/a2a.go) · [共享出站客户端](https://github.com/docker/docker-agent/blob/v1.160.0/pkg/httpclient/safeclient.go) · [接收端提取](https://github.com/docker/docker-agent/blob/v1.160.0/pkg/a2a/server.go)

复算摘要位于 `/tmp/as-otel-group-20261112/r3/original/verified.json`，SHA256 `8cc8f2b6179cca1a3842190866e03e466d256365d5f751417382350d60c6ea93`。同一试验目录保留原 OTLP 批次、PCAP、上游模型请求、原 SQLite 和 `evidence.sha256`、`inputs.sha256`。

### 原版 A2A 端到端追踪交接

2026-11-13 在 aarch64 `7.0.0-1019-nvidia` 隔离客体，原版 kagent Go ADK v0.10.6 通过自带的 `delegate` 工具，向同一个原版 Docker Agent v1.120.0 进程并发委派两次相同任务。两端使用内置 OTel，按 A2A 0.3 交换。旁路抓取原请求与回执，官方 Collector 接收原 OTLP，模型请求走现有 AS TLS 和 SQLite，上游为确定性 HTTPS 夹具。两条任务在实际模型端同时在途，原结果正文相同。双方 Agent、AS、Collector 与 QEMU 正常退出，Native 卸载通过，抓包内核丢包为 0。[kagent 出站实现](https://github.com/kagent-dev/kagent/blob/v0.10.6/go/adk/pkg/tools/remote_a2a_tool.go) · [Docker Agent 接收端](https://github.com/docker/docker-agent/blob/v1.120.0/pkg/a2a/server.go)

- 两条出站请求都实际带原 `traceparent`。每个头部的 span ID 精确定位原父端 HTTP 客户端 span，其直接父 span 为带原 `gen_ai.tool.call.id` 的 `execute_tool delegate`。接收端原 HTTP span 持有同一个客户端父引用。原 RPC ID 及消息历史核对了各自的任务回执，两个任务均为 `completed`，`contextId` 各不相同。
- 子 `invoke_agent` 的原会话 ID 与回执 `contextId` 一致。各自后代的原模型 span 保留 `gen_ai.response.id`，实际模型请求携带该模型 span 的 HTTP 后代引用，SQLite 根据同一完整子 `ProcessKey` 和原响应 ID 分别定位 Exchange 4／5。两条父模型的原工具调用及回送也在 SQLite 中按原 `tool_call_id` 核对。
- 父模型收到的工具结果正文在此版本只保留实际结果文本。原生 OTel 的工具结果属性另含 `subagent_session_id`，与实测 A2A 回执一致。仅凭 LLM 聊天可核对父工具及其回送，无法从本样本确认子任务归属。
- 对照组使用相同 kagent 版本作为两端，同样确认两条父工具到子任务边。目标端 `generate_content` 没有 `gen_ai.response.id`，实际模型 HTTP 请求也未传播 `traceparent`。两个 SQLite 交换均归同一目标进程，原任务到具体模型交换留四个候选配对。

本地 Group 用 `--a2a` 回放原件和只读 SQLite，得到两条完整关系。缺追踪头、交换两条追踪头、RPC ID 不符、连接端口不符、非任务结果、缺模型响应 ID、进程寿命不符及重复客户端 span 共八组反例都未生成错误确认边。缺模型响应 ID 时仍保留已证实的父工具到子任务。R48／R49 回放仍保留原七条已关联回调，原 SQLite 摘要未变。中间层 47 项测试、严格 Clippy 和 LSP 检查通过。

独立复算摘要 `/tmp/as-a2a-handoff-20261113/r8/original/verified.json` 的 SHA256 为 `85ae98d3c73b42b989a322747863d58916f8f6b400858c20957b26492b0c070b`。Group 输出和反例汇总位于同目录 `group.json` 与 `group-replay.json`。原 OTLP、PCAP、SQLite 和摘要清单同目录保留，对照组原件见同一试验根下的 `r7/original`。

### Group 本机接纳与正式 WS 复验

使用 R8 保留的 A2A 原件和原业务 SQLite，在 aarch64 `7.0.0-1019-nvidia` 客体启动本轮真实 `as-service`。经过本机 Unix 管理会话接纳同一证据包两次，提交 ID 相同，库内实际保留两条委派。相同请求重复出现的证据包返回 conflict，WS 提交被拒绝。正式 WS 两页与 SQLite 的原引用逐项一致。实际重启服务后仍返回原两行，撤销旧授权后新会话无 `chat_content` 查询返回 unauthorized。服务、Native 和 QEMU 正常退役。

交叉复核复现了旧实现仅按响应 ID 连接时的串线。当前复算同时检查原模型 HTTP `traceparent`、原请求 URL、实际 span 祖先及原 SQLite 引用。13 组原件派生反例覆盖缺失或串换追踪头、URL 不符、协议方言不符、未终结、模型响应串换及重复请求。R48／R49 仍复算出原七条已关联回调。正式接纳是本机管理员对已核验原件的提交，尚无持续受信采集入口。

客体原报告与数据库位于 `/tmp/as-a2a-handoff-20261113/service/final`，源码候选的客体镜像、报告和复算摘要见 `service/evidence.sha256`。Group 原件契约与调用方法见[技术约定](../middleware/agent-graph/technical.md#正式查询)。

### R9 原模型请求追踪头持久化

使用原版 kagent Go ADK v0.10.6 和同一 Docker Agent v1.120.0 进程复跑两条同内容的并发 A2A 任务。模型上游仍为隔离客体的确定性 HTTPS 夹具。受控 Flow 将原 `traceparent` 连同各自 `ExchangeKey` 写入业务 SQLite。原抓包、原 OTLP 和 SQLite 交叉复算分别连到模型交换 5 和 4。两条采集输入的模型 span 和模型请求响应 ID 同时互换后，原库不变，Group 保留两条模型交换缺口。SQL 回归单独验证跨交换请求头与响应 ID 的组合不返回事实。

同一批新原件另经本机接纳和真实 WS 查询，重启后逐条一致。最终验证器删除采集方传入的模型映射，只从 Flow 原请求头定位交换。独立交叉审核确认两次模型请求复用了同一 socket 和来源代，SQL 使用各自原 `ExchangeKey` 隔离。两条正例和交换响应 ID 的反例均从同一原库复算，结果见 `/tmp/as-a2a-handoff-20261113/r9/original/group-native-trace-result.json` 与 `group-native-trace-swapped-result.json`。以上验证只覆盖模型请求证据与现有查询。A2A 原包和 OTLP 仍由隔离采集后封存，本轮未接通持续受信来源。两个客体均正常退出并卸载 Native。原报告和摘要位于 `/tmp/as-a2a-handoff-20261113/r9/original`，最终 WS 复验位于同一试验根的 `service/native-trace/original`。

### A2A 与 OTLP 收发进程的内核旁路复验

2026-11-13 使用 R9 的同一 AS 服务和 Native 产物，在 aarch64 `7.0.0-1019-nvidia` 隔离客体复跑。原版 kagent Go ADK v0.10.6 向同一原版 Docker Agent v1.120.0 进程并发委派两次相同任务。保留两端自带的 OTel 配置。临时只读 fprobe 仅在客体采样回环 TCP 的发送和接收，未修改 Agent、注入进程或接入共享 SDK。模型仍走现有 AS TLS 与 SQLite，上游使用同一 HTTPS 夹具。[Linux TCP 实现](https://github.com/torvalds/linux/blob/v7.0/net/ipv4/tcp.c) · [Linux fprobe 接口](https://docs.kernel.org/trace/fprobe.html)

- `tcp_sendmsg_locked` 和 `tcp_recvmsg_locked` 的成功返回记录同时取得 TCP 序列位置、返回长度、网络命名空间、原 socket、TGID、TID 和进程出生时刻。结合客体 boot ID 和抓到的 TCP 握手及序号，两条 A2A 连接的四个方向均逐字节覆盖。请求归父进程，接收及回执发送归子进程，回执接收归父进程。同一连接的多次调用来自多个 TID，部分接收记录的 socket cookie 为 0。复算按当次连接和字节区间核对，未使用时间邻近、裸 TID 或零值 cookie。
- 原 OTLP 请求共含 41 个 Collector 保存的 span，逐项核对 `trace_id`、`span_id`、父 span 和参与归因的原属性。父进程发送 19 个，子进程发送 22 个，来源由内核收发证据确认。使用该来源复算原工具调用、A2A 任务终态、原模型请求追踪头及 SQLite，两条委派各自关联到 Exchange 4／5。换成错误进程、错误进程出生时刻或删掉一段内核字节记录，验证均拒绝。原版 Docker Agent 1.160.0 的旧对照原件中，两条 A2A 请求均无 `traceparent`，仍有四个候选和零条确认边。[OTLP trace 原结构](https://github.com/open-telemetry/opentelemetry-proto/blob/v1.10.0/opentelemetry/proto/trace/v1/trace.proto)
- 抓包 144 个，内核丢包 0，fprobe missed 0，trace 缓冲丢失 0，客体退出 0 且 Native 已卸载。覆盖范围是该客体的 IPv4 回环明文 A2A 和 OTLP/HTTP，模型交换由现有受控 TLS 出口提供。内核证据覆盖传输与进程归属，工具语义来自原 Agent 发布的追踪数据。该实验未接入 Group 的持续生产采集。

原 PCAP、内核记录、OTLP、SQLite、复算脚本及摘要清单保存在 `/tmp/as-group-socket-proof.sXTiXnUG`。完整关系见 `original/verified.json`，内核字节和来源见 `original/socket-verified.json`，摘要见 `witness.sha256`。

### R6 Group 持续采集真实客体验收

同一 aarch64 `7.0.0-1019-nvidia` 客体使用当前 AS 服务和 Native 模块。原版 kagent Go ADK v0.10.6 向同一个原版 Docker Agent v1.120.0 进程并发委派两个相同任务，上游 HTTPS 夹具在两次子请求均到达后返回。启用两端已有的原生 OTel；正式 Group 从内核连续采集，不读取验证器或人工提交的数据。

- 两个原任务均为完成终态。正式鉴权 WS 分两页返回不同的父工具、原 A2A 请求、任务和模型交换，Exchange 分别为 3 和 5。SQLite 保存两个原 A2A 请求／回执、5 个 OTLP 请求批次、41 个 span。复算核对每条 SQLite 原 HTTP 字节与抓包、两端进程身份、任务 ID、追踪引用和模型交换。
- 抓包 115 个、内核抓包丢失 0。Group 到达序号 262，连续序号缺口 0，采集器放弃了 22 个连接上下文；该计数不区分关闭重置和其他不可用上下文。普通通知通道另有 497 次拒绝。SQLite 写失败 0，AS 正常停机，Native 卸载成功，客体退出 0。
- R4／R5 原任务虽完成，Group WS 为空，未通过。R6 将旁路数据从普通完成通知队列分离到原 Messages 运行时的单个专用消费者。当前协议删除原件导入命令。独立审核复算摘要及两条关系，并核对原帧鉴权、缺失和冲突证据处理及停机顺序。

原件在 `/tmp/as-group-production-guest.QBOjTUe1/r6-original`，见 `ws-sqlite-byte-verified.json`。同级 `r6-artifacts.sha256` 和 `r6-evidence.sha256` 分别固定实测产物和原件。本批验证范围为所配置的 IPv4 回环 A2A HTTP/1.1 及直连 OTLP/HTTP。

独立复核在 R6 原库的内存副本中复现了重复 `traceparent` 漏检。两个不同 SQLite 行号对应同一完整 `ProcessKey`，按行号查重仍把两条原请求分别判为唯一。R7 使用 boot ID、TGID 和出生纳秒作为进程身份，复用已存的进程键行，并直接以三个字段判断冲突。定向测试和独立复算均确认同寿命的两条请求保留候选、同 PID 的新寿命保持独立。

最终 R7 在同一客体用相同双并发场景复验。两条原任务完成，正式 WS、原请求及回执字节、SQLite 引用对到模型 Exchange 4／5。SQLite 保存 2 个 A2A 交换、5 个 OTLP 批次和 40 个 span，span 归属两个进程实例。抓包 106 个，抓包丢失 0，Group 连续序号缺口 0，放弃 20 个连接上下文。普通通知单独报告 372 次拒绝。SQLite 写失败 0，服务退出，Native 正常卸载，客体退出 0。95 项适用本地测试、严格 Clippy、所改入口 LSP 通过。

现存 R7 原件只有 Group 连接放弃和普通通知拒收两项各自的总计。具体连接、通知路径及单条记录的原因未定位。

最终原件及独立复算位于 `/tmp/as-group-production-guest.QBOjTUe1/r7-original`。同级 `r7-artifacts.sha256`、`r7-sources.sha256`、`r7-evidence.sha256` 分别固定产物、工作树改动的源码和证据。

### 原版 OpenClaw R69 复核 Group R7

专用聊天使用冻结 R7 产物在隔离客体运行原版 OpenClaw。父 Agent 的原 `sessions_spawn` 回执接纳了 helper，回执中的运行和会话标识与原登记一致，子任务终态为 `terminal/ok`，原结果已保存。子结果回投时 Gateway 已进入停机，登记保留待重试。父模型交换 1／3、子交换 4 的原请求和 SSE 与独立端点逐字节相符，归同一个 Gateway 进程实例。A2A 观察端口未捕获报文，正式 WS 和 SQLite 都为空，Group 候选与确认边的正向验收均为 NOT_RUN。

抓包及独立接收端逐字节对账了 51 个 HTTP/1.1 Protobuf POST，全部使用 `Transfer-Encoding: chunked`，接收端全部返回 200。R7 的 Group HTTP 正文定界拒绝该头，实际保存 0 批次和 0 span，OTLP 采集验收 FAIL。抓包内核丢包与 Group 序号缺口均为 0。R7 原有 kagent／Docker Agent 的两条确认边复算仍通过。客体已正常退出并卸载 Native。

报告、原抓包、SQLite、WS、登记及复算清单见 `/tmp/as-openclaw-evidence-20261113/r69-delegate`。报告为 `result.json`，SHA256 `c9dcbe39a5245da2869a9c8192425a69c68d4c411f6b3c976db7adcfaa7adbfb`。

### Group R8 的分块正文复验

Group 继续用同一 HTTP/1.1 解析入口，复用 `httparse` 识别分块长度和尾字段。完整原报文用于发送身份及 SQLite 原件，解块正文用于协议解析。冻结候选在 R69 的 12 条原 TCP 流上复算了 51 个 POST，解块正文与独立接收端逐字节一致，全部解出共 477 个原 span。R8 实际客体复跑两条并发 A2A 委派，原请求和回执、正式 WS 及 SQLite 对账通过，两条关系分别连接到模型交换 4／5。原生模块正常卸载。

对 R69 的 477 个原 span 检查了 trace/span 父引用、显式 link 以及原运行／会话标识。确有带原工具调用 ID 的 `sessions_spawn` 工具 span。子 Agent 的 run、harness 和 model span 共用同一 trace，但没有该工具 span 的祖先引用，也没有显式 link 或可与原回执匹配的结构化运行／会话 ID。原 AS 模型请求保存了带原调用 ID 的父工具接纳回执，仍缺将该回执连接到实际子执行终态的生产旁路来源。

候选和复算见 `/tmp/as-group-r8.gaiTeZ9r` 的 `artifacts.sha256`、`r69-wire-replay.json`、`r69-span-counts.txt`、`r69-otel-handoff.json`、`r69-original-receipt.json` 和 `a2a-ws-sqlite-verified.json`。

### 原版 OpenClaw R70 复验 R8

冻结 R8 服务复用 R7 内核、原生模块和 BPF，原版 OpenClaw 的委派、子任务终态和父会话回投均通过。子结果进入实际父回投模型请求。父交换 1／3、子交换 4、回投交换 5 的原请求和 SSE 逐字节对账，客体正常退役。

独立接收端对 59 个分块请求均返回 200。Group 实际保存 54 个完整 HTTP 请求和 567 个 span，全部对上抓包原字节、独立 Protobuf 解析及实际 Gateway `ProcessKey`。五笔未入库集中在同一条第一代 AS 退出后建立的连接，缺第二代实际接纳该连接的驱动原件。此项属于 TCP 连接观察范围，全程覆盖仍为 FAIL。已采集源序号缺口和 SQLite 写失败均为 0。

原 OTLP 的 586 个 span 中有父工具和 helper 的子运行 span。它们共用 trace ID，缺父链、显式 link 和可与原回执匹配的子运行／会话 ID。A2A 观察端口没有报文，原版 Group 候选和确认边的正向验收均为 NOT_RUN。

报告、原件和可复算摘要在 `/tmp/as-openclaw-evidence-20261113/r70`。报告为 `result.json`，SHA256 `b6cbe2c976524b724dabb461bed2502f0f17df09dd91ecd1d9d0333d7c4246ed`。

### R9 重启后的首发接纳与原版委派

隔离客体实际停止第一代 AS 并启动第二代。同一连接在第一代退出后完成握手、在第二代就绪后才发送应用正文，两条原分块 OTLP 请求及同批新连接的一条请求均入库。另一条连接在 AS 停止时已发送正文，复启后继续发送也未进入采集，未观察到原报文起点。客体退出 0，原生模块已卸载。R9 重新运行两条并发 A2A 委派，原请求、回执和正式 WS／SQLite 对账通过。原件及产物摘要在 `/tmp/as-group-r9.yokQLcgx`，见 `restart-original/report.json`、`a2a-ws-sqlite-verified.json` 与 `artifacts.sha256`。

R70 已留存的原版 Gateway 工具起始、结果和父回投帧提供了原生交接来源。结构化工具回执中的子运行和子会话同时匹配原子运行登记，登记终态为 `ok`。父会话的原回投 ID 同时编码同一子运行和子会话，原登记也记录了 `delivered`。本地 `as-agent-graph --openclaw-handoff` 可在同一已验明 Gateway 实例的原帧及原登记快照上复算，R70 原件确认一条交接。正式 WS 仍只消费已接入的正式来源。

原版 OpenClaw R71 独立复验了配套的 R9 服务和驱动。重启前握手、重启后才首次发送的 socket 与负控均通过。同一 socket 的 cookie、原 HTTP 字节、span 和真实发送进程一致。真实 Gateway exporter 的 56 个请求则使用停采期已经发送正文的同一连接，3 个在两代之间、53 个在第二代运行回执之后，独立接收均返回 200，该 exporter 在 Group 入库为零。现有证据将缺口限定在已使用连接的接纳边界，未逐笔记录内核判定。原生委派、子任务终态、父回投及只读交接复算通过，正式 Group 候选与确认边仍未执行。客体正常退出并实际卸载原生模块。R71 原报告和只读复算位于 `/tmp/as-openclaw-evidence-20261113/r71-live`。

### R10 保留重启后取得的原 TCP 证据

R10 对已收到但无法归入已接纳连接的 TCP 负载和实际收发区间独立入库，标记 `http_boundary_unverified`。隔离客体实际停止第一代 AS 并启动第二代。新连接和重启前已握手、尚未发送正文的连接，共三条完整请求正常解析入库；停采期已经使用过的连接，在恢复后保留了原报文字节和实际收发区间，没有生成已定界 OTLP 批次。原接收端确认停采期请求成功，业务库没有补写该请求。定向客体共保存 14 条未定界证据，退出 0，原生模块已卸载。原件、产物与检查见 `/tmp/as-group-r10.aZuyn6vL`。

原版 OpenClaw R72 独立验收通过恢复采集的证据保留。业务库保存 104 个原包及 210 个收发区间。已就绪后抓包中的原版 exporter 请求字节全部对上 SQLite，并按实际 socket cookie 和发送进程核对。已证实停采期间的请求与收发区间均未补录。完整的空闲连接首发正控继续解析入库。原版委派、子执行终态和父回投通过原帧及只读登记复算。A2A 端口没有原版委派流量，正式 Group 确认边仍未观测到，本次不扩大正式来源。R10 的并发 A2A 对照仍确认两条委派，WS、原包和 SQLite 一致。R72 原件位于 `/tmp/as-openclaw-evidence-20261113/r72`，完整报告见 `result.json`。

### R11 的原版 OpenClaw 正式 Group WS 回执候选

R73 独立验收启动服务摘要 `a4ac178b0fc9f0d542a028dd0351fc617ab15120a4ae45587aff2fe04aaadc7d`。原版 OpenClaw 实际接纳一次进程内委派。原模型交换 3／5 各携带同一工具接纳回执，正式 WS 完整页及逐条分页均返回两条原观察。原始 HTTP 正文、Flow 记录和 SQLite 逐字节对账；两行分别引用原交换 3／记录 0 的 input 下标 3／4、原交换 5／记录 0 的下标 2／3。两行均报告 `child_terminal_unobserved` 和 `parent_delivery_unobserved`，正式库没有为该委派写入确认边。

独立验收从原工具事件、只读登记及父回投原帧核对到同一子运行和子会话，子终态为 `terminal/ok`，父回投为 `delivered`。这些原件仅用于独立复验。原版 Agent 未改动、未注入，采集范围沿用 R10。隔离客体正常退出并卸载原生模块。R73 原件及复算见 `/tmp/as-openclaw-evidence-20261113/r73-final/result.json` 和同目录的 `recheck.sh`。最终 R11 的两条并发 A2A 委派也分别与原报文、WS 及 SQLite 对账通过，见 `/tmp/as-group-r11-final.oV3pT8p9/a2a-ws-sqlite-verified.json`。

### R74 联合候选的三段旁路来源核验

原版 OpenClaw 在隔离客体运行，AS 源码为 `51ddc03f4`，模型为本地 Responses 夹具。两次独立 Dashboard 会话以同一工具参数打开同一原文件。正式 WS 原 Ask 均携规则的默认拒绝，人工 Allow 取得原内容，人工 Deny 返回 `EPERM`。两次等待、EventKey、规则、实际效果和业务库原值对账通过。

- 11 条原客户端 HTTP 请求中，8 条的完整 trace/span 可在独立接收的 1456 个 OTLP span 中精确定位 `openclaw.model.call`。这 8 个 span 及其原父链没有 Session／Run 字段，正式业务库也没有对应模型 span。独立 Gateway 原工具回执的原模型 call/item 可对照两条不同会话的交换 1／4，正式 Agent／Session 归属保持未观察。冻结版 `addRunAttrs` 只导出经规范化的 Agent 标签及部分运行元数据。上游文档另记录可随 `captureContent` 导出会话身份的方案，该配置也扩大内容导出范围，本轮未启用。[冻结版 exporter 源码](https://github.com/openclaw/openclaw/blob/8b18b5578f66eab1501cba4cef2310d88bfb9a41/extensions/diagnostics-otel/src/service-traces.ts#L339-L366) · [上游会话导出范围](https://docs.openclaw.ai/gateway/opentelemetry/privacy-and-trace-context#session-identity)
- 文件允许一条候选，拒绝两条候选，正式库无工具到 EventKey 的确认关系。拒绝后交换 6 保存原工具 `EPERM`。固定提示未发送，反馈事实和领取记录均为零。
- 独立原件核对 `sessions_spawn` 接纳、子终态 `terminal/ok` 和父回投 `delivered`。正式 Group WS 返回交换 9／11 的接纳候选，保留 `child_terminal_unobserved` 和 `parent_delivery_unobserved`。遍历两份原工具回执中的子键后，同值的结构化字段只在两份接纳回执出现。父回投交换虽保留实际结果文字，该文字没有结构化交接。

最终报告 `/tmp/as-openclaw-evidence-20261113/r74/result.json`，SHA256 `4221314b14e882aaf8b8a30ba1e641fcdf949a4e64103b02a6bf62b02f627688`。32 份原件与 20 项脚本通过同目录 `recheck.sh`。原版 Agent 未修改、未注入，服务及客体正常退役并卸载原生模块。完整 OTLP 解析在本轮原库未观察到。该范围单独报告。

### 原生模型路由携带的会话字段

对 R73 和 R74 正式库保留的 16 个客户端请求及 120 个上游 SSE 事件逐项核对了已知 Gateway 会话／运行身份的结构化原值。两轮自定义回环 Responses 的请求没有根级 `conversation`、`previous_response_id`、`metadata`、`prompt_cache_key` 或 Session 字段。上游 SSE 也没有与 Gateway 会话／运行原身份等值的结构化字段。两轮的 `input[*].output` 有结构化 `childSessionKey` 和 `runId`，两键是原 `sessions_spawn` 接纳回执的子任务身份。工具参数 schema 中同名字段是声明。

冻结 OpenClaw `8b18b5578` 在匹配的原生 OpenAI／Azure 路由从实际 Session ID 派生 `x-openclaw-session-id`。Responses 客户端把策略的 HTTP 头交给实际请求客户端，把 `metadata.openclaw_session_id` 交给正文构建器。另一路 `session_id` 头有独立的原生 Codex／显式兼容开关和缓存条件。显式兼容端点会按配置剥离 `prompt_cache_key`，支持该键的路径也允许显式覆盖、截断及边界后缀。缓存键不能通用作为会话身份。[传输策略](https://github.com/openclaw/openclaw/blob/8b18b5578f66eab1501cba4cef2310d88bfb9a41/extensions/openai/transport-policy.ts) · [Responses 客户端](https://github.com/openclaw/openclaw/blob/8b18b5578f66eab1501cba4cef2310d88bfb9a41/packages/ai/src/transports/openai-responses-client.ts) · [会话头条件](https://github.com/openclaw/openclaw/blob/8b18b5578f66eab1501cba4cef2310d88bfb9a41/packages/ai/src/transports/openai-transport-params.ts#L345-L364) · [缓存键来源](https://github.com/openclaw/openclaw/blob/8b18b5578f66eab1501cba4cef2310d88bfb9a41/src/agents/embedded-agent-runner/run/session-boundary-prompt-cache-key.ts)

R73／R74 的模型路由为 `as-vm-responses` 自定义回环端点，未覆盖原生分支。正式 Flow 的原请求头记录仅保留 `traceparent`，独立模型接收夹具也未保存上述 Session 头。上述头在正式原件中为未取得，截至 R73／R74，该原生路由尚未实测。正文和 SSE 的字段清单及原值等值结果保存在 `/tmp/as-identity-group-proof.ysaJTlu4/provider-identity-inventory.json`。

### 先采集原件再作验证

本轮 R76 已取得原版 native OpenAI 请求的 `x-openclaw-session-id` 与
`metadata.openclaw_session_id`，均等于原 Gateway lifecycle 的 sessionId。
此前 R75 自定义 provider 分支没有覆盖这条路径；未覆盖不能写成字段不存在。
R76 首次业务流程失败是夹具将显式默认 `:443` 与省略端口的等价 HTTPS origin 作字符串比较并拒绝，
不是字段未产生。原失败报告保存在 `/tmp/as-openclaw-evidence-20261113/r76/failure-analysis.json`。

用户修正取证顺序：在已授权范围先保全实际取得的请求／响应／错误及缺失、重复、辅助字段，
再整合分析，最后根据已知契约作离线断言。预设字段断言不能先拒掉合法请求、关闭观察或清空异常原件；
夹具或UI严格解码失败分别记录，不能冒作真实源不存在。真实鉴权、授权范围和产品规则裁决继续各归所属模块。
未知关联只限制确认结论，不限制保存已取得的事实。

R79 已完成正式 Session 的原生 A/A/B 实测和全部来源对账，报告
`/tmp/as-openclaw-evidence-20261113/r79/session-result.json` 为 PASS。
原Gateway SessionId、header/body、Flow原件、正式WS和SQLite沿同一原请求闭合；辅助请求缺字段如实保留。
正式结构及分组域归[中间层契约](../middleware/technical.md#应用会话目录)，
整包构建和执行范围见[WS证据](middleware-websocket.md#原应用-session-的正式查询链)。

## 公开消息候选与当前执行语义边界

公开ToolCall／Assistant DTO及live Frame同对象pair只证明现场持有该消息对象。普通ALS可以保留上一次工具，因此即使CAPI→Event交接真实、candidate唯一且读取一致，也不能泛化成当前ToolExecution。R12另有特定OpenClaw active tool.run语义的有限原证据，保持其已证范围，不把它套到未知store。

本批已撤生产bind_native_context自动升级，link_capture只供cfg(test)明确受控source验证；旧独立Main工具消息和直写writer不恢复。全部raw、candidate、CAPI Bound及原request/native_call_scope事实边继续保存和展示。当前缺的是“当前工具执行”的可信发布／交接，不是字段齐全、零丢样、Tool／HTTP语义等原始采集门禁。R5所报8表行属于当时旧自动升级逻辑的组件结果，不作为通用确认交付；R6只核原始事实、候选、直接TryWrite来源和正常退出，不据此计N／H／F完成。

## R12 同次 ToolCall native EventKey 研究的当前复核

2026-11-14按bot已交R12报告重新核查，报告不是不存在或整条链未证。已先read_thread `6af73ae9-6774-8191-8f9b-e30f53d63028`，读取完整原报告及关键checker/采集源，报告SHA `9777ca3195c61a275ab23bfb63c8ea32636b8b3b2e7a21fe8ef75e4c895abe26`、原result及独立AS字段复核摘要均与交接匹配。保持研究树/原盘/原DB只读，没有启动目标、AS、VM或宿主驱动，没有注入或改Agent。

原封存checker实际独立复算：`/tmp/as-gap-review-r12-20261114-r2/result.json`，passed=true、bindings=1、errors=[]；另一独立worker同样复算通过，结果 `/tmp/as-group-r12-independent-proof-20261114/result.json`。实际读取原bank/trace及新SQLite分析副本，而非只相信报告摘要。第一次以仅3个checker脚本的checker-source作为code-dir时缺research-analyze依赖，退出2；该失败为复算依赖路径错误，不改判原件。按原接口选择同封存run/source-snapshot中的分析器、decoder和heap reader后原算法/条件不变地通过。未新增夹具门槛、未导入当前业务库。

已证明：原ToolCall与assistant.content[0]同对象引用；callback marker与req/work原活动寿命、worker同次syscall、AS fprobe scope相接。该次EventKey/source `50d760fae88def9759a1a3717b2949f1` / domain1 / sequence163，wait1、return80；地址仅在原上下文寿命中核，不当跨时刻ID缓存。原32项负控结果保留，未本回合重新运行全部负控或新业务。该研究仍是准确映像OpenClaw2026.10.9/Node24.22.1/历史AS的一次Read→Ask→Allow，不是当前R87产物或任意runtime验收。

当前唯一NativeToolContext已足够承接原值：原ProcessKey（原WS/DB核实）、完整tool ID `call_r66_shared|fc_r66_shared`、name read、response_id `resp_r66_1`、provider `as-vm-responses`、api `openai-responses`、model `fixture-model`及Json原参数。未获取run/session/agent保持None，不从Gateway或HTTP倒填，不剥复合ID。HTTP→ToolCall身份仍未证明；response ID或原body一致不独自构成原HTTP交接。

接入前基线edee872逐项对照及LSP结果：drivers/linux/as_core.c的request_enter/note_request_wait/request_leave已有原syscall scope→EventKey/NativeRequestReturn；Main.publish_tool_execution有定义但无生产调用；Media已消费ToolExecution至native_tool_executions，ActivityContext已可读。bind_tool_execution仅测试调用，execute_feedback只有定义无调用。接口/消费者不等于实时来源，无需再建后半、表、总线、Owner或空publish替代生产。LSP原件 `/tmp/as-gap-detection-20261114/lsp.log`，三符号引用及相关文件诊断通过。

研究固定PC/CB/LR、原对象偏移及全图预算不能原样上线。额外只读适用性核查找到成熟[OTel Node/V8元数据读取](https://github.com/open-telemetry/opentelemetry-ebpf-profiler/blob/main/interpreter/nodev8/v8.go)、[llnode属性读取](https://github.com/nodejs/llnode/blob/main/src/llv8.cc)与[Node postmortem契约](https://github.com/nodejs/node/blob/main/doc/contributing/node-postmortem-support.md)。当前ELF有480个v8dbg/16个nodedbg；nodedbg位于BSS，不能把磁盘0当运行期偏移。实际getter/初始化指令与5条封存记录交叉核实了资源Frame槽、Map table及entry布局，可做实际映像的窄读取计划；未知指令不能猜，也不建立Node版本白名单。

仅从封存自持有字节定向读取Frame、toolCall/assistant/content/arguments，仍唯一命中seq2及原同对象引用；必要逻辑读集730次、5839个不同地址字节。它不是新callback性能实测。Promise→Realm→Env HTTP载体支路和512节点扩图不参与该核心证明，不能再作为生产Tool→Event的必要门槛。工具语义的自动getter提取器与正式生产者仍未接入，不把静态复核称为已生产。当前原始libuv观察独立接入原driver/adapter，唯一契约见[原生请求观察源](../backend/driver-adapter/runtime-observation-technical.md)；未使用的V8参数、0x10b空分支和无调用的Main额外发布方法删除，当时原ToolExecution消息／业务库仍保留；本轮消息直写入口已退役，当前只有原件／候选链，见当前量化交付点。原始观察不等ToolCall/HTTP身份；实际关系另核证据，不新SDK、不回读瞬时目标内存。

## 原始持续采集R88：原件成立，整体运行失败

R2实际service `8d4a37bacdc3ee557f4c1fdbcb5996b3114c1b3d9a468ea5592d5ad7a874cd02`、Native `b2e6f7fba1ba315246c42a1e4031be297ed01a29bc92f138e033fc8bde8a11f1`在原OpenClaw聊天R88真实启动。原件 `/tmp/as-openclaw-evidence-20261114/r88/raw-result.json` SHA `cd282ba0f56ff6006cab5e0cccf6a3cdb5f40a7067585660828f91aca5e6d7f2`；Root离线原recheck重放退出1，结果摘要逐字节不变。总体FAIL、Allow/Deny业务与实际Ask/EventBound/正式WS关联NOT_RUN，正常退役未证明；原聊天只停止其已核自有QEMU，异常停止原件保留，未把脚本清理失败说成排空成功。

实际已持久化原始请求记录12208条，和3444条原private futex一起组成15652次delivery，等于两实例media_committed的13951+1701。原字段、空值和未知关联没有变成采集门禁。628个被实际引用的request寿命中105个能按完整源域找到其原Opening，523个缺Opening原件；不能用地址、TID或时序补造它们。没有EventBound，不能把启动期原件当作业务Ask或已证ToolCall链。关系复核归原 `raw-lifetime-links.json`，不把高notice拒收数直接换算为raw丢样数。

第二AS实例实际SQL写失败后退出，原Display只留下 `business database operation failed`；原件没有SQLite基础码、扩展码或OS errno，所以具体失败原因未证。离线SQLite完整性通过及磁盘余量不能替代运行时错误码；重开连接读到的max_page_count也不证明原运行连接采用同值。此次先补数据库owner的原错误源和Native接收首错局部诊断，再用同一业务范围复验；没有按猜测增队列、扩大容量、吞错或增加采集条件。诊断候选R3已冻结交原聊天，不能将其称作R88根因已修。

本轮发现cmd17跨原命令锁执行kernel_read/uprobe_register；后者在Linux实际源码中取得目标mmap写锁。已复用既有持久准备模式，将可睡眠准备移到原写锁外，完成后重核原权限、会话和epoch；原写入计数仍保管至真实清理。只认定该源码可达的控制延迟风险，不声称R88已发生此死锁，普通security_mmap_file在mmap锁外这一实际边界继续保留。

## R89/R90与必要回执背压收敛

R89未复现SQL错误，两AS实例均正常停止，但原准备流程在Agent启动期间重载AS；Agent出现ESHUTDOWN并退出，业务NOT_RUN。原聊天通过既有Flow初始化及凭据热采用接口收拢准备顺序，R90的AS准备完成早于真实Node出生，运行期间未Stop AS，不能把R89的问题当作字段门禁或R88 SQL根因已修。

R90仍FAIL，原 `iteration-brief.md` SHA `96195d8e1d5e2fbdda0763d1a6b778fec051afe1c5554d14aad4a9f0f30c0664`、`raw-result.json` SHA `78fec9e7b17c2efc4717522f8d6f40e3a32aa8d495fb9440d24f93573fd728c5`。完整host日志250行先报告Native `event_decode_or_dispatch / Unavailable / 0x0102 / read833 / body769`，251行报告SQLite `SystemIoFailure / extended778 / disk I/O error`。这一行序不替代并发事件的精确因果，也不证明五个Unavailable分支中的具体一支；0x102解码与合法性检查的InvalidInput路径不能被叫作本次已证明坏帧。恢复副本保留13518条raw及167个原Opening引用（4个跨线程），缺Opening和未发生业务/Ask仍如实记录。只恢复异常停止的磁盘副本，原盘、恢复产物和host未截断故障日志各保来源。

现有本地测试确证旧deliver在必要Controlled/DirectResult遇Full或仅critical_pending时也返回Unavailable并关闭来源。实现按同一生产消费链收敛：既有Critical只保当前1条已读必要原件，停止继续搬普通数据，reserve_owned实际接纳才移交；原Native列表优先提供Reply和仅已交付原请求的AS_RESULT，分别归还reply_count/receipt_count。EPOLLPRI经锁定rustix安全epoll和原async-io在唯一reader内观察，不建第二通道、队列或预算；原retired_count参与真实排空。当前40字节Hello明确ControlPriority=512，无旧读法回退。真实closed/unbound/IO失败继续返回实际错误，不用丢必要原件或放行规则替代。

R4当前包 `/tmp/as-runtime-raw-package-20261114-r4/candidate.json`，195项源无漂移：service `63f128ad9b146cd5c797642e3a7e17a78a63c389aecfff82020af70c5c0b311a`、Native `3b728144f813782aad03ecf687161ba09eb2779f1580b85f91e6482267d1b813`、image `37b84add48ffb7cd5860df9efdc63898f7bc1d69b3847ec3130c398d6bba2abf`。原build.sh all、Rust1.89全workspace/all-targets239项、1.92全目标Clippy/fmt、Main软件冒烟及独立交叉复核通过；原有测试内覆盖真实Unix OOB/PRI、Full/待交接恢复、完成解锁、真实close、正常stop尾部和Group片间AS_RESULT。UI未变化，沿同一已核UI源/build/LSP结果。以上不替代新候选真实OpenClaw保护效果，R4已交原聊天按原业务范围验收；R88/R89/R90失败均保留。


## R91分项封存与本分支独立复算

原聊天以同一R4服务/模块在原版Node/OpenClaw和本地合成Responses provider中完成新两项写打开业务，分项结果在 `/tmp/as-openclaw-evidence-20261114/r91/acceptance-result.json`，SHA `636784c0b7a50a68575b22e6785c8ba43ad5e31628154cca5d549fb6275ba5bb`；`iteration-brief.md` SHA `1886fe8121fd7ad042b9c5170289ff58e6ce36c903dcda9a7358e693865a0ad7`，清单文件SHA `c9fd7e30554d6a65bbfc65d718a54efe9c0648f448164c8961b8a29e00c9d67f`。Root独立运行原recheck.sh退出0，日志 `/tmp/as-gap-detection-20261114/r91-recheck.log`；没有新建业务、运行Agent或改变原字节。

写打开业务、运行稳定及正常退出PASS：两原Question及完成回执与正式WS/SQL一致，实际答复原因为user；Allow真实写入47字节，Deny保持文件未变并产生EPERM。独立原trace核得Allow openat返回173、Deny返回-1，不能把trace冒作缺失的AS消息。原工具结果逐字进入后续模型请求和最终响应；15份provider请求/响应原body SHA核对。AS/Node/VM退出0，模块正常退役，没有NativeReceive/BusinessWrite错误。该范围不证明公网GLM、全运行时或生产精确ToolCall关联。

采集保真PASS：32453条已提交raw保留原字段、None及0，434个request→原Opening引用可核（4个跨线程），1265个缺Opening继续保缺失。没有任何EventBound，Deny的AS NativeReturn原消息也未取得；正式原始交接仍NOT_PROVEN，不能以空数组相等宣称已证、以时间相邻的WorkLeave/Done补成EventKey关系。首次过严checker把NativeReturn缺失混进业务否决、用SQL rowid判同一源及混淆Media与Question计数，已按实际契约修正；first-strict-check及原输出保留。修正判定口径没有改动采集字节，也没有把缺阶段变成准入条件。

本批交付限定为现行原始请求采集/同库保存、必要回执有界背压、现有写打开Ask/答复/执行/退出闭环；EventBound定位、Deny原NativeReturn消息、正式Tool语义/HTTP精确身份及固定拒绝自动反馈继续按缺口清单记录。独立提交451681878的Linux快速放行预制数据已接原MVP打包入口；R5服务/模块与R4逐字相同，默认规则数据变更不冒充已经在R91内核加载。


## 局部归因与时序补充：R91既有原件分项离线验收

用户授权先使用已能证明的关系并以时序补充。实现复用ContextView、原始RequestHandoff和同一SQLite所有者；中性EvidenceRelation按kind区分原明示引用与same_thread_return_sequence模式，不再以类型名将模式叫作确认。native_observation沿现ActivityContext读取，缺原Opening仍保留字段明示引用和NotProvided；时间/线程/原request/work/结果的组合不升级ToolExecution或任何防护/反馈授权。

原R91正式函数重放结果位于 `/tmp/as-native-relations-r91-replay`，新query读取派生分析副本；原库只读，32453条原JSON及有序摘要完全不变，新四列只投影原actor/boottime。Allow返回5份原事实、2条request明示引用和1条补充模式；Deny无原NativeReturn，保持0条模式；native观察锚读出2条原引用。UI对这三份实际输出严格解码/组件SSR通过，三语模拟WS点击复用原Activity读槽并核完整锚；这不是新Agent/WS实测。

原OpenClaw聊天独立离线验收为PASS，证据 `/tmp/as-local-links-evidence-20261114/r1-r91-offline`，result.json SHA `44f78362de7d45058f703e7f531ce8168e7a41be1ec4ec1bd9d07c9fd3d77d14`，清单文件SHA `eaf7820a89754db959b386fdaeb18ca721429276cbe4cab375bf2647a443cabb`。Root独立执行verify-existing.py退出0，112项原值/原序/所有旧表/索引投影/关系引用/产物/生产正文绑定核对均通过，日志 `/tmp/as-gap-detection-20261114/local-links-independent-verify.log`。未重发R91业务、未修改原件、未重编原Agent；新候选真实WS/前端连接/Agent执行均NOT_RUN，原R91业务PASS仍仅原运行范围。

本地冻结全workspace/all-targets242、Clippy/fmt/Main软件冒烟PASS；UI308+4/i18n、源码及浏览器tests类型检查/build/LSP、三语单点击流程均PASS；独立窄review无阻断。当前静态包 `/tmp/as-local-links-package-20261114-r1/candidate.json`，196项源无漂移；service `b4c7b6219560038721d322cb0ecce84e4e4cb70498abb612a2f11a4c18a6d30b`，Native仍 `3b728144f813782aad03ecf687161ba09eb2779f1580b85f91e6482267d1b813`，image `ccbbd507d1f03e5cdb56659a6d89d1afbe78dc471e6bd350283475b99c025702`。本轮没有Driver、采集范围或安全动作变更，不以模式补全EventBound或Deny原NativeReturn。
