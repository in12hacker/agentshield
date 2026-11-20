# 驱动原生通讯端点

所属大模块：内核驱动。产品边界见[驱动设计](../design.md)，组合和职责见[驱动架构](../architecture.md)，唯一入口见[驱动总契约](../technical.md)。驱动内部只读上下文见[DriverContext](context.md)，跨模块协议见[通信契约](../../contracts/driver-communication/technical.md)。本页定义本职责的唯一技术细节；不导入底层实现。

## 唯一职责

DriverTransport在驱动端实现唯一受保护原生端点的open、read、write、poll和release语义，保管端点本身的读者缓冲及传输资源。Linux使用既定字符设备；Windows原生端口属于尚未实现的平台范围，不默认增加另一套备用通道。

原生open连接回调核验真实调用者和权限，并将核验结果交DriverCore创建会话／DriverContext；消息不能自报可信身份。之后的I/O必须属于该实际授权会话，控制能力不交给普通前端／IM。初始bootstrap要求CAP_SYS_ADMIN；已有自保绑定时还须通过真实backend exec实例核验。会话随后绑定真实TGID实例及随机session，不能靠进程名、同UID、裸PID或继承FD取得控制权。真实控制进程组退出或exec成其他程序立即使来源失效，迟到的旧FD关闭不能破坏新会话。

## 接口边界

向DriverCore提供的是实际一帧字节、已绑定的本机会话、I/O完成或真实连接失效；没有配置读取、规则匹配或MCP解析接口。DriverMessages负责封套与命令解析，DriverTransport不读取message_type执行业务，也不分配另一套request_id。

底层用户态的FD／HANDLE、BPF加载器与link属于[底层DriverSession](../../backend/driver-adapter/technical.md)，不在驱动原生端点再次保管。双方各管自己的原生资源，经协议连接；不共享对象或上下文。

## 缓冲与临界区

Linux端点为/dev/as_wait，0600；每次write提交一个完整帧，read返回一条完整记录，poll反映read真正能取得的同一队列来源。Hello成功回复是该会话第一条记录，握手前不混入旧观察或旧回执。read仅在短锁内取出完整记录，copy_to_user和可能的页故障在原请求全局锁外；实际读者保管缓冲，同FD并发读不能覆盖它。

同一端点的优先列表保管管理Reply、待交付的Controlled原请求以及其AS_RESULT；read先取该列表，之后才取普通FIFO。poll在优先列表非空时同时报告EPOLLPRI和普通可读位；普通FIFO自身只报告普通可读位。Reply计入reply_count，AS_RESULT计入receipt_count，Controlled由原held／max_pending保管，不占管理Reply预算。接收端先reserve_owned占据真实受控出口槽位，再以AS_CONTROL_CREDIT授予一条交付额度；reader仅保管这一条额度，尚无额度的notice留在原pending对象中。未读notice超期撤销时归还同一额度供下一个原件使用；实际read后额度由接收端消耗，不重复归还。Initialize／Replace不清除此认证reader内的许可，Quiesce仍允许既有原件领取交付额度；闲置许可不计入原件排空条件。实际read交付Controlled时才登记delivered，超时仍从其原列表撤回。AS_RESULT只更新已存在原请求的真实终态。Retired、Facts、Direct和普通观察仍保持普通FIFO顺序，辅助原件积压不得阻断新的原请求进入统一裁决入口。

普通FIFO的单条发布、原子批发布、出队和清空共用同一处分类计数；生产者只准备当前帧，不逐处维护容器占用。分类直接使用唯一线上type，命中批不再靠last_hit指针充当队列类型。批发布保留全批接纳和一次唤醒，出队即归还对应额度；取消、故障清空也走相同移除操作，不再独立归零另一份分类清单。全部使用原core_lock，不增加队列、状态Owner或消息接口。

