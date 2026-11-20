# 驱动自保

所属大模块：内核驱动。入口和实际状态见[DriverCore](../technical.md)，私有上下文见[DriverContext](context.md)，规则顺序见[文件驱动匹配](rules.md)。本页独占自保对象绑定、真实服务身份与修改判定的设计；底层只通过已授权协议提供固定配置结果。

## 职责边界

DriverSelfProtection绑定配置模块给出的真实根、后端程序及加载器对象，保管原生引用和受保护对象集合，判断实际修改是否属于系统自保前缀项。它不读取配置文件、不发现安装目录、不承担普通文件规则、消息收发、进程树或流量解析。

系统自保对象判定先于普通文件规则。修改权限未通过时立即返回真实拒绝；FileOpen通过后继续同一File Driver池，由实际规则决定Allow／Deny／ToUser。自保系统组1保留实际修改拒绝，维护规则归同源PolicyDocument与DriverImage，不在本模块实现第二份读／执行Allow列表。DriverConfiguration只校验输入，DriverCore负责完整采用和唯一有效状态，不各自保存一份可写自保状态。

## 只读绑定与真实主体

```text
SelfProtectionBinding {
    root: 实际目录path/inode的保管引用,
    backend_executable: 实际后端程序file对象引用,
    loader_executable: 实际加载器file对象引用,
    protected_objects: 受保护inode对象集合及其寿命,
}
```

这是驱动私有逻辑记录，不是传内核指针的wire DTO。当前实现见[as_self_protection.c](../../../drivers/linux/as_self_protection.c)。控制者身份由DriverCore独占，调用时传递当前真实授权结果；拒绝日志选择归同一不可变持久快照，不在对象绑定中复制可写缓存。路径均由底层配置模块的既有布局导出；绑定后用实际对象关系，不用字符串前缀、进程名、UID相同或裸PID复用推定可信。可信写仅限当前认证服务实例在范围内的所属模块受控修改，不转授普通前端／IM或任意fork子进程。

## 真实exec身份与执行规则

`as_self_prepare_exec`只为精确已绑定backend／loader对象和具备CAP_SYS_ADMIN的当前调用建立pending身份候选；返回`0`表示身份准备步骤完成，不表示允许执行。内存分配或既有实例容量不足返回真实errno，不能伪称登记成功。只有真实`sched_process_exec`在同一TGID对象／组出生／exec变化及实际程序对象均吻合时，`as_self_exec_seen`才完成角色登记；exit沿既有生命周期释放。失败exec保留原成功身份，后续prepare只替换未提交候选；fork继承mm.exe、同UID或程序名不能形成角色。

身份准备之后，执行是否允许由普通Process Driver规则及其后端接线决定；不再有`as_self_exec`返回Allow。维护来源／目标规则是正常PolicyDocument行，参与同一顺序和digest。角色仍供真实控制端点及自保修改权限核验，普通Allow不授予这些身份或控制权。控制进程真实退出或成功exec为其他程序时即来源失败；继承／复制的FD不能延长控制者身份。

## 修改准入与普通规则

当前认证控制者的实际TGID对象、已核验backend exec实例和实际受保护对象共同决定修改准入；初始化保护栅栏先完成检查。内部`+1`只表示这份自保修改权限已通过，不构造`backend-write`允许通知。

