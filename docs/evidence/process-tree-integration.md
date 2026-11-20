# 进程树实现与验收

日期：2026-11-07。对应进程树实体、主模块组合、配置／消息接入与同连接原生事实扩展。规范见[进程树契约](../backend/process-tree/technical.md)；本记录不替代字段定义，不代表其他WP／完整MVP已经完成。

## 当前实现与边界

主模块初始化唯一ProcessTree，先开启基础观察，再做宿主procfs扫描及真实身份前后核对，消费事实栅栏后采用正式防护。强身份使用boot_id、host TGID及驱动纳秒birth；保留初始／最新视图和每字段证据代，支持父子／祖先／子树、分页、详情、实际命中归集、刷新和状态查询。

模块仅依赖as-types及现有成熟基础库。系统读取、内存状态和只读查询内聚；规则匹配、日志落盘、通信鉴权、配置写入、驱动控制归所属模块。采用标准集合和Tokio TaskTracker，没有第二份系统缓存、通用总线或额外业务线程。生产文件5个，含数据模型／注释／空行2161物理行；核心四文件1823行，数据模型文件338行，测试606行，未按测试数量计算进度。

## 冻结真实产物

实际环境为aarch64、7.0.0-1019-nvidia隔离QEMU，复用主机同一内核。现有Root代理调度；未加载宿主模块、重启宿主或修改安全策略。

- Native模块：`0b493ea3f6d6fe17cda52cd5ea1273a3c716c192dbd35f486dc69e63642aaab8`。
- 最终Main镜像：`b2002e2695667ba63ceaa3f93faf6670ea86cc40e874473d5c98fd363422d037`。
- 同Native SDK镜像：`f1ecfa83537b5e630ee845c363c7de1279fefb2ec454839c631a9f748c8c839e`。

Main Root任务`2a76ac79acf44ab18bf368c4193dde20`完成且passed=true：整机镜像包含Kernel节点，实际服务／子进程身份与Native纳秒birth一致；独立子进程通过就绪握手验证Create、exec、Exit及初始视图不变。快速补全检查使用明确的软件事件输入、真实Native进程身份，验证缓存填入exe／父信息；不冒充这条软件输入是实际拦截事件。首次鉴权后的status/detail查询取得真实Tree结果。

同轮Main仍通过原直接允许／拒绝、人工答复、超时、无前端、11项自保副作用反例、实际配置采用、健康重启、严格默认ToUser失联EIO与新AS冷恢复。最后实际排空并卸载模块。

SDK任务`2dff091938d446598e68db946c12b7fa`完成且passed=true：同一Native上的原操作保管、事实marker实际ACK、超时、跨代、临时覆盖、正常退役、控制者SIGKILL／继承FD／迟到旧FD及恢复路径通过。两任务QEMU、subject、scope和supervisor退出均0，测试子树／监督进程均回收。

## 本地检查与独立审核

Rust 1.89完整workspace／all-targets、本机安装布局软件冒烟、Clippy全部目标警告作为错误、公共API严格文档、LSP引用／诊断、原生Kbuild／BTF及Core／Self LSP通过。原生编译器／pahole工具版本差异warning保留，没有声称Native零警告。无远端CI运行。

独立审核发现并复验的实质问题已收口：父身份切换不能混用旧父路径；exec失效保留时间锚点且允许本次有效区间样本；迟到采样不能复活退出；缺口前旧字段不能被重扫重新激活；扫描与在途事件钉住退役证据；内存压力回收不淘汰活节点；非UTF8 comm不影响status数字字段；关闭／取消仍收尾；Start到最终消费栅栏之间任何丢失不得成为完整初始化。公开查询关系、分页及cursor绑定另有直接回归。

Main r3真实验收后，最后仅修改纯内存查询：Children直接使用父索引，Subtree复用遍历所得有序键分页；按最后检查的Key推进，首批已回收键不会遮住后面的存活节点。关系、游标和64个已回收键后的存活节点回归已在MSRV通过，独立审核通过；原生驱动、事实接入、存储和补全未变。Root原会话到八小时上限自动退出，socket已不存在，没有新会话；未关闭或重启代理。索引调整后的r4镜像复验未执行，不用旧产物冒称该调整已运行真实Root。

原始证据保留：

- `/tmp/as-tree-final-main-r3-root.jsonl`、`/tmp/as-tree-delivery-sdk-root.jsonl`。
- `/tmp/as-tree-main-delivery-r3`、`/tmp/as-tree-sdk-delivery-r1`、`/tmp/as-native-process-tree`。
- `/tmp/as-tree-final-delivery-tests.log`、`/tmp/as-tree-final-store-interval.log`、`/tmp/as-tree-final-query-tests.log`、`/tmp/as-tree-final3-installed.log`、`/tmp/as-tree-final-delivery-clippy.log`、`/tmp/as-tree-final-delivery-doc.log`、`/tmp/as-tree-final-delivery-lsp.log`。
- `/tmp/as-independent-tree-review/final-review.md`、`/tmp/as-independent-tree-review/final-results.log`、`/tmp/as-process-tree-validation.json`。

## 实际限制

最新值是最近确认的事实，不承诺任意时刻所有易变属性原子一致；未知／不可读／截断／失效各有状态。存储预算是模块数据与保守开销的计量，不是RSS硬上限。丢失前的短命进程、初始字段和未上报命中无法追回，保留历史缺口与不完整统计。容器实例解析、持久历史、hash／DB、全量驱动命中汇总、日志落盘、MCP及终止能力继续按模块边界处理。

本轮未运行x86真实内核、发行版安装包、Secure Boot签名环境或性能压测；真实OpenClaw仍由指定聊天执行。没有把这些范围混称为本次进程树验收完成。
