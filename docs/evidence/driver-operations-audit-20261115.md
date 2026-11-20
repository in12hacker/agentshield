# 驱动原操作、普通FIFO与停机功能核验

本记录绑定当时的功能审计候选；随后已实施[写关闭Native替换及冷恢复启动顺序整改](write-close-native-replacement-20261115.md)。下文未迁移fanotify的范围说明是该旧候选事实，不是后来当前实现。

本轮在[目录边界整理](driver-boundary-audit-20261115.md)之后，实际核对hook接纳、采集、三池分流、直接裁决、原请求等待、结果移交及停止／故障清空。逐功能契约、源码锚点和结论见 `/tmp/as-driver-operations-audit-20261115/function-audit.json`；原源码／契约及哈希保留在同目录 `before/`、`before.json`。这是功能链核验，不把局部guest扩成全部驱动或Agent业务验收。

## 已证问题与根因

旧 `drained_locked` 只列出部分分类计数，遗漏普通FIFO中的Direct、基础观察等实际记录。后端收到Quiesce后结束接收泵，尚在内核队列中的原件可能被正常关闭清掉。部分直接日志、命中计数和返回通知又能在停止观测接纳之后新增，单纯增加排空条件会让持续日志妨碍退出。

原产物已在同一隔离guest复现：独立subject完成实际Deny后，Quiesce回复前读出的Direct数为0，停止后的操作又留下可读尾部。完整恢复与清理执行后才分类失败，原件为 `guest-before.log`，未设置测试器截止。原日志的旧汇总文案把范围写得过窄，具体失败以其中 `direct_quiesce_tail` 原记录为准，未回改历史日志。

另查到直接裁决的边界耦合：已确定Allow／Deny后，可选 `capture_execution` 错误会覆盖结果并跳过既定终止。这是源码可达路径，未证明正常有效BPF返回和已预留容量能触发，不能称已实测故障。

## 就地收敛

- 复用既有 `reader.stop_facts` 作为停止新观测接纳的界线。普通原请求在原接纳锁内固定一个私有直决观测标记，Direct及命中计数共用它；已接纳采集沿原capture／observer／runtime_inflight寿命发布。原返回通知在同一短锁内检查接纳、保存当前固定标量并排队，不留下未计数的锁外发布者。普通直接规则继续执法。
- 普通FIFO只保留单条、原子批和移除的公共处理点，分类计数唯一维护，出队／取消／故障清空统一归还。批次仍整体接纳、一次唤醒；没有新增队列、容量、线程或状态Owner。排空检查实际FIFO为空及已有在途寿命，不再复制易遗漏的消息类型清单。
- 直接裁决的可选现场采集、既定信号尝试和发布收进原 `complete_direct`。采集失败只计真实观测丢失，不改变决定、不跳过信号，也不发送残缺帧；帧大小在采集完成后按实际长度计算。
- 删除Native无人读取的 `FINISHED`、只写的 `runtime_pending` 及重复 `quiesce_id`。原生完成记录移交既有AS_RESULT队列后释放，Rust能力上的完成结果仍保留原语义。所有现有采集点、实际已取得载荷字段和线上结构均未删改，没有旧接口或旧布局回退。

真实hook仍使用薄入口及既有 `file_gate／start_at／capture／finish`，不为合并参数再加注册器或通用框架。成熟分层依据已核对[AppArmor的common_perm与实际路径hook](https://github.com/torvalds/linux/blob/v7.0/security/apparmor/lsm.c#L196)，归入[既有调研](../drivers/references.md#hook到操作规则匹配的源码依据)。本轮 `as_core.c` 从2690行到2712行，净增22行；包含数据结构和声明，不冒称这是纯功能代码统计。

## 当前产物验证

完整哈希、实际记录和范围为 `/tmp/as-driver-operations-audit-20261115/result.json`，原件为 `guest-accepted.log`，包内清单为 `guest-accepted/root/suite/source-sha256.json`。当前执行源码与该清单一致，模块／BPF／加载器复制哈希一致。

- 原统一all构建通过，当前Native模块、BPF对象和静态适配组件均由原入口生成。
- 终端clangd实际AST／引用查询有效，无错误；12条已有Linux宏展开的sizeof提示保留在原LSP日志，经源码核对未改为产品缺陷。第一次Clang ABI配置失败的记录也保留，不把空诊断冒充有效AST。相关Clippy、格式和shell语法检查通过。
- 实际aarch64 guest、`7.0.0-1019-nvidia`：原直接允许／拒绝、同一次等待／答复、已配置回退、未绑定原生失败、替换退代、临时覆盖和停机流程完成。带真实StartFacts的控制进程死亡／继承FD／恢复，覆盖普通观察和命中计数的异常清空。
- 统一版 `direct_quiesce_tail`：Quiesce回复前读出Direct为1；回复后再次操作仍实际errno1，观测队列无新尾部。没有用最近事件或时间窗口证明身份。Native接收原帧逐字节保留在 `native_fd_event.frame_hex`，结果摘要只索引其行号／哈希。
- 正常恢复排空、Native模块卸载、guest退出0，宿主实际QEMU wait退出0。旧失败和第一版功能修复通过的 `guest-final.log` 都保留，最终只以统一源码的 `guest-accepted.log` 为本次依据。
- 独立只读交叉审核推动补齐命中计数的接纳界线，清除只写状态；最终复核通过，记录为同目录 `review.json`。执行证据由主代理取得，审核不冒称另跑了一次验收。

Group TCP原子批路径已审阅／编译，本guest未实际触发；没有借其他普通消息替它报运行通过。本轮没有重跑宿主签名／加载、Process／Network Ask业务集成或原版OpenClaw／GLM。既有可配置 `max_hold_ms` 及ReviewBinding期限未改，前者仍是AS原生保管窗口，不是自然网络I/O期限。fanotify迁移也不在本轮，边界保持现行契约。
