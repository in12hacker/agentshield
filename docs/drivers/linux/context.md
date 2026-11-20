# 驱动私有上下文

所属大模块：内核驱动。产品边界见[驱动设计](../design.md)，组合和职责见[驱动架构](../architecture.md)，唯一入口见[驱动总契约](../technical.md)。驱动内部只读上下文见[DriverContext](context.md)，跨模块协议见[通信契约](../../contracts/driver-communication/technical.md)。本页定义本职责的唯一技术细节；不导入底层实现。

## 定义与来源

DriverContext只属于驱动大模块，由驱动独立定义和维护。本页是唯一结构出处；它不复用、继承、嵌套或保存底层MainContext，不复制底层配置缓存或路径数据，也不是跨边界传输DTO。

```text
DriverContext {
    driver_instance: [u8;16],
    session_id: [u8;16],
}
```

- driver_instance由实际驱动实例产生；session_id由驱动为已通过原生端点鉴权的唯一控制连接产生。两者不从PID或时间推定，也不假定与事件source_instance相等。
平台编码与能力由本机实现固定生成，经Hello输出，不在上下文中另存一份可写副本。能力只表示实现支持的操作／字段／动作与资源上限；实际附着由底层加载器逐项核实，当前生效状态另外通过DriverStatus取得。

原生端点认证通过后，DriverCore创建该会话的不可变上下文，再由Hello输出协议所需信息。底层只接收DriverInfo和状态数据，不创建或保管一份DriverContext，也不取得其内核指针。

## 消费者和寿命

DriverCore组合配置验证、匹配视图、消息校验和原操作，按参数传本次会话身份与必要数据；配置和有效快照不塞进上下文。连接／驱动实例更换时核验并创建新上下文，旧原请求仍绑定旧会话，不跨会话续接。

普通重载、临时开关、同会话完整持久初始化都不重新创建上下文；它们修改的是真实运行状态。实际能力因驱动更换而变化时，先走原会话排空和重新核验，不能就地修改readonly上下文。

## 明确不放入的内容

配置文件路径、自保根定位、整个ConfigDocument、有效规则、临时覆盖、FD／HANDLE、加载器、全局当前事件、原请求、MCP字节和消息队列都不进入DriverContext。有效状态由DriverCore保管，原请求由DriverOperations保管，发送／接收资源由DriverTransport保管，通过接口参数协作。

底层初始化数据及其平台资源只按[底层驱动适配](../../backend/driver-adapter/technical.md)传到本机连接边界；驱动之后只消费[协议](../../contracts/driver-communication/technical.md)中的数据。上下文不是服务定位器、注册表、总线或第二个状态Owner。

## 运行时观察资源的私有寿命

`as_runtime.c`只保管DriverCore已鉴权采用的实际ELF探点、目标进程/mm/executable引用及原req/work在途实例；这些不进入DriverContext或MainContext。安装按真实birth、self_exec_id、inode/device和目标mm核对，缺工具布局不阻libuv原观察。重复同一实际计划保留已注册探点，失败不冒充安装成功。唯一计划、raw字段及字节结构见[运行时观察契约](../../backend/driver-adapter/runtime-observation-technical.md)。

worker关联只借当前线程的原回调scope；syscall入口复核内核公开uret实例的consumer id、实际scope cookie及当前栈边界。未arm、已清理或关联不足时不产生该worker到syscall的关联，原入口/工作观察仍独立发送。依据为Linux的[实际uret接纳与清理](https://github.com/torvalds/linux/blob/v7.0/kernel/events/uprobes.c#L2099)和对应公开headers，不能把entry handler已执行冒充return session已经建立，也不把此原始交接冒作ToolCall语义。原worker return归还scope，done只读当次实际result或errno，cleanup结束req索引；进程exec或真实最后线程退出退役该映像，普通线程退出只清自身scope。未观察cleanup的索引仅保在原有界在途容器，不以时间推定结束；容量不足仅令关联缺失，不阻原件采集。

仅在本次入口实际匹配active profile的mm／TGID／birth／exec，且成功建立自有scope、保管线程及已取得的request引用并登记非零cookie后，entry handler才返回0，向内核申请本consumer的ret_handler。未匹配profile、scope分配失败、退役复核失败、cookie耗尽或只有入口观察而没有自有返回scope的路径，统一返回官方`UPROBE_HANDLER_IGNORE`；不能返回0并以空cookie伪保管返回会话。scope建立的结果不以request或工具字段齐全为条件，已经取得的入口原件仍独立保留。该返回值只说明本consumer申请返回观察，内核是否实际建立会话仍按前述原uret实例核验；退役注销及同步完成后才释放consumer及映像引用。Linux的[handler返回值契约](https://github.com/torvalds/linux/blob/v7.0/include/linux/uprobes.h)与[handler_chain](https://github.com/torvalds/linux/blob/v7.0/kernel/events/uprobes.c)负责实际返回跳板，不由本模块改写用户寄存器、返回地址或模拟退出。

事实期可安装探点，不等待Running；普通完整Initialize保持已核实际profile。source真实改变只失效旧request引用，实际活req/work继续保管至其终态，旧原件保留原source，不向新source伪造旧key。Quiesce封新采集，已分配的同hook消息和已入普通FIFO的raw记录分别沿原在途/排空计数完成；回复不提前越过这些原件。退役在原生命周期锁区移除本代资源，锁外复用既有release_queue注销uprobes并释放引用，不由旧reader关闭误清新会话资源。
