# 驱动协议消息

所属大模块：内核驱动。产品边界见[驱动设计](../design.md)，组合和职责见[驱动架构](../architecture.md)，唯一入口见[驱动总契约](../technical.md)。驱动内部只读上下文见[DriverContext](context.md)，跨模块协议见[通信契约](../../contracts/driver-communication/technical.md)。本页定义本职责的唯一技术细节；不导入底层实现。

## 唯一职责

DriverMessages是驱动端的现行协议校验／编码与分派单元，输入为DriverTransport交来的完整帧和DriverContext。只核对封套、会话、类型、长度、分片、请求关联及命令正文的结构完整性；经DriverCore组合对应配置、匹配和原操作接口。它不校验YAML、不编译规则，也不直接改有效状态。

底层MainMessages拥有中间层和驱动两个调用端、CommandId关联及底层请求等待者。驱动消息类不导入as-main，不接收前端角色，不实现Ask UI或IM认证；它只是本机协议的另一端，不是第二个底层消息服务。

## 类型与ID

封套和命令／事件编号只在[统一通信契约](../../contracts/driver-communication/technical.md)定义。底层现有DriverFrame／FrameClass是该协议的用户态表示；原生端按相同固定线格式实现，不把Rust Vec或enum内存布局当C布局，只使用[as_protocol.h](../../../drivers/linux/as_protocol.h)中的当前线格式和编译期尺寸断言。

request_id由底层调用端在会话内分配，驱动原样回显；事件使用0。original_id由DriverOperations为原请求分配，CommandId只留底层。DriverMessages不为同一次操作建立第二种业务ID。

## 分片与状态提交

驱动端每会话下行只有一个未完成的镜像大消息，遵守[统一封套](../../contracts/driver-communication/technical.md#5-唯一消息封套与大buffer)的顺序分片契约。候选重组／长度／hash校验仅由本接收端实际保管；底层发送不可变buffer的分片，下行不另建一份Rust重组状态。上行Group 0x10a以同一封套的连续片在原普通队列原子接纳；复制、额度、接收泵唯一重组和失败边界直接引用统一封套，不另造传输。配置语义交DriverConfiguration，规则表示／条件能力校验交DriverRuleMatcher，最终只有DriverCore一次提交。

候选未完整时没有匹配效力；片段之间原答复、取消、状态和临时开关仍可处理。Abort只撤未开始采用的暂存，不撤已经完成的驱动行为。显式停止／故障使在途大消息失败，一次逻辑请求只给一份结果；后续同ID在途片段不再重复回复。首片和提交核对lifecycle_epoch，候选不在Quiesce之后复活来源。分片错误或断线不自动重试或覆盖旧有效状态。

## 上抛

需要底层裁决时编码真实受控事件；实际操作完成编码同一结果；直接规则命中保留实际RuleHit；状态只取DriverCore；生命周期事实用0x104 Observation；最后一个真实使用者结束后用0x105 PersistentRetired报告已采用持久代的退役；0x106报告事实采集完成标记；0x107报告直决计数批。消息类不制造第二份事件模型、虚构命中、完成回执或MCP数据DTO。

字节发布快速完成后，等待仍由DriverOperations在原合法执行路径承担。消息收发循环不得代替原任务等待或被人工问询占住，普通日志交底层日志消费者。

## 投递与真实退代

Controlled与AS_RESULT、命令Reply沿原端点优先列表交付；交付额度、分类容量、poll及普通FIFO的唯一边界见[通讯契约](transport.md#缓冲与临界区)，本页不另定义队列归属。Controlled实际读出后才发布其AS_RESULT，不让结果先于请求建立。Direct、状态、观察、退代、事实栅栏、计数及后续原始事实仍沿普通FIFO。旧快照的退代记录在其最后一次原生引用释放时排队；未采用候选没有退代事件。底层据实际退代释放同代配置／规则租约，不以时间猜测。Quiesce须排空实际普通FIFO及原操作寿命，不因新事实类型没有列入旧计数谓词而丢弃它。

StartFacts、InspectBatch、FactsBarrier使用同一请求关联和会话校验。编号及线格式只在[通信契约](../../contracts/driver-communication/technical.md#基础进程事实的线格式)定义。标记Reply只确认受理，实际完成必须收到0x106且经过底层快速事实FIFO；标记预先保留，不混入可丢弃的普通日志容量。DriverMessages不等待用户答复来完成事实栅栏，也不把它变成全系统冻结或另一套业务总线。

KillProcess使用现行编号16和固定目标结构，由Core核会话／实例、Running接纳及回执容量后调用[DriverTools](tools.md)。消息类不自行寻进程、不发送信号，不把信号接纳包装成OperationResult或真实退出。
