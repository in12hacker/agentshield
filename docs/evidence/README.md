# 验证证据索引

本目录保存绑定具体源码、产物及环境的原检查记录。标题中的“当前”“本轮”只指记录发生时；不能作为新主线、后续源码或不同镜像已通过的证明。活动设计与字段以[现行文档](../README.md)为准，当前能力差异归[底层清单](../backend/gaps.md)与[中间层清单](../middleware/gaps.md)。

源码路径退役后可链接固定历史提交，不能为了修导航恢复旧实现。原错误、未运行项、证据缺失与产品失败分别保留。临时原件路径是该次记录的位置，不是通用启动命令或必须复制的配置。

## 分发与前端

- [ARM 原版 OpenClaw 进程问询真实验收](arm-process-frontend-20261116.md)
- [x86_64 参会镜像封装验证](conference-vm-build.md)
- [VirtualBox 实际导入与启动记录](conference-vm-virtualbox.md)
- [Dashboard 控件批次整合](dashboard-controls-integration.md)

## 驱动、规则与底层

- [回调与进程树字段补全验收](callback-process-fields.md)
- [驱动职责边界核验](driver-boundary-audit-20261115.md)
- [驱动直决计数联调](driver-direct-hit-counts.md)
- [驱动原操作、普通FIFO与停机功能核验](driver-operations-audit-20261115.md)
- [驱动工具实现与联调](driver-tools-integration.md)
- [事件上下文所有权收敛](event-context-ownership.md)
- [普通文件模块联调证据](file-gate-integration.md)
- [截断与共享可写映射的原操作问询](file-wait-extension.md)
- [Linux驱动与主模块联调记录](linux-driver-integration.md)
- [Main授权停止闭环](main-stop-integration.md)
- [三池维护规则统一接入](maintenance-rules-integration.md)
- [进程与网络规则真实冒烟](process-network-rule-integration.md)
- [进程树实现与验收](process-tree-integration.md)
- [普通与记住规则的单条管理验证](remembered-rule-management.md)
- [规则阻止与终止接入验收](rule-termination-integration.md)
- [底层安全日志联调](security-log-integration.md)
- [DKMS 目标内核构建验证](target-kernel-build.md)

## 流量、凭证与业务

- [OpenClaw正文采集预算修复](capture-admission.md)
- [控制身份与持久聊天流分离](control-chat-delivery.md)
- [Flow普通转义参数的可读性反例](flow-redaction-readability.md)
- [Group 子任务终态与父公开回投旁路接入](group-terminal-passive.md)
- [双栈受控 TLS 与编译产物核查](ipv6-tls-ingress.md)
- [本地原数据保真联调](local-fidelity-integration.md)
- [中间层业务日志与消费边界验收](middleware-business-store.md)
- [正式 WebSocket 写打开闭环](middleware-websocket.md)
- [规则固定拒绝反馈的实现与证据](rule-feedback-integration.md)
- [脚本化中间层真实联调](scripted-middleware.md)
- [TLS实现与既有归一化契约核对](tls-contract-audit.md)
- [受控TLS与幻影key验证记录](tls-egress-integration.md)

## 整体审计与研究

- [底层与内核交付复核](backend-final-review-20261111.md)
- [底层缺口归一化与消费链重构](backend-gap-normalization.md)
- [当前代码、文档与承诺功能核对](code-document-audit.md)
- [运行中容器与重连可行性证据](container-dynamic-reconnect.md)
- [原操作执行现场采集](execution-site-capture.md)
- [成熟HIPS对照与当前补齐](hips-comparison.md)
- [公共JSON类型与业务存储读回](json-type-fidelity-20261115.md)
- [实时HIPS与协议内存边界核查](memory-boundaries.md)
- [OpenClaw启动等待与规则分流核对](openclaw-startup-analysis-20261115.md)
- [2026-11-06至09日修复收敛审计](repair-consolidation-20261109.md)
- [工具执行来源与运行时关联证据](runtime-correlation-research.md)
- [旁路采集偏向共享 SDK 的复盘](sdk-path-drift-review.md)
- [代码／文档／模块边界统一审计](unified-module-audit.md)
- [写关闭来源替换与实际联调](write-close-native-replacement-20261115.md)
