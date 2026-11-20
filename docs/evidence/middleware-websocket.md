# 正式 WebSocket 写打开闭环

版本边界：本页为多个冻结候选的顺序记录，各节“当前／本轮”、源码行号、执行条件与结论只属该节所列产物。原 FAIL／NOT_RUN 和后续有限通过分别保留；不得把任一节的局部验证合并成新工作树的完整真实交付，也不把历史测试器的计时或完整性检查推广为正常业务前置。

候选位于 `/tmp/as-ws-delivery.KB5bkZEr/candidate`。镜像 SHA256 为 `7ffc6d881e6e8af95b32cb0d81fb03a4731afdce347131b4ed2f970fa2b7c1a8`，as-service SHA256 为 `be1a62dedec9fe7f90d037a0c8a6f770be0a322f9a86890f4a934701f640d6a3`。160 项源码和 18 项产物通过清单复核。隔离 guest 使用与候选相同的服务二进制和原驱动产物，内核为 aarch64 `7.0.0-1019-nvidia`。

真实 guest 通过 WS 首次鉴权，提交错误 DecisionId 时原写打开保持等待。Allow 后原调用实际写入目标文件，Deny 后原调用返回 EPERM 且原字节一致。两次执行结果沿 WS 保留原 DecisionId、EventData 与 RuleHit。断开已绑定的 WS 触发原超时拒绝。错误令牌、未认证业务消息和不匹配 Origin 均被拒绝。业务库保存三条问询和三条执行结果，服务实际退出，原生模块卸载。

早期冒烟在 Ask 等待期间重新打开目标文件，观察读回被同类文件回调排队至原问询超时。最终冒烟改用生效前打开的只读 FD，原规则期限和生产二进制沿同一候选。原件 `/tmp/as-ws-delivery.KB5bkZEr/ws-smoke-r3.log`。Unix 对端凭据测试在支持 SO_PEERCRED 的执行环境通过，相关 Rust 包测试及严格 Clippy 通过。

原版 OpenClaw R55 在该冻结候选上通过两个 R54 场景。两次 `openat(O_RDWR)` 各一次 File.Open Ask，答复前同一原调用仍在驱动等待。Allow 后原文件 35 字节与工具实参一致，Deny 后原调用返回 EPERM，30 字节保持原值。真实 WS 原帧、原调用、业务库和安全日志的身份及结果可逐值复算。原结果 `/tmp/as-openclaw-evidence-20261111/r55/result.json` SHA256 `46d064c1ecc2de74b19cf6c591428b5767d24485256c6dbe2701e9dae8cdd2aa`，本地已执行只读原件复算通过。服务、OpenClaw 与 guest 正常退出，驱动卸载。

R55 客户端发送 Close 1000，原件没有服务端 Close。附带 Unix 聊天订阅状态为 INCOMPLETE，线上 1797 条，库内 1861 条。差异为 63 条已提交正文／修订和一条来源关闭记录。原订阅在停机时先于内部消费结束，库中仍保有尾部原记录；保留线上记录与状态，不回填缺失帧。

协议复核去除 WS 输入整帧复制，统一版本和单载荷封套，普通消息只解析一次类型化载荷。原 Ask 经原 Up 序列化，业务库与线上共享同一剩余时间快照。客户端 Close 在释放原问询能力后由 WS 库完成。该复核后的候选与 R55 冻结候选分别绑定证据。

复核候选 `/tmp/as-ws-audit.1OJwwAhN/candidate` 镜像 SHA256 `80021e7649379a953da0d0f78d9aa286e98cbbfc0c9f028cdbb9e94f020b7eba`，as-service SHA256 `70fc4c262140ecc7ed52e633bad6cba2f6e3d259921bc0dd33aa926e76f4ec03`。在同一 aarch64 内核隔离 guest 验证 Allow、Deny、Close 1000 回帧及按原规则释放在途操作，并对三个 Ask 逐条比较 WS 和 SQLite 的 remaining_ms。相关 Rust 回归与严格 Clippy 通过。原件 `/tmp/as-ws-audit.1OJwwAhN/ws-smoke.log`，guest 退出 0 且驱动实际卸载。原 OpenClaw R55 结果绑定前述初始冻结包。

## Unix 聊天订阅停机收尾

修订候选 `/tmp/as-unix-chat-drain.fneDflzI/candidate` 镜像 SHA256 `62084b675d4a36a0b6deaa15d9caeda3ee9b8c1f809ec28df04194ebbbe64cbc`，as-service SHA256 `1969f0006e95b59dfd3fba9898cb8b08ac95219818b648784f88b6669a7c48d9`。160 项源码和 18 项产物已核实。真实内核隔离 guest 的写打开 Allow、Deny、Close 及断连结果通过，服务退出 0 并卸载驱动。原件 `/tmp/as-unix-chat-drain.fneDflzI/ws-smoke.log`。

软件集成冒烟沿真实 Unix 连接接收原 Accepted，再通过原聊天消费者提交两条记录，客户端从原订阅读到 EOF。两条线上 ChatEnvelope 与 SQLite 查询结果逐条一致。Main、中间层 Rust 回归和严格 Clippy 通过，rust-analyzer 核实宿主调用原 Main 退役入口。软件冒烟原件 `/tmp/as-unix-chat-tail-software.log`。

原版 OpenClaw R56 用该候选复验通过。两次写打开各产生一条 Ask，同一正式 WS 传递原答复和执行结果。Allow 的原 `openat` 返回 FD 200，目标 35 字节与工具实参一致。Deny 的原调用返回 EPERM，目标 30 字节保持原值。双方实际交换 Close 1000，服务、OpenClaw 和 guest 退出 0，驱动卸载。

