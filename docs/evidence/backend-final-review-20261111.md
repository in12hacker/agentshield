# 底层与内核交付复核

本轮沿已审基线 `d8826cc3c` 后的容器接线、事件所有权和 R51 Flow 改动复核。覆盖原生事件生产、受控请求、规则分流、数据补全、TLS、反馈、队列消费和实际退役。配置、规则和凭证的持久事务仍由各模块保管。当前产品范围只从[缺口清单](../backend/gaps.md#当前剩余范围)读取。

## 引用与临界区

- **Native 原操作。** 在 `core_lock` 内取得真实快照引用、登记请求、提交相位和摘除回执。人工等待及实际终止在释放自旋锁后执行。最后的快照释放交现有 workqueue。设备读取的 per-reader 锁保护唯一临时帧，实际用户拷贝在释放 `core_lock` 后执行。写入的用户拷贝、镜像分配及最终释放各自避开自旋锁。见[原操作和设备实现](../../drivers/linux/as_core.c)。
- **Native 自保和工具。** 进程、文件、inode 与快照由真实对象引用保管。RCU 段内取得引用，路径、文件、inode 的潜在销毁在其所在对象锁外执行。PID 引用最后释放走 Linux 的非睡眠释放路径，PID 命名空间的实际销毁由内核工作项执行。工具信号沿原 PID 引用，实际调用与恢复凭证在 AS 自旋锁外。见[自保](../../drivers/linux/as_self_protection.c)、[工具](../../drivers/linux/as_tools.c)、[Linux PID 释放](https://raw.githubusercontent.com/torvalds/linux/v7.0/kernel/pid.c)、[PID 命名空间释放](https://raw.githubusercontent.com/torvalds/linux/v7.0/kernel/pid_namespace.c)。
- **底层原事件。** 适配解码一次并分享不可变原事件。原操作借用比较各字段，保留空值、零值和原文件参数校验。实际要补充时才复制共享事件。补全后规则、Ask、回执和日志共享形成的事件。见[事件校验](../../crates/contracts/as-types/src/dispatch_event.rs)、[原操作](../../crates/backend/as-driver-adapter/src/operation.rs)和[主调度](../../crates/backend/as-main/src/dispatch.rs)。
- **Docker 绑定和原操作状态。** `watch` 守卫只取快照引用，pidfd 检查及容器匹配放在守卫之外。唯一绑定发布者先在锁外核实退役，再替换快照。原相位锁只提交结果，释放后才通知等待者。故障路径取出设备 FD 后释放互斥锁，再释放引用。见[驱动适配](../backend/driver-adapter/technical.md#容器实例绑定)。
- **进程树补全。** 同一次读锁内取得不可变快照和证据代。锁外核对采样时间、线程身份和父身份，仅复制原事件缺失且已确认的字段。补充值移动给主模块。容器归属继续取原事件及适配事实。见[进程树](../backend/process-tree/technical.md)。
- **Flow、反馈和队列。** HTTP/2 子任务、来源关闭通知和已经移交的收尾均归原任务跟踪器。HTTP 等待者取消后，跟踪器继续保管实际待发终态。队列与反馈锁只取数据或完成原相位竞争，出锁后才匹配原 ID、发布或等待。反馈共用原解码 JSON，未修改回执保存成对的原记录与工具引用，和实际写出竞争同一个事务。见[Flow](../backend/flow/technical.md)。

## 修正与范围

Flow 已区分真实 HTTP 结束、传输中断与本地终态等待取消。中断时保留实际收到的非流式原字节，报告缺口。SSE 同批后续帧错误时保留前序帧；完整分帧的非法 JSON 保留原字节与质量标记。来源结束通知不再阻塞停止进入既有排空期限。容器绑定、受控 TLS 明文归属和普通规则的三份活动文档已与代码对齐。

驱动 C／BPF 与现行线格式未改。创建前完整原路径 Ask、普通 FD Write Ask、已有共享映射经 mprotect 增加写权限的 Ask 仍无已核实的完整等待链。Linux 7.0 的可睡眠集合与实际 hook 参数已复核。精确工具执行来源仍缺跨边界的真实交接。见[Linux BPF-LSM](https://raw.githubusercontent.com/torvalds/linux/v7.0/kernel/bpf/bpf_lsm.c)、[Linux LSM hook 参数](https://raw.githubusercontent.com/torvalds/linux/v7.0/include/linux/lsm_hook_defs.h)和[驱动范围](../drivers/linux/hooks.md#普通文件gate接线)。

## 验证

最终冻结包 `/tmp/as-bottom-review-final.2kzaSlEf/package`，镜像 SHA256 `fba10ae22afae668541835b03291020e0aef82c75fa1bc4eeb69cf39ecab6793`，168 项源码复核。五项原生资产沿用同一内核已验证字节。本地工作区 183 项通过，覆盖终态等待取消、来源关闭且队列满、同批坏帧、非流式断开、反馈精确失败及真实回环 TLS。格式、严格 Clippy 和 rustdoc 通过。LSP 诊断与 R51 基线一致。测试日志 `/tmp/as-bottom-review-final-tests.55UNOCZB.log`。

R52 原版 OpenClaw 在隔离 aarch64 7.0.0-1019-nvidia 上通过。一次实际文件 Ask 的答复前，原 `openat` 线程在冻结 Native 模块的 `schedule_timeout` 调用点等待。实际 Allow 后，读取内容与原 inode 对应文件一致，原事件和规则在 Ask、完成回执、业务库和安全日志中逐值相符。实际模型流含三次请求、376 帧 SSE、13 条各一次开始和结束的消息与 36 条连续增量。450 条投递全部在业务库中按提交顺序逐值匹配，零采集 Gap。隔离进程及 guest 正常退役，Native 实际卸载。

R52 原报告 `/tmp/as-openclaw-evidence-20261111/r52/result.json` SHA256 `119da06013fa704eb111df48da6fbaa1bd323779fe6722e28092aa098212330f`。39 项原件和 21 项脚本清单已核对，只读复算逐字节一致。R52 未执行 Deny、超时、断流、并发、宿主安装或跨来源工具因果桥。
