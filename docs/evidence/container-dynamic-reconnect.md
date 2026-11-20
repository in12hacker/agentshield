# 运行中容器与重连可行性证据

本页保留已执行的机制试验和真实 OpenClaw 结果。当前产品范围见[动态接入设计](../backend/flow/container-egress-design.md)。

## 运行后取得对象

在相同 7.0.0-1019-nvidia／aarch64 隔离内核中，普通 runc 创建并启动容器，随后观察程序自行打开运行中 init 的 pidfd，核对 runtime state、启动时间和实际 cgroup，取得独立网络命名空间 FD。原进程退出由同一 pidfd 确认。此例使用自有 runc 私有 state 作交叉核验，未声称默认 Docker 的生产绑定已实现。原日志 /tmp/as-oci-live-attach.nHs9kR/qemu.log。

此前创建时移交 pidfd 的隔离试验已取得相同种类的对象引用。另一个私有 netns 试验验证了 nft 原目标、传递 FD 和同网络域上游 socket。这些证明 Linux 机制，不等于 Docker 或 OpenClaw 产品链已交付。

## Docker 默认 runc 链的运行后绑定

隔离 guest 使用 aarch64 `7.0.0-1019-nvidia`、Docker 29.6.2、containerd 2.2.5、runc 1.3.6。以本地 BusyBox 镜像运行默认 bridge 容器。guest 的 Docker 使用 vfs，关闭 iptables／ip6tables。容器使用默认 runc，无启动回调或 pidfd 移交设置。

容器运行后，探针从真实 dockerd 的 Unix socket 读取完整 ID 和 Inspect 数据，通过 `SO_PEERCRED` 核对 daemon PID 与 root UID。先打开 init pidfd，再通过默认执行根 `/run/docker/runtime-runc/moby` 的 `runc state` 验证同 ID、运行态和同一 PID。从活 pidfd 获取 cgroup ID 与 netns FD，再核 cgroup 目录 inode 和 Docker sandbox FD 的设备号与 inode。两次 Inspect 一致，原 pidfd 在核对后仍未退出。此次使用公开 `runc state`，直接读取私有 `state.json` 的证据仅属于上一节试验。

| 实际操作 | Docker ID | init PID | cgroup ID | netns inode |
| --- | --- | ---: | ---: | ---: |
| 启动后绑定 | `3e1dac0f…` | 213 | 132 | 4026532146 |
| 重启后重新绑定 | `3e1dac0f…` | 314 | 233 | 4026532199 |
| 删除并同名重建 | `111f167a…` | 414 | 334 | 4026532252 |

探针用各自的旧 pidfd 观测实际退出，重启前后 Docker ID 相同，同名重建后完整 ID 不同。三个对象核对通过，探针及 QEMU 实际退出 0。原日志 `/tmp/as-docker-live-bind.uyWCmp/qemu-v4.log`，SHA256 `5acda4ea0b683e4daf5060a0e1b71ac7c458f120e65805f803e6c8977ccc2e10`。探针 `/tmp/as-docker-live-bind.uyWCmp/root/suite/docker_bind.py`，SHA256 `d403c82c2cf2d14e4291e3367fc22bf4323aeb827d01ed6507c095304bc48644`。同目录前三轮保留 guest 启动准备失败记录，分别缺代理程序、就绪检查超时和缺 `/var/run` 链接。

该试验只验证 init 活对象与运行时身份的核对。未执行容器内其他进程的事件归属、生产适配接线、私有网络域 TLS、凭证或 OpenClaw 验收。

## AS 生产链在隔离 Docker 的验收

同一候选的两次实际启动覆盖 Docker 先于 AS，以及 AS 先于 Docker。后者在同一原 AS 实例上等到 Docker 导入镜像并启动，进程树按实际新 init 报告绑定。AS 自己实际挂载树上的传播修改仍返回 EPERM。两轮均以完整 Docker ID 下发普通 File.Open 规则，选中容器原操作被拒绝，另一已核验容器访问同一文件通过。

