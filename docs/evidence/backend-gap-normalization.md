# 底层缺口归一化与消费链重构

本轮基线为6b3429daf707f72e81909324a1f3ed4d08ac2f60。能力与所属模块见[唯一缺口清单](../backend/gaps.md)，接口字段仍在各模块正式契约中定义。本记录只报告本轮实际实现与验证，不宣称尚未实现的产品能力已交付。

## 实现范围

- Main将原生正常处理、拒接纳退役、实际结果和版本租约交给同一Work／原FastInput及pending预算；Native答复模式删除逐回调观察与补偿任务，Observation也沿同一事实维护生命周期。Direct结果／状态／失效沿原有界notice等待消费，不再满时丢弃。
- 适配唯一接收泵对必要Controlled／Direct无法接纳时明确停源，不以单独取消加聚合计数冒充收尾成功；不增加Direct缓冲。实际内核Direct源仍有dropped边界，不能把适配收口当无限无损。R18具体丢失段未证，未根据聚合3补造回执。
- 原生负errno通过原ExecutionOutcome::Failed保留，通道失效仍独立。中间层原结果表用outcome与outcome_native_error分列，读回精确重建同一公共值；没有JSON整包、第二错误DTO或旧schema回退。
- Flow准备阶段以精确Arc的私有guard保管首await；正式监护交接与stop封口复用原feedback短锁。跨runtime晚注册反例已证明，不能用“没有await”推定没有线程抢占，也不能用外部未poll future持tracker token阻塞退出。
- 原Media内聚移入同crate唯一media.rs，正式薄宿主与软件脚本共用。SQL与Peer发送解除完成依赖；Peer有界队列不能接纳时关闭真实传输并撤权，Queued只表示队列接纳。Ask写入在返回等待future前交原DB tracker保管，EOF不取消。无可持久化原结果的生命周期维护直接释放，不作无关SQL查询。

没有新增通用bus、Owner、缓存、队列、资源额度、接口版本或兼容分支。现存模块均能承担这些运行时职责，不建立空Main global类；部署、强实例选择和来源缺口没有被偷偷塞进主模块。

## 已执行的检查

运行前source本机CI环境，验证已安装的Rust1.89、Cargo、rust-analyzer、Clippy和所需本地工具。当前环境Linux 7.0.0-1019-nvidia／aarch64；宿主测试使用普通UID1000。

- 最终Rust1.89 workspace／all-targets／locked／offline：154通过、0失败，`/tmp/as-gap-normalization-workspace-tests-20261109.log`。
- 全workspace严格Clippy、fmt、严格rustdoc通过：`/tmp/as-gap-normalization-{clippy,fmt,rustdoc}-20261109.log`。
- 最终既有业务软件冒烟通过：`/tmp/as-gap-normalization-software-smoke-20261109.log`，包含真实慢Unix接收、实际Connection撤权、原Ask超时动作及SQL持有／读回。软件驱动夹具不证明真实内核效果。
- Flow取消及stop晚注册各有真实修前失败、修后通过原件，分别为`/tmp/as-feedback-prepare-{before,after}-20261109.log`及`/tmp/as-feedback-stop-seal-{before,after}-20261109.log`；没有把早期EPERM工具失败或旧错误测试顺序改称通过。
- LSP核对实际调用与引用，Main／适配／Flow及中间层正式库无错误。configuration_smoke既有跨crate路径仍有RA E0583，实际Cargo检查、构建与运行通过；工具异常保留，不声称全量LSP零诊断。
- 独立审核按精确源码散列分别覆盖Main／Flow原Guard、后续stop seal和Middle，报告见`/tmp/as-gap-closure-review-20261109.md`、`/tmp/as-feedback-stop-seal-independent-review-20261109.md`、`/tmp/as-middle-intake-independent-review-20261109.md`。旧报告不覆盖后改源码；适配入队由父审核，同crateerrno另独立核对。

## 真实Native guest

