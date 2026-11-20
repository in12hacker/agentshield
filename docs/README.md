# AgentShield 文档

代码与文档按大模块归组；每个功能模块分别维护产品设计、架构和技术文件。产品说明为什么做及行为，架构说明职责／依赖／生命周期，技术说明唯一接口与结构。活动文档描述当前方案；执行证据和历史分别放 evidence、archive。

- [底层后端](backend/README.md)：Main、配置、规则、进程树、驱动适配、Flow、凭证和安全日志。
- [内核驱动](drivers/README.md)：独立于后端，私有 DriverContext 和各子类。
- [中间层](middleware/README.md)：业务日志数据库、证据关联、Ask 上下文、执行反馈、外部接入及本地调用图谱分析。
- [前端 Dashboard](frontend/design.md)：八页与全局/Ask控件目的、[架构](frontend/architecture.md)及[接口追踪与浏览器验证](frontend/technical.md)。
- [公共契约](contracts/README.md)：跨模块字段和窄接口的唯一出处。
- [总体边界](architecture.md)：依赖与状态所有者。
- [开发与验证](development.md)：本地构建、软件检查与真实验收的执行边界。
- [参会虚拟机](delivery/conference-vm.md)：独立来宾封装、导入及开源分发材料。
- [待明确事项](archive/unresolved-module-contracts.md)：不造假接口，也不作为无关交付门槛。
- [底层能力缺口与归属](backend/gaps.md)：区分实现缺陷、未开发能力与明确范围，接口字段仍由所属模块唯一维护。
- [中间层业务缺口与开发顺序](middleware/gaps.md)：区分接口已实现、页面消费未完成、原始来源缺失和产品待定范围。
- [路线图](roadmap.md)：近期问询计数与「记住」选项、未来业务侧脱敏及 AS 整体分发。本地全链路保持保真。

## 跨模块契约的唯一出处

- 只读公共上下文及传参：[MainContext](contracts/main-context.md)。
- 固定路径、配置字段及读写：[配置](backend/configuration/technical.md)。
- 显式位置、操作编号、匹配及规则固定拒绝文本：[规则](backend/rules/technical.md)；[rule_execution.rs](../crates/contracts/as-types/src/rule_execution.rs)、[event_operation.rs](../crates/contracts/as-types/src/event_operation.rs)。
- 进程实例、初始／最新属性、补全和查询：[进程树](backend/process-tree/technical.md)。
- 底层实际驱动会话／初始化：[驱动适配](backend/driver-adapter/technical.md)。
- 规则日志与写关闭事实的公共值：[安全审计记录](contracts/security-audit.md)，实现只保留一份契约类型。
- 驱动入口及子类：[驱动技术](drivers/technical.md)；私有 [DriverContext](drivers/linux/context.md)、[Hooks](drivers/linux/hooks.md)、[规则匹配](drivers/linux/rules.md)、[配置](drivers/linux/configuration.md)、[工具](drivers/linux/tools.md)、[自保](drivers/linux/self-protection.md)。
- 原事件字段及来源：[事件](contracts/events.md)。原生库请求观察的安装／字节／寿命见[原生请求观察源](backend/driver-adapter/runtime-observation-technical.md)，共用类型只在其链接的runtime_observation.rs定义。
- 驱动控制封套、镜像及回执：[驱动通信](contracts/driver-communication/technical.md)。
- 原管理／Ask／完成值及授权：[Main 消息](backend/main/messages/technical.md)。
- 基础JSON解码：[公共契约](contracts/README.md)，唯一实现 `as_types::json::parse`；业务语义归各消费者。
- 原协议、聊天、来源、队列与窄反馈：[Flow 公共契约](contracts/flow.md)；[实现边界](backend/flow/technical.md)、[协议](backend/flow/protocols.md)、[TLS 出口](backend/flow/tls-egress.md)、[凭证](backend/credentials/technical.md)。
- 容器后启动接入的产品范围：[动态接入](backend/flow/container-egress-design.md)。当前未完成项统一见[缺口清单](backend/gaps.md#当前剩余范围)，连接启用时点与信任条件统一见[TLS 出口](backend/flow/tls-egress.md#支持范围与成熟依据)。
- 业务查询、事实引用及确定关系：[中间层](middleware/technical.md)。
- 共用公开 Gateway 来源、原消息／字段引用和单一解码入口：[中间层 Gateway 契约](middleware/technical.md#nativegateway-共用来源与解析入口)。
- 聊天、工具、关系和交互业务数据的结构化存储：[业务日志](middleware/business-log/technical.md)。底层[安全日志](backend/security-log/technical.md)不存第二份聊天历史。
- Session 身份、RPC 交接、原生聊天投影及来源化读取类型：[业务日志 Session／聊天](middleware/business-log/technical.md#session-与原生聊天业务模型)。Group 只定义自己的[委派／终态投影](middleware/agent-graph/technical.md#gateway原件派生字段与查询引用)。
- Ask 最近20条及独立命令核对：[审计模型](middleware/audit/technical.md)。
- 规则固定拒绝反馈消费：[执行反馈](middleware/execution-feedback/technical.md)。
- 未知 Agent、无需目标改动的能力与限制：[运行时关联证据](evidence/runtime-correlation-research.md)。本地原件分析的结构与时序规则见[Agent 调用图谱契约](middleware/agent-graph/technical.md)。

公共值只依赖基础类型，不反向引用消费者；模块引用唯一字段／接口定义，不能循环定义或复制后另改。实现依赖 Main 组合公开接口，叶子模块不内部查找彼此实现。开发改变接口时同批替换全部生产者、消费者、配置与文档；不增加版本或兼容别名，内容修订标识仍用于并发和在途一致性。

队列只保未出队元素，出队即移交所有权；消费者完成即释放。原操作、协议和 SQL 在途上下文在完成前保管。必要实时状态由进程树／配置／规则各自维护；聊天历史持久保存在中间层数据库，底层没有回放／查询缓存。

## 验证证据与历史

- [验证证据索引](evidence/README.md)收录每一份实测／审计记录；只覆盖各自绑定的源码、产物和环境，不能把历史通过扩为当前整包通过。
- [历史及待定事项索引](archive/README.md)收录退役接口、研究草案和未明确产品事项；不是现行实现或交付门槛。

执行进度只维护在工作区 `.codex/HANDOFF.md`；模块文件不继续堆叠每轮任务。MCP与其它暂缓方向只按路线图和明确用户授权恢复。
