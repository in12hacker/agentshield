# 成熟HIPS对照与当前补齐

日期：2026-11-08。范围是重构版实际代码及已执行证据，不以旧项目技术作为实现依据，不把Windows产品文档当Linux驱动代码，也不把个人网络防火墙当完整HIPS。

当前真实验收结论：2026-11-09，aa23f60fe的R13指定五项Linux VM OpenClaw／GLM流程通过，见[最终原件及边界](#r13真实openclaw-vm最终验收)。旧批失败保留，不据此宣布宿主保护、持久行为图或反馈提示功能完成。

## 已读成熟来源

- Linux v6.12 [TOMOYO file.c](https://github.com/torvalds/linux/blob/v6.12/security/tomoyo/file.c#L803)及common.c：按具体操作初始化检查，在该操作未启用检查时，先返回再取路径／准备请求。AS吸收这个操作筛选与昂贵工作前短路的原则；TOMOYO的强制模式、域模型和默认拒绝不直接搬入AS。
- [TOMOYO条件说明](https://tomoyo.sourceforge.net/2.5/chapter-10.html.en)：类别、操作、目标及真实UID／GID／argv等条件分别表达，支持细化规则。AS继续使用自己的已文档化类型、按位置的argv和实际字段供给，不另起表达式语言。
- [ESET HIPS规则说明](https://help.eset.com/efsw/12.0/en-US/idh_hips_editor_single_rule.html)：规则按发起应用、操作和目标生效，包含文件修改／删除及进程相关权限。这里作为业务能力对照，不能证明闭源驱动内部实现。
- OpenSnitch的[规则及系统活动示例](https://github.com/evilsocket/opensnitch/wiki/Rules)和[operator.go](https://github.com/evilsocket/opensnitch/blob/master/daemon/rule/operator.go)：明确条件、动作与高频系统活动规则有借鉴价值。其Deny／priority仲裁不替换AS既定编号首命中；其中空hash假匹配等妥协也不采用，AS缺事实仍补取或失败。

下载固定TOMOYO源保存在/tmp/as-hips-reference-20261108，保留原SPDX。驱动沿现有GPL许可，仅吸收适用逻辑，不复制整套内建LSM或引入其安装／域管理体系。

## 真实差距

| 方面 | AS实际状态 | 处理 |
|---|---|---|
| 无相关规则的热路径 | 旧代码末尾无条件ToUser，空策略也分配、采集、等待 | 本轮增加实际启用后端操作投影；没有Driver及后端候选时采集前默认放行，无RuleHit；有后端候选仍移交 |
| 风险条件和优先级 | 已有三类、明确位置／操作／条件、编号首命中、记住优先及缺字段处理 | 保持唯一规则引擎；风险结论只取真实命中，不按名称／时间／聊天／评分猜测；系统噪音用显式Driver规则 |
| 原始结果供给 | MainNotice已有event／hit，旧上行投影丢掉它们并跳过Direct结果 | ee12ed7df补原EventData／RuleHit和真实Direct输出；无原生等待身份时decision_id=null，不造询问 |
| 消息调度 | 真实OpenClaw突发OperationResult仍会填满重复转发队列并关闭管理连接；只yield不能解决socket暂不可写 | 来源通知直交writer、可取消背压；问询／命令回复独立，来源／聊天公平发送，按真实EOF排空；不扩预算、不改HIPS动作 |
| 普通操作覆盖 | 实际可等待普通控制只有Process Execute、File Open、Network Connect／Send；23个文件自保入口不是23个普通控制入口 | 文件修改／删除／重命名及进程跟踪／跨内存等须逐hook核上下文和字段，再加明确能力及动作；不因枚举存在就开放配置 |
| 持久审计／历史 | RuleLogRequest消费者与实时通知存在，持久日志／聊天库及完整查询未接 | 由底层所属日志／存储模块补真实写入及读取；中间层不建影子库或返回空成功 |
| 回答时记住 | 手工Remembered组、Get／Update、优先匹配已存在；自动草稿／预览／保存链缺失 | 必须复用规则模块并确认真实作用范围；不拿裸PID假装进程实例有效期，不在中间层藏缓存 |
| 容器与业务关系 | 完整容器实例供给及工具到进程上下文桥缺失；LLM原生字段／进程事实已存在 | 先补实际来源及底层契约，不按容器名／PID／参数／最近消息推定；关系证据不足即缺口 |
| 产品接入 | 已有正式底层端口和单会话脚本宿主；正式WS／IM服务未实现 | 三类中间层目标文档分开，原结构复用，缺失供给在底层补齐，不把端口存在当整套产品已交付 |

差距主要在操作覆盖、实际消费链和上述热路径／调度边界，不是再加一个框架就能补齐。后续按模块补能力，每项先确认真实生产者和消费者；当前不添加没有数据源的风险字段或未验证的hook。

## 默认路由的准确边界

1. Driver规则按原编号匹配；显式Allow／Deny／ToUser不变。
2. 未命中且本kind／operation存在启用后端候选：ToUser，hit=None，保留后端记住／快速／询问机会。后端操作位仅做保守分流，不声明条件已匹配。
3. 没有相关后端候选：默认Allow，hit=None；两侧均无候选时在采集前完成。规则停用、空组、只有其他类别／操作时不生成该位。
4. 缺必要事实、不支持字段／动作、失效源和退役仍按原错误处理，不能装成确定未命中；独立自保和进程生命周期供给不关闭。

普通镜像替换为现行32字节头，明确header_bytes及三类backend_operations；Rust生产者／Native消费者／验证和文档同批替换，旧16字节布局不解析。不新增版本号、默认规则或隐藏信任名单。

## 验证边界

当前本地全workspace／all-targets、strictClippy、fmt、strict rustdoc及Python传输检查通过，日志/tmp/as-hips-routing-final-validated-*.log。Native真实构建及BTF由当前内核工具生成。Rust Analyzer核新函数／全部调用，两个Rust源诊断空；clangd实际RPC核Native源，无错误，保留未改动内核宏警告。

调度反例/tmp/as-transport-burst-before.log实际快速读者仍失败OutboundFull；修后/tmp/as-transport-burst-corrected.log相同突发通过。中间调试记录保留，不把测试producer未正确释放或CLI tweak自检错误当产品通过。

整合原件/tmp/as-hips-routing-mvp-final/run/{middleware-report,report}.json：现有真实guest规则、问询、自保、TLS／phantom与实际退出场景通过。Backend文件规则不再由测试额外插入Driver ToUser掩盖路由，显式ToUser仍由原无渠道场景验证。该结果不是实际GLM／OpenClaw验收；指定聊天继续用完整新产物执行，原空规则及旧传输失败保留。

独立只读复核ee12ed7df至a0f6f122范围无必修阻断，原件/tmp/as-hips-routing-independent-review.md；独立重核122源／18产物及image／Native／service／client一致，记录/tmp/as-hips-routing-independent-hashes.json。LSP引用及两个Rust源空诊断另存同前缀日志；没有重跑或扩大矩阵，也不把独审当真实模型通过。

## 真实GLM阶段新增的聊天背压修复

指定聊天已从实际OpenClaw原记录取得真实GLM HTTP200和助手回复，AS也提供凭证Prepared／AppliedToRequest事实；该批订阅随后outbound_full，原始正文／Finished未完整取得，不能算数据链或整体验收通过。

1374bb6d7仅修传输聊天准入：发送槽reserve成功后才poll ChatStream，避免提前释放Flow字节预算再因本地chat满退役整个认证会话；等待仍在原select内，控制／Answer继续推进，真实来源关闭只结束订阅。Native、规则、队列大小和Flow原Gap／撤权语义未变。受影响检查及static宿主构建通过，LSP诊断空；原件/tmp/as-chat-backpressure-final-*.log及as-chat-backpressure-lsp.log。独立增量复核无必修阻断，/tmp/as-chat-backpressure-independent-review.md。实际验收仍由指定聊天按新候选认定，交付标识/tmp/as-chat-backpressure-delivery/delivery.json，不把本地回归或首轮200当新候选通过。

后续R8实际管理连接保持正常，真实三阶段凭证使用及助手回复已取得；但响应输出缺Finished，lost_deliveries=1。收到的约94万字节不能证明全部生成量或峰值，因此没有据此断言唯一根因。源码另有独立固定128条队列上限；受控258条Started／Delta／Finished的全部计费低于原2MiB预算，仍实测lost1，反例/tmp/as-chat-budget-before.log。

本轮将队列容器条目上限从同一字节预算推导，不再额外用固定128截断；每条非零封套计费仍在入队前取得原Semaphore许可，消费／失败释放，配置额度与真实byte-full／Gap／回放语义不变。相同受控流的实时与回放均取得助手Finished，原真byte-full／归还许可反例仍通过。完整本地检查、LSP及静态宿主构建见/tmp/as-chat-budget-final-*.log和as-chat-budget-lsp.log；独立复核/tmp/as-chat-budget-independent-review.md无必修阻断。仍须由指定聊天按新候选确认真实OpenClaw数据链，不宣称R8唯一原因或整体验收已通过。

## 真实操作回执的发送背压

226f154的R9普通聊天取得原生最终文本及AS同交换助手Finished，真实文本逐字一致；允许工作流的read／Git／HTTPS实际有结果，但管理连接再次outbound_full，回执不完整。原件run-r9/all-original-frames-after-allow.jsonl有755帧，末20帧均为OperationResult；该项仍失败，不用工具成功替代数据链验收。后续三类拒绝实际生效另有证据，不合并成整体通过。

源码确认两处独立问题：Peer事件try_send满即close；事件再次复制到control后也会Full退出。writer正在完整发送一条大帧时，即使实际读者继续工作，短时突发也会触发这两个条件。移除事件到control的重复排队，Peer等待真实容量或实际取消，媒体保管同一次发送并继续消费日志；writer控制优先，来源／聊天公平选择。命令完成在可取消select分支内先reserve再取原结果；原Prompt单槽只清理实际过期／reply关闭的旧项。来源结束诊断沿已取得的chat许可发送且同步结束订阅，避免取消时重复诊断。字段、规则决定及配置额度不变，现行接口直接替换旧接口。

实际socket回归覆盖已阻塞的大帧、16槽来源满及恢复读取，取得全部原帧且连接未退役；另核Prompt超时／原reply关闭替换、取消唤醒和订阅终态无取消缝。本地workspace、strict Clippy／fmt／rustdoc通过，最后一处终态修正另复验Main和构建；记录/tmp/as-control-backpressure-final-*.log、as-control-backpressure-terminal-*.log、as-control-backpressure-lsp-final.log，独立报告/tmp/as-control-backpressure-independent-review.md。新夹具的非法零序号失败原件单独保留，不当作产品通过。沙箱EPERM与普通用户真实socket检查结果分开。

Main最初的有限非阻塞通知入口仍可能按既有计数拒绝，不能承诺无限洪水无损。reply_queue_full只描述即时命令回复预算耗尽，outbound_closed只描述真实发送端结束，不再把普通来源突发归为这两类。真实OpenClaw允许／Ask仍由原聊天按新冻结候选认定；无渠道时跳过询问池与已等待后断开按原超时动作是两项不同的既定语义，不能仅凭曾ready就推定操作已挂起。

3d1ba9dd3的R11已从后续同交换原件取得完整助手Finished并逐字匹配原生最终文本；初始409条观察只有思考增量，早期窗口不是全集，未据此改解析器。该批管理连接健康、Flow丢失／解析失败为0；三类Deny与Ask用户拒绝路径已取得真实回执，整体验收仍以原聊天最终报告为准。普通Allow的文件／进程已命中，HTTPS虽返回200，原curl实例尚缺TCP/6规则回执，不用UDP探测冒充TCP，也不从缺通知推定hook绕过。Native dropped不覆盖Main拒收；相关只读诊断/tmp/as-real-network-receipt-diagnosis.md未定位唯一原因。

独立核查发现Observer真实body Drop原先只有Gap、已有Streaming消息不收尾，违反现有同ID中断终态契约。小修复用finish的状态收尾，保留原Gap，只依据实际解析失败／中断产生Failed／Interrupted；已Complete不重复，不补假Complete，不解析未完整尾部。精确Drop／显式中断／正常DONE回归修前失败、修后及MSRV通过，适用Flow／Main、Clippy、fmt、rustdoc和static构建通过；原件/tmp/as-observer-drop-*.log、as-r11-completion-*.log，LSP诊断空，独审/tmp/as-observer-drop-independent-review.md无必修阻断。该缺口未被认作R11早期窗口原因。

为明确后续真实缺失阶段，脚本宿主仅采样MainIngress已有拒收计数并在变化时打印，未连接不造0。没有新增DTO／业务队列／规则／放行或预算；没有声称当前R11 Main计数已非0。Native Direct结果在C源direct_notice内受原hit.log筛选，不能仅看adapter／Main消费者而断言log=false仍上报；Backend已接纳结果和规则日志的既定分流另计。

## R13真实OpenClaw VM最终验收

原指定聊天独占执行，主聊天只提供候选／需求／标准并核原件。最终结果为PASS：真实对话、普通进程／文件／TCP允许、普通拒绝、Ask用户拒绝、Ask用户允许。运行时代码aa23f60fe5be83739ef756ede305af3968144755；service SHA256 b1140ecb27aaac9602e6852b825d3ef0c6eef8dfd6419cae88a0acb6c3bbd5d5，Native efd9604bcbeb9e3874e72952eee43ee18ea410f42431a4645317078e959913e2，client 3b05bc0665a9be3600198d361c1519675071f5ab9e8195a538abc1143101e4d8。原生驱动／规则／2MiB配置预算未因验收改动。

唯一原结果：$WORKSPACE/.runtime/openclaw-vm-20261108/r13-aa23-final-validation/result.json，SHA256 8f645435107d560a82ddf9aa9667438330d67f311d8c85adeacf6ad657188183；同目录artifact-hashes.json的47份原件经主聊天逐份重核无差异。已验收交付入口/tmp/as-rule-log-diagnostic-delivery/accepted-delivery.json；测试前delivery.json原件保持，不回填旧失败。

- 网络Allow的原RuleLog与Up.operation_result具有同一decision_id，原curl实例／uid1000／TCP（IP协议号6）／目标端口443、ordinary-network-allow及Allowed均实际取得，不以UDP或工具成功补命中。
- 两Ask均原问询一次、原答复一次、同一决策的reason=user及真实Denied／Allowed；拒绝没有私有fixture内容，允许返回原内容。没有用超时动作充用户选择。
- 原NativeID／工具名／参数、同一request里的工具结果引用及协议终态独立核对。OpenClaw不同来源实际改写的ID分别保真，不剥下划线造别名；原始协议与应用展示不设无依据恒等关系。两段报告仍按现行策略整段Redacted，保原记录出处和协议终点，不宣称披露全文；这是脱敏表达，不是已证消息丢失。
- 最终Native pending／pending_receipts／dropped为0、error=null；Flow admitted18、rejected0、active0、lost0、parse0；Main最新实际采样notices0／logs0。媒体当前实例logs_observed56、not_forwarded6、durable_write=false，初始化实例另列。6条没有逐条身份，不归因到业务或生命周期，也不推全部普通通知绝对完整。
- 在线配置／日志／环境检查未发现已知真实key，离线515个完整OpenClaw状态文件共39,870,488字节同样0命中。OpenClaw／AS／guest／QEMU均退出0，Native卸载、QEMU join、剩余VM用户进程0，无强制清理。宿主原OpenClaw／Root／MOK及用户改动未动，宿主未重启。

观察端已移除重复隐私扫描／整数递归，保持原字节检查；预开证据FD避免锁内受控FileOpen阻住Ask投递。原请求没有为补证据重发，旧R1—R12及勘误保留。复用入口在原聊天同树scripts-current/workflow.json及24份脚本清单，依赖本机冻结资产，不冒称跨机安装器。

该结论只覆盖上述五项VM流程和对应产物，不覆盖宿主保护、正式安装分发、持久日志／行为图、完整容器实例或唯一tool到syscall因果。规则固定提示词仍只有已审核设计，未纳入本轮验收；保守脱敏的可读性优化另行记录，不以关闭保护恢复可见文字。