包`/tmp/as-gap-normalization-delivery-r1-20261109`，镜像SHA256 `a319470d5a47057cfcb131bb4c89b53722ee3780f1a6b0e6a39d1c5f89e937e3`。150源码／18产物逐项匹配，manifest SHA256 `35696c89c766088e36416d0831fa7dbf9582501426d42d0fb46a4ae4442e6ccc`；核验原件`/tmp/as-gap-normalization-verification-20261109.json`。

Native C／H的12个来源文件未变，明确复用匹配的as_wait.ko与两BPF产物；模块SHA256 `f22fce932c2ef0087721d3c265d8f697fb6d3f9420d67ff4827cbe6513e441f6`。实际内核`/tmp/as-tls-native-main-r8-local-run/Image`，SHA256 `6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6`。原生帧未改变，不借旧源码构建物冒作新驱动改动。

`/tmp/as-gap-normalization-native-run-r1-20261109/report.json`通过，28个必须场景PASS；独立安全日志持久模块仍NOT_IMPLEMENTED，不以该可选项假称已实现。宿主UID1000、guest内真实root、实际模块与LSM、退出及QEMU join分别记录，未timeout、无failure。没有加载宿主模块、改变MOK、重启Root broker或操作宿主OpenClaw。

## OpenClaw与剩余能力边界

上述结果尚不等于新候选的OpenClaw验收。原指定聊天独占实例与完整方法，Root只交需求、标准及冻结metadata；实际新结果形成后再按源／产物／内核归档，失败回原模块整体重构。R18旧f465仍保持原FAIL，不能用本轮软件／guest结果倒填旧文件回执。

### R19真实失败与修正方向

原聊天已完成6ac7dddf67dade9daeda4d6ed94feba5d681f399的R19，整体FAIL。R18缺失的文件Direct拒绝回执已收到，两项Ask的原结果与SQL关系完整，五个原stock会话均final；这些通过范围不能覆盖同批暴露的防护旁路。

服务端实际四次报告`event_queue_full transport_closed=true`。普通输出压力关闭了原回答连接，Connection退出使问讯渠道不可用；同一条保持启用且前后完全相同的Ask／timeout Deny规则，在独立stock诊断中出现真实Native回执Allowed、proposal Allow、reason no_interaction、hit null。原EventKey为(source_instance=17efd2c0ca43b7b4246ba1c5368bf43d,domain=1,sequence=90803)，实际工具也读到合成私有内容。未依据计数猜测填满队列的具体帧；关闭原因、渠道撤销及后续原生允许有各自原件。这是本轮Peer断开策略的产品设计错误，不能归为观察工具问题，也不能放宽规则换通过。

原件`/tmp/as-openclaw-evidence-20261109/r19/result.json`、`pressure-channel-diagnostic/semantic-bypass-proof.json`及`close-and-nointeraction-source.json`保留。实际stock镜像dbfe8bb37ec5b87a0ab11956c7139144b528dc26f00d883920023b6c8e2cd208与原package镜像单列。普通／记住规则CRUD、停用／删除实际open与原Ask timeout／断开回退的独立Native补充六场景通过；软件补充也通过，但原“Full后撤权”验证仅覆盖旧策略，未验证后续Ask，不能当作正确HIPS语义。

SQL仍实际保存5825条聊天，已公开的2613条原字段逐项匹配；原协议与五项模型／工具终态另有独立SQL事实核对，不能将DB-only记录冒充已公开帧。所有自有stock／AS／Native／guest／QEMU已正常退役，无强制清理。原用户无前端语义保持；下一轮从根本分离控制身份／实际结果和聊天输出，聊天按同一数据库真实索引单条读取，不再靠扩大队列或关控制连接处理普通背压。

自动记住的用户确认链、未知Agent精确执行来源、普通文件新增操作、容器／TLS覆盖、StopMain和正式部署升级等剩余能力按[清单](../backend/gaps.md)如实保留。MCP仍暂停。业务脚本只能证明明确供给的业务范围，不能把模拟关系称为stock Agent已自动关联，也不能把当前guest包称为正式发行包或完整HIPS交付。
