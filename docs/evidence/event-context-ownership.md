# 事件上下文所有权收敛

本轮以 `007b0aacd` 为代码基线，按用户明确的入口补空、进程树唯一状态、事件共享和消费后释放约定调整。原生请求关联的观测结果另见[运行时证据](runtime-correlation-research.md)。

## 改动及边界

`DispatchInput`、规则 `ReviewRequest`／`Prompt`、`MainNotice`、`RuleLogRequest`、`Up::Ask` 和 `OperationResult` 的事件均使用同一 `Arc<EventData>`。完成通知、日志和消息投影的深拷贝已删除。快速／慢速事实接口仍只读事件并返回补充值，由主模块合入空字段。生产路径在发布 Ask 前结束事实填充，随后共享只读事件。

进程树的原 `SlowFacts` 实现重复调用同一个内存补全方法，本轮删除该实现、唯一调用接线及无调用方的错误转换函数。正式装配只在快速入口补充进程树事实。主模块保留已有慢事实接口，当前尚无 hash／DB 慢事实实现。

驱动适配的原始事件副本继续用于 `OriginalOperation::validate`，核对后端传回的事实是否保留原值。该副本随原操作释放，不作为可复用进程缓存。驱动和底层的原操作各自保管至真实结束，进程状态唯一所有者仍为进程树。

既有入库、Ask 和固定拒绝反馈同时统一使用活动模块的执行关系校验，删除重复的工具选择逻辑。回归证明缺少原事件时返回证据缺口，不能只凭工具协议事实接受绑定。

## 验证

- 全工作区现有 Rust 测试 176 项通过。删除重复慢查询后，主模块与进程树 55 项重跑通过。原日志 `/tmp/as-event-owner-tests.log`、`/tmp/as-event-owner-final-tests.log`。
- 既有真实规则／日志测试增加同地址引用和最终释放断言，实际确认日志与完成通知共享事件，消费结束后弱引用无法再取得对象。原日志 `/tmp/as-event-owner-alias-test.log`。
- 现有配置与主模块软件冒烟、Python 五项通过。原日志 `/tmp/as-event-owner-smoke.log`、`/tmp/as-event-owner-python.log`。本机沙箱限制导致的首次 Unix socket 测试失败没有改判为产品失败，获准的本地运行通过。
- 全工作区严格 Clippy、rustdoc、格式检查通过。LSP 扫描的生产文件无错误，既有 `configuration_smoke` 跨 crate 的 `#[path]` 报告仍保留，该路径真实存在且 Cargo 构建通过。原日志 `/tmp/as-event-owner-clippy.log`、`/tmp/as-event-owner-rustdoc.log`、`/tmp/as-event-owner-lsp.log`。

## R39 原 OpenClaw 验收

冻结候选 `2279afe6dc5693a8db05c4959971f8710ca48eac` 已通过原聊天真实验收。Driver Allow 的原事件 53493、Driver Deny 的原事件 40319 和用户允许 Ask 的原事件 48577 都取得实际 read 结果、对应工具结果及 stock final。GLM IPv4 有九次实际 HTTP 200。

Ask 的原事件和命中在公开问询、操作回执及关闭后的 SQLite 中一致。等待期进程查询核对同一生命周期，完成后的真实刷新扫描一个进程后，历史查询仍保留原事件、命中和可选动作。线上剩余时间按发送时刻重新计算，19985 ms 的历史值与 19983 ms 的发送值分别保留，未据此改写原事件。共享事件内存及引用释放由本地回归验证，跨进程的本轮验证核对实际字段和值。

首次允许规则误带拒绝文案，被校验拒绝且没有发 stock 请求。修正规则后的唯一允许请求通过，原设置失败继续保留。两个 AS 实例各接受一次 RequestStop、写出本地回复并实际退出 0，stock／QEMU 退出 0，Native 卸载。

原结果 `/tmp/as-openclaw-evidence-20261111/r39/result.json` SHA256 `cddd6e5da6802ee693904f9f2e08a653d724c7d051418bc4390981103da7d7d5`。十六份引用原件已核对摘要，只读重算与结果一致。原生 IPv6 和工具到内核的完整来源未在本轮执行，保持 NOT_RUN。
