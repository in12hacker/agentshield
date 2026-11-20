# 脚本化中间层真实联调

本页为原批次证据；已退役的 `scripted_service.rs` 链接固定到保留它的历史提交，当前服务入口见 [as-service](../../crates/middleware/as-middleware/src/bin/as-service.rs)。

日期：2026-11-08。基线152cfd5e0，范围为当前Unix消息宿主、Python中间层及其实际Linux驱动／规则／配置／进程树／Flow／凭证消费链。MCP排除；受控provider不等于真实模型或OpenClaw产品验收。

## 实现和边界

[Python入口](../../scripts/mvp-smoke/middleware.py)通过实际已鉴权的Unix连接操作底层；薄宿主组合正式Main及LinuxDriverAdapter，[transport](../../crates/middleware/as-middleware/src/transport.rs)只封装现有普通命令、聊天订阅和私有管理端口。没有软件驱动回退，不直接修改后端自保配置，不导入旧REST／SQLite编排。

规则不限定探针PID或来源程序：文件使用数据目录前缀，进程执行使用目标目录前缀，网络发送使用回环CIDR／TCP。原生程序和独立Python程序分别验证同一文件策略。各场景通过正式管理接口替换策略，不插入测试专用Allow。Flow使用AllProtected，凭证授权仍绑定真实ProcessKey生命期；扩大观察范围不扩大秘密使用权限。

## 真实暴露和修复的问题

- 首轮包装继承可组写权限，正式后端正确拒绝启动。包装已固定umask与受保护目录权限，并核实际cpio中的uid／gid／mode；没有放宽后端检查。
- 普通文件Driver group使用系统自保保留号，正式规则校验拒绝。场景改用普通编号，保留系统保留号校验。
- r4／r5核心业务通过，但真实服务退出失败。r5有限诊断记录`nft exec errno=125`、helper exit1及`flow → ingress_stop → Unavailable`。普通策略为空时，原Main提前关闭回调消费者，Flow清理nft的普通exec／file操作被取消。现先关闭外部管理及问询、排空已接纳管理任务，保持回调直到Flow／凭证／树刷新完成清理，再封回调、Quiesce及排空驱动。未加Allow豁免、未吞错误。独立退役任务继续保管调用者取消后的资源。
- 源码审核发现已提前失败的服务可被脚本误记退出成功，及LLM条件仅查非空tool ID。现所有退出都实际wait并核当前实例标记；LLM核原始字节、真实来源、文本、工具名／参数及provider实际生成ID。修前反例和原失败保留。

原件：`/tmp/as-mvp-scripted-r{1,3,4,5}/run`；未执行的r2包装不计测试结果。退役修复分析及检查见`/tmp/as-scripted-retirement-fix-report.md`，审核反例见`/tmp/as-mvp-middleware-review`。

## 最终同源实际结果

`/tmp/as-mvp-scripted-r6/run/middleware-report.json`记录27项必需场景全部PASS：真实启动／状态和树查询；Driver／Backend快规则、记住优先；Ask允许／拒绝／超时／断线及无渠道跳过；临时开关、重载／恢复与自保；文件／执行／网络发送的允许、仅阻止、终止actor、同时终止原parent；受控TLS／phantom／原始聊天／轮换／撤销；授权撤销及实际退出。

允许／阻止按原调用errno、真实文件内容／网络字节及实际waitpid信号验证。TLS正常及轮换后HTTP200，上游认证实际匹配；撤销后HTTP401，上游实际请求仍为2。原始消息及聊天投影未出现已知秘密；tool call保留provider真实ID及参数。数据库／聊天持久化分别保留NOT_IMPLEMENTED，不算PASS。

普通宿主UID1000运行QEMU，guest真实root加载原Native／BPF。最终service实际exit0并wait，无强制夹具清理；guest exit0、Native实际卸载、QEMU exit0及子进程join完成；112.12秒，无超时。不是Root broker执行或其监督回执；未操作宿主内核模块、nft或信任库。

- 镜像：`/tmp/as-mvp-scripted-r6/initramfs.cpio.gz`，SHA256 `9fca24b8cf251a647312e952c752fee1ba80af87d2a94b923124acc19685b528`。
- kernel：`/tmp/as-tls-native-main-r8-local-run/Image`，SHA256 `6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6`。
- 清单：`/tmp/as-mvp-scripted-r6/root/suite/source-mvp-sha256.json`，SHA256 `2db45f275b31a3f69e637c6f5886cf6ce1f27bf33909b6310067238371ff5428`；122源文件／18产物构建前后及运行后匹配。后续README／docs修改不改变运行源码。
- 原始串口、退出／回收结果：`/tmp/as-mvp-scripted-r6/run/qemu-full.log`及`report.json`。

## 本地检查和审核

显式source本地CI环境并检查当前工具后，MSRV1.89 workspace／all-targets、strict Clippy、fmt和rustdoc全部通过；索引`/tmp/as-mvp-final-integrated-results.json`，93源码／manifest／lock检查前后无变化。Rust Analyzer诊断及引用记录与最终transport／宿主／runtime源码hash对应，见同前缀`lsp-source-match.json`。Python六项传输／退出鲁棒性检查在普通UID通过，`/tmp/as-mvp-broad-client-tests-host.log`；沙箱bind被拒的原失败日志保留，不记通过。

有限独立源码审核见`/tmp/as-mvp-middleware-cross-review.md`；扩大规则范围后的补充审核另存`/tmp/as-mvp-broad-cross-review.md`。实际内核结果单独按上述r6记录认定，不以作者自检或源码审核代替保护效果。

## OpenClaw和未实现范围

原指定聊天已准备现有OpenClaw测试端并核健康，准备证据入口`$WORKSPACE/.runtime/openclaw-refactor-prep-20261108/handoff.json`。该实例实际使用宿主网络；这不证明精确ContainerInstance保护。旧Root会话失效后，用户恢复原代理，指定聊天已实际核验新通道并仅尝试一次正式模块加载。

当前冻结模块SHA256 `256336a0a038aba71032fdf3025f5936143415c96eab812f8069f4ff8774c2cf`在宿主加载失败：insmod exit1，原stderr为`Key was rejected by service`，内核原日志为`Loading of unsigned module is rejected`；实际signer为空、sig_enforce=Y，加载后模块不存在。Root job已终止且子树／supervisor回收，cleanup_error=false；原Root代理及OpenClaw保持运行。AS未启动、模型调用为零，OpenClaw保护效果仍为NOT_RUN。此结果证伪当前未签名产物能在该宿主直接加载，不是某条规则或幻影key的效果失败，不用上述guest通过替代。

实际反证入口`$WORKSPACE/.runtime/openclaw-refactor-live-5dce23f23-20261108/result.json`；同目录`native-admission-submit.jsonl`保留原Root终态和完整加载／内核原件，`artifact-hashes.json`绑定证据。需要正常受信签名／安装交付发生实际变化后再验证；签名安装链尚未实现。

受控出口仍限宿主netns、IPv4新TCP和DNS SNI route；独立容器netns、IPv6、持久日志／聊天库及正式远程CS宿主未实现。准备场景包括真实对话、普通项目文件／命令与网络操作，由原聊天执行，只交付需求及验收标准，不由本聊天操作或指导OpenClaw。
