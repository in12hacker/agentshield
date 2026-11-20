# 参会虚拟机

本交付把现行 AS 与官方 Docker OpenClaw 封装为独立 x86_64 来宾；不修改宿主机内核，不迁入旧版 AS 技术。镜像面向 Intel／AMD 电脑，ARM 主机需使用 QEMU 跨架构模拟，不能把 x86 OVA 当作 ARM 原生镜像。

## 当前分发状态

2026-11-20只读核对实存文件及SHA。本节是镜像状态的唯一入口；后文描述封装行为与使用目标，不能代替具体产物的验收。

- **Work中的已发布文件仍为旧R7**：`$HOME/Work/AgentShield-Conference-Ready-20261116/x86_64/AgentShield-Conference-x86_64.ova`，2309079040字节，SHA256 `bb21a2575c305ee272b19992dec06da4fe1af1a95069962daefb50ef6357dd8e`。源码2375d493d，服务5217c500…、Native ed35ae…、UI index1e76a4…；未包含后续输入与完整Provider保存修正，不能称为当前可用的幻影Key接入版本。
- **仅输入修正的审核包未发布**：`/tmp/as-conference-ready-20261116/final-ova-r7-phantom-input/AgentShield-Conference-x86_64.ova`，2309928960字节，SHA256 `6a4cc30d0413c98068e992a7185b0b53776a57b8f4b86eb6a106ed0748d88bb1`。它保留R7服务／Native／UI，只叠加5c07978a3的输入包装，未包含7508cda5d的完整Provider保存逻辑。正常封存、QEMU退出和导出通过不代表用户VirtualBox／GLM接入通过。
- **已提交的接入实现尚未进入上述两份OVA**：7508cda5d通过同一官方CLI保存完整Provider、默认模型和SSE设置，首启复用同一公开预置，不再强制restart。独立单文件`/tmp/as-phantom-key-integration-fix-20261117/Configure-OpenClaw-Phantom-Key.py`与现源码助手逐字节相同，SHA256 `e815082e044cb40f016e9b6df7dffcc644f0ea64d76cc942f1978025e016602b`。本地语法、真实非秘密配置对照和模拟调用检查通过；没有取得该实现的用户VirtualBox正常聊天通过结果。

