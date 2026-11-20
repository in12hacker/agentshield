# 本地原数据保真联调

范围只按用户明确要求：删除本地主动脱敏，实际采集、发布、入库和查询保持原值；未来业务侧脱敏列入[路线图](../roadmap.md#未来业务侧脱敏)，当前不实现。原采集字段、内容权限、HIPS规则、凭证授权及幻影key认证头替换范围不扩大。唯一字段见[Flow契约](../contracts/flow.md)，实现边界见[Flow技术](../backend/flow/technical.md#原数据保真)。

## 已实现与独审

删除RedactionLease及为内容匹配复制的秘密／phantom、EvidenceSegment和ChatPart的Redacted变体，以及生产、存储、查询、内存计费中的对应分支。Flow原字节直接进入EvidenceBody，Text／Json／原生ID及元数据按实际值和类型投影；不再sanitize、匹配前缀／关键词、遮蔽参数、生成元数据隐藏Gap或因内容改消息终态。未获认可的新matcher及其依赖／设计已取消，没有保留兼容路径。

MVP客户端删除private_patterns、内容关键词拒收、诊断替换及退役测试；私有管理回复记录范围不扩大。存储仍直接使用原结构，新增保真断言覆盖Bytes中的NUL／0xff、反斜线、看似凭证的Text／Json／ID／元数据及真实空值，关闭／重开数据库后逐值一致，不迁移历史证据库。

独立审查`/tmp/as-local-fidelity-independent-review-20261110.md`及其source hash无未解决发现；Flow、Middle作者冻结见`/tmp/as-flow-faithful-freeze-20261110.json`和`/tmp/as-raw-business-freeze-20261110.json`。Rust代码含退役测试本轮净删562行，无新依赖。

## 本地检查及真实guest

当前源码MSRV1.89 workspace/alltargets 171通过、0失败；strict Clippy、fmt、rustdoc、现有软件冒烟及Python客户端5项检查均通过。日志`/tmp/as-local-fidelity-workspace-{tests,clippy,fmt,rustdoc}-20261110.log`、`/tmp/as-local-fidelity-python-tests-20261110.log`、`/tmp/as-local-fidelity-software-20261110.log`。Root与作者实际LSP覆盖接口／调用者，正式源诊断无error；不使用xtaskx或远端CI。

包`/tmp/as-local-fidelity-delivery-r1-20261110`，镜像SHA256 `6ee85c1ab11dd3f766e3ad5f421a665089f2356fe966fd910e4f7f2f5b92e994`；151源／18运行产物逐hash无漂移。Driver C／BPF未改，复用已验证的新File gate产物，Native模块67ea23eb…及BPF99a37a7f…；匹配aarch64内核7.0.0-1019-nvidia，Image SHA256 `6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6`。

实际普通宿主UID1000的QEMU运行116.76秒，29项必需PASS，持久安全日志仍NOT_IMPLEMENTED。guest／QEMU退出0、模块实际卸载、child已join、无超时。原报告`/tmp/as-local-fidelity-native-run-r1-20261110/report.json` SHA256 `94b68b0d1e81561a7f9eb91e91ad7966fcb7337f6bc575099c838969234ca088`；middleware报告SHA256 `6f7b3df80f076523055469d058f772e15e67cd495c481f8623929b1f612debca`。没有操作宿主Root代理、OpenClaw、内核／签名／信任或宿主网络规则。

## 原OpenClaw边界

R23 已完成，**AS 实际采集、发布、SQL 入库及已执行查询有限范围 PASS**。验收绑定提交 `934606536f8802613ab23a83fd76d379f8b6531b`、151 份源码、18 个产物及本页镜像和内核。原结果 `/tmp/as-openclaw-evidence-20261110/r23/result.json`，SHA256 `0744444207246b6b65a8a8acffc2682657a25155e00ca4873f84ad3edeac122a`。

58 项实际原协议至结构化字段的比对通过。13435 条公开聊天记录与 SQL 的原标签、值、修订及提交位置一致，SQL 另有一条真实 `source_closed`。两个含普通转义的代表场景保留了原工具字段与结果。已执行的原协议和消息查询按原值与类型通过，既有 HIPS 和幻影 key 场景通过。两个 AS 实例正常请求停止并由父进程取得实际退出结果，原生模块实际卸载。

完整 UI 原文样例保留 **FAIL**。运行产物与原始记录证明 OpenClaw 在 AS 采集前已改写相关文本，见 `/tmp/as-openclaw-evidence-20261110/r23/stock-source-boundary/boundary-proof.json`，SHA256 `982d93a829c8324ab121496c8e781e05463580a25f0d276b1daf7657992263cb`。因缺原锚点的查询为 **NOT_RUN**，全链路原文保持未通过。原 R22 文件 Gate 有限 PASS 与聊天字段 FAIL 保留，其他未完成能力见[缺口清单](../backend/gaps.md)。
