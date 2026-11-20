# 主模块消息与驱动通信技术契约

公共引用：[产品设计](design.md)。公共定义以链接目标为准，本页不另定义一份。

职责见[架构](architecture.md)，配置全部字段见[配置契约](../../backend/configuration/technical.md)。本规范统一接口和数据含义；原操作捕获及合法等待点仍由[驱动契约](../../drivers/technical.md)定义。

## 1. 机制选择与依据

当前生产实现只有 LinuxDriverAdapter。下述 Windows 通信是预留设计，尚无 Windows 驱动实现或验收；平台共用契约不代表原生采集／执法实现已经跨系统。

Linux复用固定`/dev/as_wait`受保护字符设备及非阻塞read／write、poll／epoll；一帧一次提交，不增加第二条Netlink或ioctl控制路径。[Generic Netlink](https://docs.kernel.org/userspace-api/netlink/intro.html)支持双向请求／通知；[内核ioctl设计说明](https://docs.kernel.org/process/botching-up-ioctls.html)提供固定宽度、对齐、长度与保留字段校验依据，但其兼容保留建议不用于本项目。

Windows采用固定`\AgentShieldDriver`的FltMgr端口：FltCreateCommunicationPort／FilterConnectCommunicationPort建立连接，FilterSendMessage下发，FilterGetMessage配合OVERLAPPED接收，FltSendMessage上行。[微软通信文档](https://learn.microsoft.com/en-us/windows-hardware/drivers/ifs/communication-between-user-mode-and-kernel-mode)规定端口访问控制，[System Informer comms.c](https://github.com/winsiderss/systeminformer/blob/master/KSystemInformer/comms.c)展示一般安全监控消息用法。只使用一个授权底层服务连接，不照搬其多客户端和线程数量。

[DeviceIoControl](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-deviceiocontrol)、微软[event样例](https://github.com/microsoft/Windows-driver-samples/blob/main/general/event/wdm/event.c)及[cancel-safe队列](https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/cancel-safe-irp-queues)是通用驱动通信参考，不作为同时启用的后备接口。平台适配剥离原生头，业务不接触FD／HANDLE／IOCTL编号。

Linux端点限授权特权服务，Windows端口用DACL及连接回调验证实际服务身份并限制单连接；客户端payload不提供角色／authenticated／可信PID。驱动身份还须由安装来源、原生端点及接口校验确认，Hello中的字符串不是信任根。不增加自造共享口令协议。

## 2. 两端接口在主模块消息封装内

当前MainMessages组合中间层命令接纳与DriverPort；实际字段与接线见[management.rs](../../../crates/backend/as-main/src/messages/management.rs)。两端分别使用现行结构化管理封套和二进制驱动封套，不把它们强制成同一线格式。

中间层下行命令、权限及上行值只在[Main消息契约](../../backend/main/messages/technical.md#下行)定义；本页不保留GetConfiguration／GetRules等旧独立命令名或复制另一组DTO。当前没有软件包更新下行命令。

查询复用已鉴权会话；配置写入／运行期开关需要ManageService，规则写入需要规则管理权限。请求不带配置源路径、自保根、任意文件写入目标或驱动句柄。

上行包括原命令结果、实际`DriverStatus`变化、配置／规则版本及原有问询／原操作结果。中间层命令ID与驱动会话request_id单独关联，不把它们当同一个原操作ID。命令回执和主动状态均带驱动state_revision，旧回执不能覆盖较新的状态显示。

## 3. 模块接口及数据所有权

模块接口不在通信文档重复声明：

- 配置初始化、读取／写入及DriverConfigImage：以[配置接口及结构](../../backend/configuration/technical.md#4-新增结构及模块接口)为准。
- 规则初始化、管理及DriverPolicyImage：以[规则接口及结构](../../backend/rules/technical.md#普通规则的独立源与下发结构)为准。
- 底层驱动连接、初始化及用户态资源：以[底层驱动适配接口](../../backend/driver-adapter/technical.md#平台接口边界)为准。
- 主模块只组合以上公开结果；公共初始化传参以[MainContext](../main-context.md)为准。

[DriverInitParts](../../backend/driver-adapter/technical.md#平台接口边界)只组合上述两个模块的不可变镜像，不接收模块实现对象。原生对象绑定归内核驱动实际执行，未明确的绑定契约先存档，不加由主模块臆造的native_bindings参数。自保目标只能来自配置模块镜像／只读路径，驱动适配不能调用程序位置查询另算根；必要的原生目录对象绑定只是核验和使用该目标。Linux cgroup FD、Windows对象HANDLE仍归平台资源管理，不成为中间层配置字段。

文件自保进入文件类系统组1，文件Driver组2为同源维护放行，普通文件Driver组从3开始，禁止冲突或自动改号；进程／网络Driver组1为维护，普通组从2开始。配置投影的系统组1由实际对象权限guard执行；当前只上报实际拒绝项rule2，不把每个路径或受控写放行伪造成一张已存在的可枚举规则表。来源标为配置模块；完整只读列表尚无生产接口。可信写主体由驱动验证底层服务实例，不按名称或裸PID猜测。自保定位、固定路径及字段在配置契约唯一维护。

## 4. 运行时与持久状态分开

`PersistentDriverState = DriverConfigImage + DriverPolicyImage`，由两个模块准备、初始化时下发；只由真实持久更新替换。

`RuntimeFeatureOverride = Default | Enabled | Disabled`，对应process／file／network普通防护；不写盘。SetRuntimeFeature设明确临时值，ResumeRuntimeFeature移除覆盖采用当前持久默认。直到手动恢复或AS／驱动完整重新初始化为止，不增加定时器；重载、状态查询和单纯连接重建不叫重新初始化。

`DriverStatus`包含实例、固定协议标识、来源状态Dormant／Running／Quiescing／Failed、persistent_generation、state_revision、lifecycle_epoch、已采用DriverConfigVersion／RuleVersion、持久默认、临时覆盖、实际有效类别、自保实际状态、在途／待回执／丢失数量及来源错误。能力另在DriverInfo.capabilities提供。未采用状态用None表达，不凭连接成功或数字推定已安装。

persistent_generation只在完整持久镜像采用时递增；state_revision在临时覆盖本身变化（即使有效布尔值相同）、有效开关、持久提交或生命周期变化时递增，普通计数变化不递增，它们是查询时点采样，不用较旧的数量采样覆盖已知更新的控制状态。驱动是实际状态来源，主模块只持只读观察并上报中间层，不用配置默认值填补故障。

持久替换在提交时保留当前临时覆盖，不能把大镜像开始传输时的旧开关写回。Quiesce／故障均推进lifecycle_epoch，包括Dormant和重复停止；首片及最终采用都检查期望epoch，停止前的候选不能在停止后启动来源。它是生命周期并发标识，不是接口版本。初始化明确清空临时覆盖。仅普通类别开关不关闭文件自保。

文件事件先检查同一文件池的固定自保前缀组，再判断普通文件防护临时开关，最后才进入普通文件Driver组及移交链；关闭普通文件防护不能越过自保。进程／网络的普通开关只影响本类别。仍被其他模块需要的基础身份事实不因“暂停普通防护”被冒称成已经卸载，实际采集能力单独报告。

## 5. 唯一消息封套与大buffer

协议固定revision=2，只接受当前格式；开发迭代直接替换声明和全部消费者，不递增版本、不嵌套或兼容旧头。所有小控制和规则大buffer使用同一64字节、小端封套，原生唯一声明见[as_protocol.h](../../../drivers/linux/as_protocol.h)：

- offset 0：`magic:u32=0x32435341`（ASC2）、`revision:u16=2`、`class:u16`（1请求、2答复、3事件）。
- offset 8：`message_type:u16`、`reserved16:u16=0`、`body_bytes:u32`（本片正文长度）。
- offset 16：`session_id:[u8;16]`。
- offset 32：`request_id:u64`。
- offset 40：`total_bytes:u64`（完整消息正文长度）。
- offset 48：`offset:u64`（本片在完整正文中的位置）。
- offset 56：`reserved64:u64=0`。

驱动为授权连接生成session_id，首次Hello请求为0；随后严格匹配。request_id由底层分配且会话内不复用，答复回显同ID及message_type。主动事件ID=0，原操作original_id另在正文中，不能借用管理ID或PID。

小消息只有offset=0、total_bytes=body_bytes的一帧。下行仅InitializeDriver／ReplacePersistentState请求允许大正文分段；上行仅Group TCP事件0x10a允许分段，按下文整条原子入队和接收泵唯一在途上下文处理，不能按ID=0合并不同事件。其他事件和普通答复不分段。完整正文先形成所属不可变buffer/物理片，再依原偏移提交；每帧一次原生提交，不与别的帧混写。

每个传输方向的所属接收者最多保管一个未完成大消息；首片从0开始，后片的会话、ID、类型、total_bytes相同且offset必须等于已接收长度。偏移加法／长度换算检查溢出，拒绝重叠、缺口、零进展或越界。下行镜像首片包含完整镜像描述：DriverConfigVersion、RuleVersion、表示版本、配置／规则buffer各自长度及两者连接数据的SHA-256。全部接收并校验后才验证和采用；此前无匹配效力。

DriverInfo报告实际max_frame_bytes／max_transfer_bytes和接纳容量。片段大小取实际可承载长度，完整长度取实际资源上限，不用一个固定小帧上限拒绝本可分段的大规则。暂存失败、校验错误或连接关闭释放候选，保留原有效镜像；不做跨会话续传。AbortTransfer只取消尚未进入采用的暂存，不能把已开始或已完成应用报告为已撤销。

下行镜像片段之间允许插入原操作答复、临时开关、状态和停止命令；上行Group片段只与优先完成出口中的管理Reply或已读Controlled的AS_RESULT交错。驱动候选分配、校验及用户拷贝不持原请求全局锁；每消息有独立存储。数据内容只用固定宽度和显式长度，不传裸指针、size_t、long或Rust enum内存布局。

### 分片采用依据

Linux的[skb_copy_bits API](https://docs.kernel.org/networking/kapi.html)及[内核实现](https://github.com/torvalds/linux/blob/master/net/core/skbuff.c)按原offset/length复制非线性skb。现有AS hook继续调用这一成熟API，同回调复制所属字节；不保留目标内存或skb供以后查询。

Netfilter官方[nf-queue.c示例](https://netfilter.org/projects/libnetfilter_queue/doxygen/html/nf-queue_8c_source.html)按NFQA_CAP_LEN与实际payload长度辨认截断；[内核nfnetlink_queue.c](https://github.com/torvalds/linux/blob/master/net/netfilter/nfnetlink_queue.c)先构造完整消息，再在队列锁下核容量并发布。这里吸收完整长度与传输容量分开、构造后一次发布和如实报告损失的惯例；不将Netlink dump multipart冒称同一负载分片，也不引入第二通信栈。

Rust已锁定的[Tokio LengthDelimitedCodec](https://docs.rs/tokio-util/0.7.18/tokio_util/codec/length_delimited/struct.LengthDelimitedCodec.html)解决字节流到物理长度帧，并在EOF剩余未完成消息时报告错误。AS字符设备read已逐个返回完整物理帧，DriverFrame已有唯一长度/身份codec；再加FramedRead会新增无用途缓冲，不能解决先前内核丢字节。上行只增加现协议所必需的session/total/offset核对与一份在途正文，复用标准Vec移动和原接收泵。WS仍由锁定tungstenite处理，不重复写通用协议库。此处为对成熟机制与当前device_read契约的适用性判断，不声称第三方库直接实现AS私有协议。

### 持久镜像正文描述

InitializeDriver／ReplacePersistentState正文以固定136字节描述开头，随后依次是两个不可变buffer：

- `expected_persistent_generation:u64`。
- `platform:u16`，复用[MainContext的TargetPlatform](../main-context.md#数据结构)，线上编码为1 Linux、2 Windows，不另定义一个平台枚举；`reserved16:u16=0`。
- `config_encoding_revision:u32`、`rules_encoding_revision:u32`、`expected_lifecycle_epoch:u32`。
- `config_bytes:u64`、`rules_bytes:u64`。
- `driver_config_version:[u8;32]`、`rule_version:[u8;32]`。
- `buffers_sha256:[u8;32]`，覆盖配置buffer连接规则buffer的实际字节，不含描述本身。

完整正文长度必须等于136＋config_bytes＋rules_bytes，所有加法检查溢出；首片必须容纳完整描述，后片只是正文的连续区间。配置和规则表示版本分别校验，不能用一个表示版本混淆两种编码。空普通驱动池由规则模块编码明确的空池镜像，不以缺失buffer猜成空规则。

### 持久镜像内容编码

当前config_encoding与rules_encoding固定为1。布局和字段编号只在[as_protocol.h](../../../drivers/linux/as_protocol.h)定义，底层分别由[配置导出](../../../crates/backend/as-runtime-config/src/driver_image.rs)和[规则导出](../../../crates/backend/as-policy-engine/src/driver_image.rs)编码，不在主模块复制编译。

- 配置为48字节as_configuration，后接root／backend／loader三条原始路径，各按头内长度取值，不带NUL；再接写关闭目录描述。默认开关、真实接纳／保管上限、required_features及self_protection_log均显式提供；self_protection_log仅选择真实自保拒绝的日志，可信自身修改直接放行且不产生逐次上抛。offset32的`group_ports:u32`低16位为A2A端口、高16位为OTLP/HTTP端口，offset36的`gateway_ws_port:u32`为Gateway明文WebSocket端口，小端；各端口0表示不启用该来源，其他值必须为1—65535且互异。三项全0关闭观察。该范围只在初始化采用，热替换任一端口不同均返回`EINVAL`；当前镜像保持原范围。offset40的`close_directories:u32`为目录数，offset44的`close_directory_bytes:u32`为其描述总字节；每项是`length:u32`及原路径字节，不带NUL。二者均0表示未选择关闭审计目录。此来源只由配置模块下发；普通替换复用未变描述的真实目录绑定，改目录须完整初始化，不能在规则重载时按旧名称重新绑定同名新对象。没有旧格式解析或版本回退。
- 普通规则为32字节as_policy：header_bytes:u32必须为32、rules:u32、bytes:u64、backend_operations:[u32;3]、backend_routes:u32；统一小端。offset16／20／24分别为进程／文件／网络的无条件后端候选操作位图，位(operation-1)保留匹配机会；合法操作掩码为0x1／0x60379／0x3，实际启用位由原已编译规则产生。offset28为三类有条件后端候选的总条数。头后依次为rules条40字节as_rule及各自ID／条件，再接backend_routes条候选。每条普通规则及候选只属于一个kind；没有普通Driver规则和后端候选时可编码32字节零记录头。只接受当前布局，不保留旧文件路由解析或增加迭代版本号。
- 每条as_backend_route为24字节头：kind:u16（1进程／2文件／3网络）、reserved:u16必须0、operations:u32、16字节原as_condition，随后该条件的原值。每条启用后端规则最多投影一个在对应hook可获取的必要目标条件：进程TargetExecutable=13的Exact／Prefix，文件AccessMask=19的包含位（仅Open，算子5、一个非零小端u32）或FilePath=15的Exact／Prefix，网络RemotePort=23的含端点范围（算子7、两个小端u16）。投影选择顺序只引用[规则模块](../../backend/rules/technical.md)。复用原条件编码与匹配，不向候选加入动作、RuleHit或另一份身份。没有可投影且能力已支持的必要条件时，该规则保留在本类无条件操作位图。原生按kind和operation筛选；只有必要条件明确不符才排除候选，相符、缺失或无法判定均保留ToUser机会。完整规则、容器范围与最终动作仍由原规则owner匹配。
- 条件为16字节as_condition后接实际值；fields与算子编号以同一声明为准。只接受完整支持的条件／操作／动作，不删除条件、不猜通配、不将不支持的终止降为仅阻止。
- 能力固定40字节as_capabilities，独立features不混入operations；状态固定144字节as_status，末尾字段为lifecycle_epoch。所有长度、保留字段、SHA-256及能力在实际采用前验证。

`features`中的`ControlPriority=512`表示同一设备具有原完成列表的PRI可读语义：待交付Controlled、管理Reply与仅已读Controlled的AS_RESULT优先于普通FIFO，`poll`报告EPOLLPRI，普通read仍先读完成。当前适配在Hello核实际能力；缺位为Unsupported，不使用协议版本、旧回退或到背压后再等待未知PRI。Controlled沿原held预算保管；Reply与AS_RESULT仍按真实class/type分别归还原reply_count与receipt_count，能力位不增加列表、额度或用户开关。完整机制见[原生端点](../../drivers/linux/transport.md#缓冲与临界区)。

AS_CONTROL_CREDIT（0x0012）是同一认证reader内的空正文请求，Reply沿现行as_reply返回。每次由适配真实队列OwnedPermit授权一条Controlled交付；reader的control_credit仅表示尚未使用的一条交付额度，不是产品开关或时间限制。字段与原件撤回寿命见[端点临界区](../../drivers/linux/transport.md#缓冲与临界区)。

DNS Send快照能力使用features的 `DnsQuerySnapshot=1024`，只表示同一UDP／53 Send的读取来源，不开启Native Domain条件25。网络TLV9及一次解析字段由[原事件契约](../events.md#udp-dns-send-查询快照)唯一维护；沿现有Controlled／Direct事件封套传输，不增加另一条消息、队列或协议版本。没有该真实能力时，带DNS条件的后端规则导出明确Unsupported。

自保维护的`SelfMountRules=2048`只报告Driver直接Mount规则能力，不增加controlled operation_mask位。条件26使用既有u32等值操作3，期望值为[原MountEffect](../events.md)中的independent_view=1；只用于File Mount。原规则镜像及管理来源不变。它不能授权ToUser／Ask或用ProtectedTree／Unknown条件覆盖自保对象检查；原命中通过现有Direct结果及RuleHit回报。

## 6. 消息类型与完成条件

请求和答复使用相同message_type：

1. Hello：驱动身份、实际能力、当前状态和容量。
2. InitializeDriver：expected_persistent_generation及完整持久镜像；明确重新初始化并恢复持久默认。
3. ReplacePersistentState：expected_persistent_generation及完整持久镜像；保留临时覆盖，失败不部分采用。
4. GetStatus：一致读取实际DriverStatus。
5. Quiesce：停止新接纳，保持答复／回执，排空后完成；Failed不变成停用允许。
6. DeclareFailure：记录来源故障并触发真实收尾，后续正常清理不改写首原因。
7. BindReview：original_id、RuleVersion、原生绝对deadline和超时Decision；确认绑定后才展示问询，只缩短原窗口。
8. Decide：original_id及Decision（包括准确终止范围）。
9. CancelOperation：original_id；请求原执行者取消。
10. SetRuntimeFeature：指定普通防护类别及enabled，驱动应用后返回实际状态。
11. ResumeRuntimeFeature：移除指定类别的临时覆盖，返回实际有效值。
12. AbortTransfer：目标暂存request_id；未进入采用才可撤销。
13. StartFacts：在同一已鉴权会话开启基础进程观察，返回并发布开始标记，不采用普通规则或改变来源执法状态。
14. InspectBatch：核对1—64个实际宿主TGID的固定身份／轻量事实，返回每项实际结果；不是任意内核地址或字段读取接口。
15. FactsBarrier：返回一个受理标记，完成由同ID的0x106确认；不等待人工决策或原操作最终结果。
16. KillProcess：显式终止实际绑定的进程，字段见本页原进程工具段。
17. InstallRuntime：采用实际进程／exec的libuv旁路观察计划，唯一字段与完成语义见[原生请求观察源](../../backend/driver-adapter/runtime-observation-technical.md#安装命令17)；不是前端任意内存读取接口。

答复先含8字节as_reply（errno与0保留字段），再附类型对应正文。底层区分InvalidInput、Unauthenticated／Forbidden、Unsupported、Conflict、Busy、Unavailable及Stopped，原生errno按当前codec映射。发送API成功不等于收到该答复；已经完成的原操作仍领取同一真实结果。

事件类型：0x100受控原操作请求；0x101原操作实际结果；0x102驱动直接规则命中及结果；0x103实际状态变化；0x104仅观察事实；0x105持久代退役（正文为8字节generation）；0x106事实栅栏完成标记；0x107报告直决计数批；0x108原生系统调用请求返回观察；0x109 private futex 唤醒选择，0x10a Group 原 TCP 观察；0x10b写关闭审计事实；0x10c [原生请求原始交接](../../backend/driver-adapter/runtime-observation-technical.md#原始交接事件0x10c)。0x100和0x101沿端点优先列表交付，仍是class=Event，不伪装成管理Reply；其余事件沿原普通FIFO。容量、交付许可与poll边界只在[原生端点](../../drivers/linux/transport.md#缓冲与临界区)定义。0x101只在对应Controlled已实际读出后产生，不越过未读原请求。0x105只在旧快照最后一个hook／原操作引用释放且旧事件已入队后产生，其预留记录不丢弃，Quiesce按实际普通FIFO与在途寿命确认排空，retired_count仅维护该类既有预算。会话内original_id在重新Initialize后仍不复用。

### 驱动直决计数

0x107正文以16字节原事件source开始，后接一条或多条紧凑增量。每条增量由72字节as_hit_count及原as_hit（含两个ID）组成。前缀依次为首末源序号、原进程出生BOOTTIME纳秒、实际命中次数（各u64），宿主TGID（u32），事件类别和as_hit长度（各u16），规则来源摘要（32字节）。所有整数小端。源域固定为同一原生事件域1。首末为序号上下界，不通过相减计算次数。规则来源由as_hit的显式标签解释；整批必须完整解码。此帧不携带路径、参数、决定、原操作能力或执行回执。

0x108 正文为 56 字节。原事件 source 为 16 字节，后接 domain、sequence、原 wait_id、返回 BOOTTIME 纳秒四个小端 u64，原函数返回为小端 i32，末尾小端 u32 为 operation。1 至 4 分别表示 FileOpen、Execute、Connect 和 Send。适配器以已校验的当前会话和 wait_id 复用原 DecisionId 构造。观察使用普通 FIFO，容量丢弃计入原丢失统计。对象寿命见[原生系统调用请求返回](../events.md#原生系统调用请求返回)。

0x109 正文为 80 字节。前 16 字节为原 source，后接原生域 1 内的 sequence 和选择 BOOTTIME 纳秒两个小端 u64。然后是 waker、waiter 各 16 字节，依次为小端 TGID、TID 两个 u32 和出生 BOOTTIME 纳秒 u64。末尾两个小端 u64 分别为配对等待编号和观察时间。两值均为零表示等待证据缺失。按普通 FIFO 传送的观察不持有原操作，容量丢弃计入原丢失统计。[原事实](../events.md#private-futex-唤醒选择)

0x10a 使用本页既有64字节封套的 `total_bytes/offset/body_bytes` 分片；完整逻辑正文仍是一份72字节as_group_tcp及原负载，不重复元数据、不增加消息类型或协议版本。单帧仍至多16384字节，完整逻辑消息仍受原AS_TRANSFER_MAX约束。只额外允许这一事件类型分片，其他原小事件仍不分片。生产端在同一次hook内用原skb_copy_bits复制全部字节到所属物理帧，整个逻辑消息的全部帧在原普通队列中原子接纳或全部拒收；不延后回读skb/Agent内存。原观察序号每个逻辑消息分配一次，各片共用已定的session/total，offset从0连续覆盖同一正文。片段之间只可发送完成优先出口的Reply或AS_RESULT，不能插入另一个普通事件。原普通队列和Group帧配额不增大，失败仍沿原来源损失序号如实暴露。

驱动适配唯一接收泵只为当前已接纳的0x10a逻辑消息保管一个有界重组上下文；身份/session/total/连续offset逐片核对，完成后沿原group_tcp解码与队列一次移交。原片body转移所有权，不另造原件DTO/缓存或第二线程；完整包也走同一入口。结束/错序/冲突不冒作完整观察，按实际传输错误处理；接收泵真实返回结果经join进入NativeSession最终退役结果，先清理实际资源再报告，不以发出stop或旧healthy快照替代。Group业务库、TCP/WS分帧及原始记录结构不变。

0x10a 正文以 72 字节 `as_group_tcp` 开头。前 16 字节是驱动来源实例，后接两个小端 u64（观察序号、初始化代）、四个小端 u32（TCP 序号、ACK、字节数、TGID）、小端 u64 组出生纳秒、两个小端 u16（源／目的端口）、两个 u8（种类／标志）、零保留 u16 和小端 u64 socket cookie。种类 0 的原包尾部带所声明的原负载，TGID 和出生时刻为零。种类 1／2 为实际发送／接收区间，字节数来自原返回值，ACK 和包标志为零，无正文尾部。种类 3 为已建立的 OTLP socket 首次应用发送的入口观察，TCP 序号是首字节序号，cookie 非零，ACK、字节数、进程字段及标志均为零，无正文尾部。实际发送归属仍取种类 1。原包的 cookie 在内核给出实际完整 TCP socket 时取得，缺失为零。来源配置、长度和损失边界见 [Group 契约](../../middleware/agent-graph/technical.md#持续采集的来源边界)。

原生计数批在同一FIFO中拥有独立有界配额，不占用生命周期／结果的原有接纳配额。只合并尚未出队的相邻同身份项，不重传已出队增量。原生接纳失败逐次增加直决计数丢失数，适配接纳失败逐批增加直决计数拒收数。两项经原事实栅栏送至进程树，仅影响统计完整性。适配容量大于一时，计数批不占最后一个空位。

原操作请求为128字节as_controlled＋当前事件帧＋可选as_hit；结果固定64字节as_result。offset20的flags为本次实际终止范围0 None／1 Actor／2 ActorAndParent；offset24／28为actor_signal／parent_signal:i32。未请求字段必须-ENODATA；已请求字段0表示信号接纳，-ESRCH表示固定对象不存在，其他负值为真实失败（包括-ENODATA）。scope不为0只能伴随Denied，不能从错误码猜scope。as-types::operation的TerminationReport为唯一用户态结果结构，两个字段独立，不把信号失败改成原操作失败或允许。命中头20字节，显式source标签1配置／2规则文档，不从组号推断。直接规则结果进入事实／消息路径，不再创建第二个待执行操作。Allowed／Denied仅表示本次AS LSM检查结果，不代表syscall成功、已创建进程或网络已交付；真实效果须另取原操作和生命周期事实。

0x104正文为一条现行统一事实帧，解码后复用EventData且stage必须为Observation；不含original_id、可执行能力或编造的RuleHit。用于[创建／exec成功／退出生命周期](../../drivers/linux/hooks.md#进程树必需的生命周期hook)，避免把未保管的事实伪装成0x100，或把基础事实伪装成0x102规则命中。它沿已有事件通道和接收泵发送，不新增连接、业务线程或任意payload总线；同其他既有小事件一样不分片；0x10a的大负载分片是上文明确例外。原生通过正式tracepoint注册生产，底层要求实际会话标识匹配；软件回调检查不代表内核捕获证明。

0x100受控正文保留`transfer_hit:Option<RuleHit>`：Driver命中ToUser时为该规则原命中，确定未命中为None；不与后续底层最终命中混淆。其含义与log归集只按[规则整体契约](../../backend/rules/technical.md#驱动接入与既有规则的整体一致性)，不是新的动作或第二个原操作，不在事件事实TLV里塞控制权限。

BindReview答复只证明绑定，Decide／CancelOperation的投递确认不证明原操作已结束；最终只取同一OperationResult，原窗口超时先完成时取已有回执、不重发决定。驱动状态事件经主模块转发中间层，普通日志仍交日志模块，不直接推中间层。

### 基础进程事实的线格式

本节是StartFacts／InspectBatch／FactsBarrier及0x106的唯一字节定义；逻辑数据复用[process_source.rs](../../../crates/contracts/as-types/src/process_source.rs)，进程树如何扫描／补全按[所属契约](../../backend/process-tree/technical.md)。这些小消息仍使用本页现行64字节封套，不分片，不增加协议版本或第二连接。能力位ProcessFacts=32独立于普通操作位。

StartFacts和FactsBarrier请求正文为空，成功Reply的8字节as_reply后为48字节as_facts_marker。0x106正文也恰为48字节。六个小端u64依次为tracking_epoch、marker、loss_epoch、宿主初始PID命名空间inode、cutoff_boottime_ns、native_hit_loss，对应offset 0／8／16／24／32／40。cutoff在翻转采集桶时取得，之后固定。完成事件采用发布时的丢失数。适配器读取该标记时附加此前的backend_hit_rejected。

InspectBatch请求为count:u32、reserved:u32=0，随后count个host_tgid:u32；count必须1—64，PID不得为0。成功Reply的as_reply后是同样8字节count／reserved头，随后按输入顺序每项固定64字节as_process_binding：offset 0为PID:u32，4为status:i32，8为birth:u64，16为parent_pid:u32，20为kind:u32，24为parent_birth:u64，32为self_exec_id:u64，40为cgroup:u64，48／56为采样开始／结束BOOTTIME纳秒。kind 0为User、1为Kernel；status 0表示实际绑定，-ESRCH表示采样时不存在，-EACCES／-EPERM表示不可访问。整次请求结构或权限错误放在as_reply，不能伪造成所有PID不存在。所有字段只按固定类型解码，不将/proc tick换算冒充纳秒birth。

StartFacts可在Dormant／Failed中先行取得事实，不把Failed洗成Running；已有直接规则和自保继续按原状态执法。开始观察不表示整机镜像已经就绪。Initialize保留已开启事实流的source／sequence，不能在扫描中途重置事件键或替换发送队列。任何有效Quiesce都关闭该会话的新观察接纳，包括当时仍Dormant或已Failed；已接纳采集及其标记继续排空，迟到StartFacts／FactsBarrier返回Stopped，不得重新开观察。只有通过完整持久镜像、预期generation及lifecycle_epoch校验的显式Initialize成功提交，才能恢复该会话的事实接纳；旧候选不能清除停止栅栏。真实会话失联则关闭采集，底层明确使进程树失效。

两桶在途引用覆盖所有可能发布EventData的普通Controlled／Direct、Self Direct和Observation，均在采样前登记。栅栏翻桶后，新桶样本的source_boottime不早于cutoff；旧桶在事件进入设备FIFO或确定不投递后归零，才发布预留0x106。Controlled在请求入队后就释放采集引用，不能持引用等用户答复。标记独立于普通观察容量，不因日志队列满而丢弃；仅有界接纳一个尚未可发布的标记，资源不足明确Busy。

底层收到0x106后还必须经同一快速输入FIFO的事实栅栏，确认此前FastFacts已执行，再更新树的关闭时间窗并确认处理完成。它不等待Main询问队列或pending.apply；只读到设备队列为空、最大的sequence或命令Reply都不足以完成该栅栏。loss_epoch只表达原生进程观察损失，底层拒绝计数另由适配记录；两者与普通DriverStatus.dropped分开，不能从序号缺口猜完整性。

## 7. 写入与采用的真实收口

中间层UpdateConfiguration／UpdateRules分别调用底层所属模块，底层使用固定源、版本检查、完整校验和原子文件替换实际写盘。自保范围内的修改由已验证底层服务执行；没有任意路径写文件或给中间层临时放权的接口。

主模块在现有消息职责内串行组织持久修改任务，避免配置／规则更新互相覆盖；完成模块保存后，分别取得当前两个镜像再发布。配置／规则模块保管自己的版本和候选／在途租约；主模块不维护另一个历史库；模块不内部追索另一个模块，以公共契约参数传数据。声明整体成功必须完成真实保存和所有必需采用；否则报告失败及实际保存／生效版本，不声称自动回滚。

运行期开关／查询／原操作答复不进入持久修改串行门。已开始的写入、采用和退役由实际任务保管，调用方取消等待不遗弃。确认丢失时保持租约并GetStatus核实版本及有效值，不猜Applied／Rejected，不自动重发修改命令，不建立无限重试。

Windows端口只传递消息，原操作能否等待须按真实hook确认；不使用FltSendMessage无限等待替代AS原期限。[官方返回语义](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fltkernel/nf-fltkernel-fltsendmessage)中的STATUS_TIMEOUT属于NT_SUCCESS范围，仍须判断实际投递结果。未知平台动作、身份或字段不得降级成观察、Root信任或另一协议。


## 8. 规则命中来源与版本

`RuleSourceVersion = Configuration(DriverConfigVersion) | RulesDocument(RuleVersion)`，一个带标签字段同时表达来源和版本。文件自保系统组的命中指向配置投影版本，普通规则命中指向规则文档版本，不能把二者都塞进一个名为rule_version的字段。

统一RuleHit含source:RuleSourceVersion、group_id／group_number、rule_id／rule_number、log及该命中版本的deny_message；文本来源及缺失语义只在[固定拒绝提示](../../backend/rules/technical.md#固定拒绝提示)定义；来源由source标签读取，不另存一份可冲突的origin；类别来自原事件。运行时DriverVersionStamp另外携带persistent_generation、state_revision、DriverConfigVersion和RuleVersion。询问只来自普通规则，不能由自保配置组制造弹窗。

配置／规则模块分别保管候选及仍有消费者的不可变版本，主模块只保管公开租约，不建立第二份历史库。控制状态上报以驱动实例及state_revision区分新旧；断线时只能上报连接不可用，最近快照必须明确标为历史观察，不能冒充当前值。

当前主模块消息实现位于[as-main/messages](../../../crates/backend/as-main/src/messages)，没有独立as-message业务所有者；本机收发／认证复用现有薄适配，不再列为未来迁移任务。


`AppliedPersistentState { persistent_generation, status:DriverStatus }`是实际采用回执，已采用的两个摘要从同一status读取，不再重复同义字段；不代表整个AS配置或所有模块都已重载。来源ConfigVersion用于追踪导出，不与驱动投影版本混用。

`FeatureStatus { startup_default, runtime_override, effective_state }`中的effective_state为Enabled／Disabled／Unavailable，来自驱动实际执行。断线时底层另报连接不可用，历史确认状态不能作为当前值。普通类别暂停不卸载文件自保；整个AS停止／驱动卸载属于明确的服务／安装生命周期，卸载后不得声称仍有自保。


`NativeDeadline`是所属驱动会话的原生单调时间值，由OriginalOperation平台适配转换和校验；业务只使用HoldWindow，中间层不能传入内核绝对时间或续期。Linux保管窗口使用CLOCK_MONOTONIC／ktime_get_ns；事件源时点及进程birth继续使用BOOTTIME，不能混用。Windows原生计时转换尚未实现，不按Linux或墙钟解释。时间语义不能由统一消息头推定。

底层模块通过[MainContext](../main-context.md)和具体DTO／窄接口参数协作；内核驱动只消费本协议数据并维护私有DriverContext，功能边界按[统一边界](../../architecture.md)。未明确的实现细节只列[存档](../../archive/unresolved-module-contracts.md)，不追加当前开发／验收门槛。

驱动内部通讯／消息／配置／匹配子类的调用关系见[驱动架构](../../drivers/architecture.md#内部子类与组合)；DriverInitParts只在[底层适配](../../backend/driver-adapter/technical.md)定义，私有有效状态只在[DriverCore](../../drivers/technical.md#有效状态与原子提交)定义，原操作键只在[DriverOperations](../../drivers/linux/operations.md#原请求记录与状态)定义。本页继续独占线上封套、镜像描述及命令类型，不把原生I/O拆分变成第二协议或第二个MainMessages。

本协议是驱动和底层的唯一边界。底层MainContext与内核DriverContext均不进入报文；各自结构分别由所属模块独立维护。

基础进程观察、固定身份核对与事实栅栏通过本页当前消息及同一鉴权连接实现。它们只供[进程树模块](../../backend/process-tree/technical.md)读取固定事实，不赋予规则发布、任意/proc读取或新的进程执行权限。

### 指定进程工具命令

KillProcess编号16，沿同一鉴权会话和request_id，不分片。正文为32字节as_process_target：offset0为实际DriverStatus.instance[16]，16为宿主TGID:u32，20为reserved:u32=0，24为真实birth_boottime_ns:u64。复用DriverProcessTarget及既有ProcessBinding的身份字段，禁止传FD或指针；目标与结果语义唯一见[DriverTools](../../drivers/linux/tools.md)。Hello features中64 ProcessKill对应此显式工具，4／8分别对应规则Actor／Parent终止；两种调用都复用DriverTools，授权与身份输入各自明确。

成功Reply的as_reply后固定8字节：outcome:u32（0 SignalSent、1 NotRunning）及reserved:u32=0；错误仍只含as_reply。旧驱动实例／不符birth为-ESTALE；权限为-EPERM／-EACCES；非法输入为-EINVAL；停止接纳为-ESHUTDOWN。工具在Running接纳后预占原Reply容量，锁外发送；不持core_lock等信号或进程退出，control_writes保管到回执入队，Quiesce排空包含该写任务。不存在自动重试或用户态kill替代。

## Flow入口与原生请求观察

当前受控TLS入口由DriverSession.start_tls_ingress提供已核连接事实和同一原目标的UpstreamConnect能力，Main仅组合，真实TLS／HTTP和凭据处理归用户态所属模块。入口来源与部署边界见[当前Flow来源](../../drivers/linux/flow-binding.md)和[Flow公共契约](../flow.md)，不保留旧TLS `_ex`采样或受管拓扑FD导入作为默认实现。

原生库请求观察已使用本页InstallRuntime和0x10c，结构、部分读取、有效寿命与缺口只在[原生请求观察契约](../../backend/driver-adapter/runtime-observation-technical.md)定义。它产生原事实和公开消息候选，不自动成为可信工具执行交接；MCP未接入，通用工具执行来源仍单列缺口。这些缺口不阻止已有来源的普通HIPS或原始观察。

## 固定拒绝反馈的目标编码

当前40字节as_rule的offset36为report_denial:u32，小端，仅0或1；其他字段、40字节大小和原条件布局不变。由底层规则模块根据Driver Deny且原文非空派生，非Deny必须0；驱动采用时验证，不从事件或进程身份推定。

该标记只使实际Denied沿现有AS_DIRECT报告，log独立。as_hit不加入文本；仍使用原命中身份／版本，由底层规则模块在原PolicyLease查询文本。当前生产者、驱动校验及消费者已同时替换，不增加迭代版本号、旧字段别名或兼容回退。编译和实际加载／生效分别以证据为准。

## 写关闭审计事实

`features`的`FileCloseAudit=256`声明当前Native实现有写关闭采集能力；底层仍须由原加载器核实完整BPF集合实际附着后采用配置。此特性不加入普通操作掩码或规则动作。

事件`0x10b`使用同一认证会话和普通FIFO，一帧完成，不传FD／内核指针。正文固定32字节`as_file_close`，随后是path字节：offset0 reporter_tid:i32，4 device_major:u32，8 device_minor:u32，12 path_error:i32，16 inode:u64，24 path_bytes:u32，28 reserved:u32=0，全部小端。path_error=0表示读取成功，path即随后path_bytes字节（可为空）；真实负errno表示路径未取得，path_bytes为0。不能用长度0替代读取状态。映射到[公共审计记录](../security-audit.md)，不生成第二套事件DTO。

回调只持原path引用及真实回调标量，禁止复活f_count已归零的file。路径在AS设备读取上下文取得，不在可能fs为空的释放工作线程调用d_path。对象在普通队列和实际读取／释放的原在途寿命内保管，释放沿原release_queue，不重新打开路径、不新增传输或日志所有者。
