# 参会 OVA 封装

产品和分发边界见[唯一说明](../../docs/delivery/conference-vm.md)。这里是构建及使用入口，不另定义 AS 协议。

先核对[当前分发状态](../../docs/delivery/conference-vm.md#当前分发状态)：Work中的旧R7未含后续幻影Key输入／保存修正，审核包也未含7508完整Provider接入实现。以下步骤描述现行源码，不能据本README称旧OVA已更新或用户VirtualBox已验证通过。

## 使用者

1. 安装开源 VirtualBox 基础版；本镜像不需要 Extension Pack。
2. 在「导入虚拟电脑」选择 `AgentShield-Conference-x86_64.ova`。使用默认 BIOS／SATA／NAT，建议 4 CPU、8 GiB 内存。磁盘容量 40 GiB，按实际数据占用空间。
3. 开机等待图形桌面。桌面自动显示 `START-HERE.html`，包含 AS Dashboard、OpenClaw Web UI 的本机入口和该副本的管理令牌。
4. 在AS凭证与出站页导入自己的真实模型Key，绑定该副本预置连接并签发幻影Key；真实Key只进AS。使用桌面“配置 OpenClaw 幻影 Key”粘贴幻影值，按原CLI提示处理其热加载状态，再用OpenClaw发一条正常聊天。旧R7的助手尚有已证输入／保存问题，先按当前交付状态取得修正入口；不得反复重启或撤销Key来迁就旧脚本。
5. 正常使用桌面菜单关机。再次分发请使用干净 OVA，不能分发已填 Key、已有聊天的个人磁盘。

`demo` 为图形桌面和本机串口控制台自动登录的管理员，可使用正常 sudo 权限；AS 配置为 `manage_service`，可以自行管理规则、凭证、模型服务和出口。不加试用激活、功能锁、只读模式、内核升级锁或额外 Docker 能力裁剪。SSH 默认不启动，管理员可以按普通系统方式自行配置启用。首次开机生成独立系统密码、本机令牌和 OpenClaw 配置，密码显示在本机欢迎页，可自行修改；干净母盘不携带它们。本版驱动绑定所带内核，用户升级内核后需同步重建驱动。虚拟机拥有独立内核，不要求宿主机重启、登记 MOK 或安装 AS 内核模块。ARM 主机不能原生运行此 x86_64 镜像；QEMU 可进行跨架构模拟，性能另计。

### 幻影Key输入与结果

现行[openclaw-phantom.py](openclaw-phantom.py)通过xterm显示明文输入，Ctrl+Shift+V或Shift+Insert读取来宾CLIPBOARD。输入可见不等于宿主共享剪贴板已配置；不要粘贴模型商真实Key。取消不保存，失败保留原错误与退出码，窗口等待用户关闭。

助手通过stock `config set --batch-file … --strict-json`一次保存公开Provider、默认模型和SSE传输，只有幻影值进入容器。它不直接改Agent源码、不重建容器、不在保存后强制restart，也不发模型请求。原CLI返回0只是保存成功；实际聊天、凭证使用和响应各按原件核对。欢迎页的English test cases随后可用于用户主动尝试指定进程／文件／网站／DNS的允许和拒绝，不是聊天启用前置。

输入链已证失败与修正过程见[原证据](../../docs/evidence/conference-vm-build.md#幻影key输入与保存链的历史边界)。单文件修正入口在来宾中以`python3 Configure-OpenClaw-Phantom-Key.py`运行，不绕回旧安装助手；它与源码助手共享实现，正式镜像是否包含仍以交付清单为准。

## 构建者

先 source `$HOME/.local/share/agentshield-ci/env.sh`，真实检查 cargo／x86_64 工具链、npm、QEMU、qemu-img、SSH 和 xorriso。工作目录必须独占；不复用其他聊天的 VM 或 Docker。固定已核实底座 SHA、OpenClaw OCI manifest／config 摘要和当前 AS 源提交；构建输出不提交到 Git。

当前构建分为三个实际边界：

NoCloud 使用同目录 `cloud-init.yaml`，只替换 `__BUILDER_SSH_PUBLIC_KEY__`。`libelf-dev` 明确选择 Debian 官方 backports：稳定版 0.192 的静态归档缺陷为 [Debian #1102039](https://bugs.debian.org/1102039)，上游 0.193 起已修复，本次使用 0.196 backport。不向 AS 添加链接补丁或动态库回退。挂载选项作为同一个 shell 参数传递，挂载成功且载荷存在后才写构建就绪标记。

- 使用 Debian 官方 generic backports qcow2，核对官方 SHA512SUMS 后建立 40 GiB 构建磁盘。NoCloud seed 只提供临时 builder 公钥及实际所需的开源包；通过只读 9p 传入冻结源码、x86_64 静态 `as-service`、正式 UI dist、`openclaw-image.tar` 和 `downloads/openclaw-image.json`。JSON 记录官方 `image` manifest 摘要与 `local_id` config 摘要，运行时固定本地 image ID，避免重新联网解析 tag。
- 把只读驱动源码及底层驱动适配的 `native` 源码按原仓库相对目录复制到来宾 `driver-source`，通过同目录 `build-driver.sh NEW_BUILD_DIRECTORY` 调用原 `drivers/linux/build.sh`；libbpf 1.6.3 只供 pahole 的 BTF 去重使用，BPF 头与静态 loader 继续使用来宾发行版库。这个分工对应已核的 x86 BTF 去重缺陷，不修改 kfunc 类型检查。再把 `prepare-guest.sh` 冻结为来宾本地副本、核同 SHA，执行 `prepare-guest.sh NATIVE_BUILD_DIRECTORY`；运行期间不改该副本。封装只核实并安装该批产物、正式服务及图形入口，加载离线容器，不重做驱动编译。包安装完成标记为 `/var/lib/as-packages-ready`，载荷固定挂载在 `/mnt/payload`。不添加测试专用 Allow，不绕过驱动。实际内核头、BTF 和模块必须匹配。封装不改变当前产品功能。
- 保留干净母盘，另建验证副本执行首次启动与实际服务／界面检查。封存先同步等待 builder 的实际 systemd 用户 slice 停止，再删除账号；母盘去除 builder、SSH host keys、cloud-init seed、机器身份和构建临时数据；禁止把验证副本作为母盘发布。正常关机后运行 `export-ova.py SEALED_QCOW2 OUTPUT_DIRECTORY`。脚本用 qemu-img 原有锁检查停机磁盘，转换并逐扇区比较 VMDK，再输出 OVF、SHA-256 manifest 和 OVA。

编译 `as-service` 复用 Cargo.lock 与现行静态 Linux GNU 构建方式：`cargo build --locked --offline --release --target x86_64-unknown-linux-gnu -p as-middleware --bin as-service`，编译环境显式设置 `RUSTFLAGS='-C target-feature=+crt-static --cfg getrandom_backend="linux_getrandom"'`。ARM 构建者需提供真实 x86_64 交叉 CC／AR／linker；不把宿主架构产物重命名装包。UI 使用既有 `npm run build`，`VITE_AS_WS_URL=ws://127.0.0.1:8765`，AS 仅接受本机 `http://127.0.0.1:8080` Origin。

## 再分发材料

OVA 与 `sources`、`licenses`、组件清单、使用说明及 SHA256SUMS 一起分发。`collect-sources.py CACHE SOURCE_INVENTORY OUTPUT` 按真实 dpkg 的 source-package／source-version 两列 TSV 获取对应发行版源码并逐文件核对元数据哈希；缺失明确留在清单，不用其他版本顶替。内核与静态链接运行库、容器系统包以及附带 MPL 组件分别保留其准确来源。

OpenClaw 通过标准 Compose 保持容器和配置，使用官方 Docker 入口及 VM 启动选项，不在每次普通重启时删除容器。模型和 API Key 由使用者配置。

源码目录还包含 AS 冻结源码、Rust 依赖源、Dashboard 生产依赖源、固定 OpenClaw／Node 源码和已核原生 MPL 库源码。AS 驱动按各文件 GPL-2.0／BSD，应用按 MIT；不能把整个镜像重新标为 MIT。保留各组件原声明，不附带商业模型权重、Docker Desktop、Extension Pack、私人会话或真实凭证。

最终构建证据独立记录实际模块加载、AS／OpenClaw／图形会话、首次启动和停机。格式校验、QEMU 启动与 VirtualBox 实际导入分别记结果，未执行的不能报通过。
