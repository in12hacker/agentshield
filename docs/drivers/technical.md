# 驱动技术契约

所属大模块：内核驱动（BPF-LSM入口＋一个可加载原生支撑模块）。产品目标见[设计](design.md)，职责与依赖见[架构](architecture.md)，源码依据见[调研](references.md)。底层Rust适配接口单独见[driver-adapter](../backend/driver-adapter/technical.md)；两者只通过[现行通信协议](../contracts/driver-communication/technical.md)交换数据，不互传上下文或实现对象。

## 唯一入口与子类文档

DriverCore是原生驱动内部已有入口／状态编排职责，不是新增守护服务。它组合以下私有子类，细节分别由对应文档独占，其他页面只引用：

- [DriverContext](linux/context.md)：驱动自己定义的只读实例／会话／能力上下文。
- [DriverHooks](linux/hooks.md)：三类安全回调与必需生命周期、管道hook的真实参数和能力边界。
- [DriverRuleMatcher](linux/rules.md)：同类驱动规则匹配和三个派生，只读视图。
- [DriverSelfProtection](linux/self-protection.md)：真实自保对象／服务实例、修改判定及冷恢复维护边界。
- [DriverConfiguration](linux/configuration.md)：配置投影及临时覆盖输入的语义校验。
- [DriverTransport](linux/transport.md)：受保护原生端点、实际连接身份与帧I/O。
- [DriverMessages](linux/messages.md)：现行协议结构校验、重组、编码与分派。
- [DriverOperations](linux/operations.md)：原请求／对象／期限保管、合法等待、一次性收尾。

子类是代码职责边界，不强制每个类一条线程或一个编译单元；只有DriverCore接线并提交实际状态，叶子单元不查彼此全局实现。驱动不实现配置文件读写、用户态规则编译、进程树、UI／IM或MCP解析。

## 已有数据与跨边界复用

