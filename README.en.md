# AgentShield

[简体中文](README.md)

AgentShield provides rule-based protection for Linux processes, files, and network operations. It records security events, asks an operator for a decision when a rule requires one, and supports controlled TLS egress for model traffic.

## Components

- **Host protection.** The Linux driver and its Rust adapter capture events and enforce policy. The rules engine matches operations, the process tree tracks live process state, and the security log persists accepted audit records. See the [Linux driver](drivers/linux/README.md) and [rules contract](docs/backend/rules/technical.md).
- **Controlled egress.** AgentShield mediates TLS and supports HTTP/1.1 and HTTP/2. An agent presents a scoped phantom key. AgentShield holds the real upstream credential and uses it only when the request is authorized. See [TLS egress](docs/backend/flow/tls-egress.md) and the [Flow contract](docs/contracts/flow.md).
- **Middleware and dashboard.** The runtime serves management and event data over a Unix socket and loopback WebSocket. The [dashboard](ui/dashboard/README.md) presents decision requests, related operations, and captured chat exchanges. See the [middleware documentation](docs/middleware/technical.md) and [current capability limits](docs/backend/gaps.md).

The [architecture guide](docs/architecture.md), [crate index](crates/README.md), and [documentation index](docs/README.md) describe the boundaries between these components.

## Build and verify

From the repository root, run:

```sh
cargo test --workspace --all-targets --locked
bash scripts/smoke-main.sh
```

See [development and verification](docs/development.md) for the toolchain and environment setup. The software smoke test exercises configuration, authorization, request handling, and shutdown using a driver fixture. The [isolated native smoke test](scripts/native-smoke/README.md) exercises the Linux driver and enforcement path.

The installed service expects `<root>/bin/as-service`, `<root>/config/as.yaml`, and `<root>/config/rules.yaml`. Start from the [configuration example](config/as.yaml) and [rule example](config/rules.example.yaml). See the [driver guide](drivers/linux/README.md) for Linux prerequisites and the [distribution guide](docs/delivery/conference-vm.md) for the conference VM and its verification status.

## Credentials and local data

Enter real model-provider keys in AgentShield's credential management interface. Configure the agent with a phantom key issued by AgentShield. Operators can rotate or revoke phantom keys as needed. Keep credentials, local configuration, runtime data, and chat records out of the source tree.

See the [roadmap](docs/roadmap.md) for planned work and [LICENSE](LICENSE) for licensing terms.