为两轮配置私有 TLS 后，已运行的容器内客户端取得实际 HTTP 200／SSE，provider 仅在该容器的 loopback 监听且确认收到授权的真实凭证。原事件的 `ProcessKey`、容器实例及独立 procfs cgroup／PID 命名空间一致。重启保持完整 Docker ID 并采用新入口代，驱动仍为 Running。同名重建得到新完整 ID，旧规则不匹配，旧私有入口实际停止接纳。两轮均确认 AS 实际退出、驱动卸载、QEMU 退出并已等待。

- AS 先启动原件 `/tmp/as-container-guest.KzFv7r/docker-qemu-live-reader-as-first.log`，SHA256 `c9937e107d4744b1bbf13290c845a1cd54c8b43710af35e9bda80c9d13a419b7`。
- Docker 先启动原件 `/tmp/as-container-guest.KzFv7r/docker-qemu-live-reader-docker-first.log`，SHA256 `b8f9f120b37d736efb6c54978309d42e307b35649e483635d3ce866ed9cfa6d3`。
- 同一服务、模块和入口产物来自 `/tmp/as-container-guest.KzFv7r/frozen`。已核对 155 个源码与 18 个产物，清单 SHA256 `b0b6f5ce6377c7da20792df56d8ed8265f8def124cfdbf92db7ce1a335fc8420`。Rust 173 项、Python 5 项、Clippy、rustdoc 和 LSP 通过。

前序隔离原件保留。`docker-qemu-final-*` 记录自保误拦独立旧根卸载及其当时影响。`docker-qemu-frozen-*` 记录同一候选在烟测客户端阻塞等待 Docker 命令期间未消费回执，内核出现 `unbound_wait` 超时；上述通过轮在 Docker 命令执行期间持续读取原回执。冻结候选的宿主回归在只读权限撤销断言上记录失败，退出前已有 29 项通过并实际卸载，原件 `/tmp/as-container-guest.KzFv7r/frozen-regression/report.json`。这条断言与 Docker 专项分别报告。真实 OpenClaw 验收另行记录。

## R32 真实 OpenClaw 规则反例

原聊天使用候选 `657cff36af90339b49dc0cf3e1706a6603e75188`、实际 rootful Docker bridge、与宿主原镜像相同的镜像及文件系统层、原 tini 和 Gateway 启动命令。宿主原容器使用 host 网络，隔离复测使用 bridge 来覆盖私有出口。AS 已报告完整容器 ID、实际 init／Gateway 进程和私有入口就绪。

应用精确容器范围的后端 `File.Open` 规则后，stock 探针返回 EIO，AS 管理连接断开。原事件流保留 517 条无规则命中的回执，其中 420 条允许、97 条原生错误 -512。未观察到目标规则命中，模型请求未执行。退役失败，原聊天已等待本轮实际 QEMU 退出。原结果 `/tmp/as-openclaw-evidence-20261110/r32/result.json` SHA256 `31666002932aa7f50dcc797318ee6936b0da4c95832bb6399b978d817ee0480e`。原事件流 SHA256 `a14c686dcd9681b7a9ee3f5bba028d2ea106b3fe81a7750f39d7964eab795f4c`。

旧镜像的后端操作位会移交整个 Open 类别。修正让现有规则编译器投影必要的原路径条件，原生复用匹配器排除确定不符的请求；无条件候选、原路径缺失和实际容器规则命中仍交后端。参见[驱动规则分流](../drivers/linux/rules.md)。

