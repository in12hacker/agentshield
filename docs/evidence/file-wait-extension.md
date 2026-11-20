# 截断与共享可写映射的原操作问询

本轮只扩展同一文件 gate 的三个操作。路径截断接 `path_truncate`。文件对象截断接同时携带原 `ATTR_SIZE` 与 `ATTR_FILE` 的 `inode_setattr`。普通文件取得新的共享可写映射接 `mmap_file`。原 self-only 薄入口由对应 gate 替代。规则、实际回调、载荷和未覆盖点见[驱动契约](../drivers/linux/hooks.md#普通文件gate接线)。

## 已执行的检查

- 本机 7.0.0-1019-nvidia／aarch64 的原生模块、BPF 和加载器构建成功。Clang 18 实际汇编核对三个入口使用固定上下文偏移，并保留前序 LSM 拒绝。原件 `/tmp/as-file-wait-hooks-r4-20261110.s`。
- Rust 工作区 172 项、Python 5 项通过。格式、严格 Clippy、rustdoc 与目标 LSP 通过。定向覆盖两种新 TLV 的宽度、操作号以及零值与高位的 SQLite 读回。
- 最终隔离 guest 32 项通过。新增四种调用各验证一次 Driver Deny、Ask Allow 和 Ask Deny，共 12 次实际操作。核对问询前原进程尚未返回、原决定 ID 和回执同一、拒绝时文件保持原内容、允许时实际截断或映射回写。路径截断无 file 或长度字段，FD 截断和 `O_TRUNC` 目标大小保留零，只有后者带原 ATTR_OPEN，映射保留实际 PROT 和 MAP 参数。真实安全日志读回 22 个文件变更记录，服务退出 0、模块卸载，QEMU 已退出并 join。

最终包 `/tmp/as-file-wait-mvp-r5-20261110`，initramfs SHA256 `e90fc73d2ef297577c9b1b343cfd099802a8ec263ede860a02db4ee1de97ae3c`，核对 153 个源码输入和 18 个产物。报告 `/tmp/as-file-wait-run-r5-20261110/report.json` SHA256 `950fe4e0709d5485232777d02a1e970da697634774b5a3f6cc1c7ce3e6a393aa`。问询、实际文件效果及回执明细在同目录 `middleware-report.json`。

早期候选保留在 `/tmp/as-file-wait-run-20261110` 及 `/tmp/as-file-wait-run-r{2,3,4}-20261110`。首轮旧规则名单拒绝启动。后续两个重复边界在新文件事件上暴露，旧操作号范围拒绝新编号，旧 TLV 上限挡住合法字段。早期 guest 的实际启动、超时、I/O 失败及清理结果分别保留，不计为最终候选的实测。

## 真实 OpenClaw

R29 在原 OpenClaw 聊天完成真实 IPv4 有限验收，候选 `5e7feb0e1836305cf199163921c3a1a36a27449b`。三个真实会话各执行一次，均保留实际工具结果、原 Ask／决定／回执、当场鉴权的原进程实例、目标文件对象及文件前后内容。

- 路径截断用户允许，原 inode 上实际内容从 `source` 变为 `so`，原事件序号 45764。原 hook 无目标长度。
- FD 截断用户拒绝，原工具返回 EPERM，原 inode 与内容保持原值，原事件序号 52569。记录目标大小 0、原属性位 8296 与原 FD flags 131074。
- 首次共享可写映射用户允许，原 inode 上实际内容变为 `Xource`，原事件序号 58300。映射原参数为 requested_prot=3、effective_prot=3、flags=1。

两个 AS 实例各一次 RequestStop 且实际退出 0，OpenClaw、guest 和 QEMU 正常退出，驱动实际卸载。已核对十份原件哈希与 SQLite 三条原事件的规则来源版本及原参数。原件 `/tmp/as-openclaw-evidence-20261110/r29/result.json` SHA256 `b217103ac0f4c661e21a02da49431f3233b24458e68c782a662f432abe6f8e47`。

`O_TRUNC` 在隔离 guest 实测通过，真实 OpenClaw 未执行。当前授权 GLM 原生 IPv6 保留 NOT_RUN。创建前完整路径、一般已开 FD 的 Write 和已有共享映射的 mprotect 升权仍缺普通问询。

## 剩余三项的目标内核复核

以 R52 同一 aarch64 `7.0.0-1019-nvidia` 内核镜像在无网络隔离 guest 执行最小 BPF 加载探测。程序只返回 0，成功取得的描述符立即关闭，所有程序均未附着。可睡眠 LSM 的 `file_open`、`inode_create` 正对照加载成功。目标内核对可睡眠 `path_mknod`、`file_permission`、`file_mprotect` 均返回 EINVAL，并明确报告 `is not sleepable`。可睡眠 `may_o_create` 入口探测及对应三个安全包装函数的可睡眠探测均由同一检查拒绝。guest 正常关机。该结果只验证加载约束，未执行新增问询。

`inode_create` 的原参数为父 inode、目标 dentry 与 mode。完整原挂载路径由更早的 `path_mknod` 接口取得。需要把原挂载引用与同一次创建请求一起交到可等待入口，覆盖正常创建、失败和退役，才能接入正式 CreateFile Ask。[Linux LSM 参数](https://raw.githubusercontent.com/torvalds/linux/v7.0/include/linux/lsm_hook_defs.h)及[创建检查调用链](https://raw.githubusercontent.com/torvalds/linux/v7.0/fs/namei.c)。

一般 FD Write 仍缺对原文件对象和该次动作都明确的可等待入口。已有映射的 mprotect 升权还需证明原 VMA、有效权限和 mmap 写锁下的答复路径。fanotify 的 pre-content 事件复用于多种访问，受原打开模式和文件系统条件限制。seccomp 通知交付原系统调用参数，缺少此时的原 VFS 对象。当前未核实可替代三项原操作等待的接口。[fsnotify 权限路径](https://raw.githubusercontent.com/torvalds/linux/v7.0/include/linux/fsnotify.h)、[mprotect 调用链](https://raw.githubusercontent.com/torvalds/linux/v7.0/mm/mprotect.c)及[seccomp 通知](https://raw.githubusercontent.com/torvalds/linux/v7.0/Documentation/userspace-api/seccomp_filter.rst)。

原件位于 `/tmp/as-kernel-file-gap.C7xnBXq1`。内核 SHA256 `6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6`，`guest.log` SHA256 `18f9ab6eb669984da5ef0b7cf4c411a7443bfd9c2a383ea23d7dff0e32ad7f8c`。目录同时保留探测源码、目标 BTF ID 及构建产物摘要。

## 具名普通文件创建前 Ask

在 aarch64 `7.0.0-1019-nvidia` 隔离 guest 实测。经 bind mount 发起的 `open(O_CREAT)` 收到 File.CreateFile Ask，原挂载路径为 `/suite/create-view/open-allow`，实际 Allow 后在原挂载对象写入 `created`。同一挂载的 `mknodat` 收到独立 CreateFile Ask，实际 Deny 后原系统调用返回 EPERM，原路径无文件。对已存在目标再次 `open(O_CREAT)` 没有产生创建 Ask。服务实际退出，模块已卸载，guest 退出 0。

Native 在 `path_mknod` 获取真实路径引用，在同一 `inode_create` 核对子对象和父目录后复用既有文件 gate。读取未哈希 dentry 时，Linux `d_path` 会追加删除标记。新路径使用已核实的原挂载父路径及父目录锁内的实际子名称，保留原模式及缺失目标 inode。[原路径契约](../drivers/linux/hooks.md#具名普通文件创建的同次请求交接)及[Linux 路径实现](https://raw.githubusercontent.com/torvalds/linux/v7.0/fs/d_path.c)。

同目录重建曾使已改写的 BTF ID 标志重复被解析，隔离 guest 的原有非睡眠入口因此加载失败。构建脚本现在在 live-base 分支重新链接 Kbuild 模块，再解析 BTF。重复构建对照的 `.BTF_ids` 逐字节相同。初次失败日志 `/tmp/as-create-guest.KqcASLJE/guest.log` 保留。既有广覆盖 guest 的 target argv Deny 检查在候选和 R52 原生资产对照上均复现失败，具名创建定向验收单独判定。

本地工作区 183 项通过，严格 Clippy 通过。具名创建报告 `/tmp/as-create-guest.YFUD2oSV/guest.log` SHA256 `388fe19ef7f1011c69835192c431eaaebe6aa26609b710c667bc83686af89eab`。冻结 OpenClaw 候选 `/tmp/as-named-create-final.Vn49Foob/package`，镜像 SHA256 `ffe819097488762eedb531cd67d478d699e68a80f67235b4635294aeb4d11e69`。

原版 OpenClaw R53 在同一 aarch64 内核隔离 guest 通过。真实 write 请求发出前目标不存在。收到一次 File.CreateFile Ask，原事件 1/42785 的目标 inode 为空。答复前原 `openat(O_CREAT)` 在 `security_inode_create` 的 Native 等待路径上。实际 Allow 后同一原事件、决定 ID 与规则取得允许回执。退役证据中原路径的文件内容与原工具实参逐字一致，业务库和安全日志保留同一原记录。OpenClaw、AS、浏览器和 guest 正常退出，Native 卸载。OpenClaw 本轮覆盖实际 Allow，Deny 与 mknodat 使用上节独立 guest 证据。

R53 原报告 `/tmp/as-openclaw-evidence-20261111/r53/result.json` SHA256 `0d7184b8d769855ec08172ea7cd1db552e3ac35237431963c9779861f62943d0`。40 项原件和 21 项脚本清单已核实，只读复算逐字节一致。三次模型流的 706 条公开投递逐值入库，另有一条结束后独立记录的来源关闭状态。此次验收未纳入跨来源执行因果桥。

## 普通 FD 写入等待复核

Linux 的常规标量及向量写入先在 `rw_verify_area` 把原 `file` 与 `MAY_WRITE` 交给 LSM，LSM 放行后进入 `fsnotify_file_area_perm`。这是写入内容变化前的实际检查。pre-content 等待只对打开时取得 HSM 通知模式的文件描述生效，通知复用读、写等访问路径，送入 `fsnotify_pre_content` 时不传 `MAY_WRITE`。seccomp 用户通知交付系统调用参数，未持有 VFS 后续解析的原 `struct file`。[Linux VFS](https://raw.githubusercontent.com/torvalds/linux/v7.0/fs/read_write.c)、[fsnotify 权限检查](https://raw.githubusercontent.com/torvalds/linux/v7.0/include/linux/fsnotify.h)、[打开时的通知模式](https://raw.githubusercontent.com/torvalds/linux/v7.0/fs/notify/fsnotify.c)、[seccomp 用户通知](https://raw.githubusercontent.com/torvalds/linux/v7.0/Documentation/userspace-api/seccomp_filter.rst)

成熟的 AppArmor 在原 `file_permission` 回调复核文件权限，无约束主体与已缓存授权可直接通过。Ubuntu 的文件权限询问由 AppArmor 配置中要求询问的权限驱动。该机制没有给 AS 提供一个能在运行时接管既有、未受约束进程的每次原写入的回调。[AppArmor LSM](https://raw.githubusercontent.com/torvalds/linux/v7.0/security/apparmor/lsm.c)、[文件权限复核](https://raw.githubusercontent.com/torvalds/linux/v7.0/security/apparmor/file.c)、[Ubuntu 权限询问](https://discourse.ubuntu.com/t/permissions-prompting-a-deep-dive/81785)

在相同的 aarch64 `7.0.0-1019-nvidia` 内核上运行无网络隔离 guest。BPF 只尝试加载，全部程序均未附着。可睡眠 LSM `file_open` 正对照加载成功。`file_permission` LSM、`security_file_permission` fmod_ret、`rw_verify_area` fentry／fmod_ret、`vfs_write` 和 `vfs_iter_write` fentry 的可睡眠加载全部返回 `EINVAL`，验证器明确报告 `is not sleepable`。

在同一 ext4 inode 上保留两份独立打开的文件描述。标记前打开的 FD 执行 `pwrite`，没有产生 `FAN_PRE_ACCESS`，返回 4 字节且实际内容已变化。标记后打开的 FD 执行 `pwrite`，产生匹配发起 PID 和目标设备号／inode 的 `FAN_PRE_ACCESS`。答复前原调用未返回且内容保持原值；回复 `FAN_DENY` 后原调用返回 `EPERM`，内容仍未改变。探测进程与 guest 正常退出。此实测证明标记后符合条件的同一次 `pwrite` 可等待，也证明它无法覆盖监控前打开的 FD。其他常规写入变体的共用检查点来自源码，本次实际写入探测只执行了 `pwrite`。

原件位于 `/tmp/as-write-entry-proof.2Ic3nyzF`。内核 SHA256 `6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6`，`guest.log` SHA256 `0a7a3f9cf1494ee682da3262b58d9da8a30caaf6a8fbcf1bec5d76f818661609`，`initramfs.cpio.gz` SHA256 `ba7dd061463a3812b3bbf537863ca8d2b8ffa372d16e2bb8789551fb91e1cdb6`。本次没有普通 Write Ask 生产候选，OpenClaw 未执行该项。后续产品决定取消逐次 Write Ask，现行范围见[写打开与写关闭](../drivers/linux/hooks.md#写打开与写关闭)。

## 已有共享映射增加写权限的复核

Linux 先持有 `mmap_write_lock`，对每段真实 VMA 调用 `security_file_mprotect`，随后才执行 `mprotect_fixup`，更新 VMA flags 与页表权限。AppArmor 和 SELinux 在该原生 LSM 回调内使用 `vma->vm_file`、`VM_SHARED` 和有效权限复核策略。Ubuntu 的交互询问由 AppArmor profile 的 prompt 规则触发，未提供给 AS 的同次原 VMA 等待入口。seccomp 通知在进入系统调用实现前交付寄存器参数，userfaultfd 写保护通知发生在访存缺页，两个位置均未提供原 VMA 升权回调。[Linux mprotect](https://raw.githubusercontent.com/torvalds/linux/v7.0/mm/mprotect.c)、[AppArmor](https://raw.githubusercontent.com/torvalds/linux/v7.0/security/apparmor/lsm.c)、[SELinux](https://raw.githubusercontent.com/torvalds/linux/v7.0/security/selinux/hooks.c)、[Ubuntu 权限询问](https://discourse.ubuntu.com/t/permissions-prompting-a-deep-dive/81785)、[seccomp](https://docs.kernel.org/userspace-api/seccomp_filter.html)、[userfaultfd](https://docs.kernel.org/6.12/admin-guide/mm/userfaultfd.html)

- 在 aarch64 `7.0.0-1019-nvidia` 无网络 guest，`mmap_file` 可睡眠 LSM 和 `__arm64_sys_mprotect` 可睡眠 fmod_ret 加载通过。`file_mprotect` LSM、`security_file_mprotect`、`do_mprotect_pkey` 及 `mprotect_fixup` 的可睡眠加载均返回 `EINVAL`，验证器报告 `is not sleepable`。这些加载探针均未附着。
- 即时裁决对照使用单独的非睡眠 `file_mprotect` 诊断程序。程序附着前已建立普通文件的只读共享映射，并关闭原 FD。实际升级返回 `EPERM`，子进程继承的映射尝试写入触发 `SIGSEGV`，文件仍为 `A`。卸下诊断程序后，同一映射的另一次升级成功，写入并同步后文件为 `C`。该对照验证权限变化和实际文件效果，未执行人工等待或 AS 普通规则。
- 过早等待对照使用 seccomp 通知挂起同一个 `mprotect`。同一进程的另一线程在等待期间把相同虚拟地址换成不同 inode 的只读共享映射。用同一个通知 ID 继续原调用后，返回成功，原文件仍为 `A`，第二个文件经映射写入变为 `C`。该对照验证只持有系统调用与地址时，内核随后选中的 VMA 可以是另一个对象。可睡眠系统调用入口的加载结果本身不证明原 VMA 被保管。

两轮 guest 均正常退出。内核镜像 SHA256 `6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6`。加载及即时裁决原件 `/tmp/as-mprotect-entry-proof.EoRuHPJB/guest.log` SHA256 `76b772bdb97b896e2d18ba9c393e98fa1edba5847f3331bc979e930e766816ec`。过早等待原件 `/tmp/as-mprotect-early-entry.k2RUVR7m/guest.log` SHA256 `d5b1726ef0ec55acb9c55e390fd8fe7ef69f743c07a42204ebff6df4f4fd25af`。两处保留探测源码及 initramfs。本次无普通 Ask 生产候选，OpenClaw 未执行该项。见[驱动契约](../drivers/linux/hooks.md#已有共享映射增加写权限)。

## R54 写打开 Ask

原版 OpenClaw 在 aarch64 `7.0.0-1019-nvidia` 上用 R53 冻结候选通过两个已有文件写打开用例。每个原 `openat(O_RDWR)` 只产生一条 `File.Open` Ask，原 `access_mask=3` 包含规则配置的写位 `2`。答复前两次原调用均停在 `security_file_open` 的 AS 等待链。

- Allow 对应原事件 1/41800，原打开返回 FD 197。工具成功，原 inode 24 的实际内容与写入实参逐字节一致。
- Deny 对应原事件 1/50281，原打开返回 `EPERM`，工具报告同一路径的错误，原 inode 25 的内容与操作前逐字节一致。

两项的原决定、事件、规则和回执均相符，业务库与安全日志保留对应原记录。OpenClaw 和 AS 正常退出，Native 卸载，guest 与 QEMU 正常退出。验收范围为两次真实 `O_RDWR` 打开，`O_WRONLY` 未单独执行。写关闭、已开 FD 的逐次 Write、`mprotect` 升权未纳入。原报告 `/tmp/as-openclaw-evidence-20261111/r54/result.json` SHA256 `f7b0bf796bf273ad200f49628656cb4316aca68f9ce5ac573c6d25ccb25887f1`，已复核证据清单所列哈希。

## 写关闭审计采集

复用 Linux fanotify 通知和原安全日志。初次隔离启动返回 EBADF。原 `fanotify_mark` 的空路径分支使用 `fdget`，内核拒绝 `O_PATH` 目录描述符。换用按同一实际目录对象校验的只读目录描述符后，真实标记及采集通过。[Linux 标记实现](https://github.com/torvalds/linux/blob/v7.0/fs/notify/fanotify/fanotify_user.c#L1113) · [fdget 实现](https://github.com/torvalds/linux/blob/v7.0/fs/file.c#L1132)

aarch64 `7.0.0-1019-nvidia` 隔离内核与正式 WS 共九项通过。只读关闭、首个重复 FD 关闭及未标记子目录均无写关闭记录，最后一个可写 FD 关闭有记录，文件仍为空。WS 返回的内核 TID、原始路径字节、设备号和 inode 与原对象核对，原 SQLite 记录逐值匹配。原写打开 Ask 闭环及只读权限测试同时通过。服务、guest 和 QEMU 退出 0，原生模块实际卸载。

候选 `/tmp/as-write-close.3BDZKGZU/checked-candidate`，镜像 SHA256 `f37fbf630e68b426800d75adb2cf3111cee9299071ff7bb1f2a7d1bf3020157c`，as-service SHA256 `b6f0fa5f2d3a03b08aaf66f987d51f564dd3bd8f76ccb7f46526babd11bcb05e`。164 项源码、20 项产物清单 SHA256 `42eadc949b6ab174bbaf62a00043564705dc7d2ed98cd5d33661d5ceab2c40a0`。原隔离报告 `/tmp/as-write-close.3BDZKGZU/checked-run/report.json` SHA256 `08e1d183f0a820065013be3992f0f4497143b680e9fb21b290a5038fb86b697c`。保留首轮失败及诊断原件。全工作区 188 项 Rust 测试、受影响严格 Clippy 及 LSP 引用检查通过。[写关闭契约](../backend/security-log/technical.md#写关闭审计)

同一服务产物另在已标记目录启用按整个目录匹配的 Open Ask。原进程的 `O_RDWR | O_TRUNC` 产生一次 Ask，允许后原文件清空，原关闭记录经 WS 读取并与 SQLite 核对，消费期间未出现第二次 Ask。此对照核对 Linux 为 fanotify 通知创建文件 FD 时的实际行为，复用既有 AS 维护规则。服务及 guest 正常退出。原件 `/tmp/as-write-close.3BDZKGZU/same-path-ask/guest.log` SHA256 `3133a4f14442e258b0d668a26a0431189816449b705380a36c425966e455f84d`。

### R65 原版 OpenClaw

同一冻结候选用两个新的原版 OpenClaw 会话实测。原写打开在答复前等待。Allow 对应原事件 45391，原 `openat` 返回 FD 207，工具实际写入 35 字节。独立内核通知与安全 SQLite 行 10 的 TID 168、设备 253:0、inode 25 和路径原字节一致，ManageService WS 第 24 行返回同一记录。Deny 对应原事件 52500，原 `openat` 返回 EPERM，文件原 30 字节保留，独立通知及 SQLite 均无对应写关闭。两个原 Ask 各有一个原执行结果和一次目标规则命中。写关闭未增加问询、执行结果或规则命中。安全日志 11 页与 SQLite 逐行一致。

OpenClaw、AS、浏览器、guest 和 QEMU 正常退出，原生模块卸载。报告 `/tmp/as-openclaw-evidence-20261112/r65/result.json` SHA256 `061833d73d333e802b75cb03a1b39044e624905bcff7d6089fb4984025df44b5`。49 份证据及 24 项脚本摘要通过，已独立复核内核通知、WS、SQLite 和实际文件效果。原版子目录场景、扫描和精确 HTTP 工具交接未执行。
