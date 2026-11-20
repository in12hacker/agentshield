# AgentShield

[简体中文](README.zh-CN.md)

AgentShield is a Linux runtime security project for applications and AI agents. It applies rules to process, file, and network operations, presents decision requests to operators, and records the events behind those decisions. It can also mediate outbound TLS connections for model traffic.

[Getting started](#getting-started) · [Architecture](docs/architecture.md) · [Documentation](docs/README.md)

## Capabilities

- **Host protection.** The Linux driver and rules engine evaluate process execution, file access, and network operations. Rules can allow or deny an operation or ask an operator to decide.
- **Security auditing.** AgentShield tracks live processes and persists accepted audit records. The local dashboard shows decision requests, related operations, and captured model exchanges.
- **Controlled model egress.** Agents send scoped phantom keys over mediated TLS. AgentShield holds the real provider credential and uses it for authorized requests. Operators can rotate or revoke phantom keys.

## Getting started

Build and test the Rust workspace from the repository root with Rust 1.89 or later.

```sh
cargo test --workspace --all-targets --locked
```

Follow the [development guide](docs/development.md) to configure the tool environment, then run the software smoke test.

```sh
bash scripts/smoke-main.sh
```

The smoke test uses a driver fixture to exercise configuration, authorization, request handling, and shutdown. For kernel enforcement, build against the target kernel using the [Linux driver guide](drivers/linux/README.md) and run the [isolated native smoke test](scripts/native-smoke/README.md). Dashboard development uses [Node.js 24 and locked dependencies](ui/dashboard/README.md).

## Running on Linux

The driver requires a compatible Linux kernel with BPF LSM and BTF, plus a matching native support module. See the [driver guide](drivers/linux/README.md) for build prerequisites. The installed service uses `<root>/bin/as-service`, `<root>/config/as.yaml`, and `<root>/config/rules.yaml`. Start with the [configuration](config/as.yaml) and [rule](config/rules.example.yaml) examples.

For controlled TLS egress, clients must trust AgentShield's public CA. Import real provider keys through AgentShield's credential management interface and configure agents with the issued phantom keys. See the [TLS egress guide](docs/backend/flow/tls-egress.md) for routing and runtime requirements.

## Documentation

- [Architecture](docs/architecture.md) and [crate index](crates/README.md) explain how the Linux driver, Rust backend, and dashboard fit together.
- [Current capability limits](docs/backend/gaps.md) and the [roadmap](docs/roadmap.md) cover supported operations, verification scope, and planned work.
- The [distribution guide](docs/delivery/conference-vm.md) describes the conference VM and its verification status.

## License

See [LICENSE](LICENSE) and the notices in individual source files.