事件继续使用同一个公共头、公共进程信息和进程／文件／网络载荷，唯一字段／TLV定义见[统一事件](../contracts/events.md)；底层表示为[EventData](../../crates/contracts/as-types/src/dispatch_event.rs)。驱动通过hook参数、真实任务上下文及内核公开API主动采集，公共函数与锁/寿命要求见[回调内主动事实采集](linux/hooks.md#回调内主动事实采集)。捕获真实可得信息，缺失留空，不能新增驱动专用进程模型、重复actor或以PID重建执行能力。

Decision、Termination、RuleHit与RuleSourceVersion沿用既有含义；普通规则字段及DriverMatch语义以[规则契约](../backend/rules/technical.md)和[公共执行契约](../../crates/contracts/as-types/src/main_ports.rs)为准。Rust的共享类型是底层表示，内核按同一协议／镜像含义使用原生视图，不能导入as-main／as-policy-engine或直接拷贝Rust内存布局。

配置输入只来自[DriverConfigImage](../backend/configuration/technical.md#4-新增结构及模块接口)，普通规则输入只来自[DriverPolicyImage](../backend/rules/technical.md#普通规则的独立源与下发结构)。ConfigVersion、DriverConfigVersion、RuleVersion不互换；命中来源和DriverVersionStamp只按[通信契约](../contracts/driver-communication/technical.md#8-规则命中来源与版本)。驱动核验自身原生对象，不读取底层文件或重新发现自保根。

## 有效状态与原子提交

```text
DriverPersistentSnapshot {
    configuration: DriverConfigImage的已验证原生只读视图,
    policy: DriverPolicyImage的已验证原生只读视图,
    pools: DriverRulePools,
}

DriverRuntimeState {
    source_state: SourceState,
    persistent: Option<DriverPersistentSnapshot>,
    persistent_generation: u64,
    state_revision: u64,
    overrides: DriverRuntimeOverrides,
}
```

这两项为驱动私有逻辑结构，不是新wire DTO。DriverRulePools只在[匹配文档](linux/rules.md)定义，DriverRuntimeOverrides只在[配置文档](linux/configuration.md)定义。当前字节布局、条件指令和尺寸断言见唯一原生声明[as_protocol.h](../../drivers/linux/as_protocol.h)，底层按同一固定LE编码；未定义能力仍留存档。

DriverCore独占实际状态和提交入口。消息只交结构完整的请求；配置子类只验证配置／临时命令语义；匹配子类只准备并读取规则视图；二者都不独立发布active。全部候选验证完成后，DriverCore重新检查预期持久代和来源状态，短临界区一次采用配置与规则。停止／故障与提交竞争同一实际状态，失败保留原有效版本，不自动回滚磁盘或恢复Running。

初始化采用默认并清除临时覆盖；普通持久替换保留提交时的实际覆盖；Set／Resume不写盘。状态、版本递增与文件自保先于普通文件开关仅按[运行状态契约](../contracts/driver-communication/technical.md#4-运行时与持久状态分开)。DriverStatus从唯一实际状态和原操作／消息真实计数生成，只读输出不反向修改执法。

有效快照在仍有hook或在途原请求使用时保留，不因更新提前释放。当前以core_lock短临界区取得／发布快照，以refcount_t保管hook和原请求引用；最后一个真实使用者先按同一事件FIFO发布0x105 PersistentRetired，再异步释放对象。候选未采用不发退役通知，不以时间或保留全部历史代替真实寿命。

## 身份跟踪与规则维护

DriverCore把身份准备与实际Allow分开组合：exec hook先调用Self的`as_self_prepare_exec`建立必要pending候选，随后仍执行普通Process Driver匹配；真实exec成功／exit负责完成和回收角色。身份步骤不直接允许执行，不按UID、进程名或fork继承形成角色。

进程、文件、网络维护允许来自规则模块实际持久化并下发的同源PolicyDocument／DriverImage；source和target维护条件分别是独立规则行，不构造跨池规则。DriverCore不保留`maintenance(file,read/exec)`、proc／sysfs／BTF／pins读白名单，也不增加controller网络旁路。外部actor启动根内程序先经过FileOpen，故文件目标维护Open与进程目标Execute都须真实存在于规则数据。

FileOpen先执行真实自保修改准入：负值立即拒绝，`0`／`+1`继续File Driver池，最终允许来自规则。其他尚无普通可等待规则的修改hook仍只承担其自保职责，不能把Open规则覆盖推定成所有修改动作都已支持。实际dev_t控制端点识别及device_open的真实身份／唯一会话鉴权保留通信职责，普通规则Allow不能授予控制权。细节仅由[Self职责](linux/self-protection.md)与[规则契约](../backend/rules/technical.md)各自维护。

### 统一File gate流程

操作范围只见[产品定义](design.md#普通文件模块范围)，真实入口和锁只见[Hooks](linux/hooks.md#普通文件gate接线)，字段只见[事件契约](../contracts/events.md#普通文件gate字段与操作来源)。源码已接九项普通 File 操作及对应 Hello 和镜像采用约束。源码、构建和实际保护效果分别验证。

DriverCore在一次gate内复用同一已采用快照引用、版本和实际开关，先执行必要Self修改准入，再决定是否需要普通File事实／规则。初始化fence另持原真实引用，不能被普通文件开关／Allow绕过，也不冒充已采用RuleHit。自保没有拒绝且该操作确有普通规则消费者时，才沿原有界capture与File池匹配；没有消费者不新增通知或分配一份通用事件。

Open只在实际FMODE_WRITE或O_TRUNC时执行现有Self修改准入；只读打开不新增修改拒绝。其他目标操作按其真实修改语义判断；Self负值立即返回，`0`／`+1`继续普通池。相同回调参数供Self拒绝事实和普通事件捕获复用，不在两个wrapper重复构造事件；自保日志采集失败不改变原拒绝，成功维护不构造允许通知。

Allow／Deny仍在原hook返回；ToUser沿同一DriverOperations记录和答复链，不新开文件等待队列。原单调起点涵盖本次gate事实采样、匹配和排队，采用更早的原生／规则期限，不在分流或弹窗时重启计时。取消、信号中断、来源故障和停止仍只封住同一原请求一次；在已取得终止权限的Deny上复用原DriverTools绑定actor／parent，分别保真实信号结果，不先释放原引用。

File gate不保存回放路径或全局当前事件。hook栈持有的原file／dentry／mount参数只在本次调用中借用；需跨本次作用域使用的引用必须显式取得并由原操作真实释放。事件传给底层的只是采样值，不传指针，不在答复后重新按路径打开或重放操作。快照／持久代和原消息在既有refcount／FIFO退役，临界区内不等人、不读盘；其余寿命只引用[Operations](linux/operations.md)，不复制第二个状态机。

原生捕获、kfunc 和加载清单提供当前普通 File 操作，Hello 与规则镜像采用校验同一能力。Rust 转码、同源维护和事件存储按各自契约组合。普通Driver与Backend按真实操作及可供字段核验；字段存在不等于新增匹配DSL。旧验收只覆盖其列明源码／产物，不作为新入口已经生效的证据。

## 受控请求与事实通知

驱动快规则命中 Allow／Deny 时在原 hook 裁决。命中 ToUser，或快规则未命中但仍有后端候选时，驱动保管原请求并在原执行点等待。普通规则路径没有候选时默认放行，不产生 RuleHit。详细分流、故障语义与收尾分别见[驱动规则](linux/rules.md)和[原操作](linux/operations.md)。

进程创建成功／exec成功／组退出属于Observation，只发送事实；文件创建准入不是该进程事实。直接规则结果、受控请求、原操作结果、实际状态分别使用各自协议类型。不制造Ask／RuleHit或借通知构造第二个可执行操作。只有真实可等待的执行点才进入ToUser；管道消息保管缺口不能用观测或整条管道封禁冒充解决。

## 初始化、资源与退出

底层先读取／校验配置及规则，再由[底层驱动适配](../backend/driver-adapter/technical.md)加载、核验本机模块／BPF／端点并建立连接。驱动原生端点验证实际连接主体，DriverCore生成DriverContext并通过Hello报告实际身份／能力。底层取得镜像后提交InitializeDriver，驱动完整采用并确认，底层才宣布业务就绪。失败不以观察或旧协议替代。

驱动停止新接纳后保留原答复和回执并实际排空；底层再停止自身接收／线程并释放自己取得的FD／link。健康Quiesce后实际控制FD释放会摘下active并进入Dormant，原生引用安全自然退役；异常失联的Failed仍保留直接策略和自保。持久generation与停止epoch不重置。驱动自己的原生引用和等待者全部退役后才可由认可的安装生命周期卸载；不关闭外部Root broker，不卸载发行版既有LSM。

AS内部完成答复和启动闭包由真实维护规则与现有端点身份分别承担：规则解决普通进程／文件／网络允许，原生对象和exec生命周期解决控制／自保权限；两者不互相冒充。通讯失败、前端离线、已绑定Ask断线和原请求取消按各自契约分开处理。

## 日志、进程树与流量边界

驱动输出真实RuleHit、事件和结果，底层复用RuleLogRequest／MainMessage交第三线程日志消费者；log=true只控制规则日志，不停止基础生命周期事实。驱动不写业务日志文件、不等存储、不把发送成功当刷盘。

进程树算法、存量快照和丢失修复归底层事实模块。驱动为受控 TLS 出口提供实际连接身份与网络导向，明文由底层 Flow 的中介连接取得。LLM 解析、聊天推送和 TLS 中介修改均归 Flow，幻影 key 授权归凭证模块。被动明文采样仍未实现，MCP 暂缓。见[TLS 来源](linux/flow-binding.md)与[存档](../archive/unresolved-module-contracts.md)。

## 当前源码与能力范围

[as_core.c](../../drivers/linux/as_core.c)实现唯一状态、原生设备、当前协议、三类只读规则视图与原操作；[as_hooks.bpf.c](../../drivers/linux/as_hooks.bpf.c)实现 12个普通受控入口、16个自保入口及1个写关闭事实入口；[as_self_protection.c](../../drivers/linux/as_self_protection.c)独占真实自保对象和exec实例绑定。原生线格式只保留[as_protocol.h](../../drivers/linux/as_protocol.h)，旧原生头和旧解码已经删除。[as_tools.c](../../drivers/linux/as_tools.c)收口无状态进程查询和指定身份终止，接口与边界只在[DriverTools](linux/tools.md)定义。用户态加载器属于底层适配，不能把它算成内核子类。

普通可等待控制源码包括 Process.Execute、九项 File 操作和 Network.Connect／Send；进程创建成功／exec成功／最终组退出是原生tracepoint事实。其余自保动作只产生系统配置组直接裁决。Deny 的 Actor／ActorAndParent 范围复用[原操作收尾](linux/operations.md#终止与释放)。原生规则池不接收容器实例条件，已核实的 Docker 实例事实及范围匹配见[底层适配](../backend/driver-adapter/technical.md#容器实例绑定)。管道消息控制未实现；镜像请求不支持的能力时整体拒绝采用。实际hook及结果边界见[DriverHooks](linux/hooks.md)。

部署保留用户发行版内核，Linux x86_64／aarch64、deb／rpm及一个匹配运行内核的DKMS支撑模块。BPF-LSM／BTF／hook／kfunc、签名与实际加载条件如实核验；不更换内核、不自动登记密钥、关闭Secure Boot／Lockdown、改启动参数或重启。构建成功只证明构建；真实加载、原操作等待、自保和冷恢复分别由当前产物联调核实。

DKMS 以实际目标内核发布号和该内核构建目录调用同一模块构建脚本。具备目标 `vmlinux` 时直接复用 Kbuild 的 BTF 步骤。运行内核的 headers 缺完整 ELF 时才使用其 sysfs BTF。未启动内核缺目标 BTF 时停止构建。参考 [DKMS 的目标内核变量](https://github.com/dkms-project/dkms/blob/main/dkms.8.in)和 [Linux 7.0 Kbuild 模块最终链接](https://github.com/torvalds/linux/blob/v7.0/scripts/Makefile.modfinal)。具体入口见[构建说明](../../drivers/linux/README.md#构建)。

`build.sh all` 是 Native 模块、BPF 对象和两个静态加载器的唯一构建入口。隔离打包消费调用者指定的同一冻结输出目录，按各自场景选择产物并按原字节复制。测试探针和场景宿主由各自打包入口构建。产物与提交及目标内核的对应关系由冻结证据校验。

规则终止授权归DriverPersistentSnapshot：采用完整镜像时持有get_current_cred引用，最后快照释放时put_cred；不保存为另一份可变DriverContext或全局权限缓存。原操作使用同一快照的authority，工具窄范围执行后还原调用线程凭证。
