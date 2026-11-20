# 规则固定拒绝反馈的实现与证据

版本边界：本页按各节候选记录历史实现与验收，“本轮／当前”限定该节时点。旧管理会话许可、显式 previous 限制、可选事务 deadline／Expired 等不作为现行接口；2026-11-15 的 Flow 源码已移除我方网络、反馈事务及排空计时，没有 0／正值截止模式。现行边界见[Flow技术](../backend/flow/technical.md)及[执行反馈](../middleware/execution-feedback/technical.md)。原候选 hash 与 FAIL／NOT_RUN／有限 PASS 保持，未完整运行的新候选不继承它们。

本轮源基线为2538376f414f75cc7f5b03f91f087f4f55fcee4e；验证对象是其后的本轮变更。实际guest包的131个源码输入与当前树逐一SHA256相同，核对原件为`/tmp/as-fixed-feedback-final-source-verification.json`。文档不替代原运行记录，后续代码变化不自动继承本次结果。

## 已实现范围

规则可配置`deny_message`，固定原文随原命中快照保留；Driver派生拒绝回传标记与`log`独立。Main只登记内部真实完成，不接客户端自报拒绝文本；中间层消费者仅在可信显式执行关系及原事实一致时提出引用请求。Flow独占修改，使用原管理会话的内容权限及撤权令牌。

当前精确反馈仅支持Responses的显式跨请求引用及已核认证范围。原工具错误、ID、角色和状态保留，固定文本追加一次；原始观察与修改后的证据分别保存。Prepared、Applied和实际上游write／flush后的Submitted保持不同事实。Chat Completions和Anthropic采集仍可用，但当前固定反馈不支持这两类profile。通用工具执行关联生产者、正式WS和聊天持久库未实现。

