# AgentShield

[English](README.en.md)

AgentShield 提供 Linux 进程、文件和网络防护，以及原始事件审计、人工问询和受控 TLS 出口。

- `as-runtime-config`：从真实后端可执行文件一次确定固定布局；严格读取、校验、版本化写入及唯一只读快照。
- `as-main`：组合MainContext与模块接口，保管三条处理职责、消息鉴权／路由及退出。
- `as-policy-engine`：进程／文件／网络规则工厂、匹配、问询及固定规则源读写。
- `as-process-tree`：整机进程镜像、驱动事实维护、初始／最新视图、快速补全与详细查询；主模块负责初始化和通信组合。
- `as-flow`：受控双端TLS与HTTP1／HTTP2，同一正文链路采集LLM原始协议事实和聊天投影，业务关联归中间层。
- `as-credentials`：私有凭证／CA持久所有者、幻影key签发撤销及逐请求作用域授权。
- `as-security-log`：接收原规则日志请求，在固定安全日志数据库中持久保存。
- `as-middleware`：正式回环 WS 和 Unix 服务，结构化保存聊天、问询和业务关系，提供因果视图、Ask 上下文与固定拒绝反馈消费。通用工具执行关联的来源范围见[底层缺口](docs/backend/gaps.md#精确因果来源)。
- `as-types`：只读上下文、配置、事件、规则命中及模块接口；不包含目录发现和文件操作。
- `as-kernel-common`：统一事件帧声明。
- `as-driver-adapter`：Linux实际设备会话、受信BPF加载器、唯一接收泵、原操作与退出。

消息封装位于`as-main/messages`。`drivers/linux`是独立的内核模块与BPF检查入口，通过同一现行协议连接Rust底层；旧独立消息入口和旧初始化／驱动接口已删除。Linux正式启动要求实际模块、完整BPF附着和文件自保初始化全部通过。软件夹具不能证明内核保护、正式中间层或OpenClaw验收。

固定安装布局为`<root>/bin/as-service`、`config/as.yaml`和`config/rules.yaml`。配置模块公开初始化不接受目录参数；源码树中直接运行示例不满足安装布局。[配置样例](config/as.yaml)和[规则样例](config/rules.example.yaml)只使用现行字段。

MVP包预置[Driver快速放行规则](config/rules.fast-allow.yaml)，由原规则模块读取并发布。策略保留在可管理的规则数据中，匹配范围及系统挂载边界见[规则契约](docs/backend/rules/technical.md#可管理的linux快速放行预制)。

```sh
cargo test --workspace --all-targets --locked
bash scripts/smoke-main.sh
```

完整本地检查、工具链和运行环境边界见[开发与验证](docs/development.md)。

[软件冒烟脚本](scripts/smoke-main.sh)创建临时安装布局，运行真实配置初始化、文件写入、版本冲突、管理鉴权／撤权、问询消息关联和线程退出；它使用明确的软件驱动夹具，仅确认模拟采用。真实普通规则与正式主模块／自保冒烟见[隔离驱动冒烟](scripts/native-smoke/README.md)，必须分别按实际执行结果判断。`log: true` 的已接纳规则日志由[底层安全日志](docs/backend/security-log/technical.md)保存。

[脚本化中间层](scripts/mvp-smoke/README.md)通过实际Unix消息接口操作规则、问询、配置、进程树和受控TLS／幻影key，并在隔离guest中验证原操作及退出；同一个Python客户端可供真实测试端接入。MCP排除，OpenClaw由原指定聊天单独验收。[实际范围与结果](docs/evidence/scripted-middleware.md)不代替OpenClaw产品验收。

入口见[模块导航](crates/README.md)、[文档索引](docs/README.md)、[唯一职责](docs/architecture.md)和[MainContext](docs/contracts/main-context.md)。未明确能力见[存档](docs/archive/unresolved-module-contracts.md)，按模块设计后接入。

[Dashboard](ui/dashboard/README.md) 使用正式回环 WS，沿同一连接消费 Ask、Group 和原聊天交换。

受控 TLS／幻影 key 的采集范围从 AS 入口准备完成并启用后新建的连接开始。OpenClaw 进程可以早于 AS 启动。宿主入口支持 IPv4／IPv6 TCP 和 DNS SNI route，默认 rootful Docker 的实例绑定、私有 IPv4 出口与重建隔离已通过真实 OpenClaw 验收。客户端需要信任 AS 公有 CA 并通过 phantom 授权，上游采用正常证书和主机名验证。被动 TLS 采样未实现。固定入口要求 cgroupv2、相关 BPF／netfilter 能力及 nft，依赖不足停止启动。字段语义见[唯一公共契约](docs/contracts/flow.md)，实际范围见[受控出口](docs/backend/flow/tls-egress.md)与[动态接入验收](docs/evidence/container-dynamic-reconnect.md#r36-同一-as-正常运行对照)。

当前已交付范围和剩余事项统一维护在[底层缺口清单](docs/backend/gaps.md#当前剩余范围)。问询「记住」联动与整体分发安排见[路线图](docs/roadmap.md)。

分发与已知问题见[参会镜像](docs/delivery/conference-vm.md)。源码合入主分支不代表已分发 OVA 自动更新，也不代表未完成的真实验收已经通过。

## 凭证与本地配置

真实模型 Key 只填写到 AS 的凭证管理入口；目标 Agent 使用 AS 签发的幻影 Key。幻影 Key 可持续使用，按需轮转或撤销。仓库样例不提供真实凭证，私人配置、运行目录和聊天原件保留在本机，不应作为源码或演示镜像的一部分提交。

维护机的工具路径使用当前操作者的标准目录或显式 `AS_CI_ENV`，见[开发说明](docs/development.md)。公开文档使用通用定位，不依赖某个开发者的用户名或私有目录；这不改变 AS 对本地运行事实的保真采集。
