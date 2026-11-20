# 驱动人工决策等待：参考实现

本文件为[驱动技术文档](technical.md)提供可复查的机制依据。区分交互式防火墙、访问控制LSM、完整HIPS产品及仅观测项目；项目名称、弹窗截图和“同步裁决”字样都不能证明原操作仍在等待。以下固定提交的源码已读取，未在本机运行这些第三方驱动。

## TOMOYO：原生LSM等待用户态

Linux提交`d4c08d8b23b22807c712208cd05cb047e92e7672`。

- [tomoyo.c](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/security/tomoyo/tomoyo.c#L556)：同一原生LSM注册多个文件、执行及网络hook；不是每个类别一个驱动／动态库。
- [tomoyo_supervisor](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/security/tomoyo/common.c#L2051)：先检查规则，必要时分配query，在短锁内登记，解锁后用wait_event_interruptible_timeout等待，收到答复或失去观察者后收尾。
- [tomoyo_write_answer](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/security/tomoyo/common.c#L2255)：按query序号写回答案。该版本写入路径会重置待决计时，因此不能把源码注释的10秒解释为不可延长的总期限。

这是AS原生等待机制的首要源码参考。借鉴原操作保留、短临界区、请求关联、等待／唤醒及离线收尾；不照搬TOMOYO的学习模式、默认拒绝、重试或全局续期。它证明所实现检查路径可等待，不证明全部LSM hook都能安全等人。

## AppArmor＋snapd：内核请求、规则与展示分开

snapd提交`ef603d1a3b5adea0a85ddff881dce037c2370fbf`。

- [listener.Register](https://github.com/canonical/snapd/blob/ef603d1a3b5adea0a85ddff881dce037c2370fbf/sandbox/apparmor/notify/listener/listener.go#L107)：打开内核通知端点、协商协议并注册epoll；接收使用APPARMOR_NOTIF_RECV。
- [handleRequest](https://github.com/canonical/snapd/blob/ef603d1a3b5adea0a85ddff881dce037c2370fbf/overlord/ifacestate/apparmorprompting/prompting.go#L252)：先检查服务端已有规则，可决定则Reply，否则登记prompt；[HandleReply](https://github.com/canonical/snapd/blob/ef603d1a3b5adea0a85ddff881dce037c2370fbf/overlord/ifacestate/apparmorprompting/prompting.go#L446)处理客户端答复。
- [buildAndSendResponse](https://github.com/canonical/snapd/blob/ef603d1a3b5adea0a85ddff881dce037c2370fbf/sandbox/apparmor/notify/listener/listener.go#L514)：携带原通知ID，通过APPARMOR_NOTIF_SEND答复内核。

Canonical的[机制说明](https://discourse.ubuntu.com/t/permissions-prompting-a-deep-dive/81785)明确：AppArmor暂停待决操作，snapd检查规则，客户端询问，答复后内核允许或拒绝。该说明同时记录了WirePlumber通过API接入的音频询问：多技术可共用用户交互，但音频API不因此变成LSM hook。

适用边界：上述固定版本listener的[parseRequest](https://github.com/canonical/snapd/blob/ef603d1a3b5adea0a85ddff881dce037c2370fbf/sandbox/apparmor/notify/listener/listener.go#L432)只接受AA_CLASS_FILE；不能外推成全部进程／网络问询。产品说明将Prompting标为实验功能，依赖相应内核通知能力。AS借鉴分层与关联，不引入Snap接口体系、另一份规则优先算法或重启后复活问询的语义。

## OpenSnitch：NFQUEUE网络等待

提交`a1353848ba1b660320e90cefea782c3fba272c00`，属于交互式应用防火墙。

- [main.go的onPacket／acceptOrDeny](https://github.com/evilsocket/opensnitch/blob/a1353848ba1b660320e90cefea782c3fba272c00/daemon/main.go#L400)：检查规则，必要时uiClient.Ask，再向同一packet写verdict。
- [Client.Ask](https://github.com/evilsocket/opensnitch/blob/a1353848ba1b660320e90cefea782c3fba272c00/daemon/ui/client.go#L360)：调用界面询问。
- [queue.go的go_callback](https://github.com/evilsocket/opensnitch/blob/a1353848ba1b660320e90cefea782c3fba272c00/daemon/netfilter/queue.go#L191)：保留verdictChannel等待处理结果，最终交给netfilter。阻挡的是队列包，不能把socket写入本地缓冲成功理解成对端已经收到。

可借鉴数据包／决定一一关联及界面无关的原对象保管。不能照搬该版本UI离线或忙时的全局默认动作、1毫秒投递失败后的默认接受，也不采用其规则优先算法。AS只有命中询问规则才询问，已等待失联按原规则收尾。NFQUEUE只能解决特定网络执行点，不能填满文件／进程能力。

## Fort Firewall：WFP挂起与完成

提交`55df34e6eded55b7a8337b276a2636220dd166fb`，Windows交互式防火墙，只作机制对照。

- [fortcout_ale.c](https://github.com/tnodir/fort/blob/55df34e6eded55b7a8337b276a2636220dd166fb/src/driver/fortcout_ale.c#L281)：Ask路径交pending管理。
- [fort_pending_add_packet_locked](https://github.com/tnodir/fort/blob/55df34e6eded55b7a8337b276a2636220dd166fb/src/driver/fortpkt_pending.c#L212)：检查会话和容量，调用FwpsPendOperation0并保存completion_context。
- [fort_pending_packets_complete](https://github.com/tnodir/fort/blob/55df34e6eded55b7a8337b276a2636220dd166fb/src/driver/fortpkt_pending.c#L297)：短锁摘除后调用FwpsCompleteOperation0，触发重新授权，必要时重新注入保留的数据包。

这里是WFP支持的挂起／完成协议，不是让驱动在自旋锁里等弹窗。AS可借鉴原请求寿命、容量和完成路径；不移植Windows驱动，也不把WFP能力归给Windows进程创建通知。

## TOMOYO GUI实例：确有弹窗，质量不能整体照搬

[Tomoyo-Application-Firewall](https://github.com/Intika-Linux-Firewall/Tomoyo-Application-Firewall/tree/c913739c7c4d1659e98b2071926ce6a78388862e)提交`c913739c7c4d1659e98b2071926ce6a78388862e`，README明确依赖TOMOYO／AKARI及Zenity。[ccs-firewall.c](https://github.com/Intika-Linux-Firewall/Tomoyo-Application-Firewall/blob/c913739c7c4d1659e98b2071926ce6a78388862e/usr_sbin/ccs-firewall.c#L462)消费query、调用展示并按序号答复。

它证明TOMOYO query可以连接人工GUI；作者自己将其称为quick hack。相似请求复用答案、shell拼接展示和旧接口依赖不进入AS，也不把此项目标成现代完整HIPS。

## 其他机制的取舍

- [fapolicyd](https://github.com/linux-application-whitelisting/fapolicyd)：成熟文件访问策略守护进程，基于fanotify，适合参考文件规则和执行控制，不作为已经核实的人工弹窗项目。
- [fanotify](https://man7.org/linux/man-pages/man7/fanotify.7.html)：权限事件可等待；普通通知不可代替授权，组关闭会允许未决请求。新内核FAN_PRE_ACCESS须按实际内核与文件系统支持判断，不能沿用“只能控制打开”的旧结论。
- [seccomp用户通知](https://man7.org/linux/man-pages/man2/seccomp_unotify.2.html)：能挂起系统调用，但官方明确警告基于目标可变参数检查后CONTINUE的安全竞态，不作为AS通用HIPS安全裁决替代。
- [Landlock](https://docs.kernel.org/userspace-api/landlock.html)：标准接口约束调用线程及后代；搜索到的supervise讨论是RFC来源，不能当成当前目标内核已经具备人工等待。AS不以待合入方案为交付依赖。
- KubeArmor、Tetragon的BPF-LSM直接拒绝及Kunai的观察字段可以局部参考；未核实到对应人工等待链，不能按HIPS名称或同步返回值推定。

## 资料使用边界

机制依据使用本页链接的公开实现和当前[原操作契约](linux/operations.md)，不依赖已退役分支、其他工作树或旧ADR路径。旧项目不作为当前模块、协议、接口或存储方案的技术依据。

即时BPF裁决、事件上报和原操作等待是不同能力；不能从日志、停止上报或后续进程暂停推定原操作已被保管。当前普通控制由[实际hook](linux/hooks.md)及原生等待链实现；历史差异和有限验收见[HIPS对照证据](../evidence/hips-comparison.md)，不将其中旧状态作为当前开发入口。

闭源Kaspersky／ESET文档只能证明Ask／记住／超时等产品行为，不能据此断言其内部使用哪一个驱动等待API。微软[进程／线程回调建议](https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/windows-kernel-mode-process-and-thread-manager#best-practices-for-implementing-process-and-thread-related-callback-functions)要求回调简短、避免用户态调用和阻塞IPC；[FltSendMessage](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fltkernel/nf-fltkernel-fltsendmessage)的等待属于文件minifilter，不能拿来证明任意进程创建回调可无限等待。

## 对AS的选型结论

保留用户现有内核并以安装包交付，主线采用已有BPF-LSM入口与一个可加载原生等待模块；不采用自有内建LSM，也不要求启用TOMOYO。原操作保管参考TOMOYO；协议内容由一个独立传输／解析模块供给。可为已证实的特定缺口选择其他成熟机制，但每个操作明确唯一执行者，并接入同一规则、问询、结果与日志契约。先复用成熟机制，再补确有必要的适配；不默认同时启用TOMOYO、AppArmor、BPF-LSM、fanotify和NFQUEUE，也不建设运行时自动降级或多后端投票框架。

## 进程生命周期回调与进程树依据

这里区分成熟安全项目的生命周期实现与HIPS的执行前裁决，不把观测项目整体称为已验证的人工等待HIPS。

- Tetragon的[基础sensor接线](https://github.com/cilium/tetragon/blob/main/pkg/sensors/base/base.go)分别接入exec、fork、exit；[fork实现](https://github.com/cilium/tetragon/blob/main/bpf/process/bpf_fork.c)在wake_up_new_task处按线程组建立clone事实；[exit实现](https://github.com/cilium/tetragon/blob/main/bpf/process/bpf_exit.c)利用最终线程的退出路径发进程exit。可借鉴的是三种事实分开、父实例关联和最后线程判断，不复制它的缓存、限速或多入口后备方案，也不据此声明人工阻断。
- Linux v7.0的[sched tracepoint声明](https://github.com/torvalds/linux/blob/v7.0/include/trace/events/sched.h#L307)给出fork的父／子task、exec的task／old_pid／bprm以及exit的task／group_dead。[fork调用链](https://github.com/torvalds/linux/blob/v7.0/kernel/fork.c#L2497)在创建成功后通知；task_alloc更早，之后仍可能失败。AS据此选择现成sched生命周期入口，安全裁决仍留LSM，不照搬Tetragon的具体kprobe选点。
- [do_exit](https://github.com/torvalds/linux/blob/v7.0/kernel/exit.c#L870)先计算group_dead并发退出通知，之后仍有资源清理；对象回收的sched_process_free更晚。[exec成功路径](https://github.com/torvalds/linux/blob/v7.0/fs/exec.c#L1594)与[de_thread](https://github.com/torvalds/linux/blob/v7.0/fs/exec.c#L889)说明成功换镜像及非主线程exec需分别处理，不能用每次线程退出删进程节点。
- Windows官方[进程创建／退出通知](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/nc-ntddk-pcreate_process_notify_routine_ex)以CreateInfo是否为空区分创建与退出。这是HIPS常见生命周期形态的机制对照，不证明Windows回调可以任意等待，也不替代Linux真实入口或增加本轮Windows实现。

AS的具体选择、字段映射和回调约束以[驱动生命周期](linux/hooks.md#进程树必需的生命周期hook)及[统一事件](../contracts/events.md#进程载荷)为准。这里只记录可核查的源代码依据，不把调研、编译或历史入口数量说成当前生命周期已运行。

## 交付范围与成熟实现对照

- AppArmor 的 [profiles 入口](https://github.com/torvalds/linux/blob/v7.0/security/apparmor/apparmorfs.c#L2221)先检查读取权限，然后遍历已装载的 profile。AS 现有配置快照、驱动实况与原命中已分别提供保存值、生效状态和事件归因。固定自保组暂无需要逐条枚举的独立业务场景。
- KubeArmor 的 [file_permission](https://github.com/kubearmor/KubeArmor/blob/main/KubeArmor/BPF/enforcer.bpf.c#L709)按实际 WRITE/APPEND 直接裁决。Linux 7.0 的 [sleepable BPF 集合](https://github.com/torvalds/linux/blob/v7.0/kernel/bpf/bpf_lsm.c#L301)没有该入口。AS 在此点做写入自保，普通文件问询采用[写打开](linux/hooks.md#写打开与写关闭)。
- Linux Audit 的 [audit_log_lost](https://github.com/torvalds/linux/blob/v7.0/kernel/audit.c#L367)在审计记录丢失时独立增加计数。AS 的原规则计数及采集丢失边界分开。重复问询触发「记住」是已有实际需求，计数只需针对相关原问询，详见[进程树统计](../backend/process-tree/technical.md#8-命中归集修复和资源)。
- OpenSnitch 的 [Rule 消息](https://github.com/evilsocket/opensnitch/blob/master/proto/ui.proto)区分动作和持续范围，[弹窗流程](https://github.com/evilsocket/opensnitch/wiki/Pop-ups-dialogs)让用户选择是否持续应用。参考它的选择与范围，不由其源码推断 AS 的次数阈值。正式分发参考[安装分包](https://github.com/evilsocket/opensnitch/tree/master/utils/packaging)及[目标内核构建契约](technical.md)。

## 管道与MCP源码依据

- Linux v7.0的[file_permission原型](https://github.com/torvalds/linux/blob/v7.0/include/linux/lsm_hook_defs.h#L185)只提供file和mask；[rw_verify_area](https://github.com/torvalds/linux/blob/v7.0/fs/read_write.c#L425)在正常读写分派前调用。它支持对象级直接权限检查，没有MCP buffer参数；[sleepable集合](https://github.com/torvalds/linux/blob/v7.0/kernel/bpf/bpf_lsm.c#L325)不包含该hook，不能套用sleepable等待。
- [pipe.c](https://github.com/torvalds/linux/blob/v7.0/fs/pipe.c#L401)给出实际pipe读写、iov_iter和持锁复制位置；匿名pipe创建也不是普通路径file_open的同义接口。AS必须验证实际字节交付和等待位置，不能从名字为pipe_write就推定任意BPF探针能拒绝或等待。
- [MCPSpy的mcpspy.c](https://github.com/alex-ilgayev/MCPSpy/blob/main/bpf/mcpspy.c)用fexit/vfs_read、fexit/vfs_write在ret>0后发送采样。该项目是监控参考，既没有证明原消息仍被保管，也不证明可在当前消息上返回Deny；不能把观测回调移来充当HIPS拦截器。
- [MCP stdio规范](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports#stdio)定义换行分隔JSON-RPC；一次read／write不是协议消息边界。AS的重组与识别属于底层流量模块，完整保管及同一字节提交仍需独立明确，见[未定执行点](../archive/unresolved-module-contracts.md#管道mcp拦截的待定执行点)。

## hook到操作规则匹配的源码依据

本节核对Linux v7.0内核中的成熟LSM实现，不把它们各自的优先级或整套策略引入AS。

- TOMOYO的[hook接线](https://github.com/torvalds/linux/blob/v7.0/security/tomoyo/tomoyo.c#L158)让unlink、rename、open等实际回调分别产生操作参数；[file.c](https://github.com/torvalds/linux/blob/v7.0/security/tomoyo/file.c#L256)按请求operation的权限位筛选并匹配路径；[domain.c](https://github.com/torvalds/linux/blob/v7.0/security/tomoyo/domain.c#L161)先按ACL参数类型筛选，再调用具体条件检查。这说明按操作／参数形态组织匹配有真实内核实现依据，不等于每hook复制一套完整规则服务。
- AppArmor的[file_permission入口](https://github.com/torvalds/linux/blob/v7.0/security/apparmor/lsm.c#L503)把实际请求mask交公共文件权限处理；[路径匹配](https://github.com/torvalds/linux/blob/v7.0/security/apparmor/file.c#L214)取得权限结果，再与请求mask核对。其[common_perm及路径薄入口](https://github.com/torvalds/linux/blob/v7.0/security/apparmor/lsm.c#L196)保留mkdir／unlink等实际参数，复用公共权限过程。AS对应复用现有file_gate、公共采集及finish，以函数组合集中相同职责；实际参数与可睡眠条件仍各自明确，不为统一再加注册框架。可借鉴的是此分层，不复制其DFA／profile缓存或规则冲突语义。

AS据此采用的设计推论是：**保留进程／文件／网络三类派生，类内先按操作筛选同一规则序列，再按AS既定组号／规则号首条完整命中**。操作与Allow／Deny／ToUser分开；不建立每hook可写规则副本，不新增动作优先级。具体调用契约只在[驱动匹配](linux/rules.md#按操作接入hook)维护。

## BPF调用原生等待：核实到的依据

内核[kfunc文档](https://docs.kernel.org/bpf/kfuncs.html)明确支持模块导出kfunc，并以KF_SLEEPABLE限制调用程序。Linux v7.0的[bpf_testmod.c](https://github.com/torvalds/linux/blob/v7.0/tools/testing/selftests/bpf/test_kmods/bpf_testmod.c#L885)包含bpf_kfunc_call_test_sleepable，标记位于[注册集合](https://github.com/torvalds/linux/blob/v7.0/tools/testing/selftests/bpf/test_kmods/bpf_testmod.c#L1251)。该测试函数体为空，只能证明注册／校验入口，不能当成等待人工答复的完整实现；本轮未找到已核实的同构完整HIPS现成模块。

另查到[hallpass](https://github.com/alban/hallpass/tree/41f681f9e547c1b72a5d9c75bad5d1a1b6d9c098)，固定提交41f681f9e547c1b72a5d9c75bad5d1a1b6d9c098。[strict机制](https://github.com/alban/hallpass/blob/41f681f9e547c1b72a5d9c75bad5d1a1b6d9c098/docs/how-it-works/strict.md)使用seccomp通知挂起、用户答复后先安装BPF规则或单次授权，再继续系统调用；BPF在真实内核对象上再次检查。等待不发生在BPF中。[README](https://github.com/alban/hallpass/blob/41f681f9e547c1b72a5d9c75bad5d1a1b6d9c098/README.md#L33)明确标为proof of concept，不能直接标为成熟生产HIPS。源码已读取，未运行该项目。

AS借鉴Windows式的请求／等待／答复／唤醒职责，以及TOMOYO的原操作保管方法；若采用BPF入口，等待由原生代码承担。Linux具体hook的合法上下文、模块部署和本机行为仍需验证，不以设计图或空测试kfunc宣布已实现。


## 构建与用户态接入依据

- [Linux v7.0 gen-btf.sh](https://github.com/torvalds/linux/blob/v7.0/scripts/gen-btf.sh)：按pahole、resolve_btfids变换、BTF／BTF.base／BTF_ids嵌入的完整顺序生成模块元数据。只编码BTF或只产生sidecar不代表kfunc已经注册。
- [libbpf-bootstrap](https://github.com/libbpf/libbpf-bootstrap)：参考对象加载、逐程序挂接和link寿命；AS加载器管理当前源码声明的12个普通入口与16个自保入口及其真实link寿命；清单只见[当前hook](linux/hooks.md#系统自保附着集合)，不把历史四入口范围当作当前集合。
- [async-io 2.6.0](https://docs.rs/async-io/2.6.0/async_io/struct.Async.html)：使用AsFd安全注册非阻塞原生端点，避免在AS Rust代码中引入unsafe的任意FD注册。

## 文件操作必要分流与用户态等待

Linux [TOMOYO file.c](https://github.com/torvalds/linux/blob/master/security/tomoyo/file.c) 的 tomoyo_check_open_permission先由打开标志取得实际访问模式，分别检查read与write／append；不能因为存在写规则就把所有只读Open都作为写候选。AS复用现有AccessMask字段及condition_matches，只在已知必要条件不符时排除，未取得条件仍移交，不新增风险推定或默认权限。

[TOMOYO common.c](https://github.com/torvalds/linux/blob/master/security/tomoyo/common.c) 的tomoyo_supervisor先处理已经许可的请求；询问观察者存在才进入该询问路径，query短锁仅保护发布／移除，实际wait在锁外。本项目沿自己的原操作、渠道可用性及已授权规则期限实现，不照搬其学习／permissive模式或固定10秒期限。

[KubeArmor正式策略](https://docs.kubearmor.io/kubearmor/documentation/security_policy_specification)同样分别声明进程／文件／网络、来源条件、文件访问方式及容器selector；这些是匹配条件，不是依名字推风险。AS已有类型覆盖本次需要，不为调研重造第二规则模型。
