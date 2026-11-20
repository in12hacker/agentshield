# 普通文件模块联调证据

本轮只覆盖普通Open、CreateDirectory、Unlink、RemoveDirectory和Rename，交换改名保留原Rename与flags。唯一字段见[事件](../contracts/events.md)，实际hook／锁边界见[Hooks](../drivers/linux/hooks.md)，普通规则归[规则模块](../backend/rules/technical.md)。源码、包和独审由本轮不可变候选记录绑定；本页不重定义协议。

## 实现与独立审核

五项共用DriverCore私有File gate，同一实际快照／版本／起点先做必要Self准入，再匹配普通File池；保留原Operation、等待、期限、取消、回执及终止。四个旧self-only薄包装替换为对应sleepable入口，其余独立Self入口保留。加载器的27个物理程序是8个普通sleepable与19个独立Self，不叠加第二入口。

FileData原结构新增两原hook标量，原TLV／EventData校验和填缺失共用同一含义；Some(0)不擦除或推成None。普通规则按每个operation校验字段来源；源维护同一持久规则扩五项，目标根维护仍只Open。SQL原file_payloads增加独立整数列，初始化拒绝旧布局；不迁移、不补默认、不保存第二事件JSON。

Native独审原件`/tmp/as-file-gate-native-independent-review-20261110.md`，Rust独立复核`/tmp/as-file-gate-rust-root-review-20261110.md`，存储／实际调用者／冒烟独审`/tmp/as-file-gate-storage-smoke-review-20261110.md`；各报告附受审hash。存储独审发现的同组重复规则编号及旧FILE表初始化漏检已修正并复核；未作为额外门槛或扩大功能。

## 本地检查

显式source本机CI工具后执行当前候选：MSRV1.89的workspace/alltargets测试172通过、0失败，strict Clippy、fmt、rustdoc和现有软件冒烟通过；日志`/tmp/as-file-gate-workspace-{tests,clippy,fmt,rustdoc}-20261110.log`、`/tmp/as-file-gate-software-20261110.log`。软件驱动明确只证明模拟范围，不充当真实保护。

作者LSP覆盖事件／能力／Compiler和实际引用，Root补SQL初始化／字段及调用者；原件`/tmp/as-file-gate-final-lsp*`、`/tmp/as-file-gate-root-lsp*`。正式源诊断无error。Native普通UID完整构建日志`/tmp/as-file-gate-all-build-r2-20261110.log`；首轮缺libbpf头文件失败原件保留，修正明确构建输入后成功。模块BTF和kfunc／BPF段由独立审核读取核实，构建或BTF不等于加载保护。

## R1真实隔离guest

实际包`/tmp/as-file-gate-delivery-r1-20261110`，initramfs SHA256 `4ddb897a2dd25d949c08de77cfb1d9657e0ab1389493dfa9370ded9730583e4a`；运行内核7.0.0-1019-nvidia、aarch64，Image SHA256 `6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6`。宿主UID1000运行自有QEMU，真实Root只在guest内，不使用宿主broker或改宿主内核／签名／信任／网络／OpenClaw。

原报告`/tmp/as-file-gate-native-run-r1-20261110/report.json`，SHA256 `9d328141ed15bd4e3f8fe8725ef3ab546f9294815813854cf5fb3e4380b6fbd4`；原middleware-report SHA256 `91f04068be639faaa9a18046a2efb3acd3ab66e171d80c500af1f06b56eedaf5`。29项必需PASS，持久安全日志仍明确NOT_IMPLEMENTED。guest退出0、实际模块卸载、QEMU退出0且已join，未超时；耗时119.6秒。完整原串口与历史结果保留，不覆盖。

新增同一文件mutation场景共10次实际操作：mkdir、unlink、rmdir、rename和exchange各做Driver Deny及Ask Allow。拒绝真实errno=EPERM、原名字及内容未变；人工决定前原子进程仍持有操作，原Ask／completion的decision_id、完整event和hit相等，回答后真实创建／删除／改名／交换效果成立。规则覆盖整个资源目录，没有PID／探针名／唯一参数限制。

