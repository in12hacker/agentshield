# 三池维护规则统一接入

本轮范围是规则模块与驱动消费接入，不包含后续Middle进度解耦、补偿生命周期或错误链重构。基线3fd03acdef4263abf3e8c8d1251ada9bf65be4cf；实际最终源码绑定交付metadata与source manifest。

## 已实现的唯一链路

规则模块从MainContext中同一ConfigPaths读取已有自保根，在initialize／update／reload复用typed规范化函数，生成三组五条真实规则：Process Driver组1的源维护Execute和根内目标Execute；File Driver组2的源维护Open和根内目标Open；Network Driver组1的源维护Connect／Send。均用明确Prefix、Allow、log=false，源与目标分两行，无隐含OR。数据实际入同一rules.yaml、RuleVersion、管理读取与DriverPolicyImage；没有暗插buffer或另一份规则owner。

普通Driver组进程／网络从2开始、文件从3开始；配置实际对象自保仍是文件系统组1。保留编号冲突与修改同ID维护内容明确Invalid；省略维护组自动补齐，清空只清用户规则，不静默改用户顺序或保留旧号兼容。根来自配置一次定位，不重复canonicalize，不用lossy字符串处理非UTF-8。

驱动删除maintenance read／exec Allow、proc／sysfs／BTF／pins固定读集合、对应绑定与仅为它们传的instance参数；不加入网络hook控制器特判。FileOpen自保负结果立即拒绝，非负权限结果继续正常Driver匹配。exec的prepare只登记精确backend／loader候选，不代表Allow；实际成功exec／exit管理真实身份，普通Process规则决定放行。控制端点dev_t识别与open鉴权保留。外部受保护对象写／链接／改名／映射等原权限约束未删除，普通Allow不授予修改权限。

## 本地证据

规则组件与规范化／持久化回归通过：/tmp/as-maintenance-policy-tests-final-20261109.log；LSP确认normalize三条实际调用，Clippy与Rust1.89通过。完整workspace141项、strictClippy／fmt／rustdoc、软件配置冒烟与Python客户端通过，原件 /tmp/as-maintenance-workspace-*-20261109.log、/tmp/as-maintenance-config-smoke-20261109.log、/tmp/as-maintenance-python-20261109.log。测试数只说明本轮实际检查范围，不作为功能进度。

规则化Native通过原build.sh普通UID构建，模块 /tmp/as-rule-maintenance-native.AMOHGh/module/as_wait.ko，SHAf22fce932c2ef0087721d3c265d8f697fb6d3f9420d67ff4827cbe6513e441f6，匹配7.0.0-1019-nvidia/aarch64及完整BTF；Core/Self clangd均0错误，原构建warning保留，不冒称零warning或宿主load。

真实联调包 /tmp/as-maintenance-rules-delivery-r1-20261109；报告 /tmp/as-maintenance-rules-run-r1-20261109/report.json，28项必须场景通过，guest/QEMU0、真实join与Native卸载。148源码／18产物逐项匹配，/tmp/as-maintenance-rules-verification-20261109.json。这里只证明当前本地Native链，真实OpenClaw由原聊天另绑新候选验收，旧R16／R17失败不回填。

普通场景脚本已同步Driver保留编号，删除已被新维护规则覆盖的AS精确程序读guest marker Allow。没有增加测试专用AS PID／探针名条件，没有扩大队列／延长期限换绿。

独立有限审核 /tmp/as-maintenance-rules-review-20261109.md覆盖规则规范化／实际持久化／版本与Driver身份生命周期、外部修改约束；已收口无必修。其他审计指出的Middle进度／补偿／原生错误链按后续模块处理，不新增成本轮门槛。

## 原聊天R18真实验收

原指定OpenClaw聊天完成f4656c82de1bce96320d90995ad8db8351833bbb的R18验收。维护规则模块PASS：四次实际管理读回均含三组五条原规则，来源／目标分开、Prefix／动作／日志及编号正确，读回RuleVersion与保存回执、Native实际采用一致。两次非法修改明确Invalid且有效快照不变。AS自身原操作控制超时为0，有实际SQL写入及模型流量作为正例，不从没有通知推定没有操作。

**整体仍FAIL**：五个OpenClaw会话均原生final，普通允许及两项Ask链完整；普通拒绝的工具结果与模型也已结束，但普通文件Direct拒绝的OperationResult未被消费端观察到。文件读取实际EPERM不能替代缺失回执。进程／网络拒绝回执及两个原DecisionId的Ask回执可见；不能将聚合rejected_notices=3直接绑定到某条文件事件或断言具体丢失段。该结果不证明新CRUD提交的运行效果。

原件`/tmp/as-openclaw-evidence-20261109/r18/result.json`，SHA256 `144c6fe1a66dc8c474b8c196ea5842cca0ec8cbe44510c6be2d80165a543498e`；分项见同目录`stock-openclaw/maintenance-rules-verification.json`、`direct-file-denial-receipt-deficit.json`、`ask-followup-proof.json`及`business-sql-verification.json`。148源码另从f465精确冻结在`/tmp/as-maintenance-f465-source-20261109`；原18产物及kernel核验一致，实际测试fixture image `7c508a8010a1393a7d217232ca878018aca16de7881e3810b2d5c408c72f4aa3`独立记录，不冒作原package image。VM按真实退出／join退役，无强制清理；Root未直接操作OpenClaw。

## 能力与边界

路径Prefix表达实际executable／target路径字节的前缀，不等于控制会话身份或跨namespace同对象证明；鉴权与实际自保身份另有原职责。Create／Exit仍观察，普通File维护规则仍是Open，不宣称新增Write／Rename规则能力。未支持操作仍由原真实权限guard处理。正式WS、通用工具执行桥、完整Agent图谱仍未实现，不纳本轮额外门槛。
