# OpenClaw正文采集预算修复

版本边界：本页记录 R14／R15 及所列 2026-11-09 冻结候选；正文中的“当前修复”、retained 池、整交换预约与旧配置值仅属当时实现，不是现行执行方案。原 FAIL／PASS、配置原值、候选及原件 hash 保留。现行 Flow 使用原在途状态与队列，见[在途内存与消费](../backend/flow/technical.md#在途内存与消费)。

R14原运行码为375836399。原件`$WORKSPACE/.runtime/openclaw-fixed-feedback-validation-20261109/stock-openclaw/full-guest-as-events.jsonl`实际包含三条capture Gap：exchange11一条、exchange14两条，reason均为observer_capacity。只读提取和原件SHA见`/tmp/as-capture-admission-evidence.json`。它们对应缺失的Ask-deny响应，以及Ask-allow后续请求／响应；不是从流量计数推测的原因。

原实现把长期保留的AuthScope、拒绝上下文和反馈副本都计入短期解析信号量。实际配置max_connection_buffer_bytes=2097152、max_total_buffer_bytes=33554432，每个方向一次预约8×2097152。少量保留元数据就会使两个方向／并发正文无法再同时取得预约；Observer的try_acquire失败后把整条正文采集关闭，即使稍后内存空闲也不恢复。lost_deliveries和parse_failures分别计投递／解析失败，均不能证明没有这类原始capture Gap。

当前修复将两个不同生命周期分开。长期认证／反馈状态计入Flow私有retained池，上限取现有delivery_limits.replay_bytes，明确作为独立内存组成；不再消耗解析预约。启用正文采集的交换，在上游工作前一次异步预约两侧所需额度，两个Observer共享同一许可，最后一个释放才归还。并发暂满等待，可由强制停止取消；不把正文转成不采集。反馈所需正文保管也归retained池，不在持有完整解析预约时再次等待同一解析池。

配置数字保持原值，验证和实际接纳共用同一个reservation_bytes公式。默认32MiB解析池容纳一个完整双向交换，其余受现有stream／exchange容量约束异步等待；没有新增线程、Owner、规则或测试专用放行。这里的预约口径只属本页旧候选；现行内存上限和所有权见[Flow封装](../backend/flow/technical.md#在途内存与消费)。

本地workspace/all-targets测试通过，包含真实双TLS HTTP1/HTTP2和固定反馈，以及针对本次缺陷的回归：先保留真实拒绝，再接纳完整双向交换；第二交换等待，单方向结束不提前归还双向额度，结束后恢复、强制停止可取消且预算回收。严格Clippy、fmt、rustdoc和LSP引用检查通过。原日志`/tmp/as-capture-admission-*`；首次沙箱socket EPERM及初始测试词法锁Clippy失败保留，修正后的实际结果分别使用workspace和clippy-r2记录。

独立交叉审核`/tmp/as-capture-admission-review.md`已核双向预约、等待取消、所有权归还、配置与文档，无剩余必修问题。新冻结包`/tmp/as-capture-admission-delivery-r2-20261109`的131项源码输入与当前树一致；实际自有QEMU运行原件位于`/tmp/as-capture-admission-run-20261109`，29项required通过，guest／QEMU退出0、join及Native实际卸载通过，142.17秒。它验证实际Native与TLS／固定反馈组合，不能代替原聊天对OpenClaw两项Ask的采集复验。

R14五项HIPS效果和必要固定命中文本仍按其原范围通过；完整聊天采集的旧失败不回填。本轮未知Agent自动工具关联没有新增实现。新运行时的实际应用复验需绑定本次候选及其新原件，不能沿用旧R13、R14或此前受控反馈通过记录。

## R15真实OpenClaw复验

原专门聊天已对a1fca61ec18a6565d0cf73a24f1449be6ab18d28完成独立真实OpenClaw／GLM VM复验。本轮限定结果PASS，唯一入口为`$WORKSPACE/.runtime/openclaw-capture-admission-validation-20261109/result.json`，SHA256为`4e749d4979b5455cb9b577e5c216dfc5d7fd023fe81ab203b4bf0d7addf7b0c8`。

两项Ask的原用户选择、同DecisionId执行回执、后续工具结果及最终complete响应均完整，后续exchange分别为11和14。`stock-openclaw/ask-followup-proof.json`保留原行号、record／message ID和字段位置。Gateway工具ID和线上请求工具ID保持各自原值，不借这次核对建立字符清洗别名或ToolCall→系统操作的通用因果桥。

`stock-openclaw/full-guest-as-events.jsonl`含15,502帧、5,502 Chat帧。全量原始Gap审计为0，observer_capacity为0；15个已捕获请求交换均有complete响应。原始流SHA256为`6ba38d42790b085245fe4c015dcb1a069097d629f55ad77a5860fcc5fb289926`。相同离线检查仍能检出旧R14三条observer_capacity且失败退出，原件保留，未回填旧失败。原UI每个case只提交一次，未重发旧请求补数据。

131个源码输入及19项产物由原聊天独立核验；Root随后再次对131源码、service／Native／client身份和两个Ask原始行号逐值核对，并独立遍历全量原始流。核对原件为`/tmp/as-capture-admission-root-verification-20261109.json`，没有发送模型请求或操作OpenClaw实例。service SHA为`5200384f6637054aaa873ed384cccb414b5bc1935df5ee5d249fcbced3d5ad3f`，Native仍为`575aab0dcc32e7546e6ba7213f6fb7633b6ec26a7476461e121985542dced230`。

配置保持32MiB parser及8MiB retained上限；这不是总RSS或无限并发证明。五项真实工作流及其独立协议／原生核验通过。普通Allow最终响应含明确redaction，保留原record_refs，不把脱敏当丢失。各层故障域分别保留：Native无待决／未回收回执／丢弃；Flow无活动交换、投递丢失或解析失败；Main最新可见拒收样本notices5／logs0，Media观察43／未转发6且未持久化，不能把后两者归零或改称采集Gap。

取证后正常退役自有VM：OpenClaw、AS、guest和QEMU退出0，Native实际卸载、QEMU已join、所属用户进程无剩余、未强制清理。该通过仅覆盖本候选和本轮捕获完整性／原HIPS流程。通用自动工具桥、Ask20条审计窗口、Chat Completions精确反馈及后续内存路径优化仍不在已实现／已验收范围内。
