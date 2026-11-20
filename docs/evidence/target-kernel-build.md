# DKMS 目标内核构建验证

原 DKMS 已提供目标内核发布号。旧构建脚本仍要求 headers 与运行内核一致，导致未启动内核即使具备匹配构建树也无法进入 Kbuild。此次沿[构建契约](../../drivers/linux/README.md#构建)接入实际目标，复用 Kbuild 的原生 BTF 步骤和既有运行内核 sysfs 路径。驱动 hook、规则协议与执法行为未改。

当前 Linux `7.0.0-1019-nvidia` / aarch64 的 `module` 和 `all` 实际通过。模块 `vermagic` 对应目标，含 `.BTF`、`.BTF.base` 和 `.BTF_ids`。`all` 另外生成两个 BPF 对象及两个静态加载器，日志见 `/tmp/as-dkms-all-20261110.8n5esWnr/build.log`。目标发布号与实际 headers 不同、未启动内核没有目标 BTF 两项均在生成产物之前明确失败，原件位于 `/tmp/as-dkms-target-20261110.HwGkObxS`。

实际 guest 运行本轮新产物，30 项现有场景通过。匹配内核实际加载及卸载，guest 和 QEMU 均退出 0。镜像 SHA256 `3b4833f6a0d68ff2db530d1fa18e057a5d7ca85ae56ec46450d3bdc3ab9b223c`，153 份源码和 18 个产物逐项核对一致。原报告 `/tmp/as-dkms-target-native-run-20261110/report.json`，SHA256 `63e69c9eaa061219a087af17c4481fbb016c881d0a61859a85b3706e85f51b69`。

本机没有可用于正向验证的未启动目标 `vmlinux`，该路径实际构建未执行。DKMS 安装、Secure Boot 签名加载、x86_64 和正式 deb/rpm 包的安装升级未执行，由 [AS 整体分发](../roadmap.md#as-整体分发)跟踪。

## R25 真实 OpenClaw 集成验收

**PASS。** 当前内核 `7.0.0-1019-nvidia` / aarch64，候选提交 `3f677ead1f9bd09a66bc6050733933587b21ed1e`。实际启动镜像核对了本轮重建的 Native 模块和两个 BPF 对象。两个 OpenClaw 原请求各发一次，分别经用户允许实际读取成功、经用户拒绝实际返回 `EPERM`。两次均有原 Ask、提案、原完成回执及 UI 终态。

正常退役后只读核对 SQLite。`log=true` 的实际允许有一条日志，与原事件、规则版本、决定、提案及结果按值一致。`log=false` 的实际拒绝有原回执，同规则版本无日志行。两份 Ask 查询均完成。六个交换的实际幻影凭据绑定与应用、提交事实核对一致。实际协议到结构化值、发布到 SQL、原协议与消息查询均通过。两个 AS 实例授权停止并实际退出 0，驱动卸载，guest 和 QEMU 退出 0。

原结果 `/tmp/as-openclaw-evidence-20261110/r25/result.json`，SHA256 `8358e15bc8447df51cc446df2c692c978f154c695828cc88a29ff48e53dfa1fa`。本次原件保留两份 SQLite、原帧和正常退役记录。全驱动操作矩阵、其他内核或架构安装、正式安装包、近期 Ask 记住选项、日志远端查询及轮转保留均未执行。R23 原样例和 R24 有限结论继续按各自原件认定。