mkdir原hook mode实际493（0755）、新目标inode为None，device保原文件系统；subject显式umask0，来源依据是Linux v7.0的[filename_mkdirat](https://github.com/torvalds/linux/blob/v7.0/fs/namei.c#L4949)和[mode_strip_umask](https://github.com/torvalds/linux/blob/v7.0/include/linux/namei.h#L184)，同时核过匹配发行版头文件。不能据此推最终inode权限。rename原flags0、exchange原flags2，双路径与旧对象inode保留。等待期间未stat或打开被VFS锁住的目标目录，效果只在原操作返回后读取。

本轮同时保留并通过既有Open、进程／网络Allow／Deny／终止、Ask超时／断线／无前端、运行开关／自保、受控TLS／幻影key及正常停止场景；它不证明未执行的宿主或其他架构保护效果。

## 验收边界

交付候选包`/tmp/as-file-gate-delivery-r2-20261110`，镜像SHA256 `7f9ae148f567c72566721b270492aea0f2d1d2f5a53d352ce221bf892953e75a`，manifest SHA256 `69ab40423ae558a04a07dadd212a08a4c1dbe9dadb1b26b32324553638ebe0a0`。151源与当前文件逐hash无差异，18个实际运行产物与R1逐hash完全相同；源差异只有两份README事实说明。R2未另跑本地guest，不把R1镜像号改为R2；原运行产物保护效果及此次等同性核验分别记录。

原R20／R21有限范围保持各自原证据；本轮新增stock范围及独立失败见下节，不能由guest或旧批次代替。

普通文件创建前完整路径Ask、已打开FD Write Ask、完整truncate／映射等待仍未实现；MCP暂缓，容器实例绑定和独立网络命名空间／IPv6受控TLS出口另属已有缺口。Allow表示原LSM点准入，最终系统调用成功只由本轮真实文件系统效果分别证实。

## R22原OpenClaw有限真实验收

原指定聊天已完成本轮：**文件Gate有限范围PASS，新增场景聊天公开明文字段完整性FAIL，不声明整候选无条件PASS。** Root已读取原聊天最终状态及实际结果并核SHA，没有操作OpenClaw、重跑其脚本或改写其原件。

结果`/tmp/as-openclaw-evidence-20261110/r22/result.json` SHA256 `99a12f41f1816fc7860ab6bc0aa940fc090783223aa9967fd5c80411f36dfd89`；交接`handoff.json` SHA256 `80684d7219272abaee888a76e1a83211b0aa542b85398ecc5bef1fd5cf9ee774`。候选72b94e…、提交661c5c16d…、151源／18产物／R2包／匹配内核逐hash无差异；R1的本机29项实际通过与R2 Native未另执行继续分列。

stock OpenClaw新建目录、删文件、删目录、普通改名和交换改名，各Ask允许／拒绝，10次原操作全部取得原Ask、同决定回执及实际副作用；每场景只创建一次原请求，没有重发。path／new_path、device／inode、mkdir原mode0755和inodeNone、rename原flags0／exchange flags2、header、命中规则代与typed SQL精确一致。普通Open和既有5项stock主链亦通过；同一原回答连接的负载后新Ask未回答按原20秒TimeoutDeny，未重建回答身份。

SQL真实13个questions／operation_reports／question_completion，UserAllow6、UserDeny6、TimeoutDeny1。15100个公开聊天envelope与SQL15101条原tag／payload／revision／提交顺序逐条核对通过；额外一条是SQL-only真实shutdown source_closed。公开Gap与observer_capacity均为0；原字段已存不等于被遮蔽字段已恢复。

聊天字段的原协议／SQL oracle仍10次不成立：实际清点33条脱敏协议记录，其中10条client请求整段遮蔽，63条消息有Redacted part。旧第一项360秒观察器FAIL、第二项中断时RUNNING和当时8个NOT_RUN完整保留于stock-file-gates-initial。两个已完成请求从同一原件补核Gate事实，只继续8个从未发送的新场景；没有把旧观察失败改绿、用ID／时间／PID／路径推原文或推工具到内核的关系。独立Flow反例见[可读性证据](flow-redaction-readability.md)，缺口仍未修复。

真实收尾在全部16个UI请求及业务读回之后：两个AS实例各一次RequestStop，Accepted、本地written、本实例STOPPED和外部wait0分证，无AS停止信号回退；OC／AS／guest／QEMU均正常退出、实际卸载join，无强清理，原宿主身份不变。

NOT_RUN：新增mutation的stock直接Driver Deny、目录到目录rename；不能用本机R1负例改称stock通过。未实现操作仍按上节保留。完整范围及原失败均在本轮原结果中，不从顶层有限verdict单独推断完整产品通过。