原 Unix 订阅从空起始游标读至 EOF，1407 条实际线上 ChatEnvelope 与 SQLite 的提交顺序、游标、变体及完整值逐条一致，重复、缺失、额外和差异均为 0。Accepted 后交付的最后一条为 `source_closed`，游标 1407，SQLite delivery 1411，EOF 无残片。普通聊天积压跨 Accepted 由上述软件冒烟验证，R56 的真实尾部为一条来源收尾。原结果 `/tmp/as-openclaw-evidence-20261111/r56/result.json` SHA256 `3f51c6f75fcdb98a88b85d21789386044a193504eced5fe1a960f69a64796d94`，证据清单校验及 Unix 对账只读复算通过。

## LLM 聊天工作台与保护概览

冻结候选 `/tmp/as-ws-chat-activity.QaUksqjU/final2-candidate`，镜像 SHA256 `e0715bcd6f5cf56f0ad72ec6a68300f65c142fb1666de678ae55fb3b49ac59e6`，as-service SHA256 `072875bd1b1476e66490650319d182ef557710b7c4f0799009556bf2230019d9`。已逐项核对 160 项源码和 18 项产物。验收镜像只改 guest 的冒烟入口并加入独立客户端脚本，沿相同服务及驱动产物。真实内核为隔离 aarch64 `7.0.0-1019-nvidia`。

首次鉴权未授聊天权限时，历史和订阅实际拒绝，保护概览仍可读，Driver 成功与 Flow 未运行的原失败分别保留。沿配置入口授权并启动 Flow 后，三次受控 HTTPS 请求完成证书校验和幻影凭证转发，Driver 与 Flow 状态均成功。客户端分页取得两个原交换，沿原消息 ID 读取各页。第二个交换的首次 `Finished` 为 revision 3，后续用量记录产生 revision 4，历史页与 WS 的同 revision 完整值一致。活动视图返回四条带原协议引用的同交换关系，缺工具到系统操作的显式桥时返回 `NoExplicitRelation`。SQLite 回归另外覆盖原生 ToolCall／ToolResult 引用和真实同域 ID 冲突。

首次 WS 实时读取 12 条，断线续读和收尾共 34 条。46 条实际接收游标与 SQLite 原提交顺序逐条一致。第三次请求后以相同游标读取的 Unix 与 WS 16 条收尾 `ChatEnvelope` 逐字段一致。Unix 到 EOF，WS 双方交换 Close 1000，服务与 guest 退出 0，驱动已卸载。

Main 和 middleware 共 66 项 Rust 测试通过，中间层格式检查与严格 Clippy 通过，rust-analyzer 核对分页、业务路由和停机调用。受控 TLS 夹具原结果 `/tmp/as-ws-chat-activity.QaUksqjU/result.json` SHA256 `c5dc3275199dae664e99b45b2028589e1701f8d49437a98ac413b28dd39d5e12`。

### 原版 OpenClaw R57

原版 OpenClaw 在同一冻结候选完成一次真实 `read` 工具调用、最终回复和三个实际 HTTP 交换，八项验收均通过。正式 WS 实时收到 12 条，按原游标续读 397 条。409 条原帧与 SQLite 提交游标及导出的完整 `ChatEnvelope` 逐条一致。分页读取 3 个交换、13 条消息，按原 ID 和实际 revision 核对，其中一条历史回复后的后续修订由原帧证实。

三个活动查询核对原消息和协议值，工具结果视图包含一条有证据的原生工具引用，原工具调用仍明确报告缺少到系统操作的执行关系。实际 `ManageService`、`chat_content=false` 的连接收到 0 条聊天帧，订阅和敏感查询被拒，保护概览仍返回 Driver 与 Flow 的各自结果。独立 `ReadOnly` 角色在 R57 未实测。停机时订阅游标已在 SQLite 最终位置，双方交换 Close 1000，OpenClaw、AS 和隔离环境正常退役，原生模块卸载。R57 停机期间没有新增聊天帧，非空收尾由上面的隔离实机验证覆盖。

原结果 `/tmp/as-openclaw-evidence-20261111/r57/result.json` SHA256 `3b4fdb836d5b1942d851490e8e4379a1443b05ef7f589788c1fcf1b006da3888`。已复核三项冻结产物绑定、WS／SQLite 对账及 `evidence.sha256` 中的 40 项原件，清单 SHA256 `ab1d47f965025eaa05756a295a393f8136dfb39bf61190daeab3997752ec860d`。

## 防护管理正式 WS 闭环

冻结候选 `/tmp/as-ws-protection.yRnOp4Pl/final-candidate`，镜像 SHA256 `48f659bbbcb51e168bf98fd568446513593fbeb939c7c1ec181f0fa1d52259d9`，as-service SHA256 `a598061a74ebc4ba11eb3ce770f5b87cdd9f372729cc577a3248240192f52d95`。161 项源码及 18 项产物逐项匹配。隔离 guest 内核为 aarch64 `7.0.0-1019-nvidia`。

同一 CommandId 的两条命令分别由原 WS 会话收到实际回执。Remembered 组创建、查询、版本冲突、重载和删除沿原规则接口，原文件操作分别观察到拒绝、临时关闭时允许、恢复后拒绝、删除后允许。临时关闭未改变配置版本，普通重载保留覆盖。配置更新和重载返回真实保存、采用及冲突状态。改为只读后两个旧会话失效，重新鉴权的 `ReadOnly` 读回原配置，规则重载、配置重载、临时控制和私有凭证操作均被拒绝。

私有 WS 完成公有 CA 创建和读取、测试秘密导入及轮换、phantom 签发及撤销。真实隔离 HTTPS 夹具使用轮换后的材料，原请求通过证书验证并返回 200，撤销后同一 phantom 的新请求失败。服务与 guest 正常退出，模块卸载。交叉审核发现的 WS 回执逐帧撤权检查和缺失回执关联已在该候选修正。Main 和中间层共 66 项现有 Rust 测试及相关严格 Clippy 通过。隔离原结果 `/tmp/as-ws-protection.yRnOp4Pl/result.json` SHA256 `c79f526e0e1821505eb653d84b9eff64c46561a064919ac1b350f8d77477a788`。

### 原版 OpenClaw R58

