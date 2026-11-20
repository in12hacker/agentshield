# 配置字段、写入接口与驱动导出

公共引用：[MainContext公共上下文](../../contracts/main-context.md)、[模块唯一职责](../../architecture.md)。公共定义以链接目标为准，本页不另定义一份。

产品目的见[设计](design.md)，所有者见[架构](architecture.md)。本文件定义当前配置结构，不借旧字段或默认值解释现行数据；未实现的平台能力明确单列。

## 1. 配置模块私有定位与ConfigPaths

固定部署布局：

```text
<self_protection_root>/
  bin/as-service[.exe]        后端程序
  config/as.yaml             AS持久配置
  config/rules.yaml          普通规则文件
  driver/                    私有驱动分发资源
  run/                       本机通信运行资源
  data/                      数据模块布局保留
  log/security.sqlite        底层安全日志数据库
```

配置模块在自身initialize内部调用私有平台查询，取得操作系统给出的真实后端可执行文件位置，固定从bin目录上移一层得到自保根，并一次派生全部ConfigPaths。每个配置模块实例只定位和派生一次；主模块、规则模块、消息封装和驱动适配都不得再次查询程序位置或拼接自保根。读取、写入、reload和单独重新初始化驱动均复用此结果。运行期不使用cwd、argv[0]或前端输入改根；符号链接／重解析点不能把受控写入转到根外。

InstallLayout仅是配置模块的私有初始化数据，不作为公共类型或initialize入参；平台查询是该模块的私有实现，不另建布局服务。公开ConfigPaths是共享契约中的只读数据，生成入口仅由配置模块负责；主模块经shared_paths()借用同一Arc并放入MainContext，其他模块只按接口参数使用，不自行构造、定位或缓存另一份路径。主模块通过snapshot()取得同一不可变快照。其字段不序列化到可写配置：

- `backend_executable: PathBuf`：实际后端文件。
- `self_protection_root: PathBuf`：固定自保根；不能编辑。
- `configuration_file: PathBuf`：根＋`config/as.yaml`，配置模块唯一写入源。
- `rules_file: PathBuf`：根＋`config/rules.yaml`，规则模块唯一写入源。
- `driver_assets_directory: PathBuf`：根＋`driver`；具体已认可产物由驱动部署接口选择，未明确细节见存档。
- `middleware_endpoint: PathBuf`：Linux为根＋`run/middleware.sock`，Windows为固定`\\.\pipe\AgentShield.Middleware`命名管道；非可写配置。
- `middleware_database_file: PathBuf`：根＋`data/middleware/business.sqlite`；配置模块唯一派生，非可写配置。中间层业务日志通过该固定路径执行 SQLite 事务，不允许远端或目标指定文件。
- `security_log_file: PathBuf`：根＋`log/security.sqlite`。配置只派生路径，底层安全日志模块负责打开和写入。
- `data_directory`：固定布局供相应所有者使用。

PathBuf按构建目标保留原生路径，线上表示由[共享context.rs](../../../crates/contracts/as-types/src/context.rs)的唯一序列化器定义；不把Linux原字节与Windows路径表示无标记互转。路径可以展示，但不是写入授权。没有可编辑rules_directory、rules_file、configuration_file或自保根字段；不保留旧路径字段别名。Linux实际ko安装位置、Windows驱动服务路径由安装接口提供，不把系统驱动目录误当私有driver目录。

例如Linux实际后端为`/opt/agentshield/bin/as-service`，则根固定为`/opt/agentshield`，规则固定为`/opt/agentshield/config/rules.yaml`。Windows实际后端为`C:\Program Files\AgentShield\bin\as-service.exe`时，使用相同相对布局。安装器可在安装时决定安装位置，运行配置及中间层没有改根接口。

## 2. 可写ConfigDocument

顶层结构如下，唯一Rust声明见[ConfigDocument](../../../crates/contracts/as-types/src/configuration.rs)：

