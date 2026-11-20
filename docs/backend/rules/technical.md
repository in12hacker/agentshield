# 规则模块技术细节

公共引用：[MainContext公共上下文](../../contracts/main-context.md)、[模块唯一职责](../../architecture.md)。公共定义以链接目标为准，本页不另定义一份。

产品语义见[产品设计](design.md)，模块边界和线程见[架构](architecture.md)。本文定义当前规则模块的契约，不以平台示意代替已经实现的驱动能力。

## 1. 成熟依据及其适用范围

### 真正的HIPS产品行为

[Comodo Active HIPS Rules](https://help.comodo.com/topic-72-1-766-9164-Active-HIPS-Rules.html)记录了Allow／Block／Ask、规则集、例外及顺序调整。[ESET HIPS规则管理](https://help.eset.com/eav/18/en-US/idh_hips_editor_main.html)记录相同三种行为，但采用具体性优先等不同规则；其[交互窗口](https://help.eset.com/eav/18/en-US/idh_hips_alert_dialog_generic.html)支持记住与管理，并明确弹窗生成规则和手动规则等价。由此只能证明这些产品行为成熟，不能证明AS的“驱动优先、用户态记住优先、三类两池”是行业统一算法，更不能从闭源文档反推内核实现。

不能将“HIPS一般阻止在前”作为规则算法依据。历史HIPS产品[Cisco Security Agent 6.0.1官方手册，第5章Action Definitions](https://www.cisco.com/en/US/docs/security/csa/csa601/user_guide/CSAMC_UserGuide.pdf#page=140)明确高优先级Deny覆盖Allow，但普通Deny与Allow冲突时Allow优先；这只是该历史产品明确区分动作等级的设计。ESET的上述官方规范强调具体性，Comodo允许调整规则顺序。AS依用户确定的列表首条命中选择，不引入这些产品的隐式动作等级或具体性评分；论坛用户的“总是允许优先”说法不当作厂商规范。

### Linux前置授权与用户态等待源码

开源规则字段采用[KubeArmor types.go](https://github.com/kubearmor/KubeArmor/blob/3b5c8f74476ec7bcbdb2bf451a96ab3318b2ef72/KubeArmor/types/types.go#L394)的结构化设计作为依据：进程／文件／网络各有独立目标，fromSource限定来源路径，进程路径条目带AllowedArgs，动作与说明独立。KubeArmor是开放源码的Linux运行时防护执行系统，可核对字段和LSM防护实现，但不将其Kubernetes策略模型或交互能力整体移植到AS。

TOMOYO的[condition.c](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/security/tomoyo/condition.c#L864)在检查条件时读取当前任务PID／PPID，并独立处理exec.argv条件。AS沿用“静态规则读取当前请求事实”的方法，不修改共享规则字符串。Falco的[进程字段](https://falco.org/docs/rules/supported-fields/)只作为字段分类参考，其事后检测规则不能证明前置阻断。

Linux TOMOYO是访问控制LSM，不是完整的桌面HIPS产品。它提供可核对的前置防护链：[file.c的tomoyo_path_permission](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/security/tomoyo/file.c#L573)先查ACL，再经[common.c的tomoyo_supervisor](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/security/tomoyo/common.c#L2051)处理用户态query并等待答复；[network.c](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/security/tomoyo/network.c#L465)也接入授权检查。等待包含请求标识、内存额度、用户态离线唤醒和超时收尾。

这是“先在内核检查，必要时保管原请求等待用户态”的源码依据。TOMOYO自己的默认拒绝、等待时长、学习模式及重试机制不直接引入AS；也不因此改用TOMOYO或声称普通BPF LSM具备同样的用户态往返能力。

### 网络分流和观测节流只作局部依据

[OpenSnitch驱动分流规则](https://github.com/evilsocket/opensnitch/blob/a1353848ba1b660320e90cefea782c3fba272c00/daemon/firewall/iptables/rules.go#L34)按连接状态选择进入NFQUEUE的流量；[queue.go](https://github.com/evilsocket/opensnitch/blob/a1353848ba1b660320e90cefea782c3fba272c00/daemon/netfilter/queue.go#L191)把队列包交用户态并接收verdict。它是应用防火墙，借鉴网络操作筛选和原对象回执，不推定覆盖文件／进程HIPS。源码中的queue-bypass及入队超时默认接受不直接复制到AS。

[Tetragon动作源码](https://github.com/cilium/tetragon/blob/ed9bba461326afe45813c135575c49ca50ae5f1a/bpf/process/generic_calls.h#L1031)把NoPost／Post与Override等动作分开；[重复上报限速map](https://github.com/cilium/tetragon/blob/ed9bba461326afe45813c135575c49ca50ae5f1a/bpf/process/ratelimit_maps.h#L24)保存有界节流状态。它是运行时安全观测／执行项目，只借鉴动作与上报分离。其部分参数截取可用于上报压缩，不可直接作为AS授权缓存键；其[执行说明](https://tetragon.io/docs/concepts/enforcement/)也明确单独发SIGKILL不保证阻止当前操作。

### 是否按允许／阻止／移交拆三个组

公开依据要区分产品界面、冲突算法和驱动存储，不能从三个动作名称推断出三个内核匹配池：

- [ESET HIPS规则管理](https://help.eset.com/eav/18/en-US/idh_hips_editor_main.html)把Allow／Block／Ask列为每条规则的Action，另行说明具体性等优先规则；它没有公开“驱动按动作分三个池”的实现。
- [Comodo HIPS规则集](https://help.comodo.com/topic-72-1-766-9164-Active-HIPS-Rules.html)有Allowed／Blocked Files/Folders两个例外页，作用是在某项活动的动作下设置例外。这证明按动作查看／组织例外可以有用，不能据此断言驱动存在三个全局动作池。
- Linux [nft_do_chain源码](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/net/netfilter/nf_tables_core.c#L253)依次检查同一链的规则，条件未满足继续后续规则，取得终结verdict后返回；NF_ACCEPT、NF_DROP、NF_QUEUE均由该执行器处理。这是网络规则的有序混合动作实现依据，不是完整HIPS，也不表示Accept能覆盖其他链的拒绝。
- [WFP过滤仲裁](https://learn.microsoft.com/en-us/windows/win32/fwp/filter-arbitration)在一个子层内部按weight处理匹配过滤器，直到Permit／Block或列表结束；跨子层另有仲裁。它支持“顺序与动作分开定义”的机制经验，Continue并不等于交用户态，Windows机制也不作为AS的Linux实现。
- Linux [tomoyo_check_acl源码](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/security/tomoyo/domain.c#L161)按请求ACL类型跳过不适用条目，匹配后设置granted；未授权后的query由supervisor处理。这是类型筛选与授权后续处理分开的例子，不是Allow／Deny／ToUser三个动作列表的实现。

成熟实现并不只有一种冲突算法，例如[AppArmor策略](https://manpages.ubuntu.com/manpages/noble/man5/apparmor.d.5.html)明确规则书写顺序不决定含义。因此这里的结论是结合AS既定编号顺序选型：**三类独立驱动序列，动作是规则字段，组内允许混合动作；不增加固定的三动作池优先级。**

## 2. 驱动三个动作的精确定义

驱动规则动作集合为Allow、Deny、ToUser。命中Allow／Deny在驱动直接裁决，命中ToUser才明确移交用户态；未命中驱动规则时，仅有本类别／操作的实际后端候选才保留移交机会。没有本类／本操作候选则默认放行，不制造RuleHit。ToUser不是Allow别名，也不是“继续匹配下一条驱动规则”。

该集合是AS接口语义，不能原样作为所有Linux hook的返回整数。当前以LSM为核心，以下其他机制仅用于能力比较，不表示默认同时接入：

- 普通BPF LSM返回0表示本BPF检查不拒绝，负错误表示拒绝，还必须保留其他检查已经产生的拒绝。参考[内核LSM BPF说明](https://docs.kernel.org/bpf/prog_lsm.html)。在此返回0再写ring buffer，只能提供通知，不能据此声称原操作正等待用户态。
- Netfilter源码[nf_hook_slow](https://github.com/torvalds/linux/blob/d4c08d8b23b22807c712208cd05cb047e92e7672/net/netfilter/core.c#L627)分别处理NF_ACCEPT、NF_DROP、NF_QUEUE；官方[nftables queue说明](https://www.netfilter.org/projects/nftables/manpage.html)规定用户态给出接受／丢弃决定。这是网络原对象移交的机制依据，不是所有事件统一的API。
- [fanotify权限事件](https://man7.org/linux/man-pages/man7/fanotify.7.html)由用户态写FAN_ALLOW／FAN_DENY答复；普通通知事件不能替代权限请求。它仅覆盖实际支持的文件权限操作，不能推定覆盖所有文件修改。

ToUser必须绑定一个在副作用前可保持、并能由OriginalOperation完成的执行点。仅有BPF LSM直接判定或事后观察、没有原请求移交能力的操作，不允许安装需要用户态同步裁决的规则；管理界面显示具体不支持的操作／动作，而不是后台先放行再询问。BPF向其他内核检查返回0，不等于AS匹配了一条终局Allow。

未命中驱动规则但存在实际后端候选的移交同样受此能力限制；不存在“所有未命中LSM事件天然可移交”的保证。未具备移交能力的操作只能如实暴露直接裁决／观察范围，不列为用户态规则已保护。

## 3. 一条规则的单一归属与顺序

### 固定拒绝提示

以下字段已经接入当前规则模型、原命中及Driver镜像；是否能向某条LLM请求反馈还取决于真实关联与受支持的协议来源域：

- `Rule.deny_message: String`：规则作者配置的固定UTF-8提示词，缺省`""`表示无文本可发送。保持原文，包括真实空串及空白，不插值、裁剪、调用模型生成或自动补原因。非空只允许用于Deny／Ask；Allow／ToUser不接受无效的拒绝提示。文本参与同一RuleVersion内容修订，不另建版本号。
- `RuleHit.deny_message: Option<String>`：该实际命中版本取得的原文。`Some("")`是已取得的空配置，None是尚未取得；不能把查找失败改成空串。字段随原RuleHit沿既有MainNotice和OperationResult传递，不另定义一份规则命中DTO。它只供最终实际Denied反馈，不影响匹配条件、编号、动作、终止范围或log选择。
- `PolicyLease::resolve_hit_feedback(hit: &RuleHit) -> Result<RuleHit, ModuleError>`：只读接口，在同一既有快照租约中校验原source版本、组／规则身份及编号，补该版原文；不是重新匹配事件。Backend命中直接从所选规则取得；Driver命中由Main调用原DriverPort.lease取得原租约，再调用本接口，禁止按当前规则同名查找或建立中间层规则缓存。无法核实则返回实际失败，None不冒充已经配置生效；文案补取失败只标记反馈不可用，不能覆盖已经确认的Denied或重开原操作。

提示词的编辑、校验、增删改查和持久化均归本规则模块：前端沿原 `RuleCommand::Edit`／`Update` 提交完整规则字段，Main 只授权并调用 `PolicyEngine::edit`／`update`，单项编辑复用同一个原子保存和发布入口。不增加提示词专用管理命令、文件、数据库或可写缓存。中间层只消费原命中版本的 `RuleHit`；Flow 只保管该次未完成写事务需要的固定文本，不接受另传的可编辑文案。

驱动只消费是否需要真实拒绝回执的派生标记，不接收提示词。普通Driver Deny文本非空时，规则导出派生report_denial，源端须独立于log上报真实拒绝结果；log仍只控制原日志选择。标记的唯一二进制定义归[驱动规则契约](../../drivers/linux/rules.md#固定拒绝反馈的目标投影)。自保Configuration来源没有普通规则文本时不推定提示。文本仅在实际Denied后使用，Ask允许／尚未完成／仅提出Deny均不发送。业务路由见[执行反馈](../../middleware/execution-feedback/architecture.md)。

配置入口采用[model.rs](../../../crates/backend/as-policy-engine/src/model.rs)的PolicyDocument。每个组的kind只声明一次process／file／network，并确定该组规则的目标和操作类型；组有稳定id、正整数number、enabled与execution。execution使用唯一带标签结构：`{location: driver}`或`{location: backend, stage: remembered|fast|ask}`；驱动变体不接受stage字段。位置与底层阶段显式设置，不能另存level／pool／origin造成两份真相。旧顶层stage字段直接拒绝，不保留兼容或增加接口版本。每条规则有稳定id、number、enabled、scope、operations、source、target、action、deny_message（默认空）和必填log布尔值。组不另设action或scope，同一驱动组允许混合动作。

编号保留项以[配置模块文件自保规则](../configuration/technical.md#3-固定持久文件自保规则)为准：文件Driver组1为系统组，文件Driver组2为同源维护放行，普通文件Driver组从3开始，进程／网络Driver组1为维护，普通组从2开始；用户态各阶段仍可从1开始。同一类别及级别／池／来源分段内组号唯一，同组规则号唯一；重复编号拒绝，不靠文件名、加载顺序或随机ID打破平局。编号是展示与匹配顺序，稳定ID是编辑／命中引用，两者不能互相替代。RuleVersion是完整规则文档摘要，不是单条规则或运行时代号；驱动计划版本私有且只读，改变内容必须重新编译，不为驱动再建另一套规则身份。

确定的比较顺序是：

1. Driver：group_number、rule_number升序；首条命中Allow／Deny结束AS裁决，首条ToUser结束驱动扫描。
2. User Fast Remembered：group_number、rule_number升序。
3. User Fast Manual：group_number、rule_number升序。
4. User Ask：group_number、rule_number升序。

这四个阶段按上述顺序进入；每个阶段内唯一排序键是(group_number, rule_number)，动作、容器范围、条件数量、具体程度、加载时间都不参与排序。先排除停用或明确不适用的规则，再按顺序检查条件；首条完整匹配立即采用其动作，不继续扫描寻找另一个相反决定。Driver的ToUser终止驱动扫描并进入用户态；Ask是进入原请求等待，不能误当作未命中继续下一条。无法判定优先规则所需事实时先按事实补充／原执行失败路径处理，不能据此声称它未命中。

相同排序位置不靠Allow／Deny仲裁：前述编号唯一性在候选加载时检查，冲突候选不发布，保留上一有效版本。界面修改排序、停用规则或改变范围后，结果只按新的同一算法变化。驱动已终结的决定不再进入用户态仲裁。

记住来源不增加一个可自由填写的priority数值，也不覆盖驱动终局决定。驱动规则不再复制进用户态快速池。各类可引用相同的公共进程字段，但一条网络规则不能匹配文件事件。

示例：驱动网络组1／规则1阻止目标IP时，用户态记住的允许不能推翻；同一网络Driver组1／规则1为某目标ToUser时，同次扫描不再执行后面的宽泛Allow；驱动未命中时，才有用户态记住项优先于普通快速项。

### 容器适用范围契约

每条规则必填scope，只有两个互斥形式：AllProtected，或Containers(非空的ContainerInstanceRef集合)。AllProtected涵盖保护边界内的宿主机与容器，包括以后新纳入边界的对象；指定容器集合内是OR，scope与操作／主体／目标等条件之间是AND。不存在“空集合就是所有”或省略字段默认扩大范围。管理时去重实例选择，多选仍保留一个rule_id和一个顺序位置。

ContainerInstanceRef引用平台事实模块确认的容器实例：在运行时来源及其必要命名空间内使用完整容器ID，名称／短ID只用于展示。依据[Docker容器标识](https://docs.docker.com/engine/containers/run/#container-identification)，容器对象与镜像、名称须分开；[start](https://docs.docker.com/reference/cli/docker/container/start/)启动的是已有停止容器。删除重建产生新的容器对象，旧选择不转移；原对象身份不变的停止再启动仍保持选择。运行时来源改变也不能只按相同名称或ID字符串重新认领。

scope始终匹配原actor的容器归属，不匹配文件路径所在容器、网络目标容器或父进程的容器。多个所选实例中有对象删除，剩余实例仍可适用；已删除引用明确展示为失效目标，由用户删除或重新选择，不能替换成同名新实例。

当前[事件头](../../../crates/contracts/as-types/src/dispatch_event.rs)保留原 `actor_cgroup_id` 和已取得的 `actor_cgroup_lineage`。事实模块结合原来源与绑定生命周期供给可靠归属。当前正向成员确认范围为实际绑定的默认 Docker cgroup；祖先链用于下述范围排除。持久规则使用完整容器对象身份。已取得的归属随请求保留，用户答复不重新归属。

已确认是所选容器则范围命中；已确认是其他容器或宿主机则不命中；未取得可靠归属则仍待解析，不能按“不是所选容器”跳过优先规则。AllProtected在保护边界已确认后不需要额外容器身份；例如它本来排在最前就可决定，不能为一个后置规则强行增加容器查询。驱动缺少必要绑定时只可通过真实可保持原操作的路径补足，或报告无法执行；不得伪造范围未命中／宿主机身份。

实际 Docker 查询和成员索引归[驱动适配](../driver-adapter/technical.md#容器实例绑定)。普通容器范围条件由底层规则消费，原生规则池仅采用已定义的移交候选。热路径读取已发布的对象绑定。

后端规则从同一编译结果按进程／文件／网络导出有类型的必要候选。每条规则最多选一个对应hook可取得且驱动已支持的必要目标条件：进程目标执行文件精确／前缀、文件原路径精确／前缀、网络远端端口含端点范围。优先保留可表达的明确目标范围；仅文件没有可投影路径条件时使用已有access_mask，复用原包含位匹配，不能把明确目录规则扩大成所有写打开的移交。仅掩码规则中，已取得的只读掩码不满足写入条件时可明确排除，缺失掩码仍移交。复用原Condition与encode_condition；无法投影则该规则保留本类无条件操作候选。只有条件明确不符才能排除候选，相符、缺失或无法判定仍移交；不根据其他规则、当前进程或错误反面推定无候选。完整条件、容器身份和结果仍由后端决定；候选不产生RuleHit或动作，也不改变显式Driver规则优先级。唯一字节布局见[驱动通信契约](../../contracts/driver-communication/technical.md#持久镜像内容编码)。

原事件的 Workload 缺失时，规则通过 Main 注入的只读事实端口核对范围排除。原 cgroup 祖先链完整时有两种排除依据：原 actor 处在层级根，且所选对象均属于默认本地 Docker（其容器 cgroup 均为根的子级），或者每个所选对象均有覆盖原事件时点的活绑定、且这些 cgroup 都不在原链上。第一种不依赖对象仍在运行或存在。排除只作用于这条规则，不将未知 actor 写成 Host，也不让子 cgroup 被当成容器外。其余缺证继续按未知处理。默认路径来源见 [Moby cgroup 配置](https://github.com/moby/moby/blob/master/daemon/oci_linux.go#L728-L764)。

容器成员索引与AllProtected规则的候选合并后，仍按原组号／规则号处理，没有“指定容器自动更具体所以先匹配”。顺序例子：规则1 AllProtected Deny、规则2 Containers(A) Allow，A仍Deny；交换顺序后A Allow，B和宿主机仍由AllProtected Deny。驱动与用户态的同阶段匹配遵守同一原则。

记住规则保存明确确认的scope；容器事件可建议Containers(当前实例)，不能直接继承触发Ask的AllProtected。命中记录和事件摘要携带原actor归属上下文，避免在多个所选容器之间把计数混成一台进程；身份已由原事件／事实给出时不复制第二份可写容器名称状态。ProcessInfo.workload承载平台供给的Host／Container实例事实；缺失为None，wire中的原actor_cgroup_id仍是独立原始事实。规则模块不新建容器registry或按名称／当前PID扫描补身份。

### DNS 查询快照规则

NetworkTarget.domain只用于Backend的Send规则，匹配[原事件契约](../../contracts/events.md#udp-dns-send-查询快照)中同次完整UDP／53单问题Query的已解析名字。原规则条件仍为AND；编译器为这项明确的来源范围加入UDP／远端53必要条件，不能把普通SSH／GUI或Connect交成缺域名问询。响应、非DNS、短读、截断、零／多问题及解析失败原件保留，但不属于该DNS条件；不从IP缓存、后续连接或TLS源补名。DNS名字默认ASCII大小写不敏感，原快照与原名字不折叠；显式Regex选项仍属用户表达式。

此功能要求真实DnsQuerySnapshot来源；Native条件25仍不可用，Driver域名／Connect域名明确不支持。Allow／Deny／Ask继续原规则池和原操作等待，处置依据仅是hook读取快照，不保证其他线程未改写之后实际消费的缓冲。原Driver UDP53允许优先级不变；要进入这条DNS规则，正式数据必须先按既有优先级移交，不能在代码中绕过先命中的Driver规则。三目标演示数据不随此开发改写。

### 动作分池对顺序和性能的影响

以下是用户态普通快速文件池内的顺序反例，组1在此阶段合法；不占用文件Driver的保留系统组。路径表示精确文件或有明确边界的目录范围，不依赖驱动支持通配符：

1. 组1／规则1：备份程序写`/private/backup.index`，Allow。
2. 组1／规则2：任意程序写`/private/`内文件，Deny。
3. 组2／规则1：编辑器删除`/work/locked.txt`，Deny。
4. 组2／规则2：编辑器删除`/work/`内文件，Allow。

固定先查Deny池会破坏第一条例外；固定先查Allow池会破坏第三条例外。若拆池后仍要保持既定编号，就需要跨池比较命中条目的原顺序，增加一次仲裁；ToUser也须参加同一比较，不能固定抢在所有驱动决定前。

这是对AS算法的分析，不是性能实测：事件类别和操作在匹配前已知，适合作为候选筛选索引；最终动作通常要匹配条件后才知道，按动作分池本身不能保证少比较条件。第一版每类用一个有界、有序执行序列，编译时按组号／规则号展开。后续有实测需要再增加操作或字段索引，索引筛出的候选仍保留原顺序；不预建九个动作池，也不承诺未测量的性能收益。

## 4. 条件和动作的范围

source的字段包括当前事件的宿主机pid／ppid、name、executable、arguments逐位置条件、cwd、uid／gid、euid／egid、parent_name、parent_executable。PID只是该事件的数字条件，不是持久身份或终止进程的能力；终止仍交原执行者对原进程实例处理。空字段不被填成AS自身PID、空字符串或任意匹配。过程与父进程信息仍复用共同事件事实。

目标按类别独立：ProcessTarget为请求执行路径及arguments；FileTarget为原路径、新路径、设备、inode、访问mask；NetworkTarget为本地／远端CIDR、端口范围、协议、来源给出的域名。操作使用可读枚举：进程execute=1，文件open=1、create_directory=5、unlink=6、remove_directory=7、rename=9，网络connect=1、send=2。未知操作名拒绝解析，已编号但尚无完整普通控制链的文件操作在工厂明确Unsupported，不允许任意数字误报覆盖。argv保留参数边界，索引从0开始，不能将发起者命令行与请求执行的目标argv混用。

路径／字节条件使用Exact、Prefix、Glob、Regex。globset启用literal_separator，单个*不跨目录，**可跨；保持大小写。regex使用字节引擎、加载时编译，是否全串匹配由显式锚点决定，不按事件重复编译、不把无效UTF-8替换成别的路径。IP／CIDR由ipnet解析；端口范围是含端点的first／last，反向范围拒绝。

Linux的扩展操作必须按真实接口分组：[Yama](https://docs.kernel.org/admin-guide/LSM/Yama.html)约束ptrace进程跟踪；[process_vm_readv／writev](https://man7.org/linux/man-pages/man2/process_vm_readv.2.html)明确是跨地址空间读／写。前者需要原目标进程、跟踪请求等字段，后者还需要原目标与实际内存范围；这些不能复用child_pid或执行路径去猜。Windows的“远程线程创建／远程内存创建”不直接当作同名Linux事件。扩展时先补原执行点和typed目标数据，再开放操作组；不先添加收不到事实的可配置字段。

驱动只接受实际执行点能完整取得并有界匹配的字段与操作符：整数相等／范围／位掩码、完整字节串相等或有界前缀、IP／CIDR、端口／协议等。具体字段按执行点声明，不把“事件结构有这个字段”当成“驱动当时一定拿得到”。完整路径获取失败、字符串截断、关联身份缺失均不能按空值或截断前缀授予权限。正则、通配符、数据库查询、实时哈希与进程树遍历不下发驱动。

用户态快速条件只用统一事件和内存事实；询问池可在补充允许的慢信息后匹配。精确／范围匹配优先提供；需要通配符或正则时，在加载阶段使用成熟实现编译，匹配语义与字段绑定，不逐事件编译。没有设计的hash／数据库字段不预建。

规则中省略一个条件表示该条件不限制；规则写了条件而事件字段为None，表示事实待补充，不得当作任意值或满足“不等于”。若复杂阶段仍缺少优先规则所需事实，返回Unavailable并报告缺失，不把它当确定不匹配，也不跳过后采用宽泛Allow。已存在的字段不被慢信息覆盖。

快速阶段遇到优先规则需要补字段时，不能先采用后面的宽泛Allow；交复杂阶段补充后，从该用户态优先段重新匹配，随后才到Ask。驱动字段无法完整读取属于该执行点的事实／执行失败，不能通过跳过优先规则继续宽泛Allow来隐藏；其具体收尾必须随驱动实现明确。

User Fast的终局动作是Allow或Deny；User Ask是等待用户选择并带明确的超时终局动作。Deny携带唯一终止范围None／Actor／ActorAndParent，先阻止再按原进程实例执行终止，禁止按后来同PID进程补杀。Allow不携带终止动作。Driver只按实际能力导出Allow／Deny／ToUser及Deny的原终止范围，不把“阻止并终止”静默缩减为仅阻止。

规则模块 `interaction::Prompt::choices()` 统一提供正式问询的现行两项选择，按 `Allow`、`Deny { terminate: None }` 排列。原操作先接受 `ReviewBinding` 中的命中、期限和规则超时动作，规则模块随后才发布 `Prompt`。超时动作属于原规则与执行者保管的回退，和弹窗选择集合各有明确来源。终止范围仍按现有直接裁决、规则超时和执行回执契约处理。[Main 问询投影](../main/messages/technical.md#主模块代码接口)

这里的timeout_ms及原生保管窗口只属于既有HIPS规则／原操作契约，不为HTTP／TLS、测试器或停止回执派生另一个网络截止。Flow等待与取消语义只见[流量公共契约](../../contracts/flow.md#来源事实)。

## 5. 有限度地压缩洪水

处理顺序是边界／操作筛选、驱动直接决定、用户态快速规则、必要询问，最后才是日志与界面。源头筛选不能丢掉构建进程事实所需的生命周期变化；一次连接允许也不能成为整个进程永久可信的缓存。

已裁决的普通重复记录可用有界计数汇总，键包括规则版本、类别、组／规则、原进程实例、操作和实际目标范围，保留结果与计数。日志节流键即使碰撞也不影响授权；不能拿节流命中作为Allow／Deny缓存。计数和记录未完整送达时说明缺口，不把摘要称为逐条审计。

待裁决操作不做采样或跨请求去重。相同进程访问不同目标是不同原操作；即使条件相同，每个请求也保留独立decision_id、源deadline和原执行回执。第一次询问的回答只作用于该请求；其他请求要通过已明确生成、作用范围相符的记住规则处理，不能暗中共享一次授权。

接纳与内存必须有界，复杂等待和消息队列不能占满快速入口。已经有交互渠道且命中Ask后容量耗尽，或进入等待后渠道全部断开／超时，按该规则配置的超时动作结束。开始时就没有交互渠道则只执行快速规则，不进行慢查询及询问匹配；不能把它与在途请求失联混用。还未匹配Ask的队列满由原执行模块明确收尾，不推定用户允许。BPF ring buffer满时[预留直接失败](https://docs.kernel.org/bpf/ringbuf.html)，它不能承担可靠等待队列；上报失败不改变已经完成的驱动决定，并显露丢失计数。

## 6. 版本、记住规则和接入接口

规则模块从配置模块只读ConfigPaths.rules_file读取固定config/rules.yaml；样例见[配置](../../../config/rules.example.yaml)。snapshot一次读取已绑定的source、document和version，避免两次读取跨越更新。更新接口（签名见[规则接口](#普通规则的独立源与下发结构)）在同一管理临界区检查期望版本、全量校验、编译、原子保存，再发布候选；旧版本返回Conflict，保存未确认不发布新快照。remembered与普通组统一使用该入口，不提供脱离保存的replace或另一个授权缓存。模块写入回执只证明真实保存，整体采用结果由主模块等待驱动后回复。initialize只从主模块传入的MainContext取得配置模块已生成的rules_file引用，不接收InstallLayout、不拼根或重新派生路径；update／reload不再接收配置模块对象或另一写入路径。reload只检查并读取该实例已绑定的源。运行期没有配置正文／中间层路径覆盖或切换规则源的接口；不保留旧路径字段兼容。

规则快照只编译一次，再分离同版本Driver三类只读计划和底层匹配池；仅测试编译的DriverPolicy参考evaluate用于核对编号与移交，不执行内核安装。PolicyEngine允许显式Driver组，但不在用户态再次匹配该组；prepare_driver_policy按真实能力验证并导出，MainAssembly只在驱动实际采用后开放业务。不能用用户态参考evaluate代替内核执行。

Interaction::channel提供有界请求出口，connect仅供消息通讯模块在首次连接鉴权通过后登记渠道可用性；其传输所有者保管规则侧连接寿命句柄，规则模块本身不处理socket或凭据。重连由消息模块重新鉴权。每个Prompt保留原decision_id、事件、RuleHit与一次性reply；超时／取消后reply关闭，前端不得复用。超时取规则timeout_ms与原源deadline的较早值，不重新计算源预算；队列满和已在途请求失联采用规则timeout动作。初始无连接时主模块不调用prompt；Core::prompt也拒绝进入询问匹配，不创建RuleHit或Prompt。快速明确未命中的受控操作以无规则命中的NoInteraction原因返回，不写询问规则日志。

规则加载、条件编译、排序和能力校验在管理路径完成，热路径只读。驱动安装候选表示需要真实确认；不支持的动作／字段不自动改语义。内核和用户态必须对新请求使用一致的生效版本，在途请求保留原快照和源时间窗。发布失败保留上一有效版本；不能让新用户态规则配旧驱动Allow继续对外显示“全部已生效”。具体map替换与切换过程随驱动代码验证，不在规则模块另造事务框架。

文件Driver组1保留给配置模块的固定文件自保系统组，组2由规则owner管理维护放行，普通文件Driver组从3开始；进程／网络Driver组1为维护放行，普通组从2开始。冲突明确拒绝，不自动改号。配置对象自保先判实际外部修改，普通Allow不能覆盖它；ConfigManaged命中使用配置投影版本。

### 持久维护规则

PolicyEngine保留初始化传入MainContext的同一Arc<ConfigPaths>，只借`self_protection_root()`；不重新定位安装位置、不另缓存根。initialize、update、reload调用同一typed规范化入口，将三维护组放在原PolicyDocument前部，经原Compiler和DriverPolicyImage导出。它们实际存在于同一rules.yaml、管理读回和完整规则digest中，不是在native buffer里暗插的规则。初始化规范化如有变化，在驱动加载前原子保存并同步目录；重载补齐省略组也先保存，随后发布。更新保留原expected_version、固定文件绑定、真实保存／目录同步与已开始阻塞任务的保管语义。

三个组都enabled、Driver执行，规则均AllProtected、Allow、log=false、deny_message=""。原根为合法UTF-8时使用原字节加目录分隔符的Prefix，避免把同字面开头的相邻目录包含进来；非UTF-8根明确Invalid，不能lossy转换。Driver仍只支持Exact／Prefix，不借backend Glob／Regex改变格式。

- `as-maintenance-process`，组1：规则1 `as-maintenance-source-execute`以source.executable前缀匹配、target为空，允许Execute；规则2 `as-maintenance-target-execute`以target.executable前缀匹配、source为空，允许Execute。两条规则表示OR，不把两个条件塞进一条AND。Create／Exit继续只是观察，不生成保护规则。
- `as-maintenance-file`，组2：规则1 `as-maintenance-source-open`以source.executable前缀匹配、target为空，允许 Open、CreateDirectory、Unlink、RemoveDirectory、Rename、TruncatePath、TruncateOpenFile、MapSharedWritable；规则2 `as-maintenance-target-open`以target.path前缀匹配、source为空，仍只允许Open。后一条闭合外部执行者首次打开根内程序／文件的启动读取路径，不给外部来源根内修改Allow。原自保修改检查仍在普通文件匹配之前拒绝未经授权的修改。
- `as-maintenance-network`，组1：规则1 `as-maintenance-source-network`以source.executable前缀匹配、target为空，允许Connect和Send。维护流量经原Driver线性匹配直接结束；log=false Allow不制造普通上行通知，不回到自己的ToUser。仍需原事实采集，字段未知不伪称已匹配。

维护组id、编号和全部typed内容由规则owner管理。提交省略组时补齐，`groups: []`只删除用户规则；提交同id却修改条件、动作、日志、enabled、顺序或增删规则，明确Invalid，不吞掉修改。普通Driver组占用维护编号也Invalid。组与规则可读回，用相同内容再次提交保持相同版本；普通用户组及原排序不重排。源路径前缀是当前规则明确采用的真实采集路径条件，不把它声称为controller认证、exec代际或跨命名空间的同inode对象成员证明。

### 可管理的Linux快速放行预制

预制是正式发行默认规则数据：现有MVP打包和参会镜像安装入口都将同一预制文件安装为固定`config/rules.yaml`，继续由原规则owner读取、持久化、编译和管理。真实Agent验收准备保留候选已安装规则，目标询问组追加到实际读回文档，不整体覆盖产品基线或已有用户组；同号冲突由原owner明确报告，不静默覆盖或改号。独立运行时旁路采集不经过普通HIPS规则池，不能把普通Allow声称为该来源的采集豁免。

[`config/rules.fast-allow.yaml`](../../../config/rules.fast-allow.yaml)提供两组十条普通File Driver规则，均enabled、AllProtected、Allow、log=false，稳定组号3/4。组3只精确匹配`/dev/null`、`/dev/zero`、`/dev/random`、`/dev/urandom`的Open；组4只精确匹配`/proc/cpuinfo`、`/proc/meminfo`、`/proc/uptime`、`/proc/version`、`/proc/loadavg`、`/proc/stat`并要求access_mask包含READ位1。mask1会匹配R及RW，不排除WRITE位；W-only不匹配。信息入口自身的只读性质来自标准procfs处理器，不是规则能表达“仅R”。另有Network Driver组linux-fast-allow-local-control（组号3）：仅允许TCP到127.0.0.1的AS8765/OpenClaw18789鉴权控制口，及UDP到127.0.0.11/10.0.2.3端口53的镜像DNS；Connect和Send同样按真实端点匹配，不扩成任意环回或任意DNS放行，不授管理权限。它们仍是可管理的普通规则数据；AS维护组仍由原owner生成。

设备适用依据为Linux [`mem.c`](https://github.com/torvalds/linux/blob/v7.0/drivers/char/mem.c#L623)的null/zero及随机设备映射和[`random.c`](https://github.com/torvalds/linux/blob/v7.0/drivers/char/random.c#L1459)的随机设备接口；信息入口依据[内核proc文档](https://docs.kernel.org/filesystems/proc.html#collecting-system-information)及[`meminfo`](https://github.com/torvalds/linux/blob/v7.0/fs/proc/meminfo.c)、[`uptime`](https://github.com/torvalds/linux/blob/v7.0/fs/proc/uptime.c)、[`loadavg`](https://github.com/torvalds/linux/blob/v7.0/fs/proc/loadavg.c)、[`stat`](https://github.com/torvalds/linux/blob/v7.0/fs/proc/stat.c)等只读处理器。该预制是标准/dev与procfs挂载下的明确路径政策；路径被替换或绑定为其他对象时仍按原路径匹配，不能宣称认证了设备类型、inode或挂载来源。现device字段是文件系统设备号，不是字符设备i_rdev，因此不填固定字符设备major/minor冒充身份。Allow只终结本次AS Open匹配，不增加权限，不改变内核设备/随机数语义或其他操作规则。

安装时将预制作为固定rules.yaml的普通组数据，或通过已鉴权规则管理命令合入现文档；现有3/4组号冲突须显式调整，不自动覆盖或改号。初始化/update/reload继续由原PolicyEngine规范化、原子保存、发布及导出同一DriverPolicyImage，AS维护组仍由同一owner补齐。应用新RuleVersion后的驱动采用是独立结果；文件预制及编译验证不冒称已经安装或内核保护验收。

### 参会镜像的实例预设

`config/rules.conference.yaml`只提供[参会镜像指定演示目标](../../delivery/conference-vm.md#指定演示目标)的普通Ask数据，不复制Driver规则或另建匹配器。模板组保持disabled；镜像初始化从实际Compose容器和所附网络取得完整ContainerInstance及Gateway，投影该实例scope与网络目标后，通过原规则管理接口合并、校验、原子保存和发布。网络地址占位只属于封装模板，不能交规则模块当成合法CIDR。用户已有规则与禁用设置不覆盖。

其余操作不配置这些应用Ask条件，按既有规则和未命中语义处理；不再保留全部进程／文件／网络宽询问或维护目录兼容组。系统Driver基础白规则和自保保持原职责；准确容器范围仍由Backend规则池处理。原30秒／仅拒绝超时动作继续可管理，无交互渠道时按原契约处理。所有组、编号、条件和动作由已有前端管理。
### 普通文件gate能力与维护

普通 File 控制集合及其范围见[驱动 hook](../../drivers/linux/hooks.md#当前普通控制入口与结果边界)，Hello 的 operation_mask 以[DriverCapabilities](../../../crates/contracts/as-types/src/driver_control.rs)为唯一公共声明。文件规则操作位图仍以实际事件操作编号减一为索引。镜像导出、Native 采用和 Remembered／Fast／Ask 共同检查实际能力，未支持的 Write 等明确 Unsupported。

File维护仍是一份原PolicyDocument：来源维护行覆盖上述八项，原source.executable根目录Prefix、空target、Allow、log=false、空deny_message及原顺序保持；通过原规范化／保存／digest／DriverImage发布，不在Core加controller Allow。这使AS创建目录、删除／改名维护不重入自身ToUser。根内目标维护行仍只Open，用于外部冷启动读取；不扩成目标根内修改Allow。Process／Network维护行不变，Self修改权限继续先判，普通Allow不授身份或自保写权。

目标字段的物理可用性按[唯一事件操作来源](../../contracts/events.md#普通文件gate字段与操作来源)核验。Driver／Remembered／Fast／Ask 统一检查，new_path 要求全部 operations 为 Rename，access_mask 要求全部为 Open，inode 条件不能含 CreateFile 或 CreateDirectory。不可能组合明确 Invalid。Native 采用核对同一限制，真实可缺的采样保留 Missing。

原 mode、rename flags 与 mmap 参数只由[事件契约](../../contracts/events.md#普通文件gate字段与操作来源)定义，供原展示、日志请求、存储及执行解释；FileTarget／DriverCondition不以这些原始参数增加匹配项。NewPath沿既有字段在Rename接通，不为它另建操作池或换优先级。

记住规则保存用户确认的类别、主体、操作、目标及有效范围，缺失信息不能自动扩成通配；可用的临时进程范围必须包含原进程实例而非裸PID。管理入口和快速匹配使用同一规则快照。删除／停用记住项按规则更新生效，不在另一份私有缓存保留授权。

与现有契约的接入约束：

- [`RulesFactory::select_version`](../../../crates/contracts/as-types/src/main_ports.rs)先确定原驱动版本的不可变快照。未指定版本和 `select` 都使用当前快照。规则池派生由该快照按 EventKind 统一选择，同一事件沿用所选引用。
- `RulesModule::fast`实现用户态记住与普通快速顺序；`prompt`封装必要补字段后的优先规则复查、询问匹配与等待，不要求主调度知道组内算法。fast返回FastMatch::Matched、NeedsFacts、NoMatch或NoReviewMatch；NoReviewMatch仅表示当前事实已排除所有询问候选；有渠道时NeedsFacts移交慢处理，无渠道时主模块将其作为Unavailable收尾，不能将其当明确未命中或转入询问。RulesModule::can_prompt来自同一Interaction的真实渠道可用性。
- 驱动路由独立于现有`Decision::Allow/Deny`，ToUser不能塞进Allow或伪造OperationReceipt。已完成的驱动结果与待决定的DispatchInput分型；来源接入时补足真实命中和版本传递，不先修改88字节事件帧或预造一个控制FD。
- `RuleHit`字段和带标签来源版本以[共享命中契约](../../contracts/driver-communication/technical.md#8-规则命中来源与版本)为准；类别来自同一个事件，不另存可矛盾来源或版本。进程命中记录按原事件身份去重；驱动汇总命中与用户态逐条命中不能重复累加同次操作。
- 终止进程所需范围由Decision::Deny中的Termination完整传递，平台消费者按原进程实例执行，不能只在界面增加一个无法消费的配置。

Decision::Deny直接携带Termination的none／actor／actor_and_parent，Allow无法携带终止范围；ChoiceReason区分Direct／User／Timeout／NoInteraction／NoMatch。独立日志队列携带RuleLogRequest：原decision_id、用于匹配的事件、带版本及log标记的RuleHit、RuleLogDecision和Result形式的原执行结果。RuleLogDecision区分Transferred、Pending和Resolved，等待开始前把真实RuleHit登记到该请求的只写一次槽；取消等待时仍能记录Pending／Stopped，不能编造Allow或Deny。源返回错误和已匹配请求取消也可形成所选规则的日志请求；无规则命中或log=false不生成规则日志。普通Completed消息与规则日志分别分发，消费者不能见到Completed就无条件写规则日志。

日志出口是Messages职责内独立的有界SecurityLogEntry队列（普通规则包装Rule，驱动关闭事实包装FileClose）：try_send不阻塞执行，拒收计入MainIngress::rejected_logs；必要完成通知使用另一原有出口。底层SecurityLogSink消费并持久化安全日志，实际数据库结果由其所属模块报告；入队不冒充磁盘写入，也不将真实Allowed／Denied改成另一个结果。规则匹配与执行线程不直接调用日志文件I/O。

管理临界区由Arc<tokio::Mutex>串行化，OwnedMutexGuard移入实际阻塞任务，任务保管到版本检查／保存／发布真实结束；外层future被取消也不提前解锁。ArcSwap只在完整候选就绪后替换，选定的RulesModule Arc继续保有原快照。Interaction的连接计数和断开代数在同一次watch短更新中变更，等待通过通知／oneshot进行；断开后重连不能复活旧请求。持有管理锁或watch借用时均不等待用户或媒体I/O。

## 7. 既有规则语义示例

本节解释现行确定规则，不构成另一个开跑前置或完整临界矩阵；真实正常流程及后续针对性验证按[当前工作约定](../../../AGENTS.md)执行。

驱动Deny与用户态记住Allow冲突，必须保持驱动Deny；驱动ToUser后，必须能进入正确类别的记住规则。组号顺序变化必须改变首条命中，不出现隐藏的黑名单优先。记住网络项不能命中文件操作。

Containers(A,B)对A／B适用，对其他容器及宿主机不适用；AllProtected覆盖保护边界内二者。空选择不能扩大范围，同名重建不继承，删除A不影响仍存在的B。所有对象与指定容器同时匹配时由原列表位置决定；改变动作或缩小范围不改变位置。未知归属不能当成已证明范围不匹配。

高频已决定操作只增加相应命中与摘要，不能增加无关弹窗；不同目标的待决定操作仍各自受控。缺失／截断字段、队列满、驱动版本切换和交互断开必须暴露真实边界，不能制造Allow、已阻止或已生效的假结果。这些是目标行为的反例约束，不额外引入一套测试门禁。

## 8. 消息管理端口

主模块消息封装提供RuleCommand的Get／GetRule{rule_id}／Edit{expected_version,change}／Update{expected_version,document}／Reload{expected_version}，只接收首次鉴权成功的会话；Get和GetRule可由只读会话调用，修改要求服务端已授予ManageRules，不接受客户端自报权限。队列满返回Busy、关闭返回Stopped，不回复Applied。

规则管理任务通过主模块现有消息执行职责调用规则模块；文件I/O和编译仍由规则模块的阻塞工作任务处理，不另开业务循环线程。停止先关闭通讯命令生产者，再排空已接纳命令、回复并join。回复接收者断开不取消已开始的原子管理工作；普通写入返回RuleWriteReceipt，整体成功只在真实保存及必需驱动采用后返回。

命令没有文件路径、allowed_uid、访问角色或认证开关字段。首次身份验证和修改权限检查位于规则内容之外，编辑一条全局Allow不能关闭它们。当前Unix凭据验证确认本地UID，不能单独证明同UID进程一定是可信UI；跨主机／浏览器／IM身份认证和AS源文件内核自我保护需要各自真实传输与驱动实现，未接入的能力不宣称生效。

### 8.1 单条与组管理的唯一数据结构

定义位于[规则模型](../../../crates/backend/as-policy-engine/src/model.rs)，普通组和Remembered组共用，不另建“记住规则”文件或可写授权缓存。

- `RuleDefinition`：`kind=process/file/network`，`rule`直接包现有`Rule<各类Target,各类Operation>`；不复制或省略条件、动作、日志和固定拒绝提示字段。
- `RuleSnapshot { version: RuleVersion, group_id: String, rule: RuleDefinition }`：同一不可变快照中的来源版本、所属组唯一ID及完整原规则。Get列出完整组；GetRule按全文件唯一规则ID读取，不按编号或名称相似查找。不存在返回NotFound。
- `RuleChange`：显式且互斥的六种动作：`create_group{group}`、`replace_group{group_id,group}`、`delete_group{group_id}`、`create_rule{group_id,rule}`、`replace_rule{rule_id,rule}`、`delete_rule{rule_id}`。group复用RuleGroup，rule复用RuleDefinition。Create要求ID未存在；Replace要求原对象存在且ID不变，类别与原组一致；Delete要求对象存在。组的阶段、编号和enabled均由完整组明确提供，禁止推定组、作用域或条件；删除组同时删除其规则。

创建记住规则时显式提供`Backend{stage:remembered}`组；已存在的Remembered组直接CreateRule即可。修改规则的enabled可停用，number可调整组内顺序，字段仍按原规则校验。不在管理层自动生成Ask草稿，不把一次Answer持久化为未确认的通配授权。

所有Edit要求expected_version。规则owner先取同版文档，在私有管理代码中完成纯修改，再调用唯一update；真正的管理锁、当前内存与磁盘版本复查、源绑定、编译、同目录原子保存、目录同步和不可变快照发布继续由update负责。并发先完成者改变版本后，后完成的旧版编辑返回Conflict。目标不存在返回NotFound，类型／ID冲突或非法规则返回Invalid，不修改当前文件和快照；实际替换后目录同步失败仍按原Durability回报已保存版本，不伪称回滚。

AS维护组及其规则的定向Edit一律Invalid；不能删除后由规范化悄悄补回并声称删除成功。整文档Update仍可用于完整导入，与Edit共用唯一writer；省略维护组时的规范化语义保持不变。Edit成功复用原RuleWriteReceipt和RulesSaved，主模块仍等真实驱动采用才回复整体Completed；GetRule返回ReplyData::Rule。不创建新接口版本或旧命令回退。

## 9. 主循环与驱动接入的同步约束

三类工厂、组／规则编号首条命中、Driver→Remembered→Fast→Ask顺序、容器实例scope、log、原子规则快照和唯一保存入口继续复用，不因换驱动重写规则语义。

需要随主模块／驱动接入同步的接口：

1. **单问询调度**：Interaction::ask本身允许不同调用者同时等待；容量为1的输出队列不保证只有一个活动问询。由主模块唯一主循环顺序调用prompt；无可用渠道时不调用SlowFacts或prompt，规则模块不叠加全局等待锁或第二个调度线程。
2. **先绑定再展示**：ReviewRequest包含事件、只写一次命中槽和同一OriginalOperation引用，ID／时间窗／停止信号从原操作读取。OriginalOperation::bind_review在展示前检查原请求并向实际执行者登记所选规则和期限；失败不发Ask。复用现成LSM时使用其原生请求有效性接口，不要求另造内核握手。规则模块仍拥有询问匹配，不把匹配算法移到主模块。
3. **真实驱动发布与选版**：DriverPolicy的参考evaluate不执行内核规则。实际导出按原生能力验证，经DriverPort在提交前登记镜像及同一PolicyLease；到达事件精确select_version，旧代直到0x105确认退役再释放。初始化／发布经主模块消息封装调用实际驱动接口，不能用新用户态规则解释旧驱动请求。
4. **操作与能力对应**：共享event_operation.rs唯一声明操作。当前完整普通控制链与事件来源只见[现行操作字段字典](#当前配置字段字典)及[原事件来源](../../contracts/events.md#普通文件gate字段与操作来源)；其他自保观察编号不能据编号存在推定普通规则同名动作已支持。每项能力核验真实操作字段、动作和等待性，不复用code 1表达新行为。

阻止并终止复用既有Decision／Termination；实际分项回执已收口到as-types::operation::TerminationReport，经Driver直接结果／OperationReceipt／MainNotice和中间层Receipt消费，语义见[原操作终止](../../drivers/linux/operations.md#终止与释放)。已接入的Flow／凭据链按[当前流量契约](../../contracts/flow.md)消费；MCP仍未接入，本规则模型不添加猜测的协议字段。固定路径、配置写入及重载见[配置技术细节](../configuration/technical.md)。

驱动初始化和发布的通信类型以[统一驱动通信契约](../../contracts/driver-communication/technical.md)为准：主模块调用规则初始化／准备接口，不自行编译；配置和驱动规则作为一个完整状态提交，管理生效答复与原操作回执分别处理。


## 普通规则的独立源与下发结构

本节是规则模块公开接口的唯一签名出处；主模块和通信文档只链接，不另列不同顺序或另一返回类型。

- `async PolicyEngine::initialize(context: &MainContext, interaction: Arc<Interaction>) -> Result<PolicyEngine, Error>`：初始化三类规则工厂实例，只使用上下文已有路径，不是单类别RulesModule的初始化。
- `snapshot() -> PolicySnapshot`：一致读取本模块绑定源、文档和版本。
- `get_rule(rule_id: &str) -> Result<RuleSnapshot, Error>`：从同一当前快照读取单条；不存在返回NotFound。
- `async edit(expected_version: RuleVersion, change: RuleChange) -> Result<RuleWriteReceipt, Error>`：明确的单组／单条修改，复用update。
- `async update(expected_version: RuleVersion, document: PolicyDocument) -> Result<RuleWriteReceipt, Error>`：真实校验、保存及发布保存快照。
- `async reload(expected_version: RuleVersion) -> Result<PolicySnapshot, Error>`：读取同一固定源，不声称执行了文件写入或驱动采用。
- `prepare_driver_policy(capabilities: &DriverCapabilities) -> Result<(DriverPolicyImage, PolicyLease), Error>`：根据共享能力数据产生不可变驱动表示及版本保管。


规则模块从初始化参数MainContext.paths.rules_file读取并实际写入普通PolicyDocument，不能接收中间层任意源路径，也不读取配置模块的自保条目再保存一份。主模块调用initialize／update／reload／prepare_driver_policy接口，模块负责校验、保存和平台编译。

`DriverPolicyImage { rule_version, platform, encoding_revision, immutable_buffer }`采用[唯一镜像编码](../../contracts/driver-communication/technical.md#持久镜像内容编码)，包含带类别的普通Driver规则及条件，并投影三类实际启用后端规则的操作集合。候选只是保守路由：Driver未命中时结合原操作位及必要文件路径候选保留后端匹配机会，只有能够明确排除全部候选才默认Allow且无RuleHit；缺路径不伪造未命中，也不把后端动作提升为Driver规则。停用组／规则不生成位，只有其他类别／操作的规则不触发本次移交。驱动据类别／操作构造只读索引，不在公共结果另暴露可独立修改的pool数组。配置模块的系统文件组经独立DriverConfigImage提供，记住／普通快／询问规则仍由用户态持有。

`RuleWriteReceipt { saved_version, required_effects }`只报告真实保存，不能单独回复驱动已生效。主模块串行组织持久更新，取得两个模块的当前投影并等待驱动实际采用；原镜像、候选及在途版本由规则模块租约保管，不重演另一份匹配算法。

RuleHit来源版本按[通信契约](../../contracts/driver-communication/technical.md#8-规则命中来源与版本)统一。配置文件自保规则是只读系统组，不参与普通规则编辑；每条普通规则仍只属于一个类别和一个匹配池。

模块间只依赖共享契约：规则模块不导入配置／驱动／主模块实现，不访问全局配置快照或直接打开驱动。主模块将MainContext、能力DTO等作为参数传入，规则模块返回规则镜像和租约；单类别RulesModule接口仍只负责匹配。

## 固定源接口实现

规则模块只依赖共享契约，不再依赖配置实现。初始化接收现成MainContext路径和真实Interaction出口，绑定规则目录句柄，拒绝符号链接源；更新检查缓存及磁盘版本，通过绑定目录实际替换rules.yaml。替换后的快照先发布，目录同步失败报告Durability和真实保存版本。reload返回PolicySnapshot，update返回RuleWriteReceipt，不混淆读取与保存。

prepare_driver_policy已按[driver_image.rs](../../../crates/backend/as-policy-engine/src/driver_image.rs)连接，镜像编码见通信契约。它同时验证全部启用规则的操作／动作能力，不能只验证Driver组而让底层Ask引用不可保管操作。不能以用户态匹配或源写入证明驱动规则生效。

## 管道读写与规则类别

管道权限属于文件类别；Open=1、Read=2、Write=3各有唯一含义，操作编号只在[event_operation.rs](../../../crates/contracts/as-types/src/event_operation.rs)声明。当前管道普通访问只开放真实可保管的Open；Read／Write尚无普通可等待链，不能用Open代替。精确管道对象类型／实例条件尚未定义时，不能用pipe路径样式、FD编号或inode猜测一条精确管道规则；相应字段随真实流量与对象绑定设计确认。

只有实际执行点支持的直接动作才可编入驱动。file_permission的等待限制与MCP字节拦截缺口见[管道hook](../../drivers/linux/hooks.md#管道hook与mcp消息拦截)；不能把配置的ToUser／Ask自动降为Allow／Deny。MCP识别仍归底层流量模块，不增加第四规则池或跨文件／网络两池的单条规则。

驱动规则与hook的接入按[同类池内操作筛选](../../drivers/linux/rules.md#按操作接入hook)，操作只是已有operations条件，不增设允许／拒绝优先级或复制每hook规则表。Create／Exit生命周期编号及仅观察语义以[统一事件](../../contracts/events.md#进程载荷)为准，基础事实不等于新增可阻断规则能力。

## 驱动接入与既有规则的整体一致性

1. **唯一归属**：PolicyDocument／RuleGroup／Rule的字段继续使用现行模型。一个组只声明一次kind和execution；每条规则属于一个组，不因匹配多个operation复制规则ID或跨类别池。
2. **阶段和动作**：Driver仅Allow／Deny／ToUser，Remembered／Fast仅直接Allow／Deny，Ask仅Ask及其已配置timeout动作。主模块的用户阶段仍为Remembered→Fast→Ask；驱动Driver先执行，记住规则的最高优先级限定在用户态阶段，不能覆盖已完成的驱动裁决。
3. **条件含义**：scope、operations、source、target之间取AND；operations列表内取OR。按operation预筛选不能删掉主体、目标、容器条件，不能把缺失字段变成通配。类别与阶段内保持组号／规则号首条完整命中，不按动作、路径具体性或命中速度增加隐藏顺序。
4. **容器和自保**：AllProtected仍含宿主机和容器；指定容器是确切实例集合，不能把cgroup编号当实例。文件Driver系统组1只来自配置投影并先检查，文件Driver组2为同源维护放行，普通文件Driver组从3开始；普通开关和用户记住规则不能解除自保。
5. **移交含义**：Driver命中ToUser时保留该真实transfer_hit及log；确定未命中时transfer_hit=None。二者进入底层快规则，不能在驱动继续找后面的Allow。没有前端不取消这次移交，因为底层记住／快速规则仍要运行；只跳过慢查询和Ask。已经绑定Ask后断线按原规则timeout动作收尾。
6. **最终裁决和日志**：转交命中只证明移交；底层最终命中的规则有自己的RuleHit，不能覆写为先前的驱动命中或提前声称原操作成功。每次真实命中保持原规则来源／版本和log，按原操作／事件与规则身份归集；Driver结果和底层完成投影不能重复计同一次命中。生命周期Observation不参与这些阶段，不伪造命中或规则日志。
7. **执行能力**：每个操作必须同时满足字段、动作、原对象和等待能力。导出按实际能力拒绝未支持的终止范围，包括底层Ask的timeout动作，不能删掉终止后作为支持。ToUser、未命中移交及用户态Ask都需要仍被原执行者保管的操作；file_permission的直接权限能力不等于完整规则链已生效，不能自动回退。

这是一条现行规则语义到真实hook的消费链，不增加第二套策略。当前原生表示、移交命中、原版本选取和真实退代都沿同一接口；剩余未确定能力列[存档](../../archive/unresolved-module-contracts.md#驱动内部尚未确定的表示与绑定)，不以图或参考evaluate代替实际采用。

规则生效位置显式设置；前端自然语言标签不作为协议字段或匹配输入。内部结构可随真实hook统一调整，不另建GUI规则schema或自动改变执行位置。

## 当前规则实现与数据声明

执行位置以[rule_execution.rs](../../../crates/contracts/as-types/src/rule_execution.rs)为唯一类型声明，操作以[event_operation.rs](../../../crates/contracts/as-types/src/event_operation.rs)为唯一编号声明；规则模型引用这些类型，不再重复声明操作枚举或保留旧Stage。多操作仍只是单规则的operations条件。

未知事实在快速阶段返回NeedsFacts；补充后仍缺少必要条件时返回Unavailable。只有某个AND条件已明确为假时才允许确定该候选不匹配，不能因为缺失跳到后面的宽泛允许。驱动参考计划持三个分离只读序列；它仍不是内核采用证明。普通文件Driver组1被拒绝，留给配置来源的系统自保组。

执行目标argv由Native原bprm页完整有界供给，文件new_path由Rename原hook提供；网络domain只消费[同次UDP／53 Send查询快照](#dns-查询快照规则)，不作为Connect或Driver域名来源。采用准备核对这些字段的实际capability，缺供给则Unsupported，不等运行时永远NeedsFacts。通用schema／matcher存在不等于当前Linux保护链已支持该字段；Tree仅补ProcessInfo，不能替目标字段编造值。真实采样失败保原Missing，不删除条件或转用actor argv。

## 当前配置字段字典

以下键直接对应[model.rs](../../../crates/backend/as-policy-engine/src/model.rs)，是实际读取、校验、编译和下发的配置，不是前端别名。`argv`只是Linux参数向量的语义名，配置键只有`arguments`；不存在`source.argv`、`parent_arguments`、`open_flags`、child_pid、birth、cgroup或hash规则条件。事件／树内有某字段不表示规则可以配置它。

未知键、错误变体和类型明确拒绝，文本条件内部也不允许额外键；拼成`valuee`或加入未定义字段不会被静默忽略。以下Option条件省略／null表示不参与；`arguments`省略或`[]`表示没有参数条件，不接受null；source／target提供时必须是对象。缺失事件事实不是匹配成功，也不表示确定未命中。

### 文档、组和规则

- 文档只有`groups`：三类规则组数组，必须显式提供。
- 组的`kind`：`process`、`file`、`network`之一，决定该组唯一目标类型及操作枚举，不能跨池。
- 组的`id`：非空管理身份，全文唯一；`number`：正整数，在同类别及同执行位置／阶段内唯一，是排序键。
- 组的`enabled`：明确启停，不参与新事件匹配的组仍可保留配置。
- 组的`execution`：`{location: driver}`或`{location: backend, stage: remembered|fast|ask}`；不隐藏执行位置，不允许给driver补一个stage。
- 组的`rules`：该组的目标类型化规则数组。
- 规则的`id`：非空且全文唯一；`number`：组内唯一正整数；`enabled`：是否参与匹配。这两个number不按动作、具体程度或来源隐式重排。
- 规则的`scope`：`{kind: all_protected}`覆盖宿主和容器中的全部受保护对象，不自动打开被关闭的防护。`{kind: containers, instances: [...]}`选择非空且不重复的精确实例列表，每项`runtime`与完整`id`必须非空；作者域的可选`namespace`只能为None或非空字符串，Some空字符串由[Compiler scope校验](../../../crates/backend/as-policy-engine/src/compile.rs#L318)拒绝。作者输入留空表示None，仅匹配未提供命名空间的实例，不是通配。通用ContainerInstance事实中的Some空字符串仍按原值保留，不因作者域校验改变事实codec。Backend 规则只消费[适配核验的原事件归属](../driver-adapter/technical.md#容器实例绑定)。Driver 规则仍拒绝容器选择。
- 规则的`operations`：同类非空操作名列表，列表内OR；当前可用于规则的Process操作只有`execute`，Network为`connect`／`send`。File为`open`／`create_file`／`create_directory`／`unlink`／`remove_directory`／`rename`／`truncate_open_file`／`truncate_path`／`map_shared_writable`九项，对应操作编号1、4、5、6、7、9、10、18、19，唯一能力出处为[DriverCapabilities::controlled_operation_bit](../../../crates/contracts/as-types/src/driver_control.rs#L235-L256)，由[Compiler.group](../../../crates/backend/as-policy-engine/src/compile.rs#L494-L514)消费。Process `create`／`exit`只作事实通知，规则编译为Invalid；其他未取得控制位的File操作为Unsupported。原事件/协议全枚举仍按原值读取，不因作者选项收窄而删去。
- 规则的`source`：可省略为`{}`的原actor条件；`target`：必须提供本类别目标对象，可为`{}`。source、target、scope和operation之间AND。
- 规则的`action`：实际动作对象，含义和合法位置见下节；`log`：必须显式的布尔值，命中后是否生成结构化日志请求，不代表已持久化，也不改变执行动作。

### 动作及条件对象

- `{type: allow}`：仅允许，不携带terminate。
- `{type: deny, terminate: none|actor|actor_and_parent}`：阻止并选择唯一终止范围；none仅阻止，actor是原actor，actor_and_parent还包括捕获时的物理父实例。范围固定，不按当前PID或重新挂父关系追杀；结果见[原操作契约](../../drivers/linux/operations.md#终止与释放)。
- `{type: to_user}`：Driver移交同一保管原请求，不能充当允许或前端询问。
- `{type: ask, timeout_ms: N, timeout: <Decision>}`：Backend询问，N为非零毫秒，timeout只能为上述Allow／Deny终局动作；实际期限不超过原请求窗口。初始无渠道不进入询问池，已绑定后断开执行已配置timeout。
- 文本对象只有`operator`和`value`：operator为exact／prefix／glob／regex，value为UTF-8配置字符串。匹配使用原字节；不自动大小写折叠、拼绝对路径、解析符号链接或补通配。Driver当前只接受exact／prefix；Backend的glob／regex在采用时编译，不在每个事件临时编译。
- 参数对象`{index, pattern}`：index是从0开始的原argv元素位置，pattern是上述文本对象；同一列表中索引不重复，多条条件AND，不把参数拼成shell命令字符串。
- 端口对象`{first, last}`：u16闭区间，0至65535且first≤last；地址为IPv4／IPv6 CIDR字符串，地址族须与实际事件相符。

动作位置严格校验：Driver只接受Allow／Deny／ToUser；Backend remembered／fast只接受Allow／Deny；Backend ask每项必须Ask，终局Allow／Deny只放在Ask.timeout中。这个结构校验先于enabled筛选，停用项也不能带非法组合。

### 主体source：12个实际字段

以下字段在当前Native快速采样能力内（目标argv需要MMU能力）；读取失败仍可缺失。Backend只从原事件事实及符合事件时间／身份约束的树缓存补缺，不覆盖原值。

- `pid: u32`：原actor的宿主TGID，不是TID、子PID、终止句柄或动态字符串占位符；主回调先填事件真实值，再比较此数字条件。
- `ppid: u32`：捕获时物理父的宿主TGID，允许真实的0根关系；不是当前查询到的替代父进程。
- `name: TextMatch`：原actor的Linux task comm，不从可执行路径basename推导。
- `executable: TextMatch`：actor当时已经运行的可执行文件内核路径；Execute检查时是旧镜像，区别于target.executable。
- `arguments: [ArgumentMatch]`：actor当前已有argv的逐元素条件；不是请求执行目标的参数，也不是整个shell命令行。当前Native捕获有界，不承诺读取任意长参数。
- `cwd: TextMatch`：actor工作目录。当前Driver从实际fs/path供给该字段；Backend也可用同进程强身份／当前证据代／事件前有效的树缓存补充，不用事后读取倒灌历史事件。
- `uid: u32`、`gid: u32`：捕获时真实用户／组的宿主数字ID。
- `euid: u32`、`egid: u32`：捕获时有效用户／组的宿主数字ID；与真实ID分别匹配，0具有普通root数值含义。
- `parent_name: TextMatch`：捕获物理父的task comm。
- `parent_executable: TextMatch`：捕获物理父已有镜像的内核路径。进程树虽可带parent argv，当前schema没有父参数条件。

### Process target

- `executable: TextMatch`：当前bprm检查的实际`bprm->file`路径，不是原始execve字符串、child PID或原actor旧镜像。解释器链可产生各自检查点，不保证一次用户执行意图只有一次检查。
- `arguments: [ArgumentMatch]`：请求目标argv条件。当前MMU Native从bprm目标mm有界读取完整向量；无MMU或未报告TargetArguments能力的来源仍采用前Unsupported。读取失败／超限保持缺失，不能借actor.arguments替代。

`execute`是在exec原执行点裁决，fork创建成功和最终退出只是事实通知；不能称作fork前控制或事后杀进程等同于阻止exec。

### File target

- `path: TextMatch`：本操作原目标的完整内核路径；Rename为原被改名对象，CreateDirectory为真实mount内的原创建目标名。
- `new_path: TextMatch`：仅Rename的原目的路径；operations包含其他操作时明确Invalid，全局NewPath能力位不豁免此检查。
- `device: [major, minor]`：实际文件设备号，两个u32；`inode: u64`：该对象的原inode样本。二者都是匹配事实，不是不可复用的控制能力。
- `access_mask: u32`：非零，所有指定bit都须存在；当前Open源值为FMODE_READ=1／FMODE_WRITE=2，不是MAY_*掩码或O_*打开标志。open_flags虽然在事件中存在，但不是当前可配置的条件。
- `mount_effect: MountEffect`：复用[原事件挂载影响](../../contracts/events.md)枚举。本期规则只接受`independent_view`，且仅`operations: [mount]`、Driver、直接Allow或仅阻止Deny；不提供Mount Ask／ToUser或终止进程。实际保护树和无法确认的影响范围不由普通Allow覆盖。

自保的正常挂载维护放行使用`config/rules.fast-allow.yaml`中可编辑的File Driver组`linux-self-protection-independent-mounts`（组号5），规则只要求真实`mount_effect=independent_view`。它与其他规则共用PolicyDocument、固定文件、管理权限、原子保存、编译和内核匹配，不另有自保白名单。驱动自保入口只产生拓扑事实并向同一File规则匹配器查询；无真实命中不推定维护允许，命中后日志与规则版本来自原规则行。配置自保对实际AS对象的权限边界仍有效；删除、禁用或修改本规则通过同一普通规则管理接口。Native只通过`SelfMountRules`报告这一直接裁决能力，不把它记为人工等待能力。

写打开问询使用 `operations: [open]`、目标 `access_mask: 2` 和 `Ask`，按位匹配 `O_WRONLY` 与 `O_RDWR`。目标范围仍由文件规则选择，每次命中对应一个原打开。[驱动边界](../../drivers/linux/hooks.md#写打开与写关闭)

普通规则支持的操作以驱动能力为准。已打开 FD 不配置逐次 Write Ask；写关闭只作审计事实。具名 CreateFile 的范围以实际 hook 契约为准。原 flags 和 mode 用于事件保真。

### Network target

- `local_cidr`：原检查时的本地地址范围；`remote_cidr`：当前sockaddr或socket peer的远端地址范围，均为CIDR。
- `local_port`：原检查时的本地端口闭区间；`remote_port`：该次目标远端端口闭区间。
- `protocol: u8`：IANA传输协议号，TCP为6、UDP为17，不是SOCK_STREAM／SOCK_DGRAM枚举。
- `domain: TextMatch`：仅匹配同次UDP／53 Send快照中已解析的DNS查询名，大小写比较与读取边界见本页DNS规则契约。仅Backend可配置；Connect域名与Native条件25仍不支持，不从IP、反向DNS或进程名补造域名。

Connect检查可能早于自动绑定和路由选择，本地地址／端口为0.0.0.0／0或IPv6全零是合法采样，不是最终连接端点。Send优先取显式msg_name目标；没有msg_name时取当前socket peer，分别覆盖sendto和已连接send。当前原生控制仅IPv4／IPv6 Connect／Send，不含listen／accept／receive、TLS正文或MCP消息识别。

### 事实缺失与配置能力

Driver条件能力来自Hello；普通规则及Backend规则采用时核验可裁决操作和必要目标字段，不让不可能供给的条件被静默删除。Backend树补全只补ProcessInfo缺项，需同boot＋TGID＋birth和当前证据代Known、采样区间不晚于事件时间；不补目标argv、新路径或域名。

对应受控冒烟使用上述正式字段，经同一PolicyDocument、编译、原生采用和Backend工厂执行；具体执行与结论绑定各自原件：Exec用source.executable＋uid/euid与target.executable，Connect／Send用同主体字段与remote_cidr／remote_port／protocol。Allow正控通过执行后marker或真实回环连接／字节确认；阻止／终止以原结果、实际wait及接收方副作用分别确认，不从配置保存或信号发送推定完整效果。

## 流量与固定拒绝反馈的现行边界

Flow已接入受控TLS、幻影凭据和原聊天采集；当前规则schema、三类池、驱动快速规则及HIPS原操作语义不变。Flow路由是解析与凭据配置，不是另一套安全规则。MCP未接入。

已配置的固定拒绝文本由本模块唯一管理；真实Denied及明确业务交接满足当前支持范围时，中间层通过[既有窄反馈端口](../../contracts/flow.md#固定规则拒绝反馈目标契约)调用Flow协议追加。端口及软件验证存在不代表stock工具执行来源或真实固定写出已经通过。

基于聊天内容新增安全动作仍缺明确的执行点与规则设计，归[待定模块契约](../../archive/unresolved-module-contracts.md#流量处理剩余决策)，不在Flow新增第四池、通用rewrite字段或Agent整改要求。原始观察不产生OriginalOperation，也不自动授予改写权限。

log控制普通命中日志，聊天采集、业务持久化及内容权限按各自现行唯一契约执行；不以完整工具语义、可选取证或模型输出格式作为普通HIPS运行条件。

前端编辑现有组的执行位置时，必须在同一个原`replace_group`命令内为不兼容的动作选择新值；复用原动作编辑器，作用范围／目标／编号保持原值并一起预览。位置与动作不拆成必然失败的两次保存。前端只限制可选动作的显示范围，真实操作能力、权限和规则合法性仍由原规则owner校验，不放宽后端语义。
