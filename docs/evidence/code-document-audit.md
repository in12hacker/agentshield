# 当前代码、文档与承诺功能核对

日期：2026-11-07。本记录冻结于7755bc503审计时点；其中“当前／未实现”只指该快照。后续规则Actor／Parent终止接入见[新验收记录](rule-termination-integration.md)。核对对象是独立重构分支当前主模块、配置、规则、进程树、Linux驱动与适配，不把旧WP目标或已归档功能当作已完成。本记录列明当前事实和缺口，不替代[唯一技术契约](../README.md#跨模块契约的唯一出处)。

## 本轮确定问题及修正

- 配置Reload漏掉process_tree资源变化，可能返回成功但运行树仍用旧设置。现在Update／Reload复用配置快照的required_effects_from语义，明确报告需完整重新初始化及已保存／读取事实。
- 适配层在满回调队列丢消息时，仅加计数并告警，树要等下一次栅栏才失效。现在唯一丢失计数通过独立watch立即通知主模块，不能再依赖同一已满事件队列。
- Dormant／Failed状态Quiesce后，迟到StartFacts／FactsBarrier可以重新打开观察。现在reader会话停止锁存覆盖所有状态，13／15返回ESHUTDOWN；只有通过既有代／epoch校验的完整Initialize成功后解除。
- 所选问询会话A断开、B仍ready时，旧Prompt保留但无人可答。现在同一Prompt槽与所选Connection寿命关联；Drop／撤销／不可用立即释放，原规则回退。检查到发布复用短鉴权接纳锁，外部不能绕开消息入口改变渠道状态，不增加跨会话接管权限。
- 原完成通知缺少显式ID和失败结果，log=false时媒体可能完全看不到失败。现在MainNotice携真实decision_id及实际Result；成功、失败和取消统一通知。业务处理错误与源已完成结果分开，源Allowed不能被误报Failed；未知提案保持None。纯协议投影提供OperationResult／DriverStatus，当前媒体实际消费，有限发布任务保活到投递结束。
- 树重复initialize可能替换现有源；周期刷新遇忙会错误制造source gap。现在只接受首次绑定，冲突不改源；周期遇忙跳过，其他启动周期／PID0等非法刷新输入在接纳前拒绝，不污染事实置信度。mountinfo和status按必要字节字段解析，不要求无关Linux名字是UTF8。
- 已启用的目标argv／网络domain／new_path条件可能只有模型而无实际目标字段供给。采用准备现在核对真实目标能力，不支持明确Unsupported；不拿actor argv代替target argv，不删除条件，不误拒进程树可供给的cwd／父信息。

## 实际减法

删除无生产消费者的TransferBuffer及仅镜像重组的测试；真正候选重组只由Native保管，DriverFrame／FrameClass和非法帧检查继续保留。删除ConfigView与多余Arc包装，模块内部只留一份ArcSwap快照，经snapshot读取。DriverPolicy参考compile／evaluate和pin收窄为测试辅助，删除重复计划版本，正式导出只用同一Snapshot版本及PolicyLease。无状态driver_status实例包装改为有实际消费的纯协议投影。移除进程树未使用的测试依赖。

这些是删冗余和修真实消费链，不把新增回归行数计作功能进度。相对4216177e，Rust源（不含独立测试文件及examples，仍含内嵌测试／DTO／注释）增加419行、删除315行；Native增加6行、删除4行；独立测试及示例增加507行、删除15行。本轮总体净增，不能称作仓库净削减。主模块有10个生产源文件、3044物理行（含内嵌测试／DTO／注释／空行）；另有smoke.rs 1388行和test_driver.rs 63行，examples不混入这两个数字。

## 已有功能及当前消费边界

- 配置与普通规则具有固定源、版本校验、真实原子保存、只读快照和实际驱动采用收口。
- 驱动已有四个普通可等待入口：Execute／Open／Connect／Send；文件系统组自保覆盖额外修改入口。普通Open不能冒称普通Read／Write／Create／Rename控制。
- 进程树具有整机扫描、真实纳秒身份、初始／最新视图、事实失效、内存补全、关系／分页查询及易失命中汇总；不承担日志、规则执行、配置写入或通信鉴权。
- 主模块按三条业务职责组合；首次本地Unix身份鉴权、管理授权、单活动问询和结果DTO已实现。它们是库接口及媒体供给可使用的能力，不等于已发布正式中间层服务。

## 已承诺但尚未完成

以下仍是产品欠账，未因归档或当前模块合格而被取消：

- 正式可运行宿主与完整CS传输：workspace目前为库和examples，尚无正式as-service二进制、生产监听／可靠上行传输及前端／IM；StopMain命令未绑定宿主生命周期，明确Unavailable。现有shutdown底层能力不是该管理命令已经接通。
- 分发安装链：已有dkms.conf及当前内核构建脚本，尚无完整deb/rpm、签名、服务安装／升级和双架构分发CI。build.sh拒绝非运行内核，未来目标内核BTF构建方案仍缺；测试initramfs不称作产品安装包或现场VM发行镜像。
- 两项既定MVP能力：阻止后终止原进程／原父进程，MCP管道原消息裁决仍未实现。Decision／Termination／操作枚举存在不等于源执行能力存在，当前Native相应能力位未开放。
- LLM聊天、MCP识别及幻影key解析／替换：没有实际流量模块、TLS明文／管道字节保管和替换链，不能从网络连接／发送hook推出已支持。
- 指定容器实例：schema和匹配语义存在，尚无运行时实例到真实进程／cgroup的完整供给与绑定；不能按同名容器或cgroup数字猜身份。
- 规则体验：“回答时记住”的草稿／预览／确认保存链、临时强进程实例范围及系统自保组完整只读枚举未实现。已实现的是手工Remembered组和统一规则Get／Update；裸PID条件不是临时实例身份。
- 日志与历史：RuleLogRequest只是请求；没有持久日志消费者、保留／滚动和查询服务。进程树两份易失视图不是永久历史，未上报驱动命中也不是全量审计。
- 字段及树生命周期：目标启动参数和域名真实供给未实现，当前准确拒绝不支持条件；树的热重绑定和同目标刷新结果合并尚未实现。当前采用主模块完整重新启动恢复、显式并发刷新返回Conflict，周期遇忙跳过。

## 已实现但未形成完整实测结论

x86真实内核、其他发行版／内核、Secure Boot签名安装、普通Process／Network各类Deny／Ask专项、跨容器并发和洪水／吞吐压测仍未运行。真实OpenClaw只由指定聊天验收，本轮不直接操作。不能把编译、软件供给或FileOpen结果代替这些范围。

## 本轮验证

验证对象是本轮冻结源码；Root执行结束后仅追加PID0刷新接纳校验及原回归负例，另行本地验证，下面的Root包不包含该条校验。只覆盖下列实际执行范围；现有未定义功能不作为额外测试门槛，不为达到报告数量另建框架或矩阵。

- 已source本机CI环境，检查cargo、rust-analyzer、clangd、bpftool、QEMU实际入口。Rust 1.89.0的`test --locked --offline --workspace --all-targets`和严格文档检查通过；安装布局软件冒烟通过。1.89未安装Clippy／rustfmt组件，实际使用本机已安装的1.92执行全workspace/all-targets Clippy `-D warnings`和fmt，均通过，不称作1.89 Clippy通过。差异空白检查通过。
- Rust Analyzer实际查11个接口／函数引用，7个受影响源文件诊断为空；包含配置effects、树initialize／refresh、callback_loss、question、completion和协议投影。Native编译及clangd检查通过；构建保留compiler／pahole工具差异警告、无vmlinux导致模块BTF生成跳过的记录，不声称该步生成通过。最后PID0输入补充沿用现有负例，旧实现失败，修正后进程树全包测试、Clippy、fmt及LSP通过；不把Root成功链计作该非法输入的验证。
- 当前Main真实Root任务`a83a52a6b1714f4dad082746aa3e5cd4`通过。包含真实树镜像、Kernel节点、纳秒身份、子进程生命周期、初始视图保持、内存补全、认证查询、直接Allow／Deny、人工答复、绑定超时、无前端、自保11项实际副作用、配置保存采用、健康重启、strict冷恢复和排空卸载。媒体实际输出原decision_id及Allow/User、Deny/Timeout、Allow/NoInteraction结果。
- 同Native的驱动接口专项冒烟（SDK）任务`1e6970fe82ad4ec489ecdc8d7cf5dbd0`通过。覆盖原操作保管、绑定／未绑定超时、跨规则代、临时开关、排空、控制者SIGKILL且其他进程仍持FD、恢复及迟到旧FD。
- 停止反例：旧版负控任务`497a0f7406c240b6890a258801109b24`实际复现迟到Start／Barrier均返回0，探针失败符合预期；新版任务`7a0edef611a34d09bb97f0a7925e6f5a`两者均返回-108（ESHUTDOWN），探针通过。旧版没有被标作产品通过。
- 上述四次Root任务均在aarch64、7.0.0-1019-nvidia隔离QEMU中运行，QEMU／subject／scope／supervisor退出0，子树与监督进程均回收。未在宿主机加载驱动、修改Lockdown或签名信任；未关闭Root代理。guest使用测试模块，不能据此宣称签名安装通过。

本地原始记录：`/tmp/as-quality-frozen-{tests,installed,clippy,doc,fmt,lsp}.log`、`/tmp/as-quality-pid-zero-{before,after,clippy,lsp}.log`及Native的`/tmp/as-native-process-tree.log`、`/tmp/as-process-tree-core-clangd.log`；Root记录`/tmp/as-quality-{main,sdk}-root.jsonl`和`/tmp/as-quality-native-stop{,-old}-root.jsonl`。包元数据`/tmp/as-tree-final-delivery-packages.json`逐文件校验Root冻结源码；Main有78条、驱动接口专项冒烟（SDK）有44条源清单。原始证据及旧包保留，不能用旧记录代替本次SHA。

- Main镜像SHA256：`3c43e6c58483dd7dab5396adb9d6bf3b01334bb4692dcbf95c2acd0aa77ce446`。
- 驱动接口专项冒烟（SDK）镜像SHA256：`800c7fd05aceb9f494911efc3708980fa2b6777860ffd06b0f1541c065d6bf74`。
- 二者Native模块SHA256：`1eaf7834457c5c41dcfdfc07a3f0023d0225bd7a1d2890268090e78ca2e721e5`。

独立只读复核结论：本轮修复可合并，无剩余必须修改的实质阻断。复核实际核对Root终态、完整产物SHA、Main 78条／接口专项44条源清单；当前与Main包的差异仅为进程树lib.rs／tests.rs的PID0负例修正，撤回新增内容即可恢复包内清单SHA。该例外有本地失败／通过证据，不重复宣称Root覆盖。书面记录：`/tmp/as-main-config-rule-audit/final-review.md`；最终本地及Root证据摘要：`/tmp/as-quality-final-validation.json`。
