# 驱动规则匹配

所属大模块：内核驱动。产品边界见[驱动设计](../design.md)，组合和职责见[驱动架构](../architecture.md)，唯一入口见[驱动总契约](../technical.md)。驱动内部只读上下文见[DriverContext](context.md)，跨模块协议见[通信契约](../../contracts/driver-communication/technical.md)。本页定义本职责的唯一技术细节；不导入底层实现。

DriverRuleMatcher按EventKind选择三个固定派生。每个派生的输入是已有EventData的对应类别事实、同一DriverPersistentSnapshot的只读视图；输出沿用DriverMatch的动作／可选命中语义或明确协议错误。这是原生内部调用语义，不是导入Rust Result／模块实现或新增RPC方法。

- ProcessDriverRules只处理[ProcessOperation](../../../crates/contracts/as-types/src/event_operation.rs)和ProcessTarget。
- FileDriverRules只处理FileOperation和FileTarget，并在同一个文件池中先检查配置系统组，再检查普通开关和普通组。
- NetworkDriverRules只处理NetworkOperation和NetworkTarget。

共同规则是按组号／规则号首条命中，命中输出原 RuleHit。Driver 无确定命中时，有任一可能匹配的后端候选才 ToUser，否则默认 Allow。两者 `hit=None`。镜像生成只读操作索引，在本操作两端均无候选时于采集、分配和等待前短路。源失败／退役仍返回原错误，自保和进程观察保持原职责。

