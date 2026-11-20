# 中间层业务日志与消费边界验收

版本边界：本页按 R3、R16、R17、R62 等原候选分别记录实现与结果。开篇的“安全日志／正式WS未实现”、旧工具直写入口及“当前”均是对应历史时点，不是现行功能清单。后续代码只按其自己的真实运行范围认定；现状由[文档索引的唯一契约](../README.md#跨模块契约的唯一出处)维护。

本轮删除底层聊天回放、查询快照、盲目完成保管和整交换倍数预约；Flow 唯一消费输出经 Main 交中间层结构化 SQLite。配置只派生固定数据库路径；Middle 保存原聊天、工具、关系及交互业务事实，底层安全日志持久模块仍未实现。

代码与文档按 backend／drivers／middleware／contracts 归组，唯一入口为[文档索引](../README.md)。Ask20条、命令核对和已确认关系复用[审计模型](../middleware/audit/technical.md)，存储见[业务日志](../middleware/business-log/technical.md)。Agent 调用图谱仅完成三文档预留；通用工具执行桥、Agent根绑定、正式WS仍未实现，缺桥明确Gap，不把原生引用、PID、命令或时间相同当因果。

## 本地与审核

完整 workspace 本地测试138项通过、strictClippy／fmt／rustdoc／doctests、Python客户端及真实来源路径的软件配置冒烟通过，原件为 /tmp/as-final-workspace-*-20261109.log 与 /tmp/as-final-configuration-smoke-20261109.log。该轮之后交叉审核确认的两处修正，单独完成中间层22项回归、strictClippy与Rust1.89检查；原件为 /tmp/as-middle-final-fixes-*-20261109.log。最后Python客户端修正复用原测试，/tmp/as-middle-r3-python-20261109.log通过。不是以旧全量代替后改验证。

真实SQL回归覆盖原值重开、None／空值／完整u64、真实失败不返回Stored、明确桥20条固定原修订、无桥不取全局、Delta直接入库、重开claim只报Unconfirmed、不重放。已开始SQL的waiter取消后，原工作仍由同一owner保管，失败由blocking closure观察；首次窗口冻结仅写入开始后的失败计数，正常读取缺口不触发写入失败。

LSP引用／诊断原件 /tmp/as-typed-main-lsp.log、/tmp/as-flow-realtime-lsp.log、/tmp/as-middle-final-lsp.log，无活动代码错误。有限交叉审核 /tmp/as-typed-main-review.md 与 /tmp/as-middle-final-diff-review.md收口无剩余必修；源码快照在对应source.json，未把后改倒算入旧审核hash。

## 真实Native联调

最终R3产物 /tmp/as-middle-sql-delivery-r3-20261109；原报告 /tmp/as-middle-sql-run-r3-20261109/report.json，28项必须场景通过，另1项底层安全日志持久化明确未实现。实际驱动加载、文件／进程／网络允许／阻止与终止、Ask答复／超时／断开、受控TLS／phantom／原始聊天、权限撤销和真实退出均按该报告范围认定。普通宿主UID运行QEMU，guest root执行；QEMU退出0、真实join和Native卸载确认，宿主未加载模块、未改MOK／信任或Root代理。

内核及Native均匹配7.0.0-1019-nvidia、aarch64。147份源码、18份产物与R3逐项匹配，/tmp/as-middle-sql-r3-verification-20261109.json保存清单hash。R1是较早代码通过；R2最后权限场景因Python观察客户端漏识别business_reply失败，原件 /tmp/as-middle-sql-run-r2-20261109保留，不回填通过。R3补齐当前客户端格式并验证关联Unauthorized回复；不是扩大规则、关闭校验或改产品动作换绿。

## OpenClaw边界

此记录是本地组件与真实Native guest联调，不是本轮真实OpenClaw通过。新冻结候选按用户要求交原专门聊天执行，Root只交需求、标准和元数据。旧R15通过不覆盖本轮；当前调用图谱只预留设计，不增加验收门槛。最终候选与OpenClaw原件以交接metadata绑定，实际结果另记。

## R16 原候选真实 OpenClaw：FAIL

1731d9329762c796b493f9960ede7936eefaed1f 的 R16 已由原专门聊天执行完毕，整体 FAIL；不能将前面的本地／Native通过扩称整体验收通过。唯一原件 $WORKSPACE/.runtime/openclaw-middle-sql-validation-20261109/result.json，SHA b858eeab81897a49291294b241c11558fd34c2100873da34ba098191ccdaf712；handoff.json 为 COMPLETED/FAIL，原失败不回填。

普通聊天和普通拒绝通过；普通允许、Ask拒绝、Ask允许的原Agent终态失败，日志含真实 uv_resident_set_memory ECANCELED。各会话只发送一次；允许工具实际成功不替换原 aborted 终态。两个Ask及时显示及提交原选项，后续模型请求／工具结果／响应保存，但原上行OperationResult两ID均未观察到，SQL为questions=2、operation_reports=0。不能从实际效果补造完成回执。

本轮 AS 自身 SQLite WAL 写产生了大量 as-self-protection/backend-write Direct通知；最后Main采样rejected_notices=164769、rejected_logs=140754。停止前Native dropped=191、pending=1；这些故障域不能和Flow active/lost/parse=0混为无损。代码证明通知及日志队列拒收本身不直接cancel；adapter/Main关键回调接纳失败存在真实cancel链，但未将具体UV失败唯一归因于聚合计数。

4450原Chat游标逐项匹配SQL，3708协议记录及工具字段／原始字节／引用核对一致，SQL自身写失败计数0；缺的是完成回执供给／消费链，不是已证明的SQL提交丢失。原始两个body_dropped_before_eof Gap如实入库，observer_capacity=0；四次只读业务查询成功，两个Ask无桥时返回明确Gap及空agent/trigger/window，未伪造20条。

测试结束后OC／AS／guest／QEMU均0，真实join与Native卸载，所属残留0，无强制清理。原宿主OpenClaw和boot未改。后续按用户新要求清除可信自身写通知，并修复已接纳原操作完成在队列满时被丢弃的问题；新候选须另绑源码／产物复验，本节仍只记R16原事实。

## R16之后的有限修复与本地复验

用户明确要求已鉴权AS自身写自保目录／文件在驱动最快放行。实现复用原实际controller、exec实例、保护对象及初始化fence判定，+1立即走统一清理返回，不分配／上抛允许事件；删除backend-write允许分支，真实外部-EPERM及self_protection.log选择保留。没有按UID／名称授信，没有扩大队列或延长超时。旧自保通知只放大实际负载，不把其丢弃计数直接改写为已证明的UV取消原因。

原操作完成消息改为由原async处理上下文等待已有有界队列接纳，六出口均await，删除仅try_send的queue_notice；不新建任务／队列／额度，不改实际决定或执行结果。满队列回归表明第二个真实处理ctx保管完成，消费者释放后两原DecisionId和实际结果均交付；Direct／状态及普通日志仍是原明确非阻塞路径。退出仍先排空处理ctx，消息消费者随后关闭。

完整workspace139项、strictClippy／fmt／rustdoc通过，/tmp/as-self-fastallow-workspace-*-20261109.log。Driver现有build.sh真实构建新KO07104c84cf02850abb322d748dd28990800a76409063f39b93a18b0ee768b6ca，有匹配7.0.0-1019-nvidia的BTF／BTF.base／非零BTF_ids；复用已有pahole工具，初次缺pahole失败及最终Kbuild warning原件保留。clangd及Rust引用检查通过，不冒称宿主load。

新真实Native guest /tmp/as-self-fastallow-run-r1-20261109/report.json通过28项必须场景，真实join／卸载、guest/QEMU退出0；代码与产物 /tmp/as-self-fastallow-delivery-r1-20261109，147源码及18产物逐项匹配，验证 /tmp/as-self-fastallow-verification-20261109.json。有限独立交审 /tmp/as-self-fastallow-completion-review.md无必修。上述是新修复的本地／Native证明，真实OpenClaw需要新候选另行执行，不能拿此段或旧R15覆盖R16失败。

## R17：两个修复重点通过，整体仍FAIL

333eb2f320283faab66733a0445221093339ae1f 的真实R17原件 $WORKSPACE/.runtime/openclaw-self-fastallow-validation-20261109/result.json，SHA4ed49040a0626a15801eaa4a244bbdc99c54908fcb578e42e2e0538b5b3d96e8。普通聊天、普通拒绝、Ask拒绝、Ask允许四项通过；普通允许原Agent终态error，原文 Agent database execution generation was replaced，工具尚未启动。此为逻辑执行代际检查，不证明物理数据库文件被替换；整体验收仍FAIL，未重发失败请求。

已证明：4169次实际聊天提交，自身保护范围的写类上行及旧backend-write通知／RuleLog均0；两Ask原DecisionId／完整事件／RuleHit／User提案及actual outcome在原Up与SQL一致，questions=2、operation_reports=2、question_completion=2。外部哨兵同UID1000、同0666文件在自保前可写，启用后真实OpenClaw写入EPERM且内容不变，原Native deny-modification日志存在；未观察到该精确EventKey独立公开Up，不能补造。

公开4168 Chat游标及3499原协议等逐项SQL一致，DB另存公开peer关闭后的真实source_closed收尾游标4169，不能当作公开帧或假Gap。公开Gap0／observer_capacity0；活动实例最后Main samples notices1/logs0，Media logs9/not_forwarded3，Native停止前pending1/receipts0/dropped0；各域不可混为无损。真实退出OC／AS／guest／QEMU均0、join／卸载及owned残留0。

独立后续诊断 $WORKSPACE/.runtime/openclaw-r17-causality-diagnosis-20261109/result.json，将20条原Kernel unbound_wait超时日志以完整原sequence／kind／operation／actor对应到唯一原Up／EventKey；不是PID／时间推工具因果。4条是AS消息线程自身TCP443连接，16条是原OC文件打开，均约30秒原保管到期。实际Node v24.22.1/libuv1.52.1机器码及同版主源证明uv_resident_set_memory路径为/proc/self/stat（非statm）；其中原OC主TID的sequence47481被AS持有30.111813824秒。已证明AS等待／超时介入，不能称纯Agent或观察器故障；完整用户态代际替换触发链和具体队列／锁阶段仍不虚构唯一因果。

R17源码147文件另冻结于 /tmp/as-controller-net-base-333-source-20261109，逐项匹配原manifest，供下一修复期间仍可核原证据。接下来只收口真实控制器自身维护网络不回到自身ToUser，普通Agent网络保护不变；不延长期限、扩大容量或改目标Agent换绿。旧R16／R17 FAIL均保持。

## 关闭等待取消与唯一退役结果

定向反例在旧实现上先接纳一条实际 SQL 工作，在关闭等待排空时丢弃关闭 future，再释放原 SQL。接纳已经封闭，原 SQL 工作结束，SQLite 连接仍未关闭。修订后数据库 owner 在首次 close 调用即保管退役任务。重复关闭调用共享实际结果。既有回归验证关闭观察者尚未轮询时已封接纳，并验证最后一个关闭等待者取消后仍实际关库、失败 SQL 事务回滚且失败计数保留。

隔离 aarch64 候选 `/tmp/as-business-retirement.c6Gqc4vv/candidate`，镜像 SHA256 `e0025b787a2b2c5e875742a51f0e8cd4ff58e30873795816fe033e46d96bffc6`，as-service SHA256 `dd87cfe49ee95d33627a6fa499c7053e0b5e236c50fec311b8fdd4ad2094b2f4`。163 项源码和 20 项产物摘要已核对。脚本化前端 7 项通过，核对两条原 Ask、30 条聊天游标及 4 条安全日志，服务、guest 和 QEMU 退出 0，原生模块卸载。92 项 Rust 测试、相关严格 Clippy、格式和 LSP 核对通过。报告 `/tmp/as-business-retirement.c6Gqc4vv/run/report.json` SHA256 `b9f92af45b7070a4f88354799a6d26fd1db2afefaeb79c59204f7cd3e2ff7b05`。真实 Docker 的 R61 验收绑定其原冻结候选。

## 工具执行交接消费

定向组件验证沿现有消息队列提交受信交接，两次提交只保留一条关系记录。原事件或原协议缺失时关系保持不可确认。原事实到齐后，活动查询使用同一原调用，Ask 使用原交换的冻结窗口，拒绝反馈准备只引用原拒绝。加入指向同一事件的另一工具引用后，从 Ask、工具项和原消息查询均返回冲突，窗口和反馈领取均不选择调用。复核覆盖请求历史和流式未完成响应不得成为执行关联，以及同一工具结果重复领取按业务状态返回。

候选 `/tmp/as-tool-handoff.TKXfoWwW/final-candidate` 使用 aarch64 内核 7.0.0-1019-nvidia，镜像 SHA256 `cd5b6418e25d855876ff69ed8126384b7a175dceb15712c98fb28f5a69f93c52`，as-service SHA256 `b405f32955e8296634b4dd7179a1e03958862994f22b94b5c5277f82142ef6bb`。168 项源码和 20 项产物清单 SHA256 `7a4aba109a001d8e039181b438ba615c3a98bb7740d5e6e14b0224552bb58abd`。Rust 101 项、严格 Clippy 和脚本化前端 7 项通过，guest 和 QEMU 退出 0，原生模块卸载。报告 `/tmp/as-tool-handoff.TKXfoWwW/final-run/report.json` SHA256 `18958a348098f5edc438ae087045d08fb4c41d9b0fe73fa300c76ce3fbd2d700`。该候选没有真实底层交接生产者，正向交接由定向组件测试验证。

### 原版 OpenClaw R62

同候选通过原版 OpenClaw 写打开 Allow／Deny。原内核调用均在答复前等待。Allow 的原 EventKey 序号 45161，原文件实写 35 字节。Deny 的原 EventKey 序号 50914，原调用返回 EPERM，原文件 30 字节保持。两个 DecisionId 的原 WS Ask、答复、原回执及 SQLite 原事件与规则逐值核对。原版 OpenClaw、服务、浏览器、guest 和 QEMU 正常退出，原生模块卸载。

待决和完成后的四份 AskContext 及两个原工具项联查均返回 `no_explicit_relation`。四份 AskContext 的 `trigger_call` 和 `window` 为空。保留 6 次真实 HTTP、1235 帧 SSE、1460 条公开投递。Deny 命中的原规则含非空固定说明，后续原请求保留 EPERM。反馈、反馈领取、Ask 窗口和已确认的 tool_execution 均为 0。精确 ToolCall→EventKey 与原 HTTP 交接的正向验收均为 NOT_RUN。

原件 `/tmp/as-openclaw-evidence-20261112/r62/result.json` SHA256 `be4db1c17449d4b50e1a85f4740f3f6872801e61a9b44d8337f9f212a38f64cf`。46 项原件摘要、两个原 WS／SQLite 记录、四份联查、实际文件摘要及 Close 1000 均已独立复核。进程和网络 Ask 使用同一产品产物另行[实测六项](process-network-rule-integration.md#工具执行交接候选复验)。