修正候选在同一隔离内核的两种启动顺序各完成 700 次真实非目标文件打开，驱动保持 Running，目标操作仍被拒绝，另一个容器访问同一目标通过。两轮随后都取得私有 loopback TLS HTTP 200、凭证应用、重启采用新入口代、同名重建隔离和实际退出卸载。AS 先启动原件 `/tmp/as-container-guest.KzFv7r/docker-qemu-path-route-as-first.log` SHA256 `7da7655bdd8664e1cce90694bd6232a8b1e59407971fc47bc833793bb7378b59`。Docker 先启动原件 `/tmp/as-container-guest.KzFv7r/docker-qemu-path-route-docker-first.log` SHA256 `a608e2f8ef24b2dc633ee6bec73cd4498303a51064155196c1495fd0558da288`。冻结清单 `/tmp/as-container-guest.KzFv7r/path-route/root/suite/source-mvp-sha256.json` SHA256 `3756231d7c38d5ed11390f110bc7b9a3616d6be9a61545f91d1deb60f09ed134`。

修正候选 `61c400801321ea3477052e8edf86e6493fb9e7c0` 的宿主回归仍在只读权限撤销断言上失败，退出前宿主双栈受控 TLS 通过，驱动实际卸载。原件 `/tmp/as-container-guest.KzFv7r/path-route-regression/report.json` SHA256 `39c3085a4079e54c012dee3f8ca3049b4dd07a061389f2bf4716526a753140c9`。该断言与容器专项分开。

## R32 独立真实 TLS 验收

原候选 `657cff36a` 的独立轮未施加失败的文件规则。AS 先于目标、AS 后于已运行目标的两个新 stock OpenClaw 会话均取得实际最终响应。每轮私有入口接纳数增加 2，原记录核对完整 Docker ID、真实 Gateway ProcessKey、cgroup 和网络域，取得同一 exchange 的原协议、凭证 `prepared`／`applied`／`submitted` 及完成聊天事实。

同 ID 重启实际退出 0，新 Gateway ProcessKey 与原值不同，入口从第 1 代换到第 2 代。重启后的 stock UI 未发出第三条请求，该请求为 NOT_RUN。同名重建及旧凭证在本轮为 NOT_RUN。stock 容器、dockerd、AS 三个实例、guest 和 QEMU 均实际退出 0，驱动实际卸载。原结果 `/tmp/as-openclaw-evidence-20261110/r32-tls/result.json` SHA256 `78a27af37f75998aaeadef034ad8e7a77ae419a8a71ef5b5336ceaeb75d96577`。这组 TLS 证据独立于 R32 文件规则失败和修正候选的 R33 复验。

## R33 真实 OpenClaw 复验

候选 `61c400801321ea3477052e8edf86e6493fb9e7c0` 的两种顺序各取得真实模型最终响应、私有 IPv4 入口、完整原容器与进程身份、原聊天及凭证应用和提交证据。两次目标路径读取实际返回 EPERM，与原 RuleHit 及 SQLite 原记录一致。同一容器的未匹配路径正常读取，驱动保持 Running。

另一容器的读对照未执行，准备文件时 `docker cp` 被取消。实际事件 `215071` 来自宿主 dockerd，TGID 181，原 cgroup 1，`workload=None`，无 RuleHit，原回执为 `cancelled`。规则把这个未知归属的同路径打开留作潜在候选。修正增加[同一原事件的 cgroup 祖先链](../contracts/events.md)，只有实际范围排除可证明时才判这条规则不匹配。R32 的全局 EIO 与此结果分别保留。

同 ID 重启实际退出 0，取得新 ProcessKey，入口代 1→2。下一次 sessions.create 收到 Gateway 的 `agent-database-inspection-pending`，实际模型请求为 NOT_RUN。同名重建及旧凭证拒绝为 NOT_RUN。三个 AS、stock 容器、dockerd、guest 和 QEMU 实际正常退出并卸载驱动。原结果 `/tmp/as-openclaw-evidence-20261110/r33/result.json` SHA256 `23ed0bd5dbca2d4cbbf25ca5abcdd25bfc3d352a7704ed15802348eff0a0ee8f`，事件流 SHA256 `82710e73281ac59563f11b7798913b7b3c6dc09c9a9cfeb21f6a806eb6be1d48`。

