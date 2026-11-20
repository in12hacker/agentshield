# 保管专用ABI 1（归档）

此格式供核对保管专用源码与原始证据，不是新控制协议的兼容选项。当前设计见[统一驱动通信契约](../contracts/driver-communication/technical.md)。


旧内核声明as_wait.h和原ABI1 Rust适配、冒烟启动器均已删除；历史实现可在Git提交8bea67f42中查看。本文只供旧证据解释，当前主模块不接受此协议。

- `/dev/as_wait`为root 0600字符设备；open要求CAP_SYS_ADMIN，同一时刻一个控制TGID。read／write核对原控制TGID和随机source实例；传递FD不授权另一个进程答复。BPF对象与加载器必须是root所有且不可由组／其他用户写入。
- 每次read为一个完整记录，最长16384字节。104字节小端头：magic／abi／kind／正文长度，source[16]，id／started_ns／deadline_ns，policy[32]，result／actor_signal／parent_signal／features。REQUEST正文是既有88字节事件头及TLV；HELLO、RESULT和DRAINED无正文。features=15表示上述四个入口，Rust拒绝其他能力值。
- 每次write恰为96字节：abi／op、id、source[16]、decision／terminate／timeout_ms／flags、policy[32]、cgroup_fd／reserved、deadline_ns。flags和reserved必须为0。决定0允许、1拒绝；终止0无、1原actor、2原actor及原父进程，Allow不得带终止动作。
- CONFIGURE=1只在停用且无在途记录时绑定一个cgroup FD、原版本和1..300000ms原窗口；内核持有实际cgroup引用并覆盖其后代，不按cgroup路径重新绑定同名新实例。cgroup FD仅是同进程初始化控制参数，不写入事件。
- REPLY=2向原id／source／policy决定；PREPARE=3只绑定一次规则超时动作及绝对CLOCK_MONOTONIC deadline，取原期限与该deadline较早者，不能续期；CANCEL=4采用已绑定超时动作，未绑定则以ECANCELED结束；DISABLE=5以来源错误结束未决；QUIESCE=6只停止新接纳，保留已接纳请求直到全部RESULT读完后发DRAINED；FAULT=7把来源置为故障，原请求与新受保护操作都返回来源错误。QUIESCE不得覆盖故障。

保管时间使用CLOCK_MONOTONIC，事件时点与进程birth使用BOOTTIME，两者不混算。Rust采用保守采样转换为HoldWindow，已过期的合法窗口仍保留过去deadline，不重开窗口或令其他请求一起失效。

内核只给已经read交付的请求发送RESULT；尚未交付就过期／采集失败的请求在原hook返回错误并释放，不产生无主RESULT。最多64个未决及待消费结果，内核容量不足直接返回错误；普通日志不占此确认通道。

RESULT的result是本AS检查点的返回值，不是后续系统调用／I/O成功。actor_signal／parent_signal为1表示未请求、0表示信号提交成功、负值为errno；不代表真实退出。当前公共OperationReceipt只投影Allow／Deny等原操作结果，不能用它确认终止成功。

内核来源只有DORMANT／RUNNING／QUIESCING／FAILED一个状态。意外控制FD关闭从RUNNING进入FAILED，并保留原cgroup引用；仍挂接的入口对该范围的新操作返回EIO。只有明确停用或显式CONFIG恢复才改变此语义，不能把故障当无前端。控制会话或模块退役时按状态释放引用。Rust来源退役保留首原因，后续清理不把FAULT改成DISABLE；REQUEST登记与退役排空在pending锁内协调，不留下停止接收后才登记的等待项。

Rust每个原操作只有一个Execution状态：Held、Bound、Applying、Finished。RESULT写入唯一终态并唤醒该原操作；后来的主循环可领取既有回执，不能再次发执行命令。reader取消、loader异常退出或来源故障都会退役原请求；LinuxDriver只持弱引用，DriverSession及正在运行的reader保管实际资源。普通I/O readiness使用安全的async-io封装，其reactor不承担规则、问询或日志业务。

## 原生保管代码基线的采集范围

- `file_open`：文件open=1，提供原路径、file.f_flags、FMODE_READ／WRITE访问位。
- `bprm_check_security`：进程execute=1，提供目标执行文件路径；不以旧actor argv替代目标argv。
- `socket_connect`：网络connect=1；`socket_sendmsg`：网络send=2。范围为IPv4／IPv6，提供当次可取的远端地址、端口及协议。连接目标样本不作为永久socket授权或字节送达证据。
- 公共事实包括原host TGID／TID、group birth、cgroup ID、comm、exe、UID／EUID／GID／EGID，以及原物理父进程的TGID／birth／comm／exe。父进程保留原TGID的struct pid引用，终止不按数字PID重新查询。
- actor argv只在最多4096字节、完整且copy_from_user_nofault立即成功时提供；否则省略。不在回调内使用可fault的远程GUP或等待mmap锁。目标argv、cwd、容器实例、hash和数据库字段保持None。

以上入口只执行当前原生保管协议；这个端点不接受DriverPolicy安装、热更新或类别开关。规则引擎拒绝将启用的driver组交给用户态发布者生效，不把它们当普通快规则。DriverPolicy的下发与一致版本切换仍按规则发布架构单独接入。file_permission、已打开FD的后续写入、删除／重命名、fork／clone、管道字节及TLS明文不能由上述入口推出覆盖。