用户态唯一接收泵沿原Critical保管尚未接纳的辅助必要原件；辅助背压时停止普通FIFO搬运，PRI仍读取已持有真实队列许可的Controlled以及Reply／AS_RESULT。Controlled通过DriverCallbacks.controlled交Main既有快速裁决入口；辅助事实通过events进入既有Messages线程消费。Controlled不在Critical另存队列，Native交付与其唯一OwnedPermit一一对应；出口满时不再授额，原操作仍由Native原pending和原期限保管。两个原队列均按真实容量接纳，不扩大容量、不新建Ask入口。退出收完两职责出口；适配细节见[底层接纳](../../backend/driver-adapter/technical.md#必要结果接纳与普通观测)。

StartFacts／InspectBatch／FactsBarrier沿同一设备和鉴权会话，查询容量不能占满原操作答复的保留容量；固定消息布局以[通信契约](../../contracts/driver-communication/technical.md#基础进程事实的线格式)为准。基础观察可以早于普通规则采用，Initialize不替换已连接的事实发送队列。原生标记之后，适配仍等待主模块快速事实FIFO处理确认，不在设备read线程等人工。

事实停止由当前reader内部栅栏保管，不只从普通执法状态推断。Quiesce即使发生于Dormant／Failed也关闭新观察；同会话已经接纳但迟到的StartFacts／FactsBarrier不能将其重新打开。显式完整Initialize只有在现有持久代／生命周期校验通过并成功采用后才清除此栅栏，不把普通重载、查询或新事实标记当恢复操作。

冷恢复的读取与执行通过同源[三池维护规则](../../backend/rules/technical.md)执行，不维护另一份proc／sysfs／BTF／pins读白名单。真实backend／loader exec身份准备及成功／退出跟踪仍归[Self](self-protection.md)，规则Allow不授予控制权；真正device_open继续执行原鉴权。

满额、无记录、帧太小和copy_to_user失败分别返回真实errno；已经取出的记录在用户拷贝失败后不重放，底层按会话I/O故障处理。控制失联使未决移交以来源错误结束，已采用直接Allow／Deny和系统自保继续，不在传输类擅自恢复Running或释放未结束原请求。大候选、答复与普通观察事实按既定容量分开处理，不新增工作线程或持全局锁等待人工。

新的控制连接必须等旧会话的原记录、快照引用、观察和控制写任务真实退役。记录容量令牌在kfree和snapshot_put之后释放；不是Held归零就允许新Hello。加载器意外退出或控制EOF不解除bpffs pins，只有显式正常排空后才释放核验过的当前实例links。

## 健康关闭与异常失联

健康Quiesce完成且无未决原操作、待结果或控制写后，实际控制FD的release在同一状态锁下摘下有效快照并进入Dormant，释放文件自保及临时覆盖；持久generation与停止epoch继续单调，不重置成初次启动。短direct／自保回调持原快照自然结束，不因关闭瞬间仍有短读者而把健康关闭改成Failed。active已经不存在时，DriverStatus不再宣称SelfProtection Enabled。正常停后下一次AS启动重新初始化；异常失联仍只能由真实exec实例恢复。

Quiesce的原完成回执不代表用户态已经接纳全部已读结果。停止原接收泵之前，适配必须将其Critical尾部交给仍存活的原消费者；消费者真实关闭则报告失败，不能丢弃必要结果后宣称正常。内核原操作／回执排空与用户态必要结果接纳各自完成，再释放实际I/O和加载器。

内核Quiesce必须等待既有普通FIFO实际为空，包含已入队Direct及原始事实，不仅等待Held和分类计数。新观测接纳及已接纳回调的界线由[原操作排空](operations.md#有界资源与排空)维护；底层不再增加“最后再读一次”的时序猜测或第二个缓冲。命令回执仍走原保留完成列表。

最后摘下的快照也持会话寿命令牌，锁外snapshot_put结束后才释放；新连接不能先行进入并收到旧会话Retired。原生对象清理仍在release_queue，不持core_lock。此前未完成的Held／回执或真实故障不能走健康退役：SourceFailed保留已采用直接规则与自保及精确冷恢复身份，不能通过Quiesce洗成正常。
