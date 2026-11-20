# 驱动直决计数联调

四项底层缺口收口的第一个候选。唯一线格式见[驱动通信](../contracts/driver-communication/technical.md#驱动直决计数)，既有查询见[进程树统计](../backend/process-tree/technical.md#8-命中归集修复和资源)。近期问询计数及「记住」联动沿原路线图。

## 本地验证

- Rust 工作区 171 项、Python 客户端 5 项通过，严格 Clippy、格式、rustdoc 和目标接口 LSP 通过。首次沙箱运行的本地 TCP／Unix socket 被执行环境拒绝，普通用户权限复跑通过。
- 当前 aarch64 `7.0.0-1019-nvidia` 的原生模块、两个 BPF 对象及 C 加载器真实构建成功。与 R26 候选比较，`as_hooks` 的 55 个指令／对应 ELF 重定位节、`as_flow` 的 7 个对应节字节一致。新原生模块及两个 BPF 对象在隔离 guest 实际加载，最终实际卸载，服务与 QEMU 正常退出。
- 隔离 guest 32 项通过。同一个进程实例在静默允许规则下实际打开 12 次，在换版的有日志拒绝规则下得到 3 次 EPERM。进程树按两个规则版本分别返回 12 和 3，`incomplete=false`。删除规则后两次实际默认允许没有生成新命中。未做真实计数饱和压力测试。

产物 `/tmp/as-r27-hit-count-mvp-20261110` 的 initramfs SHA256 `38ae9e7b13bb9b2eeee414a777fe4b014f39ba8e9fa6d0f86ed78f602f8ccb76`。已核对 153 个源码输入及 18 个产物。隔离报告 `/tmp/as-r27-hit-count-run-20261110/report.json` SHA256 `f1ffe3ba06e2000705a12606ce0a4e414cfa904d87f22cb602a7bfe8731f004c`，计数明细见同目录 `middleware-report.json`。

## 真实 OpenClaw

首轮冻结提交 `50c4f47f5` 的真实 Gateway 已就绪且 Flow 配置已保存。AS 的原 RequestStop 回执实际写出，随后在 driver 退役阶段以 Unavailable 和退出码 1 结束。初始化 FAIL，三项真实计数场景 NOT_RUN。原 `rmmod` 返回 Resource temporarily unavailable，QEMU 实际退出且已 join。结果 `/tmp/as-openclaw-evidence-20261110/r27/result.json` SHA256 `e5d78e04c9c56178d506481691e1bcc06835250dc1d461d701fbcc6b691641e0`。

定向反例在快速输入队列有序关闭后交付计数批，原实现返回 Stopped，沿现有接收泵错误分支可使驱动进入 DeclareFailure。正常退役路径已接入同一快速输入拒接收分流。运行中的接收端关闭仍返回 Stopped，实际停止后只释放无消费者的计数批，保留驱动 Quiesce、状态与回执的实际读取。首轮原件精度仅到 driver.shutdown，是否为同一原因仍以真实复验为准。

修正候选 `76aec3f9f57b1b1b31647123dd45ba15fd77f85e` 的 Rust 172 项、Python 5 项、严格 Clippy、格式、rustdoc 和 LSP 通过。修正后隔离 guest 的计数、两次实际退出、原生卸载均通过，整轮因独立的只读权限测试断言失败保留 FAIL。报告 `/tmp/as-r27-retirement-run-20261110/report.json` SHA256 `fb2b8afcc21ade333864a1196cd76d99e16e336bcef170460a6fd24ef0a5cc46`。

R28 真实 OpenClaw IPv4 PASS。同一热 Gateway 进程贯穿三个真实会话。静默允许读到实际原内容，原规则版本计数增加 1，安全日志没有该版本记录。有日志拒绝得到 EPERM，原规则版本计数增加 1，实际原事件序号 56971 对应唯一安全日志记录。无命中默认允许读到实际内容，两个版本的计数和首末边界保持原值。两个 AS 实例各一次 RequestStop，写出回执并实际退出 0；OpenClaw、guest 和 QEMU 正常退出，原生模块实际卸载。

原结果 `/tmp/as-openclaw-evidence-20261110/r28/result.json` SHA256 `e1b9f66f16f71360e4e48de4cfe595cc6f57b158daac333343a590541a2d9340`。已核 12 份原件哈希并只读核对安全日志的规则版本、宿主 TGID、出生时间和原事件序号。初次离线脚本对计数的拒绝文案字段及维护规则条件有误判，原件保留，修正后只复核已保存数据，三个请求没有重放。真实 GLM IPv6 按用户确认采用理论验收，运行保持 NOT_RUN。
