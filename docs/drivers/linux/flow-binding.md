# TLS观察来源与底层连接边界

状态：被动TLS采样仍为设计；受控 IPv4／IPv6 新连接入口已有代码，实际验证范围另记证据。MCP来源研究暂缓。公共事件只依赖[流量公共契约](../../contracts/flow.md)。本页只维护内核hook、连接身份和共享值的实现约束；用户态接纳、私有FD交接和资源清理只在[底层适配](../../backend/driver-adapter/technical.md#linux受控入口私有实现)定义，不把它们带入内核。

## 当前实现与未接入方向

内核承担已有HIPS检查及真实进程／连接事实，底层驱动适配负责实际连接、网络域、受信句柄和加载资源；TLS解密、重加密、协议解析及凭证授权由Flow和凭证模块拥有。当前LSM网络入口通常看到密文，不能据socket流经推定已取得聊天内容。

被动TLS采样没有当前生产端口，旧CaptureEvent／TlsCaptureInput／PlaintextFunctionSample及采样预算草案不作为现行接口，设计细节只见[归档草案](../../archive/flow-design/tls-capture-interface-draft.md)。eCapture等成熟项目只能提供公开函数、返回长度和对象寿命的机制参考，不能证明AS已附着、支持某一运行时或拥有消息写入权。当前调用方不构造空采样会话，也不以补齐这些可选来源作为普通HIPS或受控TLS运行条件。

## 未接入方向的源码依据

既有调研读取了固定[eCapture openssl.h](https://github.com/gojue/ecapture/blob/bc89a0ed0ea5da82f430e2e0dc2560ad8089b345/kern/openssl.h)及[加载代码](https://github.com/gojue/ecapture/blob/bc89a0ed0ea5da82f430e2e0dc2560ad8089b345/internal/probe/openssl/openssl_probe.go)：其text模式附着SSL_read／SSL_write，不证明AS已实现，也不外推SSL_read_ex／SSL_write_ex或其他TLS库。

[OpenSSL读](https://docs.openssl.org/3.0/man3/SSL_read/)／[写](https://docs.openssl.org/3.0/man3/SSL_write/)的有效返回长度、重试和部分写语义仍是来源事实；_ex返回成功标志，长度取输出参数。SSL对象释放／重置与BIO绑定须按真实对象寿命理解，memory BIO不一定有socket FD。函数返回采样不自动成为不可变实送证明，同PID最近connect、裸指针或FD相同也不证明跨生命周期身份。这些依据保留供后续已授权来源开发使用，不产生当前端口、支持清单或验收前置。

## 连接导流的边界

[mitmproxy Linux源码](https://github.com/mitmproxy/mitmproxy_rs/blob/cddbee77659bae2e8cc210f73b6c3eaa9c34cafe/mitmproxy-linux-ebpf/src/main.rs)在cgroup sock_create根据范围绑定导流接口；这是新socket接入依据，不证明能追溯迁移既有TLS连接。这里只参考机制，不复制其PID／comm匹配为AS身份授权，也不复制sudo启动器或第二套加载器。

用户态中介沿当前请求保管实际传输和待发送正文，不能从只读采样取得写入能力。TLS CA及上游验证归底层凭证与Flow，当前类型与能力只引用[公共契约](../../contracts/flow.md)。当前导流接入的对象、启用顺序和网络范围见下文与[受控 TLS 出口](../../backend/flow/tls-egress.md)。

## 当前受控入口实现

内核BPF程序位于drivers/linux/as_flow.bpf.c，共享值布局在as_flow.h；用户态接纳器位于[驱动适配native目录](../../../crates/backend/as-driver-adapter/native/README.md)。as_tls_ingress.h只承载底层内部FD交接，不属于驱动通用控制协议。

当前来源从入口启用后的 connect4／connect6 回调建立新连接记录。已建立的旧连接没有该次 connect 记录，不由后续包到达或相同端点补造。目标进程可早于入口运行，存量进程观察仍由进程树负责。TLS 接入条件统一见[出口契约](../../backend/flow/tls-egress.md#支持范围与成熟依据)。

入口复用cgroup sock_addr／sockops、sk_storage和nft packet DNAT。connect4 与 connect6 在同一主体 birth／TGID 和明确目标命中后给原 socket 设置本会话 mark；已有非零SO_MARK的选中socket拒绝，不能覆盖其他软件标记。未命中不改socket。命中后同时核当前task网络命名空间inode及原socket的bpf_get_netns_cookie，与helper真实监听socket的SO_NETNS_COOKIE一致；不把task的netns推为socket归属。按原socket建立成功回调记录cookie、原目标和调用task事实，离开Established时按同一cookie清除，socket存储由内核随对象释放。没有TLS库探针副本。

专属 nft inet 表按实际传输族、会话 mark 和 TCP 进行 OUTPUT redirect。DNAT只改数据包，socket自身原peer不变，原Network Connect／Send匹配继续使用原目标；不能把目的地替换为proxy后再运行规则。Kernel v7源码的net/socket.c::__sys_connect_file先调用security_socket_connect，再执行socket connect；net/ipv4/tcp_ipv4.c::tcp_v4_pre_connect在原socket锁内调用cgroup connect程序。已有as_gate_sendmsg仍读取原socket sk_daddr，入口不修改该字段。最终是否生效须按真实Send Allow／Deny冒烟单独报告。

用户态的真实监听、conntrack接纳、私有FD传输和nft子进程清理均归[底层适配](../../backend/driver-adapter/technical.md#linux受控入口私有实现)，不由内核驱动重复实现。

## 入口网络域

内核计划只消费已核定的主体、目标、实际netns inode／cookie及容器cgroup ID。容器入口按真实cgroup ID限制当前任务，宿主入口不接纳其他任务网络域；每次标记继续核原socket的netns cookie。计划的用户态序列化和目录／netns FD交接归底层适配，不进入通用驱动协议。

## IPv4 与 IPv6 共用入口契约

本轮扩展宿主网络命名空间中的原生 IPv6 TCP。保留同一个 helper、计划、socket 存储、活连接表和移交端口。`connect4` 与 `connect6` 只负责从实际 hook 参数取得目标，接着复用同一主体、地址、网络命名空间和标记检查。`sockops` 确认实际建立的本地地址与原对端，地址族参与查找键。IPv4 映射的 IPv6 socket 按其实际 IPv4 传输地址进入 IPv4 分支，保留原 socket 及其连接事实。带接口作用域的 IPv6 地址及链路本地地址没有当前匹配键，计划拒绝该范围。

C／BPF共享值布局以 `drivers/linux/as_flow.h` 为准，Rust接纳端按同一值布局读取；适配内部的FD报文只在 `crates/backend/as-driver-adapter/native/as_tls_ingress.h` 声明。端点由地址族（4 或 6，原生端序）、16 字节网络序地址和网络序端口槽组成。IPv4 使用前 4 字节，余字节全零。活连接键由同一传输地址族、两个 16 字节地址和实际本地／远端端口组成，端口用原生端序。身份、cookie 与原时间字段保持原义。显式保留字段为零，旧长度不再接收。公共 `TlsIngressInput` 仍使用 `SocketAddr`，`IngressFacts` 仍使用 `IpAddr`，无需新字段或命令。

机制依据为 Linux 的 [IPv6 TCP 接入](https://github.com/torvalds/linux/blob/v7.0/net/ipv6/tcp_ipv6.c#L110)、[conntrack 原目标读取](https://github.com/torvalds/linux/blob/v7.0/net/netfilter/nf_conntrack_proto.c)及 [nftables redirect](https://netfilter.org/projects/nftables/manpage.html)。IPv4、IPv6的已有隔离TLS结果分别保留原对端、真实凭证应用及退役范围，不用一方替代另一方，也不把未执行的可选协议族变为已可运行路径的前置；OpenClaw实际完整工作流另行认定。

`bpf_sock_addr` 和 `bpf_sock_ops` 属于固定 [BPF UAPI](https://github.com/torvalds/linux/blob/v7.0/include/uapi/linux/bpf.h)。入口对象关闭 `vmlinux.h` 的隐式字段重定位，保证上下文数组按固定偏移读取。可变内核 `task_struct` 字段继续显式 `BPF_CORE_READ`。保留字节和尺寸断言覆盖 C／BPF 共享数据，实际是否可加载仍由目标内核 verifier 核实。