R7输入空白、粘贴与保存逻辑的已证失败、修正及测试范围保留在[封装证据](../evidence/conference-vm-build.md#幻影key输入与保存链的历史边界)。旧VirtualBox导入实验另见[对应产物记录](../evidence/conference-vm-virtualbox.md)，不替代用户新实例。当前仍有用户模型响应异常原帧未取得和精确固定拒绝反馈未完成的风险；不因源码合入或生成新包自动升级为通过。

## 交付与边界

采用 Debian 13 官方 generic backports 云镜像及发行版内核，QEMU 构建，标准 OVF＋streamOptimized VMDK 打成 OVA。VirtualBox 导入采用 BIOS、SATA、普通以太网和 NAT；不依赖 Extension Pack、Docker Desktop 或厂商 Guest Additions。虚拟磁盘按需分配 40 GiB，建议 4 CPU／8 GiB 内存。

来宾包含图形桌面、浏览器、AS Dashboard、正式 as-service／驱动及 Docker Engine。OpenClaw 使用标准 Compose 管理持久容器，配置跨普通重启保留。OpenClaw 使用官方发布镜像，实际摘要写入产物清单。AS 的固定布局、规则、授权和数据结构直接引用[配置契约](../backend/configuration/technical.md)、[规则契约](../backend/rules/technical.md)及[中间层契约](../middleware/technical.md)，封装脚本不另写管理或规则实现。预制规则使用现行可管理的 `config/rules.fast-allow.yaml`。

容器首次预配从唯一 `config/rules.conference.yaml` 片段读取规则，只把 Backend 询问组绑定实际 Docker 实例；Driver DNS 移交组保留模板声明的作用域及来源／目标条件。启用新组后，经原 rules.get/update 合并、保存及采用；同 ID 现有组保留，不覆盖用户禁用或修改，系统 Driver 基线及自保保持原模块所有权。模板在源码中禁用，Backend 的 AllProtected 仅为待替换占位，Network 的 `@docker_gateway_ipv4@/32` 也必须先替换为实际网关 CIDR。三个资源询问组、DNS 询问组及其 Driver 移交组均为普通规则数据，不在 hook 或错误分支增豁免。Gateway端口配置为group_observation.gateway_ws_port=18789，其他入口显式None；该来源仍限[宿主双回环TCP](../middleware/agent-graph/technical.md#持续采集的来源边界)，端口配置不证明Docker私有netns中的Agent身份／Session来源已经接通。

试用者拥有正常本机管理员及 AS `manage_service` 权限，可以自行配置规则、模型、凭证和出口；没有只读、激活、额外 Docker 能力裁剪或内核升级锁。首次启动生成该副本专用的系统密码、AS 管理令牌和 OpenClaw Gateway 令牌，桌面自动登录，系统密码在本机欢迎页提供。管理入口仅绑定来宾回环地址，桌面提供本机使用说明与入口。镜像不包含开发机账号、真实模型 Key、幻影 Key、聊天、会话、CA 私钥或 Root broker 配置。

## 首次启动的非秘密预配

使用者只需在 AS 导入自己的模型 Key，选择预置模型连接取得幻影 Key，并在预装 OpenClaw 正常模型设置中粘贴；不要求填写 Flow、CA、Scope 或 ID JSON。预置可修改的模型为 `glm-5.3-flash`，stock provider为 `as-vm-glm`，原 `openai-responses`／SSE入口 `https://open.bigmodel.cn/api/v1`；AS route精确为origin `https://open.bigmodel.cn`、POST `/api/v1/responses`、profile `open_ai_responses`。没有 `apiKey` 占位符或内置模型密钥，凭证bindings初始为空。

原first-boot先生成每副本本地身份及OpenClaw非秘密provider配置；一次性bootstrap在Docker及first-boot之后、正式AS／OpenClaw／桌面之前运行。它临时启动同一个正式as-service子进程，通过原UID0 Unix鉴权管理接口 `private.ensure_ca`／`public_ca`取得真实公有CA，并以公开PEM文件只读挂载给容器的标准 `NODE_EXTRA_CA_CERTS`。CA私钥只由AS凭证模块生成和保存，不从其他实例复制，不关闭TLS校验或修改Agent。

Compose先create，再从该实际创建结果经Docker inspect取得完整容器ID及其附着网络；网络Gateway由这些网络的Docker network inspect原IPAM配置取得，不硬写地址、CID或按PID猜测。由它构造唯一 `ContainerInstance { runtime: "docker", namespace: null, id }`，持久Flow.scope使用 `workloads`／原 `Workload::Container`，不从名称、PID或cgroup数字猜身份。bootstrap沿原configuration.get/update与expected_version写入route、实际CA引用和精确scope，保持空bindings；Flow需要Main重新初始化时，原RequestStop只确认接纳，随后真实Child.wait收初始AS退出并收原Native清理，再让原systemd正式AS启动采用配置。无新增限时、总线、常驻supervisor或管理配置副本。

预配成功标记只记录这一次完成，不保存第二份设置；普通重启不重写配置、不轮换CA或本地令牌，不覆盖用户禁用的Flow。已有用户Flow／bindings时不套预置；目录或空业务库的存在不当作用户配置意图。原普通HIPS与管理权限不由预配状态加锁，预配错误保留真实服务日志，不假称已经采集聊天。用户后来明确重建容器会产生新实例ID，不能把原scope自动猜成新实例。母盘保留空配置与原first-boot机制，实例启动才生成CA、容器身份和其配置；构建验证副本的这些状态不混入分发盘。

桌面“配置 OpenClaw 幻影 Key”保留现有明文输入界面。接入按真实x86 QA成功实例的设置执行：通过原官方CLI一次写入预置as-vm-glm完整Provider（baseUrl、openai-responses、模型目录、auth=api-key、Connection=close、输入幻影Key），同时明确默认模型和SSE传输。预置只由助手的client_preset定义，首启直接复用，避免两套默认值漂移。CA继续由既有Compose的NODE_EXTRA_CA_CERTS挂载/opt/as-conference/as-ca.pem提供，与成功x86 QA一致；助手不新增Provider局部CA路径，不读取AS真实模型秘密。

正常管理员通过stdin传值，助手以原容器runtime UID/GID调用stock config set --batch-file与--strict-json，0600临时batch由原finally清理，Key不进入argv。写入沿OpenClaw唯一配置owner及热加载策略，不在保存后强制重启；若容器原本停止，仅沿已有Compose start启动同一个对象。原CLI的stdout/stderr及重载提示实际展示；返回0只证明配置写入，不宣称模型认证或聊天已通过。失败原件继续保留在独立私有目录。公开预置与信任路径的实际依据、历史失败及有限验证范围见[封装证据](../evidence/conference-vm-build.md#幻影key输入与保存链的历史边界)。

用户已明确允许幻影Key明文展示。输入使用终端正常回显，可确认键入内容；xterm入口显式把Ctrl+Shift+V和Shift+Insert绑定到CLIPBOARD粘贴，不依赖发行版默认terminal别名或隐藏输入。`-xrm`运行时资源参数以反斜杠字符和字母n组成的字面转义序列分隔绑定；直接传实际换行会让X资源解析器只保留`#override`而丢失快捷键。cloud-init明确安装xterm，桌面直接启动同一助手；普通启动打开输入窗口，内部--prompt仅负责输入和显示，原--apply继续独占配置保存。取消输入不保存，保存失败保留原错误和退出码，结果窗口等待用户关闭。不新增保存前置网络探测、模型请求或人为截止。移除旧whiptail屏幕捕获路径，不增加图形库或第二套配置写入。虚拟机内部复制粘贴与宿主共享剪贴板分别核验，不把QEMU结果宣称为用户VirtualBox已修复。

AS 驱动按来宾实际内核，通过原 `drivers/linux/build.sh all` 构建。来宾启用 BPF LSM，模块加载失败即 AS 启动失败，不设置无驱动演示降级。关闭来宾前沿正式服务退出链等待收尾。镜像携带已验证内核及匹配模块，升级内核需要同步重建模块；自动升级重建归整体安装升级工作，不在本封装中伪造 DKMS 支持。

## 分发材料

发布目录同时提供 OVA、SHA256SUMS、组件／包／源码清单、许可证与对应源码材料。保留 AS 应用／OpenClaw MIT 声明、AS 驱动逐文件 GPL-2.0／BSD 声明、来宾 `/usr/share/doc/*/copyright` 和容器现存声明。Debian 只使用 main；不把闭源扩展、商用模型权重或个人凭证装入镜像。GPL 等组件按具体许可证携带对应源码，不用普通下载链接替代必须履行的源码义务；发布时应分发完整目录。

依据：[Debian 许可分类](https://www.debian.org/legal/licenses/)、[Debian 云镜像](https://cloud.debian.org/images/cloud/)、[OpenClaw MIT](https://github.com/openclaw/openclaw/blob/main/LICENSE)、[官方 Docker 部署](https://docs.openclaw.ai/install/docker)、[VirtualBox 许可说明](https://github.com/VirtualBox/virtualbox-website/blob/main/wiki/Licensing_FAQ.md)。不宣称开源软件不存在所有法律风险；实际再分发材料以构建清单和对应许可证为准。

## 验证范围

交付目标是从未运行的分发副本导入／开机后，AS、原版OpenClaw和两端图形入口实际可用；预制规则、实际运行UID的缓存与日志目录权限、公有CA、容器绑定及受控出口由首次启动沿原接口准备。使用者只配置自己的模型秘密／幻影Key，不承担修权限、补内核依赖、启动调试命令或填写内部JSON。是否达到该目标按[当前产物状态](#当前分发状态)及其原件认定；运行中的手工修正须回写既有封装入口和干净母盘，不能把旧副本已经修好的状态当作分发镜像已修复。

构建记录分别报告底座校验、x86 编译、实际内核加载、AS／Docker／OpenClaw 启动、首次启动隔离、关机和 OVA 格式检查。QEMU 启动不能替代 VirtualBox 实际导入结论；未执行的项目明确记录。真实 OpenClaw 保护效果仍交原专用聊天按现行链路验收，不扩充产品功能或推定归因。

脚本入口与操作见 [scripts/conference-vm](../../scripts/conference-vm/README.md)。

桌面配置助手经官方CLI写入模型凭证：只向目标容器传幻影值；batch使用该容器实际运行UID／GID拥有的0600普通临时文件，CLI实际结束后finally清理。不把socket stdin路径当作普通文件，不打印密钥；失败保留原stdout／stderr并显示真实错误。出厂应用询问仅针对下述三个指定资源及 Google DNS 查询；其余操作没有这些应用询问规则，系统Driver基线及自保继续，不将应用询问策略扩展至宿主机。

Docker桥接／Flow使用当前发行版已有的veth、nft_redir、xt_tcpudp、xt_nat模块；已在原7.2运行内核逐项核文件与实际加载结果，随镜像写标准modules-load.d配置。不能仅在验证写层手工加载后就宣称出厂已具备；nft原生入口与iptables-nft兼容扩展分别核实际错误，不换host网络或flush现有规则。

幻影配置助手沿现有Compose定位同一个完整容器ID。该容器已停止时先通过原Compose执行start，核实际结果及同ID的Running状态，再执行原官方配置命令；不重建容器身份、不另建启动顺序门槛或预检截止。启动和配置共用原命令结果处理，失败均保留原stdout／stderr；不在保存后强制重启unit，也不把容器未运行归为模型或Key错误。

## 指定演示目标

`config/rules.conference.yaml`保留三个资源Ask组及一个DNS查询Ask组：执行`/usr/bin/printf`；对`/home/node/.openclaw/workspace/as-demo/ask-file.txt`的读／写Open；向实际Docker桥网关的演示网站发起TCP Connect。DNS组只匹配实际容器中`/usr/bin/curl`发送到Docker解析器的`www.google.com` UDP查询；系统Driver基线、自保及用户正常规则管理保留。演示规则在首启得到实际容器实例及Gateway后，才通过原规则owner校验、原子保存及发布。

文件目标通过现有Compose bind映射创建在真实宿主目录内，父目录和文件采用官方镜像实际runtime UID／GID，初始正文为`AS_FILE_DEMO_OK`。准备脚本只安装公有种子；first-boot在该实例的真实bind目录中创建缺失文件，不覆盖已存在用户文件。

本地网站是来宾内`/opt/as-conference/demo-http`公有静态目录，由原systemd启动现有`python3 -m http.server`，绑定来宾`0.0.0.0`，不是新增宿主公网服务。端口只从正式网络演示规则读取，当前为18088；返回`AS_NETWORK_DEMO_OK`。实际Gateway URL由首启写入本机`AS-DEMO.html`，欢迎页链接并展示可复制到stock OpenClaw聊天的exec任务；不要求用户写JSON、配置规则或自行猜网关，不自动请求模型。本地IP网站只演示TCP Connect，Google用例单独演示DNS查询快照，不能把DNS服务器IP称为Google网站连接IP。DNS Driver组2只按curl执行文件、127.0.0.11/32、UDP53移交原后端，先于基线DNS Allow组3；Backend组101再按完整容器和原查询名Ask，其余查询不进入询问。所有策略均为同一规则owner管理的数据。

各演示规则预置可编辑的固定拒绝说明，原规则模块负责保存和发布。人工、规则超时或直接拒绝均按原实际Denied结果保留说明；后续工具完成上行追加仍由既有执行反馈链按真实对应关系处理，不能把空关联或候选当作已经向模型发送。

首次启动从同一正式模板生成桌面的英文`DEMO-TEST-CASES.en.md`及可直接打开的HTML页，欢迎页提供入口；包含进程、文件、网站Connect与Google DNS各允许／拒绝用例。实际Docker网关和域名由模板／真实网络填入，不留占位、不给使用者JSON配置任务，也不自动发模型请求。
