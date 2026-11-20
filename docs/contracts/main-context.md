# 通用MainContext

模块职责见[边界](../architecture.md)。MainContext是主模块组合并传入接口的只读公共数据，不是模块实现入口或另一份配置。

## 初始化顺序与归属

配置模块是启动第一步，initialize不接收MainContext或外部目录参数，避免“先要上下文才能定位配置”的循环依赖。它内部一次查询真实后端位置，派生并缓存公共路径。

主模块在配置初始化成功后，借用其同一份路径引用并加入编译目标平台，构造一个AS实例内的MainContext；随后传给需要公共数据的底层模块初始化接口。底层初始化驱动适配时只组合必要参数，MainContext不传入内核驱动；配置reload和底层重新连接仍复用原路径，不再次发现目录。

MainContext和公共路径数据声明位于共享契约层（as-types），构造／组合职责属于主模块，路径生成与缓存职责只属于配置模块。其他功能模块不因此导入as-main或as-runtime-config实现。

## 数据结构

```text
MainContext {
    paths: Arc<ConfigPaths>,
    platform: TargetPlatform,
}

TargetPlatform = Linux | Windows
```

- paths是配置模块给出的同一Arc，不复制路径内容，不拼接自保根，不从上下文创建第二份路径缓存。ConfigPaths字段、只读构造约束和含义以[配置契约](../backend/configuration/technical.md#1-配置模块私有定位与configpaths)为唯一来源。
- platform是本次构建目标的常量标识，由主模块组合一次，不额外执行安装位置或机器环境探测，用于明确本机输入格式；不是驱动能力报告、身份认证或可编辑配置。
- 对象不提供setter。初始化接口可借用`&MainContext`；确需保管共同路径的模块保存同一Arc引用，不保存一个可独立变化的副本。实例退出后随实际消费者释放。

配置正文、实时驱动状态、临时开关、当前PID／容器、EventData、原操作和版本租约各有所属模块／请求寿命，不进入这个常驻对象。需要它们时由对应接口参数显式传入。

## 接口组合示意

这段仅展示接口调用次序，不重新定义签名或返回类型。各接口以所属文档为准：[配置接口](../backend/configuration/technical.md#4-新增结构及模块接口)、[规则接口](../backend/rules/technical.md#普通规则的独立源与下发结构)、[底层驱动适配接口](../backend/driver-adapter/technical.md#平台接口边界)；完整启动约束见[初始化编排](driver-communication/architecture.md#初始化编排)，采用回执见[驱动数据契约](driver-communication/technical.md#8-规则命中来源与版本)。

```text
config = ConfigModule::initialize()
context = MainContext::new(config.shared_paths(), TargetPlatform::current())
(interaction, prompts) = Interaction::channel(config.snapshot().document().queues.review)
rules = PolicyEngine::initialize(&context, interaction)
tree = ProcessTree::prepare(配置中已验证的process_tree设置)
driver_input = 由主模块从既有路径、平台及tree.boot_id()组合DriverConnectInput
driver = DriverAdapter::connect(driver_input)
info = driver.info()
config_image = config.prepare_driver_config(info.capabilities)
(policy_image, policy_lease) = rules.prepare_driver_policy(info.capabilities)
parts = DriverInitParts(info.status的generation与lifecycle_epoch, config_image, policy_image)
主模块保管候选policy_lease，并接好三条执行职责与原生事件桥
tracking = driver.start_process_tracking(event_ingress)
tree.initialize(tracking, driver的窄事实端口)
applied = driver.initialize(parts)
```

示意省略错误处理；实际仍须先接好接收队列、候选选版和清理。驱动负责按传入数据绑定实际原生对象，未明确的绑定细节见存档，不由主模块臆造输入。共享上下文不会改变这些初始化依赖。

规则模块从context.paths读取现有rules_file绑定，不主动调用配置模块。底层驱动适配通过初始化参数取得已认可资源，不主动调用规则模块。内核驱动只接收协议镜像并自行验证实际原生对象；其[DriverContext](../drivers/linux/context.md)独立定义，不复用或嵌套本对象。主模块把接口结果交给下一接口，模块调用之间传递数据而非实现对象。

## 每次调用仍有独立输入

事件处理继续传EventData和同一OriginalOperation；匹配结果、RuleHit、租约及回执沿当前请求传递，不能放进共享“当前事件”。管理命令的ID、版本及候选buffer由实际管理任务保管，不能覆盖其他并发请求。

模块专属初始化设置使用具体DTO传参；需要输出时传入公共契约中的窄接口或队列端口。MainContext本身不存模块实例、DriverSession、规则工厂、配置写入器或可任意查服务的map。

生命周期信号、上下文扩展字段及额外端口只有在具体调用者和寿命明确后才加入；未明确部分先[存档](../archive/unresolved-module-contracts.md#通用上下文扩展)。不为未来字段加空对象或永远返回成功的接口。

进程树的[当前设计](../backend/process-tree/technical.md)由主模块按具体参数传入配置和窄进程事实端口；只读上下文不增加树对象、进程map或驱动控制权。其初始化时序需要观察先开、系统扫描和结束栅栏，主模块实际完成这些步骤后才返回业务就绪。进程树不需要安装路径，prepare仅接收已验证设置，不为形式传入不用的MainContext。

## 代码入口

[MainAssembly](../../crates/backend/as-main/src/assembly.rs)首先初始化配置，借用同一Arc构造上下文，再初始化规则；start先准备拥有队列的执行线程和进程树，再以树的同一宿主boot_id连接实际驱动并接好同一事件桥，开启基础观察、扫描和消费栅栏，再下发两个模块导出的镜像。只有真实Running且自保Enabled才返回业务就绪；失败实际收尾，取消启动等待不遗弃所属资源。messages只创建并返回同一份已登记管理入口，take_prompts只转交一次真实接收端。

共享上下文的实现见[context.rs](../../crates/contracts/as-types/src/context.rs)。不把候选镜像、实时开关或未实现模块放入上下文。配置／规则导出、原生采用和管理命令均由当前真实所属接口连接，连接或编译通过本身不能被当作原生保护生效。

LLM联合设计不扩展MainContext为服务容器。Main按配置参数和窄端口组合运行时来源、FlowStatusPort、唯一Chat消费输出及凭证；Flow没有路径职责，不传无用MainContext，只有实际适配／存储初始化借用所需共享路径；设置只在[配置契约](../backend/configuration/technical.md#flow运行时配置)维护，不增加受管启动或默认代理端点。接口与初始化次序见[联合架构](../backend/flow/architecture.md#运行时接入与调度)。
