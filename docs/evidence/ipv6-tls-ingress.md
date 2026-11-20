# 双栈受控 TLS 与编译产物核查

范围为宿主网络命名空间内的双栈受控 TLS 新连接。公共 `SocketAddr`、`IngressFacts`、SNI、凭证、协议采集和 HIPS 决策保持现有契约。入口共用原主体校验、计划、socket 事实、活连接表、helper 与退役链路。两族共享受鉴权的连接移交，私有记录用统一固定宽度地址。唯一布局见[驱动入口契约](../drivers/linux/flow-binding.md#ipv4-与-ipv6-共用入口契约)。

## 编译产物核查

首轮隔离 guest 的目标内核拒绝装载 `as_flow_sockops`。Clang 18 在启用 `vmlinux.h` 的隐式字段重定位时，将 IPv6 上下文数组的后续索引变成先偏移上下文指针再取值。实际 verifier 返回 `dereference of modified ctx ptr`。原件 `/tmp/as-ipv6-tls-native-run-20261110/middleware-report.json`。只加 `barrier_var` 后的汇编 `/tmp/as-ipv6-tls-r2.bpf.s` 仍有同类指令，该版本未装载运行。

最终由同一对象关闭隐式字段重定位，固定 UAPI 上下文直接读取，内核内部任务字段保留显式 `BPF_CORE_READ`。Clang 18 `-O2` 的两份实际 BPF 汇编已核对。`connect6` 直接读取地址偏移 8、12、16、20，`sockops` 直接读取 IPv6 地址偏移 32 至 60，LLVM IR 保留任务字段 CO-RE 且没有 `bpf_sock_addr` 或 `bpf_sock_ops` 字段重定位。`as_hooks` 的 27 个 LSM 入口在同一实际构建条件下没有上下文指针加减指令。GCC 13.3 原生及 helper 构建另核共享结构尺寸与显式保留位。核查只覆盖当前编译器、对象和目标内核。汇编和 IR 原件 `/tmp/as-ipv6-compile-audit-as_flow.s`、`/tmp/as-ipv6-compile-audit-as_hooks.s`、`/tmp/as-ipv6-compile-audit-flow.ll`。对应机制见 [Linux verifier](https://github.com/torvalds/linux/blob/v7.0/kernel/bpf/verifier.c) 和 [BPF UAPI](https://github.com/torvalds/linux/blob/v7.0/include/uapi/linux/bpf.h)。

R3 实际装载通过，IPv4 与 IPv4 映射 socket 的 TLS、凭证和原对端通过。原生 IPv6 用例失败。实际诊断中，测试使用的 `localhost` 只进入了 IPv4 计划，`connect6` 已运行且报告地址未匹配，nft 只有 IPv4 规则。夹具改用独立的 IPv6 名称，生产端仍使用实际解析地址。诊断原件 `/tmp/as-ipv6-tls-mark-diagnostic-r2-run-20261110/qemu-full.log`，R3 原结果 `/tmp/as-ipv6-tls-native-run-r3-20261110/report.json`。临时诊断镜像首轮曾因资产组可写被现有保护拒绝，调整诊断资产权限后才取得上述证据。

## R4 隔离实测

匹配目标 Linux `7.0.0-1019-nvidia` / aarch64 上 31 项通过。独立 IPv6 名称实际使用 `[::1]`，TLS 工作流的真实服务端凭证应用、轮换和撤销均通过。IPv4 与 IPv4 映射 IPv6 socket 保留实际 `peer_addr` 并成功转发，原生 IPv6 同样保留原对端。IPv4、IPv6 的撤销请求均返回 401。驱动已卸载，guest 和 QEMU 均退出 0。实际镜像 SHA256 `dcac8142fce472828e9d27a3c000c5e12bcfecca13e058b15d600f24f0fbfc10`。原报告 `/tmp/as-ipv6-tls-native-run-r4-20261110/report.json`，SHA256 `61012dfabc0ba96a3f4cc385e84243c2e905aafe29e37a3bd01b5da2e115b65d`，153 份源码和 18 个产物逐项一致。

Rust 工作区 171 项通过，严格 Clippy、格式、rustdoc、Python 客户端和目标 Rust 接口 LSP 核对通过。最终夹具调整后复核了 all-targets 编译。其他网络命名空间、接口作用域地址及尚未纳入的 TLS 模式仍按原边界跟踪。

## R26 真实 OpenClaw 验收

**真实 OpenClaw IPv4 工作流 PASS，真实 OpenClaw 原生 IPv6 NOT_RUN。** 匹配当前内核的实际启动镜像核对 Native、两个 BPF 对象及两个 helper，与冻结提交 `35c47f8f9aaae8f677af4a05e44e9a8f52b20b4d` 一致。两项实际读取分别成功和返回 `EPERM`，原 Ask、回执及日志选择均符合规则。原主体、幻影凭据绑定、协议到结构化值、发布到 SQL 及原查询均核对通过。退役后安全库只有一条启用日志的原记录，两个 AS 实例实际授权退出，模块卸载，guest 和 QEMU 退出 0。

同一冻结候选在隔离 R4 中通过原生 IPv6、IPv4 和 IPv4 映射连接。R26 环境中已有授权上游的两个 AAAA 地址均未通过 TCP／TLS 直连探测，因此真实 OpenClaw 原生 IPv6 工作流未执行。结果 `/tmp/as-openclaw-evidence-20261110/r26/result.json`，SHA256 `53064ad2a32bc05ab4132e856dfd64511007eef982b3f25815eb60a043e2fc3c`。已核对原帧、两份 SQLite 和退役原件哈希。环境原件 `/tmp/as-openclaw-evidence-20261110/r26/authorized-upstream-ipv6-preflight.json`。R23–R25 和早期隔离失败结论保留。

历史限定：下述 GLM IPv6 可达性只指 R26 环境及用户当时确认，不推定今日第三方行为，也不是 AS 其他场景的运行前置。

按用户确认，当前 GLM 上游无法通过 IPv6 访问，真实 GLM／OpenClaw 的 IPv6 场景只做理论验收，真实运行状态保留 NOT_RUN。理论验收依据同一候选的双栈代码路径及隔离 guest 实测。
