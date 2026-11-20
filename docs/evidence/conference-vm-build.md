# x86_64 参会镜像分项封装证据

本页按冻结输入保留各轮原件，不维护当前发布指针；当前镜像、审核包与未装包源码的唯一状态见[交付说明](../delivery/conference-vm.md#当前分发状态)。2026-11-14初版只封装当时的AS与Docker OpenClaw，未改AS Rust／C执法源码。该轮源码基线为`52d2eb571499570fa9def66f3676afcf39aaad43`，权限约定另提交`0902863e7`；用户`docs/roadmap.md`改动未纳入。下文初版结果不外推后续R7、用户VirtualBox或最新助手。

## 环境与组成

ARM64 开发机上使用 QEMU x86_64 TCG 构建及运行；来宾为官方 Debian 13 generic backports `20261111-2659`，内核 `7.2.12+deb13-amd64`。官方底座 SHA-512 已核验，记录在 `/tmp/as-conference-vm-20261114/downloads/SHA512SUMS`。未修改宿主内核或 Root broker。

AS 使用当前静态 x86_64 `as-service`、正式 Dashboard、原 `drivers/linux/build.sh` 的模块／BPF／静态 helper。OpenClaw 固定官方 2026.11.3，manifest `sha256:4f97f60d52e67f4acad1934c10d2abd52ed1a586dd9a4acd0bd4e23a7d1d98cc`，image ID `sha256:1301d38ecf8de06316e58572c7ffb9573a3ce7a70f57758db0ffb6c9c0a9a7bc`。

## 实际结果

- 实际 x86 模块加载／设备创建／卸载已执行。正式 AS 加载全部 BPF 后输出 `AS_SCRIPTED_SERVICE_READY`。
- 正式 WS 首次鉴权实际返回 `manage_service` 与 `chat_content=true`；没有只读、激活、时限或额外 Docker 能力裁剪。
- 图形桌面、Firefox 欢迎页实际启动并截图；AS HTTP、OpenClaw HTTP 均取得 200。新的网卡 MAC／PCI 地址下 DHCP 已取得地址。
- Docker 镜像真实运行用户为 uid/gid 1000；使用该默认用户与相应目录属主后 OpenClaw 正常启动。Compose 管理持久容器，普通重启保留配置。
- VirtualBox 官方 7.2.18 工具在独立工具目录成功执行 OVA 描述符导入预检，识别 Debian_64、4 CPU、8192 MiB、NAT、AHCI／SATA port 0。该预检针对同一导出器的初版硬件描述，不冒充最终版本在实体 VirtualBox 中已启动。
- QEMU 使用 SATA、变化后的 MAC 和导出 VMDK 的验证副本实际启动。最终清理后正常关机记录包含 `reboot: Power down`。

没有运行模型聊天、Agent 工具操作或新一轮完整 OpenClaw HIPS 验收；不以此宣称精确归因、幻影 Key 或全模型能力已新增通过。模型 Key 与受控出口由试用者配置，现有 R91 结论保持原范围。

## 已修正的装包问题

1. cloud-init 挂载参数中的逗号被 YAML flow sequence 分成两个 argv；改为明确的一条 shell 参数串，挂载成功并看到载荷后才写构建就绪记录。
2. 原版 Debian libelf 0.192 静态归档缺符号，与 [Debian #1102039](https://bugs.debian.org/1102039) 一致；采用官方 backports 0.196，不补写符号或动态库回退。
3. x86 模块 BTF 类型去重失败导致内核报告同名 file 类型不一致。按[内核维护者的分析](https://lkml.iu.edu/hypermail/linux/kernel/2606.1/02607.html)，pahole 的去重器使用 libbpf 1.6.3。仅更换该模块 BTF 后原 BPF 与 loader 即真实启动；原执行类型检查未放宽。BPF 头与静态 loader 仍使用来宾发行版库，避免混入与当前内核不同的无关 kfunc 声明。
4. 安装器把日志目录创建为 0755，违背现有日志模块私有目录契约；收敛为 data／log 0700，调用原模块而非放宽校验。
5. OpenClaw 目录与实际容器用户不一致，造成缓存目录拒绝写入；装包阶段真实查询镜像 UID／GID，使用默认镜像用户。
6. 安装脚本在共享目录运行时被更新，造成 Bash 读取错位；执行前冻结本地副本。同一装包过程现在直接复用已构建驱动，桌面配置修改不重编驱动。
7. 临时构建账号退出请求不是退出完成；按实际 systemd 用户 slice 同步停止后删除。内核短在途引用归零前立即 modprobe -r 会失败，现沿既有停止期限使用 kmod 标准等待，不强制卸载或吞错。

完整原件位于 `/tmp/as-conference-vm-20261114/evidence`。运行数据库和私有状态在删除前通过强公钥 SSH 导出并核 SHA，位于同目录 `private-evidence`，不随公开镜像发布。对应源码／原声明共随分发包交付，不把本地材料下载完成作为试用启动条件。

## 幻影Key输入与保存链的历史边界

以下失败和修正分别保留，不能把最后一次局部检查覆盖前面的失败，也不能用输入可见替代官方CLI保存或模型回复。

1. **R7原whiptail入口**：助手从stdout取值，原交互输入未进入该捕获通道，出现空白／无有效保存输入。它是已发布脚本的问题，不能归为使用者没有粘贴Key；旧Work R7中的助手仍为该代。
2. **lxterm默认行为**：旧桌面终端别名没有提供预期粘贴方式。改用明确安装的xterm和显式CLIPBOARD绑定；来宾内部粘贴与宿主共享剪贴板分别认定。
3. **独立getpass入口**：隐藏输入不能满足用户已明确要求的可见明文确认。该入口仅作为当时的修复尝试，不保留为现行并行路径；后续使用正常回显的input。
4. **d7c39b9fe的Xrm参数**：明文输入及粘贴绑定已写进脚本，但传给X资源解析器的是实际换行，最终只保留`#override`。5c07978a3改成字面`\\n`分隔，并用实际libX11资源解析核保留两条绑定。保留原PTY结果、模拟保存失败17、取消不调用apply及来宾内复制测试；这不等于最终默认桌面启动链或用户VirtualBox剪贴板已通过。
5. **7508cda5d完整保存**：原助手仅填apiKey，不负责将实际成功连接的其余字段一并收敛。本次以真实x86 QA成功配置为依据，官方CLI batch同时保存`models.providers.as-vm-glm`、`agents.defaults.model.primary`及该模型`params.transport=sse`；first-boot复用同一个client_preset，删除重复默认值。保存后不再强制restart，显示stock CLI的原重载提示。语法、非秘密配置逐值和模拟调用检查通过；新单文件可独立运行，未取得它在用户VirtualBox的完整正常聊天结果。

输入原件在`/tmp/as-phantom-key-pty-fix-20261117/`：`pty-test-original.txt`、`visible-prompt-pty-final-original.txt`、`visible-input-review-current.json`及原截图。`visible-input-review-current.json`同时保留首轮审核未发现Xrm错误、后续实际解析推翻该判断的事实；该首轮结论不继续当作通过依据。精确默认桌面运行曾被锁屏中断，保留为测试器未完成。自有无KeyR7在同一次自然重启中先HTTP reset104后恢复HTTP200，不证明用户的重启／模型异常已经解决。

实际成功配置在`/tmp/as-openclaw-final-r3-20261116/phantom-success-settings-20261117/`的非秘密原件及`comparison.json`。固定官方Provider为`as-vm-glm`、`https://open.bigmodel.cn/api/v1`、`openai-responses`、`auth=api-key`、`Connection=close`和`glm-5.3-flash`，默认SSE；CA沿Compose的`NODE_EXTRA_CA_CERTS`公有文件。早期R95 Python在另一环境曾使用Provider局部CA，不能直接迁入此x86信任路径。真实Key不复制进分发盘或文档，CID、CA和凭证绑定均由每个副本生成。

原QA同Key／同CID的一次普通GLM聊天确已完成：run`61bd09af-752a-49f3-b656-728f87787d67`，CredentialUse187／188／189分别为prepared／applied_to_request／submitted，原请求16160和response.completed16292均在库，工具调用0。之后容器ExitCode139、OOMfalse另存，原因未证；这一次成功不扩为稳定交付或7508新实现／用户新实例通过。原件为`r7-restart-chat-20261117/result.json`及同目录容器退出原件；本次文档整理没有重跑模型或分析core。

7508本地检查与独立脚本在`/tmp/as-phantom-key-integration-fix-20261117/`，脚本SHA256 `e815082e044cb40f016e9b6df7dffcc644f0ea64d76cc942f1978025e016602b`；`local-check.json`明确`live_user_vbox_or_model_not_executed=true`。用户后续malformed SSE的原始异常帧未取得，不从报错反推Key、协议或AS改写为根因。

## 下一次封装的准确替换范围

本节只列已提交输入／保存修正的装包差量，不授权运行构建、部署、模型或恢复暂停的Native修正。

- `scripts/conference-vm/openclaw-phantom.py` → `/opt/as-conference/openclaw-phantom.py`：7508助手；另行发出的独立脚本与它同字节。
- `scripts/conference-vm/first-boot.py` → `/opt/as-conference/first-boot.py`：同一公开client_preset的首启入口；只替换干净母盘内脚本，不覆盖已使用实例的配置或首启身份。
- `scripts/conference-vm/prepare-guest.sh`所生成的`/home/demo/Desktop/openclaw-phantom.desktop`：直接执行上述助手、Terminal=false，由助手自己开xterm；不继续经过lxterm或whiptail。
- `scripts/conference-vm/cloud-init.yaml`的xterm发行版包依赖：审核5c包已含xterm398-1，旧Work包不能从源码有依赖推定已装。按实际dpkg结果认定，不为文字更新重编服务／Native／UI。
- 对应源码归档、源清单、README、BuildSummary、PackagingRecord、Acceptance、FirstBootReport和SHA256SUMS与新OVA同批更新；输入／保存修正不替换AS服务、Native或UI二进制。新的主线源码归档与保留二进制各注明真实源码范围，不把后来提交冒充二进制构建来源。

必要核对沿同一条正常使用链：冻结源码和真实安装字节相等；从干净副本的默认桌面入口确认可见输入／实际粘贴，官方CLI保存三条配置并显示原结果，再由已授权的正常OpenClaw聊天取得实际凭证／响应结果。取消／真实失败仍原样记录；不强制restart、不复制QA身份／秘密、不补猜测SSE type，不新增测试矩阵或固定截止。VirtualBox实际结果与QEMU、单文件和模拟调用分开记。仅导出新OVA和哈希不能把尚未执行的使用链记为通过。