统一 gate 的普通 File 操作的Self实际负值立即拒绝；自保返回`0`或`+1`后都继续普通File Driver匹配，维护允许来自实际下发规则，不由正值直接覆盖规则。独立自保入口按各自权限点即时检查，后续 FD 写入仍受自保约束。普通问询使用[写打开规则](hooks.md#写打开与写关闭)。

### File gate边界

统一gate的[操作](../design.md#普通文件模块范围)在原同hook内复用上述真实主体／对象判定。Open仍只对FMODE_WRITE或O_TRUNC执行修改准入，不把只读／冷启动Open改成必须有修改权；目录创建、unlink、rmdir、rename、path_truncate 和 mmap_file 的适用修改先判原自保权限。真实负值立即终止，`0`／`+1`均继续同一普通File池；身份通过不是普通Allow。

旧mkdir／unlink／rmdir／rename self-only薄包装已被同物理hook的统一入口替换；Self对象、祖先、硬链接／跨根、交换两端、不可写程序对象及初始化fence保留，不复制到普通Matcher。拒绝事实复用原gate参数和现有通知路径，实际参数存在时保留raw flags／mode，不先造一份普通事件／允许通知。FD 写、file_truncate、link、metadata、file_mprotect 及 mount 等独立自保检查点没有可替代的普通入口，本轮不删除或宣称已获普通问询能力。

实际`-EPERM`按原结果返回，并在采用快照选择记录时由`self_denied_notice`发布`as-self-protection/deny-modification`事实，系统组号1、规则号2及配置来源明确。日志分配／队列失败只报告丢失，不改变原拒绝。实际对象跟踪、程序对象不可写、祖先／跨根修改约束和初始化栅栏继续执行；普通文件Open Allow不能授予外部修改权。

该拒绝通知在原自保回调尚有效时复用既有`capture_execution(r, NULL)`，保留当时能取得的原syscall快照和当前内核栈，字段仍由[事件契约](../../contracts/events.md)定义。没有可用用户态寄存器或内核栈支持时保持原缺失，不在回调结束后补读；`NULL` hook facts不生成用户栈或原点mount namespace。构造事件与路径前先在同一帧容量中预留`AS_EXECUTION_BYTES`，由既有快照函数恢复后插入公共区，不能把预留重复加到实际缓冲之外。采集和帧构造失败仍只沿原通知丢失出口计数，不改变已取得的`-EPERM`或跳过原动作；不新增FileData字段、表或协议版本。

## 保护的修改与边界

覆盖目标按实际hook分别核实：创建、后续FD写、截断、删除、重命名、硬链接、权限／属主／元数据修改、新共享可写映射及mprotect。扩展属性、ACL与文件flags分别接实际inode_setxattr／removexattr、inode_set_acl／remove_acl和inode_file_setattr，不能把inode_setattr视为这些接口的总入口。根本身、祖先和覆盖根的mount／unmount／move-mount同样需要处理，不能只检查普通文件open。

挂载拓扑检查区分实际 VFS 挂载关系与共享 dentry；clone的dentry祖先关系本身不证明它能改变AS原挂载树。同一原回调产生三态`mount_effect`事实：`IndependentView`表示已核独立视图操作；`ProtectedTree`表示实际VFS包含或共享受保护superblock；`Unknown`表示未能确证。`None`只表示未采集。事实不授予Allow，字段／编码与可编辑条件由[事件契约](../../contracts/events.md)及[规则owner](../../backend/rules/technical.md)唯一维护。

目标与固定自保根存在双向`path_is_under`包含时分类为`ProtectedTree`。没有这种实际重叠时，纯传播操作`MS_SHARED／MS_PRIVATE／MS_SLAVE／MS_UNBINDABLE`（仅可另带`MS_REC／MS_SILENT`）和`MS_BIND|MS_REMOUNT`按Linux每挂载树／每vfsmount语义分类；普通`MS_REMOUNT`仍核同一受保护superblock，包括现有受保护inode对象集合中的嵌套文件系统，不能因namespace不同断言独立。

new／bind挂载由原`sb_mount`回调以CO-RE读取同一target mount的真实传播组和下游slaves：两者均表明无向外传播且不重叠原树，才分类为独立。只传该回调的明确布尔事实，不保存mount指针或推定namespace身份。原快照和初始化fence共同约束分类；fence不由普通规则覆盖。只有`IndependentView`经同一快照的原File Driver matcher命中明确含该条件的真实Allow，才可替代旧dentry祖先拒绝，并保留原RuleHit／log链。真实受保护树、未知和未命中仍沿原自保边界；本轮Mount规则仅Driver Allow／Deny，无Ask／ToUser等待。

`MS_MOVE`本轮不扩能力，from／to仍走原move-mount检查。`sb_umount`复用同一无向外传播读取，仍只在真实`MNT_DETACH`、目标是调用者当前根的直接子挂载、与自保根无实际包含时允许脱离。原操作类别依据[Linux path_mount／do_change_type／remount](https://github.com/torvalds/linux/blob/v7.0/fs/namespace.c)与[change_mnt_propagation](https://github.com/torvalds/linux/blob/v7.0/fs/pnode.c)，不按进程名、路径字符串或裸namespace数值设豁免。

已有Root外硬链接仍可指向同一inode，必须由实际对象集合或同等可靠绑定覆盖；不能仅检查当前dentry的目录祖先。新跨保护边界的link／rename不能把受保护对象带出去再绕过检查。RENAME_EXCHANGE的两端都要核实，bind通过实际path与inode关系判断。Overlay的upper／lower外部别名及copy-up关系当前没有完整绑定，根或内部挂载为Overlay时明确返回Unsupported，不声明覆盖该布局。依据[OverlayFS对底层修改的边界](https://docs.kernel.org/filesystems/overlayfs.html#changes-to-underlying-filesystems)，不能把其未定义行为当作AS已保护的证据。

自保从启用后发生的新修改和新权限授予开始；**不承诺追溯撤销启用前已经存在的可写共享映射**。后续FD写仍按实际file_permission检查，新的共享写映射及mprotect必须在授权点拦截。这个明确边界不等于允许遗漏启用后新取得的写权限。

## 初始化保护栅栏与提交

全部镜像先完成结构和语义检查，再在实际应用阶段建立绑定／初始化保护栅栏，阻止所有线程同时制造新的修改、链接、重命名或挂载绕过（也包括控制进程的其他线程）；在栅栏下完成对象集合绑定。栅栏是必要的原生安全操作，不是候选普通规则已经投入匹配。

DriverCore仍只有一次完整持久提交；成功前不宣称SelfProtection Enabled。失败撤掉临时栅栏并保留旧有效策略，不重放期间已被拒绝的操作。来源停止／故障必须终止旧应用并保管清理到完成；排空包括实际应用中的绑定／栅栏，不仅是Held数量。栅栏拒绝不能伪造已采用系统RuleHit，诊断须明确是初始化维护拒绝。

## 冷恢复与维护通道

来源失败后系统自保和确定的驱动Allow／Deny继续；依赖ToUser的操作报告真实来源失败。此时从独立shell重新exec后端仍必须能够走到精确维护通道，不能用已运行控制器代替恢复验证。

绑定要求是实际核验的静态后端与静态加载器，减少ELF解释器和动态库闭包。绑定程序若有未提供的动态依赖，明确返回Unsupported；本模块不再绑定procfs／sysfs／BTF／pins读白名单。普通维护读取与执行通过实际规则数据处理，不按UID或进程名另设允许。

实际底层目录写、规则更新和驱动更新仍通过所属模块受控接口执行，普通规则Allow不授予维护权。若完整对象、修改hook或冷恢复闭包尚未落地，能力报告保持不支持，正式配置初始化停止，不能为通过冒烟删掉自保要求。

### 普通维护规则与启动闭包

[规则模块](../../backend/rules/technical.md#普通文件gate能力与维护)负责生成、持久化及下发同源维护规则；各池及File来源操作集合只由该页定义。规则字段、顺序与实际镜像归同一来源，不在Self复制规则表或静默fallback。

AS来源维护行覆盖新普通操作，避免清理／存储等真实维护重入自己的ToUser；唯一操作集见[规则契约](../../backend/rules/technical.md#普通文件gate能力与维护)。根内目标维护行仍只Open，不扩成外部mkdir／unlink／rmdir／rename Allow；本模块不以controller、同UID或同名程序硬编码替代该规则数据。

目标FileOpen维护规则是实际启动依赖：外部shell／恢复actor打开根内backend或loader的执行文件时，FileOpen先于Process Execute，来源规则尚不匹配；后续Process目标规则不能代替这次Open。实际exec成功后，角色登记仍须独立核验，目标Open／Execute Allow本身不授控制身份。

实际`/dev/as_wait`控制端点仍在LSM打开阶段按S_ISCHR与本次misc_register返回的真实dev_t识别；这是到达端点鉴权的通信职责，设备open仍检查CAP_SYS_ADMIN、真实backend exec身份及唯一控制会话。它不依赖同名路径、普通Allow或旧维护读白名单。

当前绑定有界保管受保护inode与精确exec实例；超出容量真实失败，不丢弃外部hardlink关联。普通文件开关关闭不关闭系统自保修改约束。网络维护直接使用Network Driver数据，不存在controller网络特判或新增维护信任服务。

本职责是固定安装文件自保。保留bpffs pins避免控制者崩溃造成检查入口消失，不等于阻止具有内核管理权限的主体主动卸载模块、解绑BPF link或删除pins；这些管理入口不属于当前保护承诺，不能由文件自保Enabled推定。

## 成熟源码的借鉴范围

- [Landlock fs.c](https://github.com/torvalds/linux/blob/v7.0/security/landlock/fs.c)提供实际inode对象寿命、目录祖先以及link／rename两端检查的参考；不把其仅约束调用线程及后代的作用范围照搬为全机自保。
- [is_subdir](https://github.com/torvalds/linux/blob/v7.0/fs/dcache.c)可用于已pin dentry的祖先关系，不能单独解决外部硬链接、挂载和Overlay。
- [AppArmor hook接线](https://github.com/torvalds/linux/blob/v7.0/security/apparmor/lsm.c)与[文件权限重验](https://github.com/torvalds/linux/blob/v7.0/security/apparmor/file.c)分别处理后续FD权限、truncate、共享写映射及mprotect，不能由open-only推出这些保护。
- [inode-local storage](https://github.com/torvalds/linux/blob/v7.0/kernel/bpf/bpf_inode_storage.c)是可靠标记实际inode的可选现成机制；只有在实际方案采用时才接入，不预建第二套标签框架，也不能用它承诺撤销既有映射。


控制端点不能在security_file_open中依赖最终file_operations识别：[do_dentry_open](https://github.com/torvalds/linux/blob/v7.0/fs/open.c)先调用security_file_open，随后才由[chrdev_open](https://github.com/torvalds/linux/blob/v7.0/fs/char_dev.c)和[misc_open](https://github.com/torvalds/linux/blob/v7.0/drivers/char/misc.c)替换f_op并调用本设备open。精确注册设备号只使调用到达原端点鉴权，不豁免任意/dev文件、普通同名文件或其他root主体的会话认证。