范围排除修正在两种隔离 Docker 顺序下通过真实宿主读取、dockerd 从另一容器复制文件、该另一容器读取及目标拒绝。私有 TLS、凭证、重启换代、同名重建隔离和实际卸载仍通过。原祖先链已按序结构化入库，重开测试保留原 u64。Rust 174 项、Python 5 项、严格 Clippy 与 LSP 通过。AS 先启动日志 `/tmp/as-container-guest.KzFv7r/docker-qemu-scope-store-as-first.log` SHA256 `10e1d2ee9026f44671bbc7a83b52d64c000ce2a0d773d4daaceff550831a1098`，Docker 先启动日志 `/tmp/as-container-guest.KzFv7r/docker-qemu-scope-store-docker-first.log` SHA256 `b64cbbdabfc429287104b107eef20ea887437195ac4af7751fcef6e5d3a8a892`。修正候选产物清单 `/tmp/as-container-guest.KzFv7r/scope-store/root/suite/source-mvp-sha256.json` SHA256 `cfbe73f1bd8c80f8b4ac72929e1d183ee2906280c5598899f89519f7444f6121`。

## R34 同路径及退役实例复验

候选 `0e70b04c2bc5a7b19e9acc6cb0594611f7ae6d36` 实测通过目标 EPERM、未匹配路径读取、旧实例仍运行时的范围外 dockerd 同路径复制和另一容器的同路径读取。dockerd 原事件的 cgroup 祖先链 `[1]`，`workload=None`、`hit=None`，允许结果与原回执一致。

原选中实例删除后，同名新实例已取得不同完整 ID，准备文件时 dockerd 在同一个层级根的同路径打开再次取消，新实例未启动，旧规则及凭证的实际新实例探针为 NOT_RUN。原结果 `/tmp/as-openclaw-evidence-20261110/r34/result.json` SHA256 `2a744b042ab042b77144bfdfe06e97ec1b51f29214c6dfebe0259abfd5fcfd53`。修正依据为完整祖先链只有根一项时，默认 Docker 容器只能处在该根的子树，范围排除不再要求已删除实例持有活绑定。其余原事实未知不更改。

修正候选在两种隔离 Docker 顺序中实际删除规则选中的旧完整 ID，并在同名新实例仍未启动时复测 dockerd 复制与宿主同路径读取，均成功。随后新实例正常启动、同路径读取通过，旧入口已停收。两轮仍通过目标拒绝、私有 TLS、凭证、同 ID 重启换代与实际退出卸载。AS 先启动日志 `/tmp/as-container-guest.KzFv7r/docker-qemu-retired-scope-as-first.log` SHA256 `ae3a519049d54d735c05d0b02e51ccbee09f4e31e7cb720462fc0369f717ce37`，Docker 先启动日志 `/tmp/as-container-guest.KzFv7r/docker-qemu-retired-scope-docker-first.log` SHA256 `3561b6de76d8a9def5af58ec8a98329360ae61e83f82f5071dc29b4cc82ea647`。冻结清单 `/tmp/as-container-guest.KzFv7r/retired-scope/root/suite/source-mvp-sha256.json` SHA256 `cc6ca683bc9ea759a1a24e494c0aa9481b827e59147d4e9f1db6fe986cc8c755`。

R34 同候选的独立模型补测取得唯一一条原生 `sessions.create` 和真实最终响应，私有入口接纳 +1，同一 exchange 原请求、凭证应用和提交及完成聊天均通过。stock 容器、两个 AS、Docker 和 QEMU 实际退出 0，驱动实际卸载。模型结果 `/tmp/as-openclaw-evidence-20261110/r34-model/result.json` SHA256 `2184e6752e479b0bc6e806b797be88937408ca4ccf78d53877cba5f460d9061a`。R34 范围事件流 `/tmp/as-openclaw-evidence-20261110/r34/as-events.jsonl` SHA256 `94ea8b07668a0e6b57ad3d9de877711132f23edafe83a9d87a19d926707f5761`。两条验收记录分别判定。

