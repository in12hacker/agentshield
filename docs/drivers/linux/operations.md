# 驱动原操作保管

所属大模块：内核驱动。产品边界见[驱动设计](../design.md)，组合和职责见[驱动架构](../architecture.md)，唯一入口见[驱动总契约](../technical.md)。驱动内部只读上下文见[DriverContext](context.md)，跨模块协议见[通信契约](../../contracts/driver-communication/technical.md)。本页定义本职责的唯一技术细节；不导入底层实现。

原请求状态只有一个所有者：登记待决后等待；用户答复、规则期限、源取消或关闭竞争同一次收尾，胜者提交结果，迟到者只能取得已结束／无效状态，不能重新执行。必要的任务／文件／socket引用保留到原等待者、答复处理及结果投影都不再使用后释放。

登记／抢占／摘除只短持原请求锁，释放自有锁后使用wait_event或completion等合法等待原语；不能持自旋锁、禁抢占或跨任意内核临界区等人。还须确认调用者持锁不会被底层答复、日志或补信息反向依赖；可睡眠不代表任意长等待无死锁风险。原生完成对象寿命参考[completion规则](https://docs.kernel.org/scheduler/completion.html)。端点read缓冲、用户拷贝和并发读锁只由[通讯子类](transport.md)维护，原操作类不再实现一套I/O。

当前原生保管窗口由下发的DriverLimits.max_hold_ms生成，从原请求产生时开始并包含排队；这是现有AS原操作限制，不是操作系统网络I/O的天然截止，也不等同用户Ask规则期限。Ask当前取该窗口与规则期限较早者。实际执行者维护采用的期限及超时动作，界面不续命。现成LSM的原生超时／来源故障单独表达，不冒充已按用户规则执行。TOMOYO的全局timer重置机制不复制到AS。

按故障位置区分：

1. 后端正常而没有交互渠道：记住／快速规则照常；跳过慢查询和询问池，不生成询问规则命中或日志。
2. 已绑定Ask后全部交互渠道断开：立即提交该规则的超时动作。
3. 控制进程失联：采用真实LSM的来源失效处理，并报告来源不可用或原生回执；不把服务故障冒充前端断开后已执行规则超时动作。原请求失效后不得重放决定。
4. 原任务退出、请求取消或过期：不能再答复给复用PID／连接；报告原来源状态。
5. 接纳容量不足：未决请求由来源明确拒绝接纳并返回适合该hook的错误；普通审计丢失不能影响已经确定的直接裁决。

直接裁决先固定原Allow／Deny及终止范围。可选执行现场采集在原回调、实际信号尝试前完成；采集或消息发布失败只记录真实观测丢失，不覆盖原裁决、不跳过既定终止，也不发送不完整的Direct帧。受控请求尚未取得决定，其必要事件／结果的接纳仍按原契约核验；两者不混同。

不同hook的errno及被信号中断语义由其适配实现固定。源故障拒绝不是用户Deny规则命中，审计与界面要分开表达。原生等待不承诺任意无限等待，也不因用户态取消future就提前释放内核对象。

## 单次原生错误与通道失效

同一原操作的原生回执使用[ExecutionOutcome](../../../crates/contracts/as-types/src/operation.rs)：0保留Allowed，-EPERM／-EACCES保留Denied，-ECANCELED保留Cancelled；其他实际负errno保留为`Failed { native_error: i32 }`。native_error是Linux原来源返回的负值，例如-ETIMEDOUT为-110、-EIO为-5，不从errno推定发生阶段、调用方身份或整个来源失效。Unknown仍仅表示没有物理结果确认；单次错误不是Unknown，也不把原错误改为规则Deny或新的Allow。Allowed只确认该LSM检查未阻止，不保证后续系统调用成功。

合法Controlled结果先按原会话、original_id、generation与规则版本匹配，再形成同一OperationReceipt；`Failed`仍是已取得原回执的执行结果，不转成`ModuleError::Unavailable`。已有终止分项独立保留，不能因为某次操作失败而提交额外信号或重放决定。线上原生结果正文仍是已有i32结果码及原字段，不新增版本、stage字段或另一错误封套；公共类型中的原单位结果取值保持不变。

真实通道读取失败、EOF、会话／封套无效等才沿适配层现有来源失效路径处理尚未完成的Original；不能把一条合法结果的-110或-5当通道已经断开。当前Direct帧生产者只发布有真实命中的Allow／Deny，适配层也只接受这两种Direct结果；Direct内其他取值属于该帧类型的协议矛盾，不能从负errno补造RuleChoice。该限制不改变Controlled原结果对实际负errno的保留。

## 原请求记录与状态

驱动内部使用以下键关联原操作：

```text
OriginalKey {
    session_id: [u8;16],
    original_id: u64,
}
```

session_id取已认证实际驱动会话，original_id为该会话内驱动分配的非零原请求号且不复用。线上只使用现行封套中的session_id和原操作正文中的original_id，不另复制一份session字段。管理request_id、EventHeader的source_instance／sequence、上层DecisionId与这个键各有既定作用，禁止互相替代或用PID重建。OriginalKey留驱动内部；上层仍只保管OriginalOperation能力。

原生待决请求只保留`Held → Bound → Completing`三个实际可读取状态；无Ask绑定的直接用户态决定可从Held进入Completing。取消、期限、来源故障及用户回答竞争同一个Completing转移。执行者进入Completing后从待决表摘下，仍持原对象及容量到实际动作完成；将真实结果移交原完成队列后销毁原请求，不另存Finished请求或可查历史。Bound使用已有ReviewBinding的规则、期限和超时动作语义，原生边界把时间转换为NativeDeadline，不传Tokio Instant。

[as_core.c](../../../drivers/linux/as_core.c)在原hook调用栈保管实际执行点、不可变规则快照、物理父进程实例引用和有界记录；只使用当前[as_protocol.h](../../../drivers/linux/as_protocol.h)。原任务通过wait_event_interruptible_timeout等待，窗口使用CLOCK_MONOTONIC；事件时间与进程birth仍为CLOCK_BOOTTIME，不能混用。

## 上抛和等待的确定顺序

1. 驱动快规则Allow／Deny命中时在原hook直接完成；显式ToUser或未命中且仍有本类／本操作的后端候选才移交。没有相关候选在源有效时直接默认放行，不采集／分配／等待，不捏造规则命中；源失败／退役不改成允许，自保及独立生命周期供给保持。
2. DriverOperations登记唯一原请求，保管原对象、规则版本与自产生起的窗口；DriverHooks只填真实可得事实，缺失仍留空。
3. 经驱动消息／通讯接口快速发布受控事件。底层异步接收后执行记住／快速规则，必要时串行慢匹配／问询。上报完成不是原hook返回允许；原操作仍在合法的原生等待路径。
4. 用户态决定经原会话和OriginalKey回到本请求，竞争同一次收尾；原窗口到期、取消或来源失败也参与同一竞争。
5. 实际执行者保存决定并唤醒原等待者，原hook按决定返回；底层取得该次LSM检查的真实回执。Allow不证明后续系统调用成功，见[hook结果边界](hooks.md#当前普通控制入口与结果边界)。已完成请求只领取原结果，不重放。

创建／exec成功／退出等Observation不登记待决请求、不进入等待。当前不合法或尚未确认可等待的管道入口不得伪造这一链；缺口见[管道待定执行点](../../archive/unresolved-module-contracts.md#管道mcp拦截的待定执行点)。

## 终止与释放

Termination::None只阻止；Actor阻止并请求SIGKILL原actor；ActorAndParent同时请求原actor和捕获时的物理父进程。驱动直接Deny、底层记住／快速决定、Ask回答、已绑定期限到期及渠道断开／取消，都沿同一个原请求选择scope，最后只由原执行者收尾一次。Allow不得携带终止，来源故障／信号中断等错误不得借-EPERM触发终止。

正式弹窗的现行选择由[规则模块原 Prompt](../../backend/rules/technical.md#4-条件和动作的范围)提供。执行者在展示前核对并保管原 `ReviewBinding` 的期限及超时动作，收到裁决后仍按原操作校验与执行。驱动支持的终止范围与弹窗提供的选择分别由各自契约定义。

capture在最初RCU读取real_parent时固定actor／parent的struct pid引用，controller在接纳时固定；等待期间退出、PID复用、重新挂父或exec均不以数字查找替代对象。策略快照保管已鉴权采用者的cred到最后原请求结束。每项终止复用[DriverTools绑定入口](tools.md)，仅在signal调用内override_creds／revert，使用kill_pid(SIGKILL,0)保留系统凭证和LSM判断；不借被拦截者自身权限决定能否杀父。

直接Deny选定后先封住原操作，再在core_lock外尝试信号；Controlled保持Completing、held／capture／快照寿命到两项尝试结束，最后发布原回执。Both逐项独立尝试，不因一项失败跳过另一项，也不回退成允许。保护宿主PID1、内核线程和原controller；目标已消失和权限失败明确返回。SignalSent仅为发送接纳，D状态／内核信号语义不承诺立即退出。真实退出由生命周期或实际wait确认。

OperationReceipt／Driver直接结果／MainNotice／日志请求及中间层结果保留同一TerminationReport。requested明确范围；actor与parent分别NotRequested、SignalSent、NotRunning或Failed(native_error)，不吞分项错误、不伪造退出。未请求字段与已请求但返回-ENODATA的失败通过scope区分。所有原生引用、完成记录和等待者退役后释放；底层future断开不能提前释放。

## 有界资源与排空

Held上限与capture记录预算分开，询问占满Held不能占掉所有快匹配记录。原记录从分配前登记到物理释放后都持容量令牌，期限从分配前的原起点计算。观察使用独立有界预算；在线启用时容量丢失增加真实dropped，不伪造完整进程树。

排空使用同一短锁下的真实谓词：快照读者、完整寿命capture、观察、Held、待领取结果及控制写任务全部结束。最后释放路径唤醒read／poll；不依赖额外GetStatus轮询推进。候选分配、对象绑定、等待、用户拷贝和实际资源清理都不在core_lock内执行。

普通FIFO是否排空以实际容器为空为准，不枚举一份可能漏掉新帧类型的计数清单。Quiesce在既有reader.stop_facts下关闭新的观测接纳；直接规则继续执法。普通原请求在同一接纳锁内固定是否已获直决观测接纳，命中计数和Direct都使用该标记，已接纳的记录由原capture寿命保管到发布；自保／生命周期／运行时的既有在途计数继续覆盖各自已接纳采集。原请求返回通知在同一短锁内核接纳、复制本次固定标量并入队，不留下无寿命计数的锁外待发布记录。已接纳记录及普通FIFO全部消费后，才回复Quiesce；之后才由底层停止接收泵。停止接纳不是丢弃已经排队的Direct，也不让停止后的持续直决日志无限延长退出。

入队前期限已尽和Held后尚未绑定规则超时动作的等待到期，分别产生`before_admission`与`unbound_wait`内核限速诊断；字段来自该原请求已采集的kind／operation／TGID／TID／源sequence、实际单调时钟elapsed_ns及快照max_hold_ms。诊断在core_lock外输出，不含路径、argv或秘密，不改变原期限或返回值。入队前失败尚无Controlled发布，不能由已收到的其他操作回执推定它成功；DriverStatus.running只表达当前源状态。
