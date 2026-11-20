# 控制身份与持久聊天流分离

本轮修正6ac7dddf67dade9daeda4d6ed94feba5d681f399在[R19真实验收](backend-gap-normalization.md#r19真实失败与修正方向)中暴露的输出压力撤权／Ask旁路，旧FAIL及原件保持。规则和真正无前端的用户语义不改，不以增加Allow、扩队列或延期限消除失败。

## 实现与边界

Main将原Flow唯一ChatReceiver交给同一个MessageModule的consume_chat，复用Messages职责内原任务与JoinHandle。MainMessage::Chat／chat_notice桥删除，控制结果与聊天不再混队；无前端仍逐条入库，结束按原生产者关闭、消费EOF、真实join退役。接口唯一出处为[Main技术契约](../backend/main/technical.md#消息与聊天消费端口)。

同一业务数据库增加原输入标签／精确载荷引用索引；九变体均按原始标签重建，Started／Finished不能用最新revision猜测。原ChatCursor用于身份定位，读序按实际delivery提交位置；提交后只发合并唤醒，前端一次读取／保管一条原ChatEnvelope，没有底层聊天缓存、SQL封套JSON或第二个库。字段与四个窄接口仅在[业务日志技术契约](../middleware/business-log/technical.md)定义。

实际控制结果保留原bounded可靠发送，满时等待而不撤问讯身份。Ask从原单槽在完整帧边界优先处理，不等普通回复容量或旧SQL工作；写入接纳和实际commit分别报告。尚未写出的过期Ask只释放自身，Main窄接口核同grant／同ID／真实到期；已部分写出的JSONL继续完成原行，不延原操作期限，也不让旧ID取消新问讯。真正EOF、授权撤销或不可修复I/O仍收口连接。同一socket的部分帧存在物理队头阻塞，不承诺绝对控制时延，原请求仍按其期限和规则超时动作收尾。

合法聊天／查询可能超过现有传输帧长。未写出时按内容／业务查询／管理回执区分局部输出失败，保留控制身份；同ID管理诊断只说明回执未交付，不能假称管理动作没生效或自动重试。Python当前客户端按原CommandId消费该诊断，要求读回保存与实际采用状态。没有截断原Kernel结果或伪造小型事件，没有新分段框架、限额或版本。唯一输出定义见[中间层消息技术](../middleware/smoke/technical.md)。

## 已执行

源自本机CI环境、Rust1.89的完整workspace／all-targets／locked／offline测试162通过、0失败：`/tmp/as-r20-r2-workspace-tests-20261109.log`。全workspace严格Clippy、rustdoc与fmt通过，分别为`/tmp/as-r20-r2-clippy-20261109.log`、`/tmp/as-r20-r2-rustdoc-20261109.log`、`/tmp/as-r20-r2-fmt-20261109.log`。

最终既有业务软件冒烟通过`/tmp/as-r20-r2-software-smoke-20261109.log`：真实Unix聊天背压不撤问讯身份，原Ask超时后下一Ask仍可获用户选择，未知续读游标不改变渠道，真正EOF才撤权；配置重载与规则CRUD原检查保持。其明确使用SoftwareDriver／ReviewSource，不称真实内核或stock保护。

writer五项真实Unix回归通过，含零字节过期后的下一控制帧、部分帧过期继续完整行、部分授权撤销及未写出大内容局部失败；Root已审核生产分支，作者原件`/tmp/as-control-writer-authorization-tests-20261109.log`。原“队满撤权”测试断言已退役，不回填成正确HIPS语义。

Root最终LSP核六个正式库源无错误、实际接口引用有效：`/tmp/as-r20-final-lsp-diagnostics.json`及`/tmp/as-r20-final-lsp-references.json`。此前例子跨crate路径的工具诊断仍按原证据范围，不假称整仓所有辅助工具零诊断。

独立Main审核见`/tmp/as-chat-control-main-independent-review-20261109.md`；数据库／Media／reader另按最终源审核，writer由父审核而非作者自认独审通过。检查结果只覆盖绑定的源码，不能套旧freeze到后改文件。

## 冻结产物

包`/tmp/as-r20-control-chat-delivery-r2-20261109`，镜像SHA256 `0681271858451a7c321fed01f0f75aca9d3d1eb25b8c51ee1b8f0c4ec43c02ad`。151源码／18产物核验0差异，manifest SHA256 `542643e0be216ca3670de33e845eda52749c82780eab0f8edb991acd285653ec`，原件`/tmp/as-r20-r2-source-verification-20261109.json`。

Native C、原生帧与匹配BPF产物未变，明确复用as_wait.ko `f22fce932c2ef0087721d3c265d8f697fb6d3f9420d67ff4827cbe6513e441f6`及原7.0.0-1019-nvidia／aarch64内核`6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6`。不通过宿主加载或信任变更证明guest能力。

真实Native r1在权限重载后的旧连接关闭场景FAIL，原件`/tmp/as-r20-native-run-r1-20261109`保留；其只有invalid_frame，不反推具体撤权分支。代码统一区分实际输出授权检查失败、watch结束但检查成功、输入协议错误与I/O。客户端只在明确权限变更及同case精确授权关闭证据时，把缺失回执记为未确认，不重发修改；新连接必须验证完整保存内容／版本及实际权限。独立契约审核见`/tmp/as-r20-revocation-client-contract-review-20261109.md`。

r2真实guest通过：`/tmp/as-r20-native-run-r2-20261109/report.json`，SHA256 `3c40f9d4cb400e8704af449ef70bf0c3e0ca25aa21668e322256e1ac9503b8ed`。28必须场景PASS、1独立安全日志持久化仍NOT_IMPLEMENTED；宿主UID1000、QEMU实际join、无timeout或failure。只读场景本次实际收到Completed回执，完整配置／新版本读回及同Native实例、新ReadOnly的规则／私有／业务拒绝通过；没有观察到授权关闭，因此不把r2成功当r1具体归因。不能接受任意EOF为通过，也没有延迟撤权。

## 原聊天R20真实验收

原指定聊天完成5963f1117cd5dfe70b804d063bae0710c5b997c1的有限验收，结论PASS。原件`/tmp/as-openclaw-evidence-20261109/r20/result.json`，SHA256 `800878b82e91c28ac2e8be79f97c0b8ab50fa73a70baa20614b4f3d2c22e0f4a`。151冻结源码／18产物／原包与内核逐项匹配，实际stock镜像`850ae824e9733d02cd04bc2320c744d59210356a24ec070822e7bf20b69965a3`独立记录。

五个真实stock工作流全部通过，普通文件Direct拒绝回执完整。两次用户答复Ask的原DecisionId／Event／RuleHit／User提案及物理结果与SQL一致。主负载后仍用同一个未重建回答连接，新增独立stock读取收到Ask `44ad15c2589e9adbb1b68410ff87e0a1`，原remaining19990ms、发送答案0次，真实回执与SQL均为Timeout／Deny／denied；工具EPERM且模型final。未观察到R19的NoInteraction放行，不以计数0替代此实际反例复验。

公开6394条Chat均与SQL原标签／精确payload及revision行／原cursor及delivery位置一致，数据库实际6395条；额外真实SQL-only source_closed不伪造为公开帧。协议5477条，83逻辑消息／102独立revision，questions／operation_reports／question_completion各3。受控Native七项补充与软件场景通过，范围分别记录，不当作stock通用执行桥或所有分支已live执行。

公开保留一条`body_dropped_before_eof`，原请求体证实为辅助会话标题请求，SQL原样保存；其具体中断原因不从角色或计数推定，也不补成完整正文。主工作流及新增读取的原协议终态、工具引用／内容和SQL均已分别核过。该来源中断不被写成“全部零缺口”；原`global-gap-inventory-final.json`的全源完整性边界保留。

stock／AS／Native与两份自有VM均真实正常退役、卸载及join，无forced或owned残留。原宿主OC仍为同CID／image／Nov2启动及原boot，Root未操作其实例。R19 FAIL与Native r1 FAIL原件不改。原聊天独占完整方法，Root仅提供需求、标准和冻结metadata。

剩余产品能力仍见[底层清单](../backend/gaps.md)：通用精确执行来源、自动记住业务链、完整文件／容器／TLS覆盖、正式服务与部署等均未被本次有限验收补成已实现。本文不称完整HIPS发行交付。
