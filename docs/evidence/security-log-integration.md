# 底层安全日志联调

[安全日志](../backend/security-log/technical.md)消费现有 `RuleLogRequest` 队列。配置派生 `log/security.sqlite`，Main 独立调度通知和日志消费者。中间层原诊断旁路已删除。完整 `RuleLogRequest` 在底层持久化边界序列化，沿用锁定的 SQLite 依赖。

Rust 1.89 工作区测试 171 项通过。严格 Clippy、格式检查、rustdoc、Python 客户端及软件冒烟通过。LSP 核对路径字段、消费接口及 Main 调用者，目标文件无错误。代码复核确认独立接收端与阻塞 I/O 不占用控制通知容量，主线程退出仍等待日志实际排空。运行时包逐项核对 153 份源与 18 个产物。

第一轮的脚本读回误访问了正在执行的检查项，触发 `KeyError: evidence`。服务退出码为 0，实际模块卸载通过，日志读回当时未执行。失败原件保留于 `/tmp/as-security-log-native-run-r1-20261110/report.json`。第二轮只修正脚本的取证范围，产品二进制逐项一致。

第二轮实际运行镜像 `89078bb2687dc2e2b0894a4924f40d7774757417607478ae778b864bf94ae7ae`。原报告 `/tmp/as-security-log-native-run-r2-20261110/report.json`，SHA256 `6df3b40303a205a08ea138cbbc459432bbe8666ea9f5dab84f15482512609293`。30 项场景通过。只读打开已退出服务的安全日志库，核对 10 条原事件、规则、裁决和回执，确认两条 `log=false` 规则仍有完成回执且未写入日志。匹配内核为 Linux 7.0.0-1019-nvidia/aarch64，guest、QEMU 退出 0，驱动实际卸载。

## R24 原 OpenClaw 有限验收

**PASS**，绑定提交 `5080b0a6649918a123d5e4c492e2ad5a33bd26d8` 和 153 份源、18 个产物。两个真实文件 Open 问询均有完成回执，业务查询均返回 `completed`。服务实际退出后，`log=true` 的允许决定对应一条 SQLite 记录，事件、规则版本、DecisionId、提案、结果及终止报告与原回执一致。`log=false` 的拒绝有完成回执，同一规则版本无对应日志行。实际授权停止、父进程等待、驱动卸载和 QEMU 退出均通过。

原结果 `/tmp/as-openclaw-evidence-20261110/r24/result.json` 的 SHA256 为 `e526a914da1268e89bb2b1f17ef36a12d555e48dfb0f2267c7dcbd582b48ccc0`，SQLite 原件 SHA256 为 `d4e3d5bd21602969b51fc9670f8363b172efb42e4971c8407fd10bca92c4426d`。已核对结果和四份原件哈希，并只读确认数据库内只有一条记录。全驱动矩阵、远端查询、轮转、保留策略、写失败注入和断电恢复未执行。R23 保真范围及原样例结果保留。
