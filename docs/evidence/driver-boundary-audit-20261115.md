# 驱动职责边界核验

本记录绑定本轮目录整理的源码／产物快照；后续原操作及队列功能整改见[功能核验](driver-operations-audit-20261115.md)，不能把本页旧guest结果覆盖后来的源码。

本轮按文件核对驱动的职责、注册入口、接口引用和资源归属；不是全驱动逐行鲁棒性审定。逐文件记录及源码 SHA-256 为 `/tmp/as-driver-boundary-audit-20261115/file-audit.json`，覆盖23个当前驱动目录文件和4个底层原生组件文件。起点是 `28dada3efb47a7d893d66ace861aec5d0f843fb2` 加现有未提交改动，不能把HEAD本身当成本轮候选。

## 已收敛的归属

- 用户态 `as_loader.c`、`as_flow_loader.c` 移入[底层适配native目录](../../crates/backend/as-driver-adapter/native/README.md)。原生模块、BPF源码及共享值继续归[内核驱动](../drivers/architecture.md)，构建仍复用 `drivers/linux/build.sh all`，产物名称不变。
- 仅用户态消费的112字节FD接纳报文移入底层私有 `as_tls_ingress.h`；`as_flow.h`只保留原BPF共享值。字段顺序、尺寸和Rust读取偏移未改变，没有旧布局回退。
- 两份既有Native打包入口收录迁移后的源码哈希。参会镜像的原构建脚本按仓库相对位置复制驱动和底层原生组件，避免新目录被漏掉；未新增打包器。
- 删除无当前构建、打包或生产调用方的 `as_flow_probe.c`（51行）。它是诊断消费者，不是内核采集生产者。旧源码与原验收证据保留在Git及历史记录，不把文件移动的行数算作代码消融。
- 监听、conntrack接纳、私有FD交接、nft子进程及清理的细节归[底层适配技术文档](../backend/driver-adapter/technical.md#linux受控入口私有实现)；驱动文档只维护实际BPF入口、连接身份和共享值。

## 采集范围与其他机制

生命周期、原请求返回、private futex、TCP字节及运行时回调采集均保留。fprobe的实际调用／返回事实、netfilter的实际TCP字节和uprobes的原请求交接用途不同，不因没有当前业务消费者而删除，也不据此声称精确归因已全部完成。Group的packet入口返回 `NF_ACCEPT`，不是另一套普通HIPS裁决。公共事实继续复用原 `capture`／`append_process_info`，瞬时采样不改为历史缓存。

当前fanotify写关闭来源未替换。Linux已有 `file_release` 不证明AS已经实现等价的目录、路径时点及对象寿命，未取得等价接线前不删除原来源；边界见[安全日志](../backend/security-log/technical.md#写关闭审计)。原生 `max_hold_ms` 生产契约本轮未改，不能把它称为自然模型超时；它与测试器额外截止分别核验。

## 旧诊断入口的就地重构

实际执行暴露旧消费者与当前接口不一致：旧规则头在初始化被拒绝；修正后，旧subject调用缺少 `open` 参数，helper实际退出2。沿同一入口核对后，一并更新当前规则头、唯一CLI／`result`字段及 `Failed { native_error: -110 }` 原生结果，不添加接口别名或旧结果解析。

删除该入口10／30秒测试器截止及FD诊断的15秒poll截止。整个正常阶段共用一次等待，直接观察原消费者的真实结束并传播其错误；正常停机在该阶段完成后执行，不靠测试器计时中断仍在推进的原请求。规则ReviewBinding期限及原生保管期限仍按各自原契约执行，不伪称这些期限已删除。

旧FD诊断还在关闭控制连接前发送加载器退役命令，顺序与正式DriverSession相反。按正式顺序收敛为Quiesce实际排空、关闭控制FD、退役本实例links、waitpid核实退出。修改前失败和修改后结果均保留；未取得旧unpin的具体errno，不补造错误码或扩大为新的内核缺陷。

## 实际验证与范围

最终结果及哈希：`/tmp/as-driver-boundary-audit-20261115/result.json`。实际执行：

- 原统一构建及复制目录后的同一构建均通过，生成Native模块、BPF对象和两个静态用户态组件。
- 终端clangd及rust-analyzer的LSP检查、当前诊断例子的严格Clippy、格式和相关shell语法检查通过。本轮无Codex原生LSP工具，未冒称使用它。
- `/tmp/as-driver-boundary-audit-20261115/guest-reviewed.log`：真实aarch64 guest、内核 `7.0.0-1019-nvidia`；直接允许／拒绝、原请求绑定和答复、已配置回退、未绑定原生失败、持久替换退代、临时控制及停机均取得实际结果。原控制进程死亡及继承FD、恢复后旧FD释放、真实恢复排空均通过。最终模块卸载、guest退出0及宿主QEMU实际wait退出0。
- 当前可执行源码与包内清单逐项一致，复制的模块、BPF及加载器哈希与本轮构建一致。收口后更新的冒烟README说明在结果中单列，不冒称它的哈希仍等于包内旧说明。旧失败记录 `guest.log`、`guest-r2.log`、`guest-current.log` 原样保留，没有用最终通过覆盖历史。
- 独立只读交叉审核发现并促成消费者退代等待的退出处理收敛；最终限定复核通过，记录于同目录 `review.json`。

本轮不证明宿主签名／加载、TLS入口实际导流、进程或网络Ask、OpenClaw／GLM完整验收、x86_64参会镜像重建或VirtualBox启动。没有重新启动这些范围，也没有修改宿主驱动、Root broker、用户roadmap或旧WP Goal。