同一冻结候选四项均通过。原版 OpenClaw 的四次独立读取依次为 `EPERM`、读到原文、`EPERM`、读到原文。Remembered 规则保存、驱动实际采用、查询、版本冲突和删除都可用原回执复算，两次拒绝与安全日志的原事件、路径及规则版本一致。临时控制的实际状态按关闭／恢复变化，配置版本保持不变。

WS 配置保存、实际采用及读回一致，旧版本返回 `conflict`。降权使命令发起连接和旁证连接同时失效，发起连接未收到写回执。实际 `ReadOnly` 会话重新鉴权后读回已保存配置，规则、配置、状态读取通过，四类写入或私有操作被拒绝。

私有 CA、秘密导入／替换和 phantom 签发／撤销取得原回执，两个实际已鉴权旁证收到零条私有回复。绑定实际 Gateway 进程及 GLM 路径的真实模型调用成功，撤销后的独立请求取得 `scope_rejected/revoked`，该交换没有提交上游。2076 条公开聊天投递与 SQLite 逐值一致。OpenClaw、AS、guest、QEMU 和浏览器实际退出 0，模块卸载。

首版离线核验漏解原 `GetRule` 的 `File` 变体，原误判和脚本保留，使用同一批原件只读复算通过。结果 `/tmp/as-openclaw-evidence-20261111/r58/result.json` SHA256 `6f9b1e4376b369cb1494117008743678ea464672df4f436985298e19d5b5f569`，37 项证据原件摘要已复核。

## 连接接入问询路由审计