镜像的三类typed backend_routes由同一规则编译结果派生，复用原as_condition。每条规则最多使用一个必要目标条件：进程目标执行文件精确／前缀、文件访问掩码或原路径精确／前缀、网络远端端口范围；投影选择顺序由[规则模块](../../backend/rules/technical.md)唯一维护，不增加匹配器。无可投影条件保留无条件操作候选。原生只在条件明确不符时排除，相符、缺失或无法判定仍移交。候选不授予动作、不产生RuleHit；唯一字节定义归[通信契约](../../contracts/driver-communication/technical.md#持久镜像内容编码)。

结果中的Action只允许Driver阶段的Allow／Deny／ToUser；Ask仍只属于用户态询问池。显式ToUser即使没有后端候选也照常移交。Termination字段仍保留原语义；原生支持None／Actor／ActorAndParent，Deny的实际终止由原执行者复用DriverTools完成；信号结果与原操作拒绝分别报告，不把仅阻止或信号接纳伪报为已退出。缺少必要事实或不支持条件时明确返回错误，不能当成未命中或删条件后的允许。

规则输入只接受规则模块已按实际能力编译的Driver表示。当前镜像编译只允许Exact／Prefix驱动文本条件；Glob／Regex继续属于已定义的用户态匹配，不能因新增派生就声称内核支持正则。操作枚举和字段按既有模型；Process.Create／Process.Exit按[hook契约](hooks.md)供给观察事实，不因此开放新的可阻断规则动作；文件创建准入与进程出生不是同一操作。

```text
DriverRulePools {
    process: ProcessDriverRules,
    file: FileDriverRules,
    network: NetworkDriverRules,
}
```

DriverPersistentSnapshot由[DriverCore](../technical.md#有效状态与原子提交)保管；这里的DriverRulePools只是其中的只读匹配视图，不是新的可编辑配置。三个派生只持有效快照中两个来源镜像的只读视图，不能各保存另一份规则内容或active版本。文件视图按配置系统前缀＋普通文件组组合，仍是一类池；组号和RuleHit.source由原镜像确定。

原生视图表示镜像中已经定义的内容及实际绑定，不是把Rust的Vec、Arc或公开镜像对象直接复制到内核。当前使用固定LE规则／条件记录和每类只读指针视图，字节定义见[as_protocol.h](../../../drivers/linux/as_protocol.h)与[通信契约](../../contracts/driver-communication/technical.md)，不复制另一份布局。用户态DriverPolicy::evaluate是语义参照，不能成为内核执法实现或加载证明。

## 固定拒绝反馈的目标投影

本节已接入当前镜像及原生裁决回执。提示词只在底层规则模块保存；驱动不保存字符串，也不执行LLM修改。规则导出仅将非空文本的Driver Deny派生为report_denial；唯一字段及编码见[驱动通信目标扩展](../../contracts/driver-communication/technical.md#固定拒绝反馈的目标编码)。它是上报选择，不是新匹配条件、动作、优先级或规则池。

实际直接拒绝且该标记为1时，使用现有AS_DIRECT／原EventData／原命中／原执行回执上报，不受log=false吞掉。log仍按原设置决定日志；Allow噪音规则仍按原选择过滤。内核采集失败、未命中或非实际Denied不制造反馈拒绝；底层从原PolicyLease补固定文本，不能在内核请求提示词或人工等待。

## 直决计数

普通规则实际命中并直决Allow或Deny时，在原执行路径产生一次紧凑计数增量。系统自保采用的真实拒绝按原配置规则身份计数。无原进程身份的系统操作、初始化栅栏、未命中默认放行及已走受控链的ToUser不写该增量。计数不受log或固定拒绝反馈开关控制；原执行结果及日志上报条件继续独立。驱动不按查询保管累计历史，不等待底层消费。编码与丢失契约见[驱动直决计数](../../contracts/driver-communication/technical.md#驱动直决计数)。

## 实现边界

规则文件读取、模式解析及平台镜像编译属于底层规则模块。驱动只校验镜像与本机能力并构造只读视图，在原hook使用它；不调用PolicyEngine或DriverPolicy::evaluate代替内核匹配。现有DriverMatch／Action的语义和RuleHit／Decision等协议含义复用已有定义；原生表示按唯一镜像契约实现，不把Rust对象入核。

三个派生不持各自的active指针、不独立发布版本、不写日志。文件系统组只占同一文件池的前缀组，不在配置子类另建匹配器。Allow／Deny是原hook直接结果；显式ToUser或有后端候选时的未命中交原操作保管，不能先返回0。无相关候选的默认Allow不构造规则命中。

## 按操作接入hook

本节的“操作”使用已有event_operation枚举；“裁决”是Allow／Deny／ToUser。当前和目标可用操作分别见下文，不能从枚举存在推出支持。hook表明实际检查位置，操作表明本次权限请求，两者不是一一对应的类名。成熟实现依据见[操作匹配调研](../references.md#hook到操作规则匹配的源码依据)。AS采用以下单一路径：

```text
实际hook参数
  → DriverHooks填约定EventData，并保留原执行点
  → DriverCore检查stage与实际能力，取得同一有效快照
  → EventKind选择Process／File／Network派生
  → 本派生按operation筛出同一池中的适用规则
  → 按原group_number／rule_number顺序，首条完整命中
  → Allow／Deny原点返回；ToUser交DriverOperations上抛并等待
```

- bprm_check_security 接 Execute。文件入口与实际操作按[统一接线](hooks.md#普通文件gate接线)进入 File 池。
- file_permission当前只实施文件系统组的实际后续写自保，不开放普通Read／Write的可等待规则；管道权限／内容仍按[管道边界](hooks.md#管道hook与mcp消息拦截)处理，不另建Pipe派生。
- socket_connect／socket_sendmsg分别接网络派生的Connect／Send。
- sched_process_fork／exec／exit接Observation事实路径，不参与驱动裁决匹配。创建前task_alloc与这些事实入口不混用，其控制字段及可等待能力尚未确认时不开放对应规则。

普通 Driver 控制源码包括 Process.Execute、九项 File 操作和 Network.Connect／Send。其余系统自保操作的能力以实际入口为准。见[hook 契约](hooks.md)。

### 普通文件gate匹配

[文件操作](../design.md#普通文件模块范围)接入同一FileDriverRules；Driver／Backend能力与维护源操作集只在[规则owner](../../backend/rules/technical.md#普通文件gate能力与维护)定义。没有新的操作规则池、每hook可写缓存或另一套优先级；Self仍先检查真实修改权限，普通阶段继续按组号／规则号首条命中。

FileTarget仍使用原path／new_path／device／inode／FMODE access_mask条件；Rename的两个路径是同一原操作的两个角色。一个规则内仍AND，同类别多条规则依原顺序表达不同覆盖条件；RENAME_EXCHANGE不自动交换字段再跑第二个Allow／Deny，也不隐藏增加目标侧阻止优先。Ask展示原双路径和raw flag，用户可在既有字段中显式限制两端；本轮不增加flags／mode条件或从参数值推其他动作。

NewPath 取 Rename 原目的路径，access_mask 只适用于 Open，inode 条件不能包含 CreateFile 或 CreateDirectory。C 采用和底层编译核对同一约束。字段缺失沿原 Missing 语义保留，日志继续由原 log 字段及结果链控制。

规则只保存一份，使用既有operations条件作操作筛选；多操作规则仍只属于一个类别／阶段池，不复制成多个规则或独立优先列表。开始时沿用这个简单筛选，不新增每hook可写规则缓存；具体内核表示若采用操作位集合，位定义随同一镜像编码确定，不与可编辑operations形成两份可矛盾内容。

hook选择操作不能调整既定优先级或丢掉前面的ToUser。文件系统自保组仍先于普通文件开关和普通规则；其他规则的Allow／Deny／ToUser仍混排。一个hook若实际请求多个权限，在组合语义尚未明确时不能只取第一个bit，也不能拆成多个可独立放行的原请求，缺口先[存档](../../archive/unresolved-module-contracts.md#驱动内部尚未确定的表示与绑定)。

DriverHooks只采样；DriverRuleMatcher只筛选和匹配；DriverCore只组合结果；DriverOperations执行保管与一次性收尾。规则类不知道具体内核函数名，不将某hook的错误码或等待上下文写进共享匹配算法；这些由实际执行者按已确认能力处理。

## 与底层规则体系的唯一接点

组／规则字段、阶段动作、容器、系统自保前缀、移交命中及日志一致性均以[既有规则整体契约](../../backend/rules/technical.md#驱动接入与既有规则的整体一致性)为准。本类只消费其中Driver阶段的不可变表示，不定义另一份字段、优先级或“无前端”开关；Remembered／Fast／Ask只由底层规则模块处理。
