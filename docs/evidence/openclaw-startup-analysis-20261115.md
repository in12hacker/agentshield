# OpenClaw启动等待与规则分流核对

本轮真实GLM／幻影key验收仍由原OpenClaw聊天执行。以下只核已有原件和当前代码，不把启动、脚本检查或恢复操作记作八项业务通过。

## 测试未提交的已证直接原因

原聊天在成熟最小路线实际手动操作后确认：原无头连接locale为zh-CN，生成的agent-browser.cjs却以英文`Start session`精确定位按钮。实际页面按钮为“开始会话”，原动作一直未完成，prepared和submitted意图不能代表sessions.create。手动单击后已实际创建会话并由GLM处理原提示；当前轮不热改入口，后续由同一脚本owner收敛语言依赖。这解释了为什么业务没有提交，不据此推断此前全部握手等待或驱动开销的因果。

## 已取得事实

- 本轮原件位于 `/tmp/as-openclaw-write-close-20261115/acceptance-final`。登录有效令牌之后仍发生 `auth_validated` 握手超时；原Gateway明确返回配置热重载的 `GatewayHotReloadRecoveryError`、cron monitor恢复失败，以及重启期间配置读取不可用。对应初轮采样时点实际业务请求和Ask均为0；后来UI提交意图和控制器停止标记不等同原Agent已开始或完成业务，不代表整个后续轮次的终态。
- 同实例正式 `DriverStatus` 原件为 `owner-live-driver-status.json`：来源running、自保enabled、pending=0、dropped=125482。公共状态没有held字段，不用pending推造held=0；全局dropped不能分摊到文件关闭、运行时或某个普通Hook。
- 通过既有只读 `read_as_evidence` 取得已保存Main帧的固定原前缀，未修改运行实例。原件为 `/tmp/as-write-close-native-replacement-20261115/runtime-main-original-prefix.jsonl`，分析为同目录 `runtime-main-frame-analysis.json`。2662115字节、862帧中，844条OperationResult均无RuleHit：843条为no_match提案后Allowed，另1条proposal=null、实际Failed(native_error=-512)，不据原码猜原因。类型总数为Send746、Connect89、Execute9，没有File结果。其事件源时间跨度约990.9秒；这只是保留的公开结果，不是所有内核回调的计数，也不含独立runtime原始投影。无RuleHit不能反推成功；最初将844条都算Allowed的统计已由独立审核纠正。
- 旧 `/tmp/as-controls-normal-flow/acceptance` 和 `/tmp/as-control-live-store-fixed/acceptance` 的原日志已经有相同登录超时、设备配对SQLite等待及8–33秒量级的元数据准备。它们不是相同负载的性能基准，不能据此证明没有开销；但不能把这些现象描述成新增file_release后才出现。
- 对比上次Execute Allow实际候选源码 `/tmp/as-controls-no-cutoff-20261115/source` 与本轮冻结源：206份共同编译输入中，runtime_observation、as_runtime／capture／futex及规则owner／编译／Driver导出均未变化。主要Native改动在关闭来源、队列与释放；原VM启动器、页面动作和环境脚本相同。原文diff分别在上述分析目录 `last-working-vs-current.diff`、`last-working-test-entry.diff`；快照未覆盖的文件不当作源码删除。这些差异定位不代替实际性能因果验证。

## 已确认并修正的规则入口问题

准备入口只复制候选配置、不复制已安装rules.yaml；随后configure_review整体替换普通组，导致正式两组十条预制规则没有进入本轮实际策略。该行为已从准备、原规则采用回执和effective文档交叉确认。

唯一脚本入口现在保留候选rules.yaml，并沿原get／expected_version／update流程将目标询问组追加到实际当前普通组。AS维护组仍由规则owner规范化，不增加豁免分支或第二份规则源。现有六项脚本检查及实际configure_review的既有普通组保留检查通过；它们不代替真实Hook验收。

原聊天已通过同一实例正常规则owner恢复两组十条基线，保留四目标Ask和产品生成的三维护组，采用后为九组。恢复后实际receipt／effective位于 `/tmp/as-openclaw-write-close-20261115/acceptance-recovered/live-report.json` 的policy字段，saved_version、effective.version与applied.rule_version一致；修复前owner-adopted-rules-original.json仍为七组，不能用来证明恢复。原件与恢复后结果分开保存；AS与VM未因此更换，旧GUI没有发出的请求不冒充业务终态。

## 已确认的重复元数据准备

同目录 `current-runtime-diagnostic.json` 是现有只读diagnostic的原返回。保留AS日志尾部中，PID176和186各有68条完全相同birth／exec_cookie的metadata_started，PID144为67条、PID1为63条。当前Resources在成功读完但没有探针符号时返回Ok(false)后删除准备标记，下一次inspect再次读取ELF；这是实际重复工作，不是额外回调数量的推断。

准备生命周期已改为明确NoProbeSymbols／Installed终态，原owner对同一exec实例完成的检查均去重，真实错误仍按原实例释放标记；不新增ELF／目标内存缓存。旧任务失败不能移除较新exec，退出只清理匹配出生的实例。该修改是否解决Gateway超时仍须由真实运行结果认定，不能由代码修改或单元检查提前报成功。

## 尚未证明的性能归因

当前Driver文件后端候选已有精确／前缀路径必要条件预筛；进程和网络只导出操作位，无关同类操作仍可移交底层后得到no_match。这是代码与实际结果共同确认的分流过宽，不能仅凭存在这些往返就断言它造成了Gateway握手超时。

`capture_facts` 在普通Driver匹配前按类别／操作候选采集现场；独立libuv uprobes与private futex来源也不经过普通快速规则。普通Driver Allow不能被称作这些独立来源的采集豁免。运行时投影自身已有同回调begin／end时间和原字节，正式业务库保留该原件；公开Main结果不含这些投影，不能从公开结果捏造它们的耗时或数量。

目前没有各独立采集点耗时及丢弃归属的完整原件，故尚未证明主要超时来源。后续原因核查不成为正常业务前置，不扩大白名单或容量来掩盖问题，不修改目标Agent、注入观察器或增加共享SDK。
