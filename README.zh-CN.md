# AgentShield

[English](README.md)

AgentShield 是面向应用和 AI Agent 的 Linux 运行时安全项目。它按规则处理进程、文件与网络操作，在需要时向操作员发出问询，并记录决策对应的安全事件。它也可以为模型流量提供受控 TLS 出口。

[快速开始](#快速开始) · [架构](docs/architecture.md) · [文档](docs/README.md)

## 功能

- **主机防护。** Linux 驱动和规则引擎判断进程执行、文件访问和网络操作。规则可以放行、阻止，或交由操作员决定。
- **安全审计。** AgentShield 跟踪运行中的进程，并持久化已接纳的审计记录。本地面板展示问询、关联操作和已采集的模型交换。
- **受控模型出口。** Agent 通过受控 TLS 连接提交限定范围的幻影 Key。AgentShield 保管真实模型凭证，并将其用于授权请求。操作员可以轮转或撤销幻影 Key。

## 快速开始

使用 Rust 1.89 或更高版本，在仓库根目录构建并测试工作区。

```sh
cargo test --workspace --all-targets --locked
```

按[开发指南](docs/development.md)配置工具环境，再运行软件冒烟。

```sh
bash scripts/smoke-main.sh
```

软件冒烟使用驱动夹具验证配置、鉴权、请求处理和退出。验证内核执法时，请按 [Linux 驱动指南](drivers/linux/README.md)针对目标内核构建，并运行[隔离驱动冒烟](scripts/native-smoke/README.md)。面板开发使用 [Node.js 24 和锁定依赖](ui/dashboard/README.md)。

## 在 Linux 上运行

驱动需要支持 BPF LSM 和 BTF 的兼容内核，以及与目标内核匹配的原生支撑模块。构建条件见[驱动指南](drivers/linux/README.md)。服务安装布局为 `<root>/bin/as-service`、`<root>/config/as.yaml` 和 `<root>/config/rules.yaml`，可从[配置](config/as.yaml)和[规则](config/rules.example.yaml)样例开始。

使用受控 TLS 出口时，客户端需要信任 AgentShield 的公开 CA。真实模型凭证通过 AgentShield 的凭证管理入口导入，Agent 侧配置签发的幻影 Key。路由和运行条件见 [TLS 出口指南](docs/backend/flow/tls-egress.md)。

## 文档

- [架构](docs/architecture.md)和 [crate 索引](crates/README.md)介绍 Linux 驱动、Rust 后端和面板如何配合。
- [当前能力边界](docs/backend/gaps.md)和[路线图](docs/roadmap.md)记录支持的操作、验证范围和后续工作。
- [分发指南](docs/delivery/conference-vm.md)介绍参会虚拟机及其验证状态。

## 许可

见 [LICENSE](LICENSE) 和各源文件的许可声明。
