# 原操作执行现场采集

## 公共线程事实与BPF用户栈

本轮沿原capture/append_process_info及原as_original补入namespace TID链、普通BPF hook内的mount namespace及用户原生IP栈。唯一结构和TLV见[事件契约](../contracts/events.md)。公共BPF helper在候选预检查后取current及公开栈API，272字节私有标量输入只在当前hook栈存在，Native在等待前写进原事件，不保存BPF指针、map或另一条队列。rename的原flags随同标量输入保真传入；全部普通kfunc最多5参。

用户栈的实际负errno、成功0和成功IP序列分别保留。内核栈与用户栈在业务库共用event_stacks/event_stack_frames及同一算法，旧kernel专表已删除，缺当前schema的旧库明确拒绝。原记录消费结束释放内存，审计只保留历史事实，不能据历史地址取得当前对象。

目标7.0.0-1019-nvidia、aarch64的模块/BPF/静态加载器构建、clangd、Rust LSP、198项本地工作区回归和严格Clippy通过。隔离guest的真实verifier及WS十项通过：原Open Ask的TID链与等待线程/proc NSpid一致，mount字段与/proc namespace链接一致，用户栈成功正返回、首IP与原syscallPC一致；Ask/回执和SQLite保真，实际Allow/Deny与退出/模块卸载通过。报告 `/tmp/as-callback-facts.ExwNZJ71/guest/report.json`，SHA256 `41c9e02f05c60da8b9dd81eecaa6db8b2676b1d1fe171709b8aa4af6f3863bc9`，该guest镜像SHA256 `b61d0842668c8814f06980ca10df2c737716c8c9d91ffff2d5b0183e5e2c8f28`。