连接、撤权、问询槽位及原回执分别对照 [Main 连接](../backend/main/messages/technical.md#主模块代码接口)与[中间层问询契约](../middleware/technical.md#原问询闭环)。移除传输层可答复标记，路由观察原 Main 连接的实时可用性。槽位拒收时原 Prompt 退回同一媒体消费者，接纳后由单一槽位持有。独立审核核对授权寿命、锁序和拒收前的原 Prompt 归属。

用 R58 冻结产物复现。Unix 会话保持打开且原状态查询已返回未授权，新的真实 WS 会话已登记可答复。旧路由仍选中已撤权的 Unix 会话，新 WS 未收到 Ask，只收到该原文件操作的超时拒绝，调用返回 EPERM。原件 `/tmp/as-review-route.YrchCDzu/baseline-confirm.log` SHA256 `6d9cd8b9fa5f1d9bc1c6c7dc900fb302606a9c61cc357b8f35b0cdeed60b193c`。

修订候选 `/tmp/as-review-route.YrchCDzu/final-candidate` 的镜像 SHA256 `5cfbf1aeaea90ede86675bd335992ffef4d12a45c3ed12ee5327ccb043c5c39f`，as-service SHA256 `8ccf244cba0d68b640a9936533f80abdab440692fff00b5219ea12b95dc46304`。161 项源码、18 项产物已核。在同一内核隔离 guest 复验同一文件问询场景，新 WS 收到原文件 Ask，答复前原操作仍等待，Allow 后成功，原 EventData 与 DecisionId 匹配完成回执。服务和 guest 退出 0，模块卸载。66 项 Rust 测试、相关严格 Clippy、格式与 LSP 引用核对通过。原件 `/tmp/as-review-route.YrchCDzu/final.log` SHA256 `7236cc4b4d52e4c8978c1bedf8a4329a5e2fb62e1c7f274df153b4143ef2b7fc`。该修订候选执行隔离实机定向复验。

## 问询协调选择归属

规则模块的原 Prompt 统一提供 Allow 和 Deny(None)，Main 用同一选择生成 Ask 和校验原连接的答复。冻结候选 `/tmp/as-prompt-choices.jRUTfPmP/candidate`，镜像 SHA256 `84bd63780f5d3728e3c2ba0f2305693ac616f822dcf913bfbe3167d2b7c6205f`，as-service SHA256 `a6f82ac0d473492184acb349f72f056f2aa562065fe9e7192e8d4d618ea64124`。161 项源码、18 项产物已逐项核对。

同一隔离 aarch64 内核实测三次真实文件 Ask，WS 的两项有序选择与 SQLite 三条原问询一致。错误连接和 ID 返回 forbidden，未提供的终止动作返回 invalid，答复前原操作保持等待，重放返回 stopped。原操作分别按人工允许成功、人工仅阻止返回 EPERM，以及所选连接关闭后按该规则的原 Allow 回退成功。各实际结果保持原 DecisionId、事件和命中，服务和 guest 退出 0，驱动卸载。

规则、Main 和中间层 92 项 Rust 测试及相关严格 Clippy 通过。LSP 核对唯一的选择读出及所有 Main 投影调用。交叉审核确认现行原生冒烟的人工答复使用 None，直接裁决及超时／断开场景继续覆盖原终止范围。隔离结果 `/tmp/as-prompt-choices.jRUTfPmP/result.json` SHA256 `c3c94839a12601dac5c42d7df4567d226c8c82c411926ca9b652178392716eaf`。

### 原版 OpenClaw R59

原版 OpenClaw 在同一冻结候选完成两次真实写打开 Ask。WS 和 SQLite 的选择顺序均为 Allow、Deny(None)，两条原 remaining_ms、原事件、规则和结果逐值匹配。Allow 场景先提交同一 DecisionId 的未提供终止动作，返回 `request_rejected / invalid`，原线程身份、openat 参数及等待栈前后相同。随后合法 Allow 返回原 FD 203，实际 35 字节与工具实参一致。Deny 的原 openat 返回 EPERM，实际 30 字节保持原值。两次均保留同一 DecisionId 及原执行结果，WS 双向 Close 1000，OpenClaw、AS 和隔离环境正常退役，驱动卸载。

结果 `/tmp/as-openclaw-evidence-20261111/r59/result.json` SHA256 `a50241aad2282a6633df7fcf52321a6964fb389051da23b10de1697476796d38`。44 项原件摘要、两条 SQLite 选择记录和原等待证明已复核。错误连接、错误 ID、重放及断开回退由前述隔离实机证据覆盖。

## 活动关系作用域复核

在既有 SQLite 用例复现另一 HTTP 交换的记录被误用来证明工具结果。修订后，同一 Flow 实例但交换序号不同的原记录仍可追溯，活动视图和 SQLite 均无由该证据产生的 NativeToolReference，并保留实际冲突。已有同交换工具引用仍可读。交叉审核确认 SameExchange 与 QuestionCompletion 共用活动模块校验，存储层避免对同一消息重复写入已存的同交换关系。库表及对外信封沿原定义。

独立候选 `/tmp/as-activity-scope.GQksiIrP/final-candidate` 镜像 SHA256 `7fb6ddf3f7b5facb633d3c5756d7d9949a164812144ce49af6457644b757218e`，as-service SHA256 `909c8e7916528d4ea6fedc617e1e73928e42943996c4164af98a72a4c78b74b2`。隔离 aarch64 guest 的三次受控 HTTPS 请求及正式 WS 活动联查通过，四条同交换关系带原协议引用，46 条原游标与 SQLite 提交顺序一致。Unix／WS 16 条收尾逐值一致，正常退出并卸载驱动。92 项 Rust 测试、严格 Clippy、格式和 LSP 引用核对通过。原件 `/tmp/as-activity-scope.GQksiIrP/result.json` SHA256 `089d08b5cc9da8b5ee2ef0acda87af06908443e958a1735ca0ec88e70d13da65`。该活动修订候选执行隔离实机复验，R59 证据绑定上一候选。

## 仓库脚本化前端

使用 `package.sh --frontend-ws` 与现有 `run-guest.py` 在 aarch64 `7.0.0-1019-nvidia` 运行正式 WebSocket 客户端。冻结候选 `/tmp/as-scripted-frontend.In2lhxBd/final-candidate`，镜像 SHA256 `a18516fa4bd70988ecc1c49ead493fea367776897912e6bd5c1e572e3ee15da5`。163 项源码和 20 项产物摘要已核对。

6 项检查均通过。Remembered 规则实际产生拒绝、临时关闭后允许、恢复后拒绝，配置保存与采用读回及公有 CA 专属往返通过。两条原 Ask 的允许与拒绝产生对应原生文件结果，并与 SQLite 两条选择记录一致。两次受控 HTTPS 交换产生 12 条实时和 18 条续读记录，30 条原游标与 SQLite 提交顺序匹配。原消息修订分页和活动联查通过。原管理会话撤权后，实际 ReadOnly 会话可读状态，规则修改及聊天历史被拒绝。所有自有子进程正常退役，QEMU 和 guest 退出 0，Native 模块卸载。

原 host 报告 `/tmp/as-scripted-frontend.In2lhxBd/final-run/report.json` SHA256 `a5221d6c79fd90250b23228f8d26d5d3b9d31e4a88919d0c61c07565c274c320`，guest 报告同目录 `middleware-report.json` SHA256 `9551a3c77123e6ee604d180df1563bc308bb859ad3f5aed10262f88297bb07c3`。首次入口缺少 Unix 夹具管理端点，原失败保留在 `/tmp/as-scripted-frontend.In2lhxBd/run`，修订候选复验通过。本轮使用受控 HTTPS 夹具。运行命令见[脚本入口](../../scripts/mvp-smoke/README.md#scripted-websocket-front-end)。

## 正式 WS 运行审计

冻结候选 `/tmp/as-runtime-audit.KQ4aQW1u/release-candidate`，镜像 SHA256 `f40e7ccdbe005b5c5938316737a486ccceac2f13c5b44992111e3517fac3edc7`，as-service SHA256 `d9419c1379373faa256830c41a8aa7a06b5675898d023762fc09d54b1b3bd49b`。163 项源码和 20 项产物逐项核对，隔离 aarch64 内核为 `7.0.0-1019-nvidia`。

脚本化前端 7 项通过。两个原 Ask 的实际 ProcessKey、节点及规则命中从正式 WS 读回。进程分页保留原游标，跨筛选条件返回 cursor_expired，未观测到的容器实例无匹配，显式刷新以原 ID 返回 unsupported。4 条安全日志按实际行号倒序分页，服务退出后与原 SQLite 逐值相同。ReadOnly 可查进程状态，完整日志请求返回 unauthorized。两条原 Ask 和 30 条聊天游标保持一致，服务、guest 和 QEMU 退出 0，驱动卸载。

92 项 Rust 测试、相关严格 Clippy 和 LSP 核对通过。独立审核确认原 owner、日志授权及分页边界。脚本改为等待实际 ProcessKey 和日志提交，不把探针就绪或收到完成消息等同于后台已消费。隔离报告 `/tmp/as-runtime-audit.KQ4aQW1u/release-run/report.json` SHA256 `57695b91436e5429d05b3a742bbc441ad96373e16c2829f6ccb3648774e6d14c`。该 guest 未启动 Docker，正向容器归属不在该次实测范围。

### 原版 OpenClaw R60

同一冻结候选六项通过。两次原版真实文件操作分别写入成功和返回 EPERM。正式 WS 读到实际 Gateway 的完整 ProcessKey，两条原规则各命中一次，首末 EventKey 与原事件相同。进程分页原游标可续读，跨查询返回 cursor_expired。实际 Workload 为 null，正向容器归属未运行。

ManageService 的一次总页和 10 个单行页均与只读原 SQLite 逐值相同，行号从 10 递减至 1。目标记录 9、10 对应原事件、DecisionId、RuleHit、选择和实际结果。正式 WS 显式刷新以原 CommandId 返回 unsupported。原管理会话换权后结束，实际 ReadOnly 读回进程状态和原详情，完整安全日志请求返回 unauthorized。双向 Close 1000，OpenClaw、AS、guest 和 QEMU 实际退出 0，驱动卸载。

原结果 `/tmp/as-openclaw-evidence-20261111/r60/result.json` SHA256 `8db0a72fdf64661c0c14c894c50c9640eb6deea8a4938954d2626d197402eb85`。42 项原件摘要、实际 WS 11 条日志查询回执、SQLite 10 条原记录和权限拒绝已独立复核。

### 原版 OpenClaw R61 正向 Docker 归属

同一冻结候选在真实 rootful Docker bridge 上六项通过。独立 Docker Inspect、进程 cgroup 和命名空间核实目标及对照容器的完整身份。正式 WS 的原 Gateway ProcessKey 在工具运行前后保持一致，非空 Workload 的 runtime、namespace 和完整 ID 与原 Docker 事实一致。正向筛选的 31 个节点中原 Gateway 恰好一个；对照容器的两个节点均归对照容器，排除了原 Gateway。

原版 OpenClaw 的真实 read 对目标路径返回 EPERM。原 ProcessKey 上该规则命中计数为 2，首末 EventKey 与两条安全日志及独立原操作回执相同，WS 安全日志页与原 SQLite 逐值相同。两个实际容器、服务、Docker、guest 和 QEMU 正常退役，驱动卸载。两个命中独立按原事件对账，工具到系统操作的精确因果仍依赖真实执行来源。

原结果 `/tmp/as-openclaw-evidence-20261112/r61/result.json` SHA256 `dedfacadeb7271a545d5db29675f99fb6115821e867697f7b6223d112249caed`。33 项原件摘要、两次 Docker 身份快照、正式 WS 正反筛选、原命中和 SQLite 原值已独立复核。

## WS 停机时客户端提前关闭

2026-11-12 在同一 aarch64 `7.0.0-1019-nvidia` 隔离内核对照 R67 冻结服务与修订服务。两边使用逐字节相同的五项原生资产。夹具在实际 Flow 已接纳一条 HTTPS 交换后，从 Unix 管理会话发出原 `RequestStop`。收到实际写出的 `Accepted` 后，仍在订阅的 WS 客户端主动发送 Close，并收到 Close 1000。修前服务退出 0，未报告订阅收尾缺失。修后服务实际退出 1 并报告 `chat_drain_incomplete`，guest 和 QEMU 仍退出 0，模块实际卸载。首轮误从 WS 发出停止命令，正式入口拒绝，原结果留在不带 `-r2` 的目录。

修订只用原 writer 确认的最终 SQLite 游标判定聊天退役。管理回执写出、Close 握手、最终游标写出、客户端实际接收和资源退役各有自己的证据。提前关闭仍释放原连接和问询身份，错误沿既有宿主退役结果返回。[停机契约](../middleware/technical.md#聊天订阅停机收尾)

- 修前服务 SHA256 `fbd1eda23a58d461403fbd33e5ea5d10ab3eda86cc6381e2fe82d766c73a1d6b`，原报告 `/tmp/as-alignment-early-baseline-r2/run/report.json`，SHA256 `19fd28a5bc31c2b54715efe1928d36c13bf44f486f5f2282785d4b0a355ac474`。
- 修订服务 SHA256 `49f368fdf7757af61c040f97f886063324996ecfe4c795103876e1a69f656dd5`，原报告 `/tmp/as-alignment-early-fixed-r2/run/report.json`，SHA256 `a48116141533ca01cd9cee8a9b97fa1e2e0fb3cc0781b635952a352b01ebbe4d`。两份报告的具体退出与诊断见各自目录的 `middleware-report.json`。
- 同一修订服务的正式脚本化 WS 九项通过，原报告 `/tmp/as-alignment-reviewed-guest/report.json`，SHA256 `e52a35f3c6930634f473493d6f28139f79919f3bf8977f666104339e8f246734`。194 项 Rust 测试、严格 Clippy、rustdoc 和格式检查通过。OpenClaw R67 原件只读复算通过，验收范围绑定原冻结服务。

## Dashboard 与冻结服务的真实联调

2026-11-12，使用 `19d19f45a` 对应冻结包的 `as-service`、BPF 和 Native 模块，在 aarch64 7.0.0-1019-nvidia 隔离客体中加载。Chromium 打开 Dashboard 的生产构建，通过只转发字节的客体连接访问真实回环 WS。浏览器完成鉴权、登记问询可用和查询防护概况。

R68 前端的严格事件字段表缺少现行 `EventData.user_stack`。实际 Ask 到达后，前端拒绝该帧并断开。已在 Dashboard 候选同步当前字段表和问询夹具。`user_stack` 必须存在，保留原对象或显式 `null`。同时把概况测试中已经过时的中文文本断言改为核对消息键，翻译检查继续由现有三语检查负责。

修复后，浏览器对两次原文件打开各提交一次 Answer。允许操作的 EventKey 序号为 41，原打开成功。拒绝操作的序号为 56，原打开返回 `EPERM`。两次回执均为用户选择所致，`DecisionId`、完整 `EventKey`、原事件和命中规则与各自 Ask 一致，页面显示对应结果。服务接受停止请求并实际退出，模块卸载成功，QEMU 退出码为 0。

该轮 Dashboard 测试和生产构建通过。随后读取的同轮候选包含测试收集配置，构建产物与真实浏览器使用的文件逐字节一致。此次使用独立文件操作夹具，OpenClaw 证据沿用原有 R66/R68。

原件目录为 `/tmp/as-dashboard-live.wysLy2aO`。`result.json` 的 SHA256 为 `63bd7ec4a7b7bd68518bb8829944f123d9ee488aaa16ede597a577119826bf1b`。`delivery.json` 记录前端源码、实测构建及证据摘要，`dashboard-source.tar.gz` 保存可构建源码。`first-attempt/` 保留浏览器语言选择器的夹具失败，`contract-mismatch/` 保留实际字段错配的失败帧和客体日志。

## 活动详情原话的离线复核

2026-11-13 在当前重构候选回放 R66／R68 原件。R66 三条已确认工具交接各保留一条原请求用户消息，R68 两条候选均未附原话。新回放库关闭重开后逐值一致。定向回归验证最近用户投影的正文缺失或凭证作用域冲突时，活动视图保留缺口，较早的消息不会成为预览。53 项中间层测试、Clippy 和 LSP 检查通过。

同一份原件在独立 Dashboard 副本复现了原逻辑漏显示。原请求与响应的 `ProtocolSource` 方向和观测时间不同，前端的完整对象等值比较把有效用户消息滤掉。修订后的前端展示逻辑通过相同原件复核。严格解码检查仍因该副本要求 Ask 携带现行后端未发送的 `timeout` 字段失败。该副本的全量构建另报 `processes.tsx:174` JSX 语法错误。本轮没有执行真实浏览器全链路验收。[活动详情契约](../middleware/audit/technical.md#原驱动事件为锚的行为链)

原件与对照位于 `/tmp/as-activity-recheck.qb0RPHJs`，见 `rechecked/graphs.json`、`dashboard-projection-before.log`、`dashboard-projection-originals.log`、`dashboard-originals.log` 和 `dashboard-build.log`。

## 联合候选的 Ask 默认动作

2026-11-13 使用同一联合候选，在 Chromium 和 aarch64 `7.0.0-1019-nvidia` 隔离客体之间通过正式 WS 鉴权和登记可答复。三条原 Ask 均收到规则绑定的 `Deny(None)` 默认动作。实际用户 Allow 的文件打开成功，实际用户 Deny 和无人答复超时均返回 `EPERM`。Esc 未关闭未完成问询，超时期间客户端没有发送 Answer。三条原 `DecisionId`、事件和命中与完成回执相符，超时回执的 proposal 等于原 Ask 默认动作，原因是 `timeout`。服务和客体正常退出，原生模块已卸载。

首轮的夹具结果读取上限与规则期限同为 15 秒，未得到第三条完成证据。复验只调整隔离夹具的规则期限和观察等待，前后端产物字节一致。210 项 Rust 工作区测试、仓库软件冒烟、277 项 Dashboard 测试及 4 项字典检查通过。SQLite 重开和冲突重投由定向测试核对，旧库拒绝打开。

最终浏览器原件位于 `/tmp/as-joint-integration-final.pIVchTss`，见 `result.json`、`browser-frames.jsonl` 和 `artifacts.sha256`。首轮失败位于 `/tmp/as-joint-integration.aC2sdVWH/result.json`。本段使用独立文件操作夹具，未启动新的原版 OpenClaw 验收。

同一服务二进制和原生模块另在隔离客体运行两条原版 Agent 的并发 A2A 委派。正式鉴权 WS 返回两条完整 Group 记录，原 HTTP 字节、工具、任务、模型交换及 SQLite 逐项一致。用原完整 `ProcessKey` 查询进程详情，并读回实际 Flow 的 `ChatExchanges` 和选定交换的 `ChatMessages`。当前仓库 Dashboard 严格解码上述真实入出帧通过。此部分使用现有 WS 客户端采集，再由 Dashboard 解码复核。原件位于 `/tmp/as-joint-integration.aC2sdVWH/group-original`，见 `group-chat-ws.jsonl`、`report.json`、同级 `group-verified.json` 与 `live-contract.log`。

## R75 Group 原交换和拒绝反馈视图

联合聊天和 OpenCode 完成 Dashboard 消费，正式业务层只读取已保存的反馈事实。冻结候选服务 SHA256 为 `0ba70e9b63bca979366072bd16aea43c4bcbb2cc9413d07d3dda23d0c01a2966`。210 项 Rust 1.89 测试、严格 Clippy、LSP、281 项 Dashboard 测试和三语构建通过。另一隔离客体上的 Chromium 正式 WS 验证了真实文件允许、拒绝和无人答复超时。原件及摘要见 `/tmp/as-identity-group-proof.ysaJTlu4/native-browser`。

R75 在同一冻结服务上使用未修改的原版 OpenClaw。两条同参文件操作的正式 Ask、实际允许和拒绝、原完成及正常退役通过。两条原 EventKey 的 `ActivityContext` 返回各自实际完成；库中无反馈或领取记录，WS 也没有反馈事实。两条 Group 接纳回执分别通过正式 WS 读到其引用交换的 8 条和 6 条原消息，Dashboard 使用这批归档的真实 WS 响应复验跳转。正式 Group 继续标出子终态和父回投未观察。独立 Gateway 原件及只读登记核实了子终态和父回投。

反馈阶段展示的正控是独立 SQLite 组件夹具。真实入库后的 BusinessData 原编码逐项验证 `submitted`、仅领取和竞争冲突三种情况；该正控未调用真实上游。R75 原件见 `/tmp/as-openclaw-evidence-20261113/r75` 的 `result.json`、`context-result.json`、`dashboard-result.json` 和 `recheck.sh`。R74 的来源复核及原生 Session 头路径见[运行时证据](runtime-correlation-research.md#原生模型路由携带的会话字段)。

## 原应用 Session 的正式查询链

本轮先核对冻结原版原生 OpenAI 分支，再在 R76 实际请求取得会话头和正文 metadata。
原件先取得、再分析、最后断言的流程和 R76 夹具失败见[运行时证据](runtime-correlation-research.md#先采集原件再作验证)。
这补正此前只覆盖自定义 provider 的验证范围，不把未捕获的 Header 写成不存在。

正式代码把路由选定的 `SessionField` 随同原 ProtocolObservation 保存；SessionObservation 不替代 provider
Conversation，也不冒充 Agent ID。业务库按完整原来源域直接查询，SessionRef 仍为原 ProtocolRecordId；
随后复用 ChatExchanges 和 ChatMessages。Header 重复、空值、缺来源、重复 JSON 键及解析质量缺口保留原件，
不确认会话分组。字段、分页和权限唯一契约见[中间层技术文档](../middleware/technical.md#应用会话目录)。

冻结后端 `/tmp/as-session-final.921ac0ds`：service SHA256
`a91ccfbc032033f8c3c52e31373f6587a375e78d791555da44ce97b43d5648e0`，原生模块未修改，SHA256
`cf9d5de8439a6ee8e5f70d4970892feac1e0916688ecc48723f648dc80b25e63`。source.sha256 逐项核对无差异。
当前本地验证：Rust1.89 工作区全目标213项通过；1.92严格Clippy、fmt、Rustdoc通过；既有Python5项通过；
现有软件冒烟真实退出／订阅尾部／停止回执通过。LSP核对6个正式声明及调用引用，相关5文件诊断无错误。
只读交叉审核及受审哈希在 `/tmp/as-session-cross-review.md`，修正了所选会话接受其他会话分页游标的问题，
未新增事实源或聊天缓存。组件原编码帧在 `/tmp/as-session-component.jsonl`，不冒作真实 OpenClaw。

R78 在首个业务请求前初始化失败，OpenClaw 未启动、场景未提交，结论 NOT_RUN。该包误用了动态链接debug二进制；
`as_self_protection.c` 的 static_elf 检查按契约返回 EOPNOTSUPP。
原 package.sh 已明确静态链接参数，此次是打包操作遗漏，不修改或绕过驱动自保。R78原件在
`/tmp/as-openclaw-evidence-20261113/r78`；后续改用按现行参数重建的静态包，另记真实验收结果。

正式静态交付回到已有 `scripts/mvp-smoke/package.sh --frontend-ws`，原脚本负责编译、静态ELF检查、strip、
复制、权限和产物／源码清单。唯一包 `/tmp/as-session-fixed-package-20261113`，image SHA256
`d22a6bff2bf548be9da0fe1443a985a5f07fedd2522006ff7a223f6fe9c65264`，包内service SHA256
`f9f4d10b23170fa40e6343c2808e43626c96d64213b32c28cc9701a43bbdc302`。
R79 已接纳的 pre-strip静态service为 `4c7f46af807c4bbb8f4848e3c91247a5f7078061db886d0b51dd8f4a6a1636bf`；
对该原件执行原脚本的 `strip --strip-debug` 后，与正式包内文件逐字节相同，证明原件为
`/tmp/as-session-strip-proof.djtnj1f7/result.json`，不以该变换证明尚未执行的运行场景。
Dashboard整合后285项Vitest、4项i18n和生产构建通过；三语言320px组件交叉验证的原件在
`/tmp/as-session-dashboard.ksISzD/verification`，独立OpenCode复核及最终撤权状态收敛同目录。
前端源码和dist哈希、完整本地验证日志索引在正式包目录；真实运行结论待原验收聊天给回执。

固定包首轮真实联调的 Native 初始化、问询 Allow/Deny、管理、进程及写关闭均通过；聊天步骤因旧 Python
期望未写入新增的 `ProviderRoute.session=null` 失败，整体仍为 FAIL，原件在首包 formal-ws-run。
本轮只同步既有 middleware.py 的配置结构和 frontend.py 的查询字段，不放宽比较、不加假 Allow、不改生产代码。

最终固定入口产物 `/tmp/as-session-fixed-package-20261113-r2`，image SHA256
`5ab67474422bdd859ba2376f8a8887841c6459de8c5a1f1468be1d7acb002b7c`；service 与首固定包逐字节相同。
直接复用现有 run-guest.py 完成10项真实内核WS联调，报告 formal-ws-run/report.json 为 passed=true，
guest_exit=0、Native实际卸载、QEMU实际回收。管理、原Ask允许/拒绝实际效果、进程与写关闭审计、聊天续读／历史／活动、
候选查询、ReadOnly、实际停机及原SQLite均通过。不包含 OpenClaw，R79原版结论单独认定。

R79 沿原版 native OpenAI 路由完成同 Session 两次及另一 Session 的独立调用，实际 Ask Allow 和 Deny、
原调用等待及文件效果通过；隔离栈正常退役。已取得4组原 Session 目录、9个原交换与其消息，
另22份未携带所选 Session 字段的辅助请求保留原件。最终会话来源对账以同目录 native session 报告为准。
原生origin在Flow和SQLite均含显式`:443`，保留原值；离线脚本不能先要求省略默认端口再判断原件。
Group 的两条接纳回执仍保留 child_terminal_unobserved / parent_delivery_unobserved；
独立Gateway和registry已取得的终态、父回投不冒作正式旁路来源，固定拒绝提示仍未实际发送。

R79 的 `session-result.json` 已完成来源对账，status=PASS：原Gateway sessionId、原生header/body、Flow原件、
正式WS目录与筛选、消息和SQLite原值及完整来源域一致。31份原请求中9份带所选Session、22份辅助请求
缺字段并保留原件/SQL Missing；4组、9交换、57份返回消息修订通过核对，实际observer_capacity Gap为0。
这不宣称跨Agent身份、Group终态或精确工具执行归因已经存在。

联合前端独立核对R79归档原WS，报告 `/tmp/as-r79-frontend.qboocq0e/report.json`：17个聊天页保持原树，
4组Session→9个Exchange→9个消息页、57条原消息的引用和三语渲染通过。
另2条ToolAttribution回执不属于当前Dashboard消费接口，明确列unconsumed；外部调用者前两条查询省略after
也保留原形，不替原件补null。该验证是归档WS的解码、原查询编解码和渲染，不冒称新的实时内核执行。

R79最终回执已冻结，`recheck.sh` 用原件逐项重算并比较通过。`session-result.json` SHA256
`0943080aa6a014793a156bf2c6ce90587a1c6e63d4ace895da6d4712629d686a`，
`dashboard-result.json` SHA256 `9997fdffd0f5dfb13d7b431b2887ad0e787b06603c6ef0052775b086a04dd820`。
冻结Dashboard对原WS回放完成两组Session→Exchange→消息与两条Group→原交换点击；明确是归档原回复回放，
真实内核执行依据仍为该轮原Ask／效果原件。原子任务终态/父回投的独立原件通过，正式来源缺口不变。
完整入口 `/tmp/as-openclaw-evidence-20261113/r79/workflow.md`；R76失败、R77未运行、R78错误打包失败均保留。

## 审计候选与完整 Ask 窗口消费

本批复用原AttributionRequest／ToolAttribution／AskAuditContext，未新增后端字段、关联器、规则或内核采集点。
候选条件显式提交，秒输入用BigInt精确换算纳秒；Activity和Ask沿当前读槽核CommandId、原身份和pattern。
合法结构的错身份只令本次读取失败，原Ask仍可答。统一内容资格修复复用另一控件批次已核的ReviewSession单owner，
不以聊天授权拒绝撤掉问询或管理权限。完整原窗口复用OriginalChatMessage，保留20条内的全部原引用、角色、parts、
空值、修订、缺口和冲突；旧三条截断路径删除。查看原交换仅收起展示，原时钟/答复能力保留，返回入口可恢复。

冻结UI `/tmp/as-audit-consumption-final.e5rgw92w`，固定npm build入口及明确默认WS
`ws://127.0.0.1:21854/`见delivery.json。ui-source.sha256与主树逐项无差异，dist清单独立保存。
后端直接复用固定脚本包 `/tmp/as-session-fixed-package-20261113-r2`，本批没有改Rust/驱动或手工拼装后端。

当前适用本地CI：301项Vitest及4项i18n检查、TypeScript编译和生产构建通过。
一对一交叉审核27个源码/测试/文档，未发现未修复阻断，报告 `/tmp/as-audit-consumption-cross-review.md`。
复用锁定TypeScript原生语言服务核对6个声明的调用引用，语义诊断为空；没有安装独立TS LSP程序，不冒称该工具运行。
沿另一控件批次既有Peer和标准Playwright入口，英文桌面及日文320px组件工作流通过：显式表单、两候选、20条完整
消息及工具结果、原交换导航、返回原Ask及一次答复。原件 `/tmp/as-audit-browser.5v9zs97d/browser-final.log`，
首次环境/验证脚本失败与截图保留；不是新原生保护验收，也不靠修产品去迎合旧夹具。

原版OpenClaw由原验收聊天独立执行；结论按本批实际取得的原件记录，缺源不补造。

R80原件保全在 `/tmp/as-openclaw-evidence-20261113/r80`。该轮冻结Dashboard没有取得正式WS握手或鉴权，
本界面消费目标为NOT_RUN；另一个已登记工作台收到的原Ask／期限回执独立保留，不能替代本UI证明。
`connection-diagnosis.json` 的ERR_CONNECTION_REFUSED探测发生在VM退役后，只能说明该探测时刻的状态，
不能反推原初失败原因。不据此改产品、增加端口回退或放宽鉴权；原验收聊天另沿活体传输原件定位。

R81-transport在仅AS存活、无OpenClaw业务的条件下核对同一冻结dist、正式WS、Origin和凭证：
页面route.fulfill方式触发Chromium `ERR_BLOCKED_BY_LOCAL_NETWORK_ACCESS_CHECKS`，真实HTTP服务加载则
正式鉴权成功且console无错误，转发原WS独立客户端也成功。原件在
`/tmp/as-openclaw-evidence-20261113/r81-transport/connection-diagnosis.json`及`forwarded-authentication.json`。
据此复用既有真实HTTP提供方式，不修改产品、不放宽鉴权或关闭浏览器安全检查。离线回放的页面供给
不能未经核实直接当作真实网络验收上下文；传输诊断成功仍不等于本Goal的OpenClaw业务已经验收。

R82已取得真实HTTP101/鉴权/review声明，但旧测试转发器select.select的65秒空闲截止在首次Ask前关闭连接。
原WS的最后帧到socket_closed约65062毫秒，与该入口实际代码一致；本UI消费仍为NOT_RUN，原件保留。
只修测试入口生命周期，保留受控VM寿命和原20秒Ask，不以假ping、产品端口回退或延长规则期限规避。

R83复用同一冻结UI、固定r2服务、模块与规则范围，`audit-result.json` 为PASS：同一真实Dashboard连接
接收原Ask，先普通空归因阅读，再显式提交条件，读取完整原窗口、打开原交换、返回并实际答复。
Allow取得1候选/4条原消息，Deny取得2候选/各4条原消息；角色、parts、空值、修订、引用与Flow源及SQLite一致。
两项答复在原20秒期限内约4.04秒和5.64秒发出，实际效果、原回执、安全SQL、后续和最终响应通过。
真实取消普通读取后，旧回复约49.99毫秒才到，界面保持cancelled而未复活读槽。候选始终为候选，确认工具执行边和反馈事实均为0。

现场断线重连步骤未运行：额外脚本用了短显示文本Lock，实际控件可访问名称为Lock management；原DOM及错误保留，
不据此判产品失败或修改控件迁就脚本。本轮其余取消/重连/跨连接/失权边界归本地软件检查与交叉审核范围，
不冒称新的原生实测。原件入口 `/tmp/as-openclaw-evidence-20261113/r83` 的audit-result.json与file-effects.json。

主开发另用原verify-audit.py离线重算并与冻结audit-result.json逐字节比较通过，结果SHA256
`336c26c75567181ad71a9862007027abf4fbc4faca7e624d357576a9c638f302`，
独立输出 `/tmp/as-audit-r83-root-recheck.json`。未重跑OpenClaw或引入新的通过条件。

R83最终入口为 `/tmp/as-openclaw-evidence-20261113/r83/workflow.md`，audit-result.json、file-effects.json和
joint-result.json均PASS；主开发执行最终recheck.sh，原件／脚本摘要和各项离线复算逐字节比较通过。
本轮窗口实际parts为text/tool_call，未在窗口取得tool_result；真实工具返回由joint-result独立核验。
组件中的完整角色／工具结果／20条边界与现场4条窗口按各自来源记录，不补造缺少内容。

唯一持久复用入口已固化到原验收聊天的
`$WORKSPACE/.runtime/openclaw-audit-acceptance/run-owned.sh`，README、entry.json与entry.sha256
绑定同一冻结模板、内核、运行盘、UI和已验证本地浏览器依赖。已实际执行--prepare-only，新目录准备包与R83实测包
逐字节相同，通用复算入口对R83通过，固化后新增模型业务为0。后续复用该入口，不另从单轮临时副本拼实现。
