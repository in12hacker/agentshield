# Group 子任务终态与父公开回投旁路接入

当前交付：r4已通过原OpenClaw R87正式旁路验收，原件/引用/并发隔离及正常退役核验PASS；具体执行边界见本页末。

本批从 `c8000b857` 出发，只补 Group 公开协议持续来源、所属配置及消费契约。归因与 Ask20 的生产逻辑未改；独立 `docs/roadmap.md` 修改不纳入。唯一现行字段与来源边界见[Group 技术契约](../middleware/agent-graph/technical.md#公开-gateway-旁路来源)，不在本页重复定义。

## 来源原件

既有 R75/R79 录制器只保存部分 agent/chat，不能据其缺少子终态认定协议没有来源。原 OpenClaw 聊天 R84 的完整公开 WebUI 流补到两个并发委派的原接纳、子会话 key/lastRunId/status=done 及父完整 announce 的 chat.final。普通 WebUI 自身原生订阅在场；AS 没有主动连接、发查询、读运行登记或注入 Agent。

原件 `/tmp/as-oc-r84/run/gateway-frames.jsonl` 的该次稳定输入为1239行，SHA256 `56b93add51e3c0e2cca2404fc5547d7625203df35c21e05d6ebf65305b065b60`。原 event=agent 接纳在606/617行；607/618为另一个公开 event=session.tool，不混称同事件。当前解析器只按已定 agent 接纳分支派生；其他原帧保留。

冻结代码离线复算 `/tmp/as-group-r84-parser-replay.json` 和 `-binding.json` 得到2条接纳、每子运行6条公开会话终态与2条父公开发布观察。它只证明这些原消息字段可解释，不证明新内核采集、进程身份或正式WS入库链已运行。两条观察连接没有close原记录，不能宣称整条连接完整。

## 实现与审核

复用现有驱动 Group TCP 原通道、唯一消费者、业务 SQLite 与 GroupDelegations 查询。标准帧头、遮罩、分片与UTF-8沿锁定 tungstenite 0.29.0；纯内存输入禁止任何写出。已完成消息消费即释放，原wire只存一份，派生字段只引用原件；无 Agent 私有函数/布局识别、SDK、注入、导入入口或第二聊天缓存。

审核收拢了关闭后继续索引、歧义端口丢原件、owner改变后残片继续持有以及Gap借用握手进程身份四处边界。改变来源或退出会保存已取得的未完成原片与真实缺口。共享JSON严格解码移到 `as_types::json::parse` 唯一owner，Flow与Group调用同一实现。

独立交叉审核覆盖配置/Native投影、采集/重组/退出、公开字段/SQLite/页面消费，未留阻断。记录分别在 `/tmp/as-group-ports.a_9qo7d4/delivery.md`、`/tmp/as-group-collector-lsp/final.log`、`/tmp/as-group-ts-language.json` 及本批子代理交接；不把未成功的C clangd AST查询声明为通过。实际Kbuild与完整原构建分别执行。

## 冻结产物与验证

R85使用的初始候选 `/tmp/as-group-terminal-package-20261113-r2` 由原 `scripts/mvp-smoke/package.sh --frontend-ws` 生成；Native由原 `drivers/linux/build.sh all` 重建。固定包清单绑定185项源码，冻结时漂移为空；Dashboard源及dist绑定在candidate.json。未加载宿主驱动、未重启宿主、未运行远端CI。

- 静态service：`f88663b848f3d7c56d1771c5f46ba8babd26e13de90573d9add4bb4e1c5694fc`。
- as_wait.ko：`5c0955933fa39f50996f5cfd0af15200c9440687e1e079931e3534fea0718a9c`，带.BTF/.BTF_ids。
- initramfs：`8052b45a0c6c48abd16ef95e8fe07e8defd1649d6bbab1a9a53605b248c1755f`。

显式source本机CI环境后，Rust1.89全workspace/all-targets锁定离线测试、全目标Clippy -Dwarnings、软件主模块冒烟通过；沙箱首次禁止本地监听的失败保留，授权本地回环环境复验通过。冻结r2的完整复验为230项Rust测试及全目标Clippy通过，原件 `/tmp/as-group-r2-full-tests.log`、`/tmp/as-group-r2-clippy.log`、`/tmp/as-group-main-smoke.log`；`local-verification.json`绑定各日志摘要且源码漂移为空。完整原Native构建通过；共享UAPI的内核专用u8修为__u8，布局未变，没有编译宏别名或绕过原入口。

Dashboard锁定离线安装、304项Vitest与4项i18n、build、原浏览器tsconfig及现有三语宽窄174项浏览器场景通过（`/tmp/as-group-browser.log`）。这些场景验证页面与当前协议供给，不等同真实内核或OpenClaw验收。原OpenClaw新候选验收结果在完成后记入本页。Rust LSP引用与诊断、TypeScript原生语言服务诊断通过，后者不混称独立TS LSP已安装。

## 实际范围

仅配置端口上的宿主初始网络命名空间IPv4回环明文HTTP Upgrade/WS；协商扩展无法解释时保留原帧并停止语义索引。没有客户端发布的阶段、停采期间的消息或真实未取得内容保持缺口。父公开结果观察不表示私有registry的耐久delivery、用户已读或子任务成功；终态原status与其他实际执行结果分别保存。

## R85 真实失败与根因收敛

原OpenClaw聊天用r2全套匹配资产实跑R85，正式链FAIL，公共来源仍PASS；不以公共正例替代正式链。原件 `/tmp/as-openclaw-evidence-20261113/r85`：实际库2连接、7原消息、0派生事实，原消息仅早期连接前缀；4条Flow回执仍有三类unobserved缺口。业务写失败0、实际server身份与Flow actor一致，排除SQL写入/身份JOIN断点。已保存7条wire与独立pcap逐字节相符，不能据此声称其余未采内容也完整。

来源日志实际loss_boundaries=81。独立原包与原send/recv区间同时证明18851/19590字节负载存在，原生产端却只接纳16384−64−72=16248字节负载，超限仍推进来源序号，导致长连接退役且后续不能补造握手。全部81个边界尚未逐一证明同因，保留其他真实分配/队列损失可能；独立pcap还报告11个捕获丢包，不能拿它补造缺失原件。

修正先落[唯一分片契约](../contracts/driver-communication/technical.md)，沿原封套/原队列完整传输同次回调取得的正文，不扩大帧、总量或队列额度；删除“单帧容量就是原TCP负载上限”的错误路径。后续新候选另行完整绑定并交原聊天复验，r2与R85失败原件保留。

## R85根因修正后的冻结r3

当前候选 `/tmp/as-group-terminal-package-20261113-r3`，同一原打包入口。完整源码185项核对无漂移；service `dbfda2cc8d66a00dfe409a6636f56f41611f49c71cd81144294b724ab882186c`，module `a4822e2ea5b0ecca9627f7fd2330f396d2a72a01672664b2811353b6dfe4e0df`，image `54ef172241a1821ca814fcf26d2f758940a41db52a690765c37804ca4360db23`。完整原生资产来自 `/tmp/as-group-fragments.yv1jhe9_/native`，原all构建、源/产物SHA及真实原函数边界探针通过。

修正贯穿同回调复制、原子批入原队列、原封套分片、唯一接收泵重组及真实reader join。旧单帧拒绝整包路径已删除，未新增通信栈、通用框架或历史缓存，额度未扩大。R85真实18851/19590字节wire离线经过当前正式Frame codec与接收重组后逐字节一致；它不替代新guest效果。

冻结r3完整本地CI为233项Rust测试、全目标Clippy -Dwarnings、fmt、主模块软件冒烟通过；源与日志绑定在包内local-verification.json。UI源码/dist与r2逐项摘要相同，复用已通过的304+4及174浏览器结果，不重跑无变化矩阵。独立逐行交叉审核与Rust LSP通过。实际OpenClaw验收由原聊天执行，结果另记，不把源码/离线证明称为已交付保护。

## R86 真实复验与流生命周期收敛

r3真实R86正式链仍FAIL，公共源PASS。原件 `/tmp/as-openclaw-evidence-20261113/r86`：4条已核Upgrade、14个初期消息、0派生事实；4个Flow行仍有三类缺口。大包已经完整进原Native/SQL：unframed834/859的18896/19635字节hello wire与原请求ID、浏览器和pcap逐字一致。不能继续归咎旧16KB传输缺陷。

原聊天见证pcap为0 drops；独立逐流复算显示两条正式WS的全部应用字节在SQL中，除最后8字节Close缺实际server recv外，业务正文send/recv区间都完整。字节缺口仅见HTTP资源流18789→50622的三个区间，原cookie 4119/4120；两WS实际端点cookie分别4129/4130、4132/4133。不能据三段缺字节推定它们逐个等于日志的三个来源序号缺口。未改旧SQL或倒填旧时刻socket身份，旧握手未持久化该字段的局限保留。

源码明确在任意全局序号缺口retire_all；完整hello与业务消息随后留为unframed。故按[Group唯一契约](../middleware/agent-graph/technical.md#采样通道与原socket生命周期)区分采样通道和原socket在途上下文：复用现有真实cookie、TCP序号/字节和实际I/O核验，不补字节、不猜丢包归属、不扩大额度。source/gen变更仍退役；没有完整双socket/Upgrade证明的不接续。新SYN独立开始协议上下文，不能把socket身份当成一次TCP连接代，也不新增“必须观察旧终态才允许新连接”的门槛。具体实际退役原因增加原source/gen/ordinal/firstsyn/ports诊断，避免用可达代码风险替代每一次实际因果。

## 流生命周期收敛后的冻结r4

当前候选 `/tmp/as-group-terminal-package-20261113-r4`，source清单185项无漂移；service `717200f9b7effacac2a783ef2b55d43875b5a08c7701317e45d3f1caf109db93`，module沿用已核r3的`a4822e2ea5b0ecca9627f7fd2330f396d2a72a01672664b2811353b6dfe4e0df`，image `897e7b5874dcc14c367e2f2d3d39aeb92e4ccd2eec762237eef8b4f286d6f8e5`。原Native构建19项源输入与本轮逐项SHA相同，资产没有混装。UI源/dist也逐项相同，复用已过304+4与174浏览器结果。

收敛只使用现有NativeGroupTcp原socket字段和现有TCP/WS上下文；新增实际socket身份同握手事务保存。未补RootR86旧时刻字段。正常消息仍要求完整原字节及send/recv，自己的缺口不会越过。新非零cookie initialSYN可以替换旧协议上下文，包含同ISN但明确不同socket的未建立候选；未采用的Gateway早期I/O保留原件，不额外要求旧终态。原控制元数据与真实空负载保留，来源变更/退役原因各用所属source/ordinal。

冻结代码236项Rust全workspace测试、全目标Clippy、fmt、软件Main冒烟及独立交叉审核通过。包内local-verification.json绑定日志；实际原版OpenClaw R87正式结果PASS，见下节；本地与运行证明仍各自按绑定范围记录。

## R87 正式验收通过

原OpenClaw聊天以完整r4服务/Native资产，在隔离guest的同一个原版Gateway进程通过普通WebUI并发提交两个相同任务。`formal-result.json`与`source-result.json`均PASS。104对连续正式WS请求/回执与SQL逐项一致，两组原child run/session严格隔离；4条Flow行表示历史请求重新携带的4次原观察，不表示4个子任务。每行沿原子任务两键取得1份Gateway接纳、6份子本对象终态、2份父公开发布，gaps为空；两个观察客户端保留各自原引用，不合并为额外任务或完成次数。

原库有4条Upgrade/双socket身份、1265个完整消息及4条停采残片记录（1298个原片）、39条派生协议事实。39包括其他运行的终态，不把它全计为两个子任务。全部正常和残片wire逐字节对独立pcap，全部派生字段对原JSON，全部正式WS观察引用/原值对SQLite均PASS；pcap kernel drops=0。任务关联只使用实际Source/ProcessKey、完整原身份及实际TCP/IO，不使用时序或文本相似。

原Source日志loss_boundaries=6如实保留；已证两条Gateway仍凭实际socket/字节/I/O工作，未以未丢任何采样记录冒称全覆盖。缺字节的资源流、停采尾部及不能证明归属的控制仍保留原件/缺口，没有补造。父公开发布不宣称registry耐久delivery或用户已读，registry只作独立见证。

AS、原版Gateway、转发器和QEMU均实际正常退出、模块实际卸载；没有宿主加载/重启。范围为普通WebUI自身原生订阅在场、guest初始网络命名空间的IPv4回环实际来源，网络转发只搬原字节；不外推无客户端或其他来源自动发布。

封存入口 `/tmp/as-openclaw-evidence-20261113/r87`；`formal-result.json`、`source-result.json`、`business.sqlite`、`group-loopback.pcap`、`deployment-binding.json`、`launched-archive-verification.json`及真实退役记录共同绑定。当前原DB SHA256 `c1afe2887d64ea16860d87a9aa74af2a66f62f7d654da589efcae6a308db9fea`。R85/R86失败原件保留，不能用r4通过改写旧结论。

R87最终封存入口为 `workflow.md`、`evidence.sha256`、`scripts.sha256`及`recheck.sh`；后者只读重算，不启动Agent/AS/VM或新业务。补充接纳范围：浏览器/CDP共1251条应用帧，1209条获正常消息接纳，尾部42条不冒称已正常接纳，原观察/pcap/已取得残片各自保留。原日志adapter full=36、source_stopped残片4及source loss6都保留，不把功能三阶段PASS外推为全流无损。详见原 `frame-coverage-diff.json` 与封存工作流；本轮没有为附加全流计数扩大门槛。

主开发另执行原R87 `recheck.sh`，退出0，两个结果与封存报告逐字节相同，原件/脚本SHA均通过；日志 `/tmp/as-group-r87-root-recheck.log`，封存摘要 `/tmp/as-group-r87-seal-binding.json`。该复算不启动原Agent或新验收。