随后用户指定用现有原件接内部行为链、禁止重跑OpenClaw；最终业务查询仅进行了离线原件验证与相应本地检查，不能把本次guest结果称为新的OpenClaw验收。最终冻结与行为链结果见[离线接入](runtime-correlation-research.md#现有原件的行为链接入离线)。BPF原生用户IP栈不等于完整语言/异步栈，也没有凭此创建工具归因。

## R67基线实现与边界

2026-11-12，本批在普通进程 Execute、文件控制操作及网络 Connect／Send 的原事件中接入系统调用寄存器快照与内核调用栈。唯一字段／TLV来源见[事件契约](../contracts/events.md#原操作执行现场)。驱动快规则先分流，只有需要上报的 Controlled／Direct 在原线程、等待和终止之前采集。无候选默认放行、无报告的快速命中及生命周期／自保通知不新增栈展开。

Linux `syscall_get_arch/nr/arguments`、`task_pt_regs`、`user_stack_pointer`、`stack_trace_save` 提供实际值；没有读取Agent私有对象、共享SDK、固定Node布局或延迟解引用。PC/SP是保存的用户入口寄存器，不宣称PC必为syscall指令地址。六个参数按内核返回位模式保留，不按事件类别猜系统调用或参数意义。栈缓冲满只报buffer_full，不据此断言完整或截断。

原 as_original 缓冲预留352字节，在快速匹配后一次原位插入共同TLV；原规则容量检查同批计入预留。数据由原队列及Arc事件拥有者传递，瞬时字段禁止后补／覆盖，不进入进程树；必要业务记录在原事务内按列保存，安全日志和WS沿既有EventData保真。数组序列化复用现有u64规则，内存计费按实际Vec容量计入一次，无第二套事件缓存。

本批没有实现完整用户态语言栈、同步对象的提交／唤醒链或通用业务多条件匹配；没有新增推断关系或修改原HIPS动作。真实系统调用结果和驱动权限回执仍各自保留原义。

## 冻结产物

- 目录 `/tmp/as-execution-facts.999h76XO/delivery`，基于独立重构工作树，未修改main或宿主内核。
- initramfs SHA256：`ec363cc81a118765bfb44ea4d465dab58b85801821733cd2154165b860a39055`。
- as-service：`fbd1eda23a58d461403fbd33e5ea5d10ab3eda86cc6381e2fe82d766c73a1d6b`。
- as_wait.ko：`30768a808ed954e6a149d5c6a08115c3d6ce1a20248f23439eeb68e49b901b65`。
- 原清单 `root/suite/source-mvp-sha256.json`：`384236a90a0750df4bce3885eab2015f06717642158b59cbd39169e5f0406192`，165份源码与20项产物逐项一致。
- 当前验证内核 `7.0.0-1019-nvidia`，aarch64；Image SHA256 `6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6`。

本地构建先source现行CI环境。缺失的pahole／libbpf开发文件使用Ubuntu Noble官方包解到本任务临时目录，没有安装系统软件。首次构建因缺pahole停止，补齐后完整构建成功；该失败没有当作驱动失败或触发远端CI。

## 本地与交叉验证

193项Rust工作区测试通过，严格Clippy和格式检查通过。新增定向验证覆盖六参数、0／u64::MAX／大于JS精度边界、空栈／重复帧／满缓冲、TLV非法长度与flags、瞬时字段不能后补、两Vec真实内存计费，以及Ask／完成记录重开后的保真。未按测试数量计产品进度。

Rust LSP诊断为空，C clangd最终0错误。独立源码交审发现的CONFIG_STACKTRACE条件编译问题已修；没有该内核能力时不伪造空栈。C检查另发现既有自保线格式标签使用无NUL数组，已以__nonstring明确其真实用途，未改变字节格式或规则逻辑。

完整消费链独立交审覆盖原记录独占／插入边界、352字节预算、初始化字节、Direct共用谓词、字段不可后补、事务回滚及SQL读回，未发现剩余实质问题。

日志：`/tmp/as-execution-facts.999h76XO/{native-build.log,workspace-tests.log,clippy.log,fmt.log,clangd.log}`；契约LSP及定向结果 `/tmp/as-callback-contract-review`；数据库LSP及15项定向结果 `/tmp/as-execution-snapshot-sql.9XVEfvm5`。

## 最终隔离内核与正式WS

最终冻结产物运行现有脚本化前端，9项通过：启动、防护管理、原Ask答复与效果、运行审计、写关闭、聊天订阅续读与历史、只读授权、真实停止、SQLite原件对账。原文件Allow／Deny保留真实效果；Ask与完成整事件一致，六项参数及内核栈与SQLite逐字段相同。未增加PID／名字专用Allow或缩小规则范围来避错。

实际guest／QEMU退出均0，原模块卸载；宿主执行UID1000，使用隔离guest真实root，未加载宿主模块。耗时95.05秒。报告 `/tmp/as-execution-facts.999h76XO/delivery-run/report.json` SHA256 `eab446d752a2ecd77c5a58845cbeb466a4209a68303804e28aad965d34d7134c`；业务报告同目录 `middleware-report.json` SHA256 `c82f0510636cff9377023ea73ee3d9e059720909b52aa5a6d93a8843a06b79dd`。此前未补__nonstring的候选同样9项通过，原结果保留在同任务目录的guest子目录，最终结论使用delivery-run。

## 原版OpenClaw

原指定聊天使用同一最终候选完成 R67，8/8 PASS。原版OpenClaw运行在隔离aarch64客体，模型使用客体回环的脚本化Responses端点；没有修改目标源码或接入SDK。文件Open、进程Execute、TCP Connect和Send各验证一项Allow与一项Deny。

同一原来源中的事件序号分别为：Open 40857／46689、Execute 51334／54748、Connect 57287／59173、Send 61477／63795。实际系统调用号为56、221、203、206，来自原采集值而非按EventKind推导。Allow分别得到有效FD、程序标记改写、连接建立、对端收到21字节；Deny均返回EPERM且对应效果未发生。

八项原请求在答复前均观察到真实等待，AUDIT_ARCH、号、PC/SP及六参数与独立proc见证逐值一致。现场快照由生产回调一次取得，proc对照仅用于本次验收，不成为生产后读或对象之后不变的接纳条件。16–21帧内核栈及buffer_full=false在正式WS Ask／完成、安全日志和业务SQLite中一致；504条原聊天投递补充核对通过，无Gap或写入失败。WS双向Close 1000，隔离资源正常退役，当前模块实际卸载。

40份证据与22项脚本摘要及只读复算已核。报告 `/tmp/as-openclaw-evidence-20261112/r67-live/result.json` SHA256 `0f6ce607878e469688ea2d631385d7b045671ec2397c00ea7cf2b8d421f36a17`；复算同目录 `recheck.sh`，主开发复核日志 `/tmp/as-execution-facts.999h76XO/openclaw-recheck.log`。初始夹具在零会话提交时发现其基线Open也命中规则，改为预开只读见证FD后重新执行，规则范围未缩窄；原记录 `/tmp/as-openclaw-evidence-20261112/r67/preflight-result.json` 保留，无旧请求回放。

本轮不证明宿主保护、外部模型服务、真实满栈场景、完整语言栈或精确工具归因。这些边界不影响本批原操作现场字段的交付结论。