字段和接口唯一出处分别是[规则](../backend/rules/technical.md#固定拒绝提示)、[Main消息](../backend/main/messages/technical.md#flow-输出及内容授权)、[Flow公共契约](../contracts/flow.md#固定规则拒绝反馈目标契约)和[中间层消费](../middleware/execution-feedback/technical.md)，本页不另定义结构。

## 真实内核与TLS链路

原件目录`/tmp/as-fixed-feedback-native-tls-chain-run-20261109`包含`report.json`、`middleware-report.json`、`qemu-full.log`及派生索引`feedback-chain-summary.json`。

该次由普通宿主UID1000运行QEMU，guest root加载实际Native模块；29项required通过，持久日志和聊天库两项如实为NOT_IMPLEMENTED。guest及QEMU退出0，子进程已join，Native实际卸载。未使用宿主Root broker、未加载宿主模块，未操作OpenClaw。

新增完整链路场景由客户端解析实际Responses工具调用，保管原调用后执行真实`File::open`。正式目录范围规则没有PID／探针名称条件，`log=false`；原操作实际返回`EPERM`。同一客户端的可信受控执行上下文供给原调用与原事件关系，中间层生产消费者形成仅含两个引用的请求，经真实Unix管理端口准备。

客户端随后发送原工具错误。真实TLS上游收到原错误加固定原文一次，原`call_id`／`is_error`与角色缺省保持；实际取得对应Submitted。该执行关系由受控执行者明确提供，`stock_application_bridge=false`，不能称为普通应用自动关联。

冻结包为`/tmp/as-fixed-feedback-native-tls-chain-20261109`，镜像SHA256为`7276b7b0194e7ebb4d55ff0c7551f25d7d54781a6de9213b9f087aebf21e9b4a`；匹配kernel为`6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6`，Native模块为`575aab0dcc32e7546e6ba7213f6fb7633b6ec26a7476461e121985542dced230`。这不是宿主加载或OpenClaw新功能验收。

原OpenClaw专门聊天另行独立复跑同一冻结包，原件位于`$WORKSPACE/.runtime/openclaw-fixed-feedback-validation-20261109/controlled-native-tls/`。该次29项required通过，142.68秒，guest／QEMU退出0、join与Native实际卸载通过；`independent-fixed-feedback-check.json`逐值核过实际EPERM／Denied、log=false、原错误及其他字段保持、首尾空白原文一次、实际Submitted。`source-input-verification.json`的131项输入全部匹配。该复跑仍是受控显式执行关系，不能扩大为stock自动关联。

## 普通OpenClaw自动关联

原专门聊天只读审计了R13原件与实际安装的OpenClaw2026.10.9／build8b18b55，结果为`$WORKSPACE/.runtime/openclaw-native-binding-audit-20261109/result.json`，同目录保留原字段与源码SHA。

- LLM协议原生工具引用、Gateway工具开始／更新／结束，以及AS原事件／规则／结果分别有真实来源，不能把记录同时存在当作执行因果。
- 部分exec更新直接携带`toolCallId`及`sessionId`／PID；这是实际生产者发出的局部关联。更新在spawn后由输出触发，不保证短执行、无输出或启动失败时存在。会话`startedAt`为墙钟，不是内核出生标识；shell／wrapper PID不代表每个后代的具体操作。
- read实现没有将`toolCallId`传入文件操作；write源码也未提供逐文件操作绑定，且R13没有执行write，不能把源码审计算实测通过。
- OpenClaw内部工具ID清洗包含模式、碰撞和出现次序处理；内部有原ID映射，现有对外证据没有完整导出。AS保留各来源原值，不自行删除字符建立别名。
- AS已提供进程生命周期键、原事件键及执行回执；缺的是执行者权威提供的“这次工具执行对应这些原操作”关系，并在需要的执行前阶段可用。现有证据不足以实现通用、唯一的自动闭环。PID、父子关系、命令、路径及时间接近均不补造该关系。

现有`ProcessKey`可证明同一进程实例；TLS事实证明连接建立者，不能证明每个请求当前写入者，也不能自动把子进程拒绝回传给父Node。进程域通知是另一项产品语义，本轮没有用它替换精确工具反馈。

375836399的新一轮真实OpenClaw分项验收已完成，单入口为`$WORKSPACE/.runtime/openclaw-fixed-feedback-validation-20261109/result.json`：五个真实HIPS／Ask流程及Driver／Backend实际命中的固定原文、log=false必要回执通过；stock工具结果没有固定原文，RuleFeedback事实0，通用桥仍未实现。完整LLM原事实采集本轮为NOT_PASSED，两项Ask的后续／最终wire字段不全；后续被动截面也未补出。随后对原始流定位到exchange11／14的三条observer_capacity，原因及当前修复见[采集接纳](capture-admission.md)；不能以Flow的lost／parse计数0改判旧运行完整，也不能用旧R13通过覆盖它。该采集问题与工具关联桥分别处理。

## 本地检查与交叉审核

最终workspace／all-targets测试、严格Clippy、fmt、严格rustdoc及已有Python六项检查通过，日志为`/tmp/as-fixed-feedback-final-*`。首次测试因沙箱禁止loopback socket失败，普通UID相同命令重跑通过，原失败日志保留。LSP用于接口与引用检查，不替代编译或真实保护证据。

生产增量及真实夹具分别由独立代理只读交审；生产审计原件为`/tmp/as-rule-feedback-implementation-review.md`，夹具审计为`/tmp/as-rule-feedback-smoke-independent-review.md`。已证的显式响应冲突、同域原生ID歧义及真实body失败分类问题已修并复核。没有用旧R13通过记录覆盖本轮新反馈能力，也没有触发远端CI。

## 2026-11-14：反馈职责收敛与主仓库只读核对

本节与上面的历史运行分开。当前用户要求保留“Agent 向 LLM 上报工具执行完成时追加规则原文”，拆除无效修改和过度设计。本轮不改 syscall 返回值，不修改 Agent 源码，不新增关联来源。

代码删除 `FeedbackAccess`、`RuleFeedbackPermit`、会话权限过滤及无人消费的 Main 反馈端口转交。规则编辑与聊天查询的既有鉴权保持；固定反馈执行改为窄 `FlowFeedbackPort` 参数，取消仅对应未完成反馈上下文。规则固定文本与真实 Denied 仍取原 MainNotice，不要求另取 NativeRequestReturn。工具／原记录／实际认证域的校验仍由现有消费者及 Flow 执行，缺关系不选择发送目标。原 Prepared／Applied／Submitted 及真实终态、SQL 去重和在途释放机制复用。

主仓库 `$LEGACY_WORKSPACE` 在只读核对时为 master `7b90c202bc371722c71c8a65c593aba71fdd9542`。检查的是实际源码及调用方，不迁移其架构、缓存或同义结构：

- `as-egress/src/proxy/events.rs::emit_parsed_chain_events` 从同次响应产生上下文和 ToolCall，共用 AS 生成的 trace_id，并保留原 call_id、context_event_id 和 archive_id。
- `cmd/agentshield/src/daemon/tasks.rs` 还可将工具锚点 trace 写入容器独占成员的 CGROUP_INTENTS，已有 trace 的系统事件按它进入链。这是 cgroup 范围登记，不是每个工具请求独立持有的交接键。缺 trace 时，`as-telemetry-storage/src/sqlite/chains/ancestry.rs` 沿进程实例和父链查找；`chain_stitch.rs::tool_call_hit_at_agent_pid` 最后执行事件前按 timestamp 倒序 LIMIT 1。该查询不是原工具对象向 syscall 的明确交接证明。当前原生 kernel payload 在 `chains/ingest.rs` 中明确不进入旧 UTC/PID 链；不能据此声称原生四段已经接通。
- `chains/tool_result.rs::originating_call` 按归一化 call_id、容器及实际取得的进程／实例／会话限制查候选，冲突不建边；Result 的 parent 是 ToolCall，不是每个内核操作的返回。归一化契约及来源限制不作为重构分支的技术依据。
- `as-policy/src/fanotify/review.rs::resolve_tool_call_id` 优先读 chain_context，但旧分支还可查询容器／进程最近调用。`l4_denial_cache.rs` 按容器和归一化 call_id 消费拒绝票据；其原始归属仍受上述来源限制，不因后续 ID 匹配变成准确因果。
- `as-egress/src/proxy/events.rs::apply_round2_l4_enrichment` 在下一次 LLM 请求的 role=tool 消息修改 content；真实调用位于 `proxy/forward/mod.rs`，安装 enricher 位于 `cmd/agentshield/src/daemon/proxy.rs`。主仓库当前实现会替换正文。本轮按用户最新要求保留原正文并追加，继续复用重构分支已跑通的 Responses 写入器，不恢复旧拒绝票据缓存或最近调用回退。

本地 workspace/all-targets 编译、严格 Clippy、LSP 引用与诊断、实际 HTTP/1.1 与 HTTP/2 TLS 反馈测试、业务 SQL 路由回归及 UI 协议检查分别记录于 `/tmp/as-feedback-cleanup-20261114` 及 `/tmp/as-feedback-cleanup-*.log`。初次沙箱 loopback bind 的 EPERM 单独保留；同一命令以普通用户在允许本地 socket 的环境复验。测试中的明确工具关系不冒称 stock OpenClaw 生产关联。

独立只读子代理复核本批 diff、原契约、当前调用方和测试日志，未发现阻断问题；未运行新 VM，报告为 `/tmp/as-feedback-cleanup-20261114/review.md`。

当前仍未接通的是正式服务自动反馈调用及通用工具执行语义来源，唯一现状见[执行反馈缺口](../middleware/execution-feedback/technical.md#必须如实暴露的缺口)。本轮取消前端绑定不构造这段来源，也不把旧受控夹具或主仓库源码当作新候选 OpenClaw 验收。参会 VirtualBox 的实际验证已由用户交给 Bot，本聊天不再继续操作该验证环境。

## 2026-11-14：正式完成消费者接线

本批次基线 `6260fdfe56cd4813b07b75b698065e5979b226c9`。Main 初始化真实 Flow 后把其窄反馈端口随唯一 ChatReceiver 传给 MessageModule；Media 一次安装该依赖。原 Completed 提交原 notice 后调用现有 execute_feedback，不依赖前端，不新增队列或拒绝票据缓存。业务库保留配置固定文本的真实 Denied，即使 log=false、没有 Ask 或原生工具事实；其他普通系统结果继续按原业务归属保存。已有明确 ToolExecution 才调用 Flow；候选和缺失不升级为关系。

生产反馈不增加新时间窗口：可选 deadline=None 沿实际聊天消费源取消和 Flow 退役收尾，显式有限调用使用 Some(Instant)。保留原容量、SQL claim、准备／真实写出／终态释放；原工具上行正文只追加原规则文本，原结果、ID、状态及原始协议记录保留。当前写入范围仍是具备原 response 引用和真实认证域的 Responses；没有扩大到其他 profile 或补造工具语义来源。

本地 workspace/all-targets、严格 Clippy/fmt 通过。原实际 HTTP/1.1／HTTP/2 TLS 追加测试改以正式无限定时间事务验证；业务 SQLite 回归沿 Media.run 的真实完成入口，核无前端／无 NativeReturn、缺关系不调用、明确关系准备一次及原消费源退出取消。LSP 确认 execute_feedback 已有 Media 生产调用；全部 consume_chat 接口／示例迁移，无旧签名或兼容别名。最初 middleware 测试中的 socket EPERM 单独保留，允许普通 UID 本地 socket 后完整复验通过；非反馈分流旧断言按本次持久化契约修正，未吞产品错误。

独立只读审核未发现可确认代码缺陷，修正一处 backend/gaps.md 对正式调用入口的过时描述。原件 `/tmp/as-fixed-feedback-live-20261114/review.md`；本地日志 `/tmp/as-fixed-feedback-live-*.log` 和该目录 LSP 原件。

原打包脚本生成冻结候选 `/tmp/as-fixed-feedback-live-package-20261114-r1/candidate.json`，196项源与编译输入无漂移。service SHA `8d81a6aabe65c2fabb9cacf19e479f159b16b49b129a14e6bd77c85ec92470c6`；原 ko SHA `3b728144f813782aad03ecf687161ba09eb2779f1580b85f91e6482267d1b813`；镜像 SHA `842b3e05a8a83e45c02b8c6e51b0c9cc8d0d38c655d0e215aedce97a0d8d9d88`。原 OpenClaw 聊天已接收候选和分项标准，按用户要求优先真实 GLM，且 OpenClaw 只持 GLM 幻影 key；真实 Key 归 AS 凭证模块，正式受控 TLS 出口注入。直连可用性探测不算该链路验收。验收尚未返回，不把本地真实 TLS、代码接线或打包升级为 stock OpenClaw 自动精确反馈通过。

R92 的正式初始化 ready，但夹具两条规则重复 number=1，被原规则校验拒收，模型业务0、cases=[]，保留 NOT_RUN，不算产品初始化失败。早期说明误称 order，已由当前 Rust PolicyDocument 和实际文档复核纠正。只读核 live-report.json SHA `dbc561d971142e99ee6cacecb2b9e04fd04ff9f421bde4c9cd7f5fa12069ccb3`。R93 修正时又误加入模型不接受的 order 字段；实际反序列化复现 unknown field order，管理连接报告 invalid_frame，未发现 Native／业务 SQL 失败，业务仍为 NOT_RUN。两次夹具失败与异常收尾分别保留；同一冻结服务继续用于修正 number=1/2、删除未知字段后的下一轮。按用户要求走官方 GLM Responses 与 AS 幻影 key，不改 Agent 或补造工具来源，不把尚未执行的结果写为通过。

额外只读协议复核：R91 派生库的 client_request/open_ai_responses 原 native ID 行中 ToolCall 与 ToolResultReference 各2，PreviousResponse=0。官方 Function calling 示例明确携回原 response.output 再追加对应 call_id 的结果，无 previous_response_id；当前 Flow 的强制 previous 检查是已声明的窄支持限制，不是协议必要条件，也不能视作内核因果证明。独立边界复核确认该限制属于普通 stateless 上报覆盖缺口；R1／R93冻结未改。可行扩展须仍保留已核 EventKey→ChatPartRef，只在同一真实认证域核原调用的完整字段及同请求结果，已有引用冲突不得绕过，并共用正常修改和未修改收尾；不把仅有数量、裸 ID 或相似参数升级为内核因果。参考[官方调用循环](https://developers.openai.com/api/docs/guides/function-calling)与[智谱接入](https://docs.bigmodel.cn/cn/coding-plan/quick-start)。

## 2026-11-14：普通 Responses 上报收敛到同一选择器（R2）

普通完整历史上报已接入原反馈模块：有同域待决反馈时，Observer 在原请求解析／Codec 参数解码后生成一个私有 RequestTarget，DecodedRequest 和未改写观察共用该 Arc。它仅保管原引用、零基 input 下标、原 ID 标量及工具名与完整 Codec 规范化参数的 SHA256；没有参数正文副本、聊天历史缓存或第二次参数解析。原响应引用与同请求原调用重放共用选择器；已有 previous／conversation 冲突不绕过；可选 item ID 仅在两侧取得时核冲突。内核 EventKey→ChatPartRef 仍须由原业务证据明确提供，不能用该协议选择器生成因果。

独立审核发现并已同根修正：Codec 请求原先通用 messages 优先会把错误根字段的参数绑定到 input 的 ID，现按 profile 读取正式字段；工具参数的完整和流式解析改复用现有严格 JSON 解码，重复键不能先覆盖后参与确定匹配；新增 item ID 和选择信息拥有的 Vec／标量堆字节纳入原容量，超限按实际未修改路径收尾并释放，不增额度。无本域待决时不生成选择信息或哈希。反例覆盖错误根字段、重复内层键、原引用冲突、字段缺失／改变、重复调用／结果；未修改 TooLate 与正常写出共用相同选择依据。

现有本地真实 HTTP/1.1／HTTP/2 TLS 测试同时覆盖显式响应引用和同请求完整调用两种方式：实际使用幻影凭证、保留原工具结果与状态、规则原文只追加一次，真实 write／flush 才报告 Submitted。首轮新 TLS case 暴露测试上游也用 previous_response_id 识别结果；该过拟合已退役，上游按原 function_call_output 语义接收并回显实际正文。原失败日志保留，未把修夹具称为产品修复。

最终 workspace/all-targets、严格 Clippy/fmt、LSP 及独立只读复核通过。日志 `/tmp/as-feedback-stateless-*.log`，LSP、审核、打包在 `/tmp/as-feedback-r2-20261114`。最终 R2 使用原打包脚本：`/tmp/as-fixed-feedback-live-package-20261114-r2/candidate.json`，196源及编译输入无漂移，同源副本 `/source`。service SHA `e698322d65766135f658bb896abe9b6bef8ae710bdb0d98964c6ec4de987c813`，原 ko SHA `3b728144f813782aad03ecf687161ba09eb2779f1580b85f91e6482267d1b813` 不变，镜像 SHA `f17a8d2141e9cdf62c9fc346599de587477e098fb90fc475bd2937d6e8ca54e1`。R1原产物与源码副本保留，不把正常开发树变化称为其原件消失。

R94在旧R1被本轮替换后，0业务时取消宿主业务控制；既有初始化完成后按原 RequestStop 正常收尾，AS／Node／VM退出，旧候选没有新增模型业务。R92／R93错误与异常收尾分别保留，不回写为通过。最终 R2 已交原 OpenClaw 聊天，按用户要求走 GLM 官方 Responses、AS 幻影 key、正常 WS／OpenClaw 浏览器流程；实际验收尚未返回。本地 TLS 不替代真实 GLM／stock OpenClaw，当前不宣称通用精确工具来源已接通。

## R95：真实 GLM 幻影链与候选不改写

原 OpenClaw 聊天使用最终 R2，公开原件 `/tmp/as-openclaw-evidence-20261114/r95-glm-responses`。实际测试编排镜像 SHA `26c83e5e76e708218e642ed04429e743e735cc1a7f89e70e6ffaaec1bc5f4acd` 与基包镜像分别保留；服务 SHA `e698322d65766135f658bb896abe9b6bef8ae710bdb0d98964c6ec4de987c813` 和原生资产保持该候选。不是 R1 或旧轮次的替代验收。

- 真实上游响应 model 为 glm-5.3-flash、profile=open_ai_responses、origin=https://open.bigmodel.cn:443；未启动合成 provider。6组凭据 Prepared→AppliedToRequest→Submitted 逐组核同一原绑定、真实 Gateway Actor 和上游，GLM 幻影替换 PASS。真实 Key 只导入 AS，OpenClaw只持phantom；不从模型名或端点宣称账户费用为零。
- 正式脚本 WS 前端与原版 OpenClaw Chromium 页面完成两场景。Allow 的原 write 工具报告写入25字节，目标效果核验通过；Deny 原工具报 EPERM／isError=true，目标不变。两次原 Ask、用户答复与实际完成在原 WS／SQL 一致；Deny 的 log=false 与原固定文本保留。AS Dashboard 浏览器界面 NOT_RUN，不能拿脚本 WS 代替。
- 实际后续上行保留原工具内容、call_id 和错误状态，同交换真实 GLM completed 文本与 Gateway final 相同。两个上行均完整携回 function_call 与 function_call_output，没有 previous_response_id，与本轮去除多余协议前提的依据相符。
- **Stock 固定提示未写出，不认定精确追加正例。** SQLite 中 questions=2、operation_reports=2、native_tool_executions=0、feedback=0、feedback_bindings=0；实际反馈事件和原上行固定文本出现次数均0。候选不改写 PASS，未人工注入工具交接。通用工具语义正式来源／到 ChatPartRef 的绑定仍未接通，不从普通 HIPS 实际拒绝或协议 ID 反推。
- 两项业务与正常退役 PASS，AS／Node退出0、VM正常收尾；未取得 Native接收／业务SQL错误。独立软件回归使用同源代码复验正式完成消费者、H1/H2真实本地TLS追加一次、stateless完整选择及来源安全，全部PASS；该正例仍是受控软件来源，不替代真实stock GLM反馈正例。

主开发已只读核 `glm-result.json`、`uplink-result.json`、`software-results.json`、`guest-retirement.json` 的上述范围。最终文件摘要与原聊天封存结果一并维护，不把记录同时存在升级为工具到内核因果。

R95正式封存结果 `acceptance-result.json` SHA `056113d24958b8d8f63b682a5b9a0ce564a962fecdd374f5c61f911ffcc04af7`；`evidence.sha256` 文件 SHA `c929f7770c949808ac1fef4eea2a74aca8861d8b92238a1062a587cc6c961734`。主开发已逐项限定manifest仅含本轮相对路径，并运行现有SHA校验退出0，日志 `/tmp/as-feedback-r2-20261114/r95-independent-hash-check.log`。`formal_completion_feedback_entry` 以冻结控制流及实际 await 后输出联合证明，未记录独立函数span；不能将它或软件受控源正例称为stock实际Flow写出。

## 2026-11-15：提示词唯一维护归属与完成投影收敛

本轮只审核规则固定拒绝提示链，不新增工具归因来源或协议能力。字段与编辑语义仍以[规则唯一契约](../backend/rules/technical.md#固定拒绝提示)为准。

- 编辑链：Dashboard `rules.tsx` 原样读取 `deny_message`，`session.editRules` 使用原 `RuleChange` 和 `expected_version`；WS／Unix 共用管理接纳，Main 按 `ManageRules` 授权，调用规则模块的 `edit`／`update`。`edit` 复用同一个固定 `rules.yaml` 原子写入和快照发布入口；没有提示词专用命令、文件或中间层可写配置。
- 命中链：Compiler 将原配置投影到当前 `RuleHit`；Driver 只取得拒绝回报标记，Main 经原 `PolicyLease::resolve_hit_feedback` 补原版本文本。Backend 命中直接取得原文本；不按当前同名规则重查。
- 确证重复及清理：原 Media、`record_notice` 和 `execute_feedback` 分别调用 `operation_result`，同一完成被投影三次，重复克隆命中规则和提示词。现仅 Media 投影一次，SQL 的 `record_completion` 接收同一 `Arc<OperationResult>`，反馈借用该结果，前端分发包装同一 Arc。旧 `record_notice` 删除，写入和执行反馈入口收窄到 crate 内；没有新 DTO、缓存或兼容入口。
- 保真及寿命：SQL 的内部 `driver`／`execution`／`error` 来源仍取原 `MainNotice.result`，不从投影失败反推。SQL 闭包持有原结果 Arc 至真实结束；存储错误仍在转交原完成后报告。Flow 的原文本副本仅属本次在途写事务，校验后释放完整输入，终态移除；业务库保存原命中和反馈事实，不成为第二份规则编辑源。
- 校验：原规则 CRUD／重开测试纳入非空、含空白和换行的配置原文，改 Allow 时明确清空；既有原版本租约、Driver 标记、无关系不改写、正式完成消费者、SQLite 原结果及 H1／H2 本地真实 TLS 用例通过。相关包 all-targets、严格 Clippy、fmt／diff 和终端 rust-analyzer LSP 引用／诊断通过，独立只读复核无阻断。原件位于 `/tmp/as-denial-boundary-review-20261115/`。

这次代码复核不代表 stock OpenClaw 的精确归因或固定提示实际写出已经通过。当前独立运行的真实 GLM 候选仍绑定其原冻结源码，不能用其结果覆盖本次随后发生的源码变更；当前运行入口及实际状态只在 HANDOFF 维护。

## 2026-11-16：原 Session、工具回执与上行报文复核

本轮只读复核已运行的 stock OpenClaw／真实 GLM 幻影链原件，没有修改 Agent、原数据库或环境，没有重发模型请求，也没有改写上行。完整结果为 `/tmp/as-openclaw-final-r3-20261116/existing-denial-handoff-findings.json`；原对账脚本 `analyze-existing-link-evidence.py`、原帧解码脚本 `decode-existing-unframed-ws.py` 在同目录。它们复算已取得原件，不向生产库导入关系。

- 当前 stock Session 使用 SQLite。原 Session 中26条 `tool.call` 和26条 `tool.result` 的 ToolCall ID 均与 AS Responses 原 `call_id`＋原 item ID 对应；26份参数、原结果 content 和上行 `function_call_output.output` 逐值一致。不能再以旧文件日志未出现 ToolCall ID 为由称 Agent 没有这些字段。
- AS 上行的26个工具结果对象只有 `type/call_id/output`，没有结构化 `details`。原 `pid/startedAt` 和进程任务 `sessionId` 不能从自然语言 output 反造；应用 Session UUID、sessionKey、进程任务句柄和内核 ProcessKey 分别保留各自命名空间。
- AS 已保存的6026份服务端原 packet 正文中，在原包起点严格解得2413个完整未压缩 WS text JSON，包含22条工具 start、22条 result、4条 update。44条 start/result 的 run、sessionKey、参数／结果与 stock 原记录逐值一致，22条 result 确有结构化 `details`。原件为 `existing-as-unframed-server-bodies-original.json`、`existing-gateway-offline-tool-json.json`。这是原字节的离线解析，不等于正式 Gateway 来源已验证。
- 当前正式 Gateway／Session 投影表和 native request/tool execution 表均为0；但 `group_unframed` 有19277份原 packet／transfer，不能称没有采集。实际宿主段 sender/receiver 包括 docker-proxy 和 sshd-session，不能把转发进程改名为容器 Agent。已成功消费的 SYN 不一定进入 unframed 表，故该表 SYN 数为0也不证明整个来源没有观察握手。原 stream／frame 元数据与已有 wire 应在原解析器中分别复核。
- ProcessDeny 原 Gateway 6760有 `sharp-ember`／PID685的 running 回执；AS 对应原上行首次观察处在原 Execute Event148947 与 NativeReturn(-1)之间。后续 poll 及终态126沿相同原任务句柄可定位到原上行 `/input/53`。但现有证据缺 PID685 到原父进程实例18035的 namespace 映射，不能用相邻PID686补齐。FileDeny 缺 read 到具体 worker 原工作项的交接；ConnectDeny 的同步结果没有进程PID；GoogleDeny 有 `tender-ridge`／PID1475及后续终态，但缺到父进程20874的 namespace 映射。等待区间事实不替代这些身份缺口。
- 旧 Process/File/Connect 三个原拒绝的 `RuleHit.deny_message` 为空，不能读取后来编辑的同名规则填回旧事件。Google四次原拒绝的文本非空，但原上行没有追加。当前 Header 选择器在重初始化后确已采用；所核 Google 运行的五个原请求 `x-client-request-id` 仍为 Missing。旧配置命令的中间采用失败不能覆盖后来的真实运行状态。

生产断点分属现有 owner：Gateway 原来源／成帧尚未形成正式消息；特定系统任务到当前工具执行的交接仍缺；`native_tool_executions` 的来源写入目前仅供测试，不能直接去掉测试限定把共享消息候选升为执行事实。另有源码可达的消费次序问题：原 Completed 只尝试一次反馈，后到关系不会重评；它不是当前原件缺真实执行关系的原因。后续修正复用原业务库、原 claim 与 Flow 在途写入，不建拒绝票据缓存，不以 Session、最近调用、唯一候选或同文本替代缺失交接。

结论：Session／ToolCall／原上行结果的协议段已证；不能将问题继续归为 Agent 不给字段。特定 Event 到工具执行及正式来源仍未证，固定拒绝说明没有取得真实追加正例。

### 同库启动暴露的保留索引扫描

R7更换服务后，实际启动停在业务库初始化，尚未进入安全库、Native会话和控制接口；不是GLM或GUI错误。原库已有623351条 `native_futex_selections`，`auto_vacuum=1`。原回填比较的 INTEGER 主键亲和传播到无亲和的 `retention_records.object`，实际计划只按 `kind` 查索引，每个根记录再次扫描整个同类集合。原QA只读计划和复算在 `r7-start-retention-query-plan-original.json`、`r7-start-retention-affinity-explain.json`、`r7-original-store-affinity-readcheck.json`；没有清库或改索引绕过。

原业务库owner按[SQLite表达式亲和规则](https://www.sqlite.org/datatype3.html#expraff)修正同一回填入口，保留原 INTEGER／BLOB 值和 generation，恢复 `(kind,object)` 完整索引点查。锁定SQLite3.51.3的同语句1024行实测由2633259降至12331 VM步；512→1024行的修后工作量约2倍。这里的程序步数说明本地查询计划，不代替真实同库启动。首次慢初始化的AS24141按真实SIGKILL9退出，旧库/WAL/SHM保留；新服务须在同一原库验证恢复，不将终止记为正常退出或测试器超时。原规则、Node、凭证及Native资产未因该修正改变。