## R35 旧 ID 删除后的真实复验

候选 `92e47a2d17626f2798cd811dbbb281e0102d61e5` 中，旧 ID 实际退出 0 并由 Docker 回查确认已删除。同名新 ID 创建后仍未启动时，dockerd 同路径复制成功。原 frames 29／30 为层级根 `[1]`，`workload=None`、`hit=None`、实际 `allowed`。同名新实例实际 Running，具有不同完整 ID、新 ProcessKey、真实 cgroup 和 netns。同路径读取成功，frames 38／39 均归属新 ID，无旧 RuleHit。

新 stock 进程在旧凭证探针前实际退出 1，退出早于退役。应用 stderr 未保留，根因未确认，该轮旧凭证探针为 NOT_RUN。三个 AS、dockerd、guest 和 QEMU 正常退出，模块实际卸载。原结果 `/tmp/as-openclaw-evidence-20261110/r35/result.json` SHA256 `5e88655c2085ce0b70e81826e51c7e786755150c388a1bf34be0ef59c653af9f`。

同一冻结候选的隔离 Docker 凭证对照先验证旧实例正常授权，再让同名新实例通过实际私有入口送出旧 phantom。原绑定仍指向旧完整 ID，新实例收到 `403 credential_scope`，AS 记录的 `scope_rejected` 使用新实例的真实 ProcessKey 和 Workload，真实上游收到 0 条 HTTP 请求。实际退出并卸载通过。原日志 `/tmp/as-container-guest.KzFv7r/old-token-proof/qemu.log` SHA256 `2a883685b6593506d66e8bad5236f1a33e4fbb49cb053d6a8202ebad19493a76`。这是同一候选的隔离 Docker 执行证据，与 R35 的 stock 负例 NOT_RUN 分开。

R35 诊断轮在同一冻结候选再次证实旧 ID 删除后复制成功、新实例同路径读取且无旧 RuleHit。真实新 stock 容器的 Node 以旧 phantom 发出负对照，实际 IPv4 HTTP 403，原事件在新完整 ID 上为 `scope_rejected/scope_denied`，无提交。该轮新 Gateway 原日志记录插件文件打开 ESHUTDOWN 及后续 sidecar TypeError，实际退出 1；期间存在管理性 AS 停止，原件未证明异常根因。三个 AS、dockerd、guest/QEMU 正常退出且驱动实际卸载。诊断结果 `/tmp/as-openclaw-evidence-20261110/r35-diagnostic/result.json` SHA256 `d889d523db8d400c8c47b16a9c9314b0ce8f62c46f2c05b35f2c916a4f1bf82b`。旧规则和凭证隔离通过，目标稳定运行该轮仍失败，独立核对。

## R36 同一 AS 正常运行对照

最终代码候选 `92e47a2d17626f2798cd811dbbb281e0102d61e5`。同一 AS 进程 PID 441 的启动 ticks 和实例身份贯穿旧、新 stock 容器两个生命周期，943.530 秒窗口内 149 次实际采样均为 Running，窗口内未发 RequestStop，未观察到公开源错误或 EIO／ESHUTDOWN 回执。旧、新完整 Docker ID 不同，原容器实际退出 0 并删除。两次 Gateway 都取得 `/startupz` 与 `/readyz` 的 HTTP 200，实际状态为 `started`、`ready=true`、`coreReady=true`、`failing=[]`。新 Gateway 的五次就绪采样跨 26.764 秒。CPU 压力提示按原日志保留。