- `format_version: u16`：现行值2，必填；不接受旧格式、未知字段或宽泛extras。
- `queues: QueueSettings`：底层实际接纳容量；各值为正整数。
- `log_storage.total_bytes: NonZeroU64`：安全审计库与业务库共享的字节预算，出厂为 1073741824（1GiB），不是每库各一份。变化需要完整主模块重新初始化。Main 只组合同一预算与两个原库端口；各库所有者仅淘汰自己的最旧完整保留单元。跨库先选择实际占用较大者，不宣称两个库存在事件时间总序。计量包括数据库、WAL、SHM 与回滚日志；原 SQLite 事务运行期间的暂态占用不是已收敛的保留量。
- `process_tree: ProcessTreeSettings`：存储计量预算、退役保留、核对周期和页大小，字段与含义只在进程树契约维护；变化需要完整主模块重新初始化。
- `driver: DriverPersistentConfig`：初始化和持久重载需要下发的内容。
- `middleware: MiddlewareAccess`：中间层连接的服务端授权策略。
- `flow: Option<FlowSettings>`、`credentials: CredentialSettings`：受控出口及非秘密凭据绑定，字段只在[Flow运行时配置](#flow运行时配置)定义。
- `file_close_audit: Vec<PathBuf>`：写关闭观察目录，见[写关闭审计范围](#写关闭审计范围)。
- `group_observation: Option<GroupObservation>`：已配置的公开协议观察范围，见[Group运行时来源范围](#group-回环观察范围)。

### 日志共享保留预算

`as_types::log_storage::LogStorageBudget::new(LogStorageSettings)`由Main组合一次。两个原库通过`bind(LogDatabase, &Path, Weak<dyn LogRetention>)`登记自身配置固定路径和回收能力；弱引用不延长owner寿命，已关闭库的实际文件仍计入总量。端口只有`retire_oldest() -> io::Result<bool>`，只处理本库。写入前在原blocking SQL工作内先取共同协调锁，再取自身Connection锁；提交后释放Connection锁才可请求另一owner回收，禁止持自身Connection等待另一连接。

`usage()`按固定两个数据库及SQLite sidecar返回实际字节、总预算与维护延期计数；`deferred_maintenance()`是供宿主诊断使用的不阻塞原子计数。共同协调没有内容存储、后台任务或新输入队列。单次SQLite提交事实与后续维护结果分开，checkpoint暂忙和当前活引用阻止回收不能伪造write failure／SourceUnavailable。具体本库删除边界见[业务日志](../../middleware/business-log/technical.md#共享容量与滚动保留)与[安全日志](../security-log/technical.md#只读分页)。

### QueueSettings

- `callbacks: NonZeroUsize`：驱动接收／快速处理队列容量。
- `review: NonZeroUsize`：等待串行主循环的慢处理容量；不是并发弹窗数。
- `messages: NonZeroUsize`：普通内部消息及管理入口容量。
- `driver_urgent: NonZeroUsize`：原操作答复、取消、紧急运行时控制和退役的保留发送容量；大规则片段不能占用。

容量不是线程数；不因进程、文件、网络增加业务线程。配置唯一校验入口拒绝超过Tokio Semaphore::MAX_PERMITS的值，callbacks还为适配的代预算保留一个位置。安装配置给出完整值，实际能力不足报告资源原因，不用测试次数或机器名决定能否运行。保留现行已有64容量作为起始配置，不宣称未经测量的吞吐。

### DriverPersistentConfig

- `startup_protection.process: bool`：完整初始化后普通进程防护的默认值。
- `startup_protection.file: bool`：完整初始化后普通文件防护默认值，不包含取消文件自保。
- `startup_protection.network: bool`：完整初始化后普通网络防护默认值。
- `limits.max_pending: NonZeroU32`：驱动未决原操作总接纳容量；须在真实能力范围。
- `limits.max_hold_ms: NonZeroU32`：原来源自产生起的最大保管窗口，包含排队；实际询问仍取原窗口和规则timeout较早者，不重置预算。
- `self_protection.log: bool`：实际自保拒绝是否产生规则日志请求；不改变防护和访问权限。已鉴权 AS 自身在自保范围内的受控修改由驱动静默快速放行，不因该值为 true 逐次构造通知或日志。

自保根、递归范围、修改阻断集合及可信写主体不作为用户可改的路径／白名单。配置模块按下节固定语义导出它们。这里不保存普通匹配规则、询问动作、每条普通规则log或临时开关。

### MiddlewareAccess

- `peer: Principal`：允许连接中间层的真实本机身份；Linux为uid，Windows为明确SID。是服务端策略，不来自消息自报。
- `access: Access`：read_only只查询；manage_rules增加规则修改；manage_service再增加配置写入、临时控制和服务管理。上一级包含下一级权限。
- `chat_content: bool`：是否授权该真实连接读取已采集的聊天内容，独立于 access。原 Ask／答复权限仍按 ManageRules 检查。

- `websocket: Option<WebSocketAccess>`：显式配置中间层正式回环入口，None 时不绑定。listen 是非零端口的回环 SocketAddr，origin 是允许的单个浏览器 Origin，None 时只接受无 Origin 的原生连接。token_sha256 是部署者生成的 64 个十六进制字符令牌的 UTF-8 字节的 SHA-256 摘要，按 32 个 u8 保存。客户端经既有受信部署途径取得原令牌，配置只存摘要。[中间层授权与管理](../../middleware/technical.md#首次鉴权与真实底层授权)

MiddlewareAccess 更新触发原 ReconnectMiddleware 撤销旧授权。WebSocket 监听地址变化另要求 ReinitializeMain。Main 按当前配置验证后续握手，新地址在宿主重新初始化后绑定。

连接首次认证身份，命令仍检查相应授权。授权策略变化时旧连接不能继续保有被撤销权限，应在完成实际配置应用后使其重新鉴权。端点位置固定，不配置默认未鉴权TCP监听、共享明文口令或关闭认证开关。

### 必需值及边界

初次安装提供完整现行配置；缺失必需字段或无效值返回明确错误，不用旧字段、环境猜测或另一份默认文件补齐。实际窗口、消息尺寸和资源上限由DriverInfo核实，不能从配置true推定功能存在。

驱动设备名、协议版本、具体BPF／模块产物、数据帧长度、片段长度和实际支持动作来自安装／协议／能力接口，不作为普通用户调参字段。

## 3. 固定持久文件自保规则

配置模块读取持久策略参数，使用自身已缓存的ConfigPaths导出DriverConfigImage不可变buffer。固定自保语义如下；当前没有独立的系统规则列表DTO：

- 文件Driver系统组1，只占同一个文件池；文件Driver组2为同源维护放行，普通文件Driver组从3开始。
- target为只读自保根的真实对象绑定，递归范围不使用字符串前缀通配。
- 受信底层服务实例在范围内执行所属模块的受控修改；中间层不获得写票据。
- 其他主体的创建、后续写、截断、删除、改名、链接、元数据及新写映射按实际入口拒绝；OS原权限不被Allow提升。
- 日志选择来自self_protection.log，命中来源为配置投影版本。

现有 `ConfigSnapshot` 提供保存设置，`DriverStatus` 提供实际生效状态，实际 `RuleHit` 标识系统组和来源。普通规则 Get 只读规则文件。固定自保组单独枚举没有独立业务消费者，当前沿用上述来源。

Root目录本身／祖先重命名、已打开FD、映射写、符号链接／硬链接及Windows重解析点必须落到真实对象与操作能力，不靠`/opt/as`字符串前缀保护`/opt/as-other`。仅有file_open不能声明目录修改保护完整。不支持所需能力时返回Unsupported并指出缺口，不删减规则后显示成功。

驱动须精确识别自身控制端点及必要的底层启动读取，避免故障拒绝锁死合法初始化；这些权限依赖原生对象和认证身份，不按程序名宽泛放行。自保字段不承诺抵抗任意已控制内核的攻击者。

## 4. 新增结构及模块接口

`ConfigVersion([u8;32])`标识规范化现行配置内容；`DriverConfigVersion([u8;32])`标识驱动投影。两者不假定相等，队列／中间层权限变化不伪装成内核设置已经变化。

`ConfigSnapshot { document: Arc<ConfigDocument>, version: ConfigVersion, paths: Arc<ConfigPaths> }`是唯一只读缓存。更新／重载可替换document和version，但保留同一个只读paths引用；不会重新计算目录。路径只读，不由配置正文覆盖。

本节是配置模块公开接口与导出结构的唯一出处；其他模块只引用，不重列另一套初始化入参或返回类型。

配置模块公开职责：

- `ConfigModule::initialize() -> Result<ConfigModule, ConfigError>`：内部一次定位、派生路径、绑定固定源并读取／校验／缓存；不接收外部路径或InstallLayout。
- `shared_paths() -> Arc<ConfigPaths>`：导出同一只读路径引用，供主模块组合MainContext；不重新查询或复制路径。
- `snapshot() -> Arc<ConfigSnapshot>`：一致获取内容、版本及只读路径。快照的required_effects_from(before)由所属模块统一给出采用要求，Update与Reload复用此语义；资源变化均不得假报已应用。
- `async update(self: &Arc<Self>, expected_version: ConfigVersion, document: ConfigDocument) -> Result<ConfigWriteReceipt, ConfigError>`：真实校验并原子写入固定源；不接收路径。
- `async reload(self: &Arc<Self>, expected_version: ConfigVersion) -> Result<Arc<ConfigSnapshot>, ConfigError>`：重新读取同一固定源，拒绝过时请求。
- `prepare_driver_config(capabilities: &DriverCapabilities) -> Result<DriverConfigImage, ConfigError>`：从本实例的当前快照和已缓存路径导出持久默认、来源限制及固定文件自保规则，不接受另一份配置缓存或路径，不在主模块复制实现。

已导出镜像中的同一Arc不可变buffer由DriverPort按实际代保管，不另造ConfigLease服务或另一份可写缓存。

`ConfigWriteReceipt { saved_version, changed_sections, required_effects }`只报告真实保存；required_effects明确为可热采用、需中间层重连或需完整重新初始化。主模块调用相应模块接口组织采用，不能从此回执推定驱动已生效。

`DriverConfigImage { config_version, driver_config_version, platform, encoding_revision, immutable_buffer }`是下发结果；字节由配置模块定义／编码，主模块消息封装负责分段运输。驱动报告采用的是DriverConfigVersion，不能以该值声明整个AS配置都已采用；来源ConfigVersion仅用于追踪导出自哪份配置。规则模块独立提供DriverPolicyImage，不由配置模块读取规则正文再编译一次。

## 5. 底层写入与成功含义

中间层`UpdateConfiguration{expected_version,document}`经主模块消息入口认证和ManageService授权后，交配置模块。模块持有管理锁完成版本检查、完整校验、同目录临时文件写入和同步、原子替换，再发布真实保存快照；任务由底层保管到完成，外层等待取消不提前释放。

自保目录中的临时文件、重命名、权限设置也由底层实际执行。规则修改同样由规则模块写固定规则文件；中间层只能提交内容，不能自己编辑、指定备份／临时路径或获得维护票据。其他自保文件修改由底层所属模块通过有明确范围的接口完成，没有任意路径WriteFile。

主模块随后调用各模块准备／应用接口，经唯一驱动端口提交持久镜像。只有保存及全部必需采用确认后，整体配置修改才返回成功。失败报告saved_version和driver已采用版本等真实事实；不能声称全局回滚，不自动重启或将未生效项改成已生效。

普通持久重载保留临时覆盖。SetRuntimeFeature和ResumeRuntimeFeature不经过update／保存，不改变ConfigVersion；完整重新初始化才清除覆盖并采用startup_protection。恢复默认时若默认为关闭，必须如实报告仍关闭。

## 6. 扩展边界与成熟依据

底层安全日志已使用配置派生的固定路径持久化，远端查询和保留策略另行明确。Flow 和 credentials 由同一配置所有者维护。进程树设置已在[模块设计](../process-tree/technical.md#8-命中归集修复和资源)明确，ConfigDocument.process_tree 由配置模块统一读取／校验／写入。其余未明确消费者的字段不加入有效配置。

[fapolicyd配置说明](https://github.com/linux-application-whitelisting/fapolicyd/blob/main/doc/fapolicyd.conf.5)区分队列、运行参数与策略；AS只借鉴明确容量／实际统计，不复制线程数、信任数据库、permissive和学习逻辑。[ESET HIPS](https://help.eset.com/esu/18/en-US/idh_hips_main.html)将自我保护明确针对重要进程和文件；这里落实文件自保，不冒称已有其进程保护。[KubeArmor主机策略](https://docs.kubearmor.io/kubearmor/documentation/host_security_policy_specification)给出目录、递归、只读及主体字段，[路径查找文档](https://docs.kernel.org/filesystems/path-lookup.html)说明实际对象／重命名并发不能由字符串推断。借鉴这些职责和字段，不复制其优先级或宣称全部后端效果等同。

## Linux实现细节

[配置实现](../../../crates/backend/as-runtime-config/src/lib.rs)内部使用current_exe和canonicalize取得真实位置，要求实际目录为bin；保留根和config目录句柄。openat2以NO_SYMLINKS读取固定普通文件，更新前核对目录绑定和磁盘版本。临时文件、sync和rename均在绑定目录内完成；reload保留同一ConfigPaths引用。

ConfigVersion使用SHA-256标识ConfigDocument按固定结构字段顺序序列化的JSON字节；结构不包含map。YAML空白和注释不改变版本。旧字段、未知字段、缺失字段、零容量和错误平台身份均拒绝。ConfigDocument／ConfigPaths声明见[公共配置](../../../crates/contracts/as-types/src/configuration.rs)和[公共路径](../../../crates/contracts/as-types/src/context.rs)。

rename成功后先发布真实可见快照，再同步目录。目录同步失败返回Durability及saved_version，不声称回滚；主模块仍根据真实快照撤销已变更的授权。ConfigWriteReceipt只报告保存。prepare_driver_config由[唯一导出实现](../../../crates/backend/as-runtime-config/src/driver_image.rs)提供真实能力校验及Linux不可变镜像，主模块另等实际采用，不返回空镜像或假成功。配置文件访问实现限Linux，Windows原生路径写入尚未实现。

内核驱动的配置子类只验证和使用此投影，职责见[DriverConfiguration](../../drivers/linux/configuration.md)。它没有as.yaml读取／写入或目录定位接口，真实状态由内核DriverCore产生，经底层适配回传。

Update 与 Reload 共用保存后采用流程。配置模块保留本次初始化实际使用的原配置引用；需重建的设置与该引用比较，后续同值保存或重载仍报告待重建，直到设置恢复到运行值或新实例初始化。失败发生在实际发布之后时，仍撤销旧权限并核对凭证采用，保留真实快照及失败原因。

## Flow运行时配置

当前ConfigDocument接受flow:Option<FlowSettings>与credentials:CredentialSettings；flow:null明确未配置出口。当前唯一类型见[flow.rs](../../../crates/contracts/as-types/src/flow.rs)，未知／旧字段拒绝，不增加接口版本。配置模块统一读取／校验／写入和提供快照，Main只传不可变参数。

当前FlowSettings字段及含义：

本文独占FlowSettings和ProviderRoute等设置结构；FlowMode、ProviderProfile及基础数据只引用[公共契约](../../contracts/flow.md)，不能从Flow实现导入。原有ConfigVersion仍为基础内容标识，公共契约不依赖整个ConfigDocument。

```text
FlowScope = AllProtected | Workloads(Vec<Workload>) | Processes(Vec<ProcessKey>)
ProviderRoute { id: RouteId, origin: String, method: String, path: String,
                profile: ProviderProfile, session: Option<SessionField> }
SessionField = Header { name: String } | JsonPointer { pointer: String }
DecodeLimits { max_connection_buffer_bytes: u64, max_message_bytes: u64,
               max_active_exchanges: u32, max_json_depth: u32 }
FlowSettings {
  enabled: bool, scope: FlowScope, mode: FlowMode,
  provider_routes: Vec<ProviderRoute>,
  capture_content: bool, decode_limits: DecodeLimits,
  event_queue_bytes: u64,
  egress: Option<TlsEgressSettings>
}
TlsEgressSettings {
  ca: CaRef, max_connections: u32, max_streams_per_connection: u32
}
CredentialBinding {
  id: BindingId, secret: SecretRef, enabled: bool,
  subjects: Vec<CredentialSubject>, routes: Vec<RouteId>, slot: AuthSlot
}
CredentialSettings { bindings: Vec<CredentialBinding> }
```

Main 将 FlowScope 原样交给驱动适配。AllProtected 启用宿主及已核验的本地 Docker 入口，Processes 选择宿主进程的精确寿命，Workloads 选择完整的本地 Docker 容器对象。尚无活绑定的容器保留待接入，只有已启用的入口计入运行状态。已经绑定的入口启动失败会报告错误。重建对象使用新完整 ID，未知归属保持缺失。唯一接线见[FlowRuntime](../../../crates/backend/as-main/src/flow_runtime.rs)与[动态接入目标](../flow/container-egress-design.md)。

ProviderRoute只按URL解析后的origin、实际method及原始编码path匹配；path不含query、不自行解码后再折叠路径。重复或冲突的同一匹配元组在配置校验时拒绝，不定义另一套优先级。原始请求仍完整留给Flow作协议事实，路由规范化不能改写原文。

- `enabled: bool`、`scope`：启用及受保护对象选择。适配按实际 ProcessKey 或完整 Docker 实例身份核验。
- `mode: observe_tls | mediate_tls`。当前 FlowSettings 默认值为 mediate_tls，Flow 初始化只实现该模式。observe_tls 保留在类型中，实际启用时明确拒绝。mode 的选择不代替入口就绪状态。
- `provider_routes`：明确的origin、请求method、规范化path与ProviderProfile绑定；仍核实际HTTP／JSON格式。只在此维护协议路由，不成为另一套HIPS放行／阻止规则。
- `ProviderRoute.session`：显式指定该路由原客户端请求中应用已提供的会话字段。Header 的 name 使用小写 HTTP 字段名，JsonPointer 使用原 JSON 中的 RFC 6901 位置。None 时只保留原交换。字段和值各自保真，重复头或非字符串／数字的 JSON 值不产生单值归属。声明只赋予该路由的应用会话语义，不产生用户授权或安全裁决。
- `capture_content: bool`：是否采集聊天正文。关闭时仅有来源／流量摘要，不声称每条正文仍可见。
- `decode_limits`：当前解码状态字节上限、单协议单元字节上限、并发交换数和 JSON 深度；不预约完整会话最坏值。单位和字段名以同页当前结构为准。
- `event_queue_bytes`：唯一 Flow 消费队列的字节容量，正数且可表示为 u32；单条超限立即失败，队列暂满等出队。没有回放／TTL／逐订阅预算。

这些资源字段使用工程默认值并允许维护调整，不要求普通用户逐项填写才能观察；落实现时数值与真实容量校验一起确定，不声称未经测试的吞吐。字节数与数量按原字段单位计量，非零／上限／预算包含关系由配置模块校验一次，其他模块使用已验证快照和实际分配结果。

MediateTls必须有egress，ObserveTls禁止携带egress以免字段无效却被接纳。egress.ca只引用凭证模块已有CA；上游验证固定使用正常系统信任和主机名校验，不配置忽略验证开关。连接和并发容量保留明确单位及真实分配校验。网络连接、TLS 协商、原请求读写与 Flow 退役沿原 I/O 自然等待或显式取消，不配置 AS 计时截断；旧 connect_timeout_ms／handshake_timeout_ms／drain_timeout_ms 字段直接拒绝，不保留 0 开关或兼容分支。首版mTLS、ECH、QUIC、pinning绕过和在线CA轮换均不支持，采用前明确拒绝，不列“可能支持”。

顶层credentials:CredentialSettings，只保存非秘密BindingId／SecretRef、实际主体和route引用，全部私钥／真实key／phantom值禁止进入ConfigDocument。每个phantom只对应一个BindingId，以token的真实映射选绑定，无允许／拒绝排序歧义；同一个route可服务不同已授权主体的独立绑定。移除绑定或禁用后，新授权立即拒绝。

配置模块校验唯一ID、引用、scope形状和预算，提供同一只读快照；凭证模块只保管传入Arc，不另存一份可写策略。Main按required_effects调用凭证apply_settings和Flow必要重建；未实际采用不得回复Applied。ConfigVersion保留来源，授权检查以当前绑定／秘密修订为准，普通无关配置变化不自动废弃所有TLS连接。

新增固定路径仍由配置独占派生：credential_store_file为data/credentials/store，ca_public_file为data/credentials/ca.crt；均位于同一自保根。凭证模块执行真实私有写入，不由Main、中间层或Flow自行拼根。普通data_directory不是允许外部指定任意秘密写入目标的接口。

MiddlewareAccess具有`chat_content: bool`，控制该已认证peer是否可读取当前采集范围正文；独立于read_only／manage_rules／manage_service，无需每条再次授权。未授权只有摘要；变更后旧订阅撤权，不能让客户端自报true。当前结构／文件／校验已同批接入，不留旧接口别名。

中间层业务日志的固定文件只由本模块派生为 data/middleware/business.sqlite，公开只读 getter。业务日志模块执行真实数据库初始化／事务；不存在底层聊天存档端口、data/chat 旧目标或任意客户端指定路径。

资源／mode改变沿现行required_effects报告重建范围；已采集会话持原快照，新会话采用新值。内容读取权限撤销立即作用于订阅，不等待旧会话结束。实际来源Unavailable不会被配置true覆盖，已有HIPS初始化失败停止规则不改变。

受控出口在初始化时对 route 解析 DNS，形成当前静态 IPv4／IPv6 地址集合。无地址结果明确失败；容器scope按[实际动态入口绑定](../flow/container-egress-design.md)逐个核验和接入，未知归属或具体不支持条件明确保留，不能把容器范围一概写成未实现或已接纳。DNS更新／Flow资源变化需要完整重建。凭证绑定修改独立产生ApplyCredentials，由Main立即调用凭证所有者采用；其他Flow设置变化产生ReinitializeMain，不把保存等同于运行时采用。公有CA与私有store固定路径已由ConfigPaths导出，凭证模块接收同一protected_root以建立并保管目录句柄。
当前受控出口要求DNS SNI，IP-literal origin在配置校验时返回Unsupported；没有隐含直连或禁验选项。原始地址仍由入口事实记录，域名／IP是不同事实字段。

无消费者的capture_limits／tls_runtime_profiles／attach_deadline_ms已从当前配置删除，旧键被严格拒绝；原意图见[归档](../../archive/flow-design/configuration-before-unified-audit.md)。Flow入口初始化沿实际就绪、I/O结果和显式停止收敛，不能把已退役字段当作生效控制。

DecodeLimits.reservation_bytes是当前解码保留额度关系的唯一计算，配置校验及实际分配共同调用。当前按connection buffer的8倍作并存raw／decoded／projection缓冲保留计量，先checked转换并与total核对；不是进程RSS测量或TLS库全部内存硬上限。
## 写关闭审计范围

`ConfigDocument.file_close_audit` 为 Linux 上的绝对目录列表，`[]` 明确停用。配置所有者拒绝相对路径、重复目录及 `..`。配置变化产生 `FileCloseAudit` 分区变更与 `ReinitializeMain`，初始化时由本模块的同一DriverConfigImage导出给驱动绑定真实目录对象。普通发布使用启动时该范围，目录变化仍要求完整重新初始化；Main不重读、日志模块不解析。字段见[安全审计契约](../../contracts/security-audit.md)，编码见[驱动通信](../../contracts/driver-communication/technical.md#写关闭审计事实)。

## Group 回环观察范围

可选的 `group_observation` 为 None 时关闭观察；非空对象必须显式提供 `a2a_port`、`otlp_http_port`、`gateway_ws_port` 三项。每项为 `Option<NonZeroU16>`：null 不启用该来源，整数 1—65535 指定宿主初始网络命名空间内、两端地址均为 `127.0.0.1` 的 IPv4 TCP 端口。至少启用一个来源，所有启用端口互异；不用占位端口，也不接受省略字段或旧两字段对象。三项分别仅选择 A2A HTTP、直连 OTLP/HTTP、公开 Gateway 明文 WebSocket 字节的采集范围，不按 Agent 名称或内容裁决。

配置模块在初始化时固定本实例的 Group 范围，驱动投影和业务消费者使用同一范围。保存的新范围在完整初始化时生效，保存后重复更新或重载仍报告需要重新初始化。规则热替换及其他驱动配置更新继续使用本实例的固定范围，三项端口一起比较，不能只更新其中一种来源。原生48字节投影的编码唯一见[驱动通信](../../contracts/driver-communication/technical.md#持久镜像内容编码)，采集边界见[Group 技术约定](../../middleware/agent-graph/technical.md#持续采集的来源边界)。

Session Header 选择器不接受 authorization、x-api-key、proxy-authorization、cookie、set-cookie；这些属于凭证／认证内容，不能作为非秘密会话元数据暴露。这只约束新增会话字段选择，不丢弃已按现行范围取得的正文、错误或缺失事实。
