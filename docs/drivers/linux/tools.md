# 驱动工具接口

产品目的见[驱动设计](../design.md)，组合边界见[驱动架构](../architecture.md)。本页独占DriverTools的职责、身份与结果语义；线格式仍由[通信契约](../../contracts/driver-communication/technical.md)定义。

## 职责与边界

DriverTools封装驱动内部小型、无状态的系统操作。当前只有真实进程身份查询与指定进程终止，不预建通用命令执行器、工作队列、缓存或另一份DriverContext。C实现为as_tools.c／h；类表示职责，不引入C++。

DriverCore负责调用主体／会话鉴权、生命周期接纳、命令长度校验和回执容量；工具函数只使用显式参数。身份查询继续实现已有ProcessFactsPort，只读调用者不会因此获得终止权限。原操作的规则决定、等待、期限和结果仍归DriverOperations；规则Actor／Parent决定由DriverOperations复用同一工具的绑定入口，工具不读取或选择规则。

## 身份与接口

复用DriverStatus.instance、ProcessBinding.host_tgid与birth_boottime_ns组成DriverProcessTarget。instance是实际驱动加载实例；进程号固定为宿主TGID，birth必须来自真实内核查询／事件，不使用/proc时钟tick换算、不按名字找替代对象。保存目标时必须保存全部三项，不能取旧PID／birth再补当前instance。

底层公开DriverTools::kill_process(target) → Result<KillProcessOutcome, ModuleError>，由已有DriverSession承载，Main经DriverPort调用同一会话。没有前端新增传输或未鉴权入口。DriverTools不写规则、配置或日志。

内核工具接口：

- as_tools_inspect_processes(pids, count, output)：使用已有as_process_binding，不重复建立进程属性模型；不启动／停止事实流。
- as_tools_kill_process(host_tgid, birth, controller)：固定为SIGKILL，目标是一个线程组；没有任意信号、进程组广播、负PID、递归树或隐含父进程操作。
- as_tools_kill_bound_process(target, controller, authority)：原操作传已有pid引用和快照采用者cred，不重新按PID定位。此内核入口不接受外部指针，显式命令仍只有固定32字节目标。

## 发送与临界区

先验证驱动instance和已鉴权会话，接纳仅在Running；停止接纳后不启动新终止命令。接纳前预留原有Reply队列容量；已接受命令沿原control_writes寿命完成，停止排空等待它结束。工具调用期间不持core_lock，不等待目标退出，不新增长驻线程。

在RCU下取得宿主PID对象引用及TGID任务引用，验证出生时间后保留同一struct pid，释放任务引用，再用kill_pid(pid, SIGKILL, 0)。Linux自身处理信号锁、退出和exec换主线程竞争。发送不再按数字PID查找，所以不能误杀随后复用该数字的新对象。拒绝0／超出正pid_t范围、宿主PID1、内核线程和本控制者；不隐含放大成杀父进程。使用0保留凭证、CAP_KILL和security_task_kill检查，不用SEND_SIG_PRIV绕过系统安全策略。

KillProcessOutcome::SignalSent仅表示内核接受信号，不能标作已退出；退出仍由生命周期事实／调用者对自己子进程的wait确认。NotRunning表示固定对象已不存在；出生时间／实例不符返回Conflict，权限拒绝返回Forbidden，停止接纳返回Stopped。传输失败时结果未知，不自动重试、不改为用户态kill兜底。D状态任务可能延迟退出，不承诺立即结束或阻塞通讯等待。

## 复用依据

[Linux pid.h](https://github.com/torvalds/linux/blob/v6.18/include/linux/pid.h)解释引用struct pid的复用安全性；[signal.c](https://github.com/torvalds/linux/blob/v6.18/kernel/signal.c)的kill_pid／kill_pid_info处理TGID和de_thread竞争，并由check_kill_permission核凭证与LSM。成熟调用见[VT驱动](https://github.com/torvalds/linux/blob/v6.18/drivers/tty/vt/vt_ioctl.c)保留pid引用后发送信号；本项目使用非privileged模式保留既有系统授权链。

规则绑定入口在当前actor的内核上下文调用，因此不复用显式控制入口的“当前线程必须位于宿主PIDNS”检查；目标对象与PID1保护都按真实宿主身份。授权cred仅包围同步signal，始终revert；工具内部没有cred缓存。采用者凭证引用及原生pid寿命归原操作／快照保管，不新增Owner。依据[Linux凭证模型](https://docs.kernel.org/security/credentials.html)及[现行内核cred.h](https://github.com/torvalds/linux/blob/v6.18/include/linux/cred.h)。