新 stock 容器、Docker、两个 AS、guest 和 QEMU 正常退出 0，驱动实际卸载。R36 不发送模型请求，真实模型及凭证链沿 R33 和 R34 独立记录，同名重建的规则及旧 phantom 隔离沿同候选 R35 记录。R35 失败原件不改，R36 不定位其异常根因。R36 原结果 `/tmp/as-openclaw-evidence-20261110/r36/result.json` SHA256 `38a41deed6a3c15ef19f4c7a2d90ee83260103787c06fd1168fdc410b400c2ee`，已核对结果引用的十份原件。默认 rootful Docker 私有 IPv4 工作流及实例更替的本项验收闭合，stock 原生 IPv6 仍为 NOT_RUN。

## 定向关闭原 socket

自有隔离 guest 创建目标 TCP 连接和一条独立对照连接。SOCK_DESTROY 使用原四元组与实际 socket cookie。错误 cookie 返回 ENOENT，原连接仍能通信。正确 cookie 使原客户端出现 ECONNABORTED、对端出现 ECONNRESET，独立连接继续通信。随后测试进程新建 socket，取得不同 cookie 并完成实际发送。该新连接由测试进程主动建立，只证明内核关闭和再次连接的效果。

原日志 /tmp/as-owned-socket-destroy-proof-20261110/qemu.log，SHA256 af3bd7a80e378822a5e6e1d85daa9f56757c5bc1c3b2711844bff61bfc5a2906。Linux 实现核对 namespace、连接身份及 cookie，再执行实际关闭。[TCP diag](https://github.com/torvalds/linux/blob/v7.0/net/ipv4/tcp_diag.c#L525-L619) · [iproute2 既有调用](https://github.com/iproute2/iproute2/blob/main/misc/ss.c#L3830-L3851)

## R30 真实 OpenClaw

候选 6156a7ad2 的实际直连基线已取得 GLM HTTP 200 和流式响应。原 active 请求在 AS 入口就绪前结束且 socket 自然关闭，idle 场景也未保留可操作的原 socket，两项定向断开为 NOT_RUN。

独立的不同 stock 会话保持同一 Gateway 进程、现有 CA 和 key。新连接进入 AS，原生接纳数增加到 8。原 OpenClaw 日志实际出现 SELF_SIGNED_CERT_IN_CHAIN，会话以 error 结束，HTTP 授权未执行。此结果证伪当前环境下的透明 TLS 端到端切入。它不证明定向关闭机制失败。

三个 AS 实例实际退出 0，OpenClaw 与 QEMU 正常退出，模块已卸载。原结果 /tmp/as-openclaw-evidence-20261110/r30/result.json，SHA256 681fd4ddb4a1b1c15ed8f07b27d2a3d1275c44189cbb0b041fffa37a2a0333ce。已核原结果引用的十份原件哈希。

## R31 进行中的原请求

同一冻结候选的另一条新真实会话，在原 Gateway 直连 GLM 并收到流式 HTTP 200 后启动 AS。旧 socket 在 AS 启用期间继续接收 133739 字节，入口就绪时仍然存在。执行一次定向关闭，核对原 inode 7342、cookie 1 及四元组。原 Gateway 进程保持运行，随后出现新 inode 8459、cookie 2006，AS 实际接纳数由 0 增至 6。

定向关闭和新 TCP 连接通过。原 OpenClaw 日志第 246 行记录 terminated，第 289 行对同一 runId 汇总 retry=6。后续实际证书错误为 SELF_SIGNED_CERT_IN_CHAIN，原会话以 error 结束。TLS 信任失败，HTTP 授权未执行，AS 未取得后续聊天事实。没有手工重发原请求或替换客户端 CA／key。空闲连接场景沿 R30 保留 NOT_RUN。

两个 AS 实例均由各自一次 RequestStop 退出 0，OpenClaw、guest 和 QEMU 正常退出，驱动实际卸载。原结果 /tmp/as-openclaw-evidence-20261110/r31/result.json，SHA256 77f281dcca0c0d71494d56783361e07e233fe1b82b2ee8bde2115204078d1527。已核十份原件哈希及原重试日志。当前既有环境下的自动切入端到端失败，强断重连没有进入生产代码。
