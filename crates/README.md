# 大模块与代码导航

backend/ 为底层后端，middleware/ 为业务层，contracts/ 为公共值与窄端口；内核代码独立放根目录 drivers/。文档使用同样的大模块边界，见[文档索引](../docs/README.md)。

- [主调度与模块组合](backend/as-main/src/assembly.rs)
- [主模块统一消息入口](backend/as-main/src/messages/mod.rs)
- [规则工厂、匹配与询问](backend/as-policy-engine/src/lib.rs)
- [配置唯一读写入口](backend/as-runtime-config/src/lib.rs)
- [进程树](backend/as-process-tree/src/lib.rs)：唯一进程状态所有者、系统读取、补全与查询。
- [受控TLS与LLM原始事实](backend/as-flow/src/lib.rs)：唯一HTTP正文链路、协议解析与有界上行。
- [凭证与幻影key](backend/as-credentials/src/lib.rs)：真实秘密、CA及作用域授权的唯一所有者。
- [底层安全日志](backend/as-security-log/src/lib.rs)：原规则日志的唯一持久消费者。
- [只读MainContext](contracts/as-types/src/context.rs)
- [公共配置数据](contracts/as-types/src/configuration.rs)
- [事件与模块接口](contracts/as-types/src/lib.rs)
- [驱动共享事件帧](contracts/as-kernel-common/src/lib.rs)
- [Linux底层驱动适配](backend/as-driver-adapter/src/lib.rs)：单会话、唯一接收泵、原操作结果和真实退役。

内核驱动是独立大模块，见[驱动目录说明](../drivers/linux/README.md)；底层只通过公共协议与其通信。

- [中间层执行反馈](middleware/as-middleware/src/lib.rs)：结构化业务日志、证据审计、Ask上下文与固定反馈消费，外部接入及JSON在此封装，不推定因果。
- [Session／聊天纯业务投影](middleware/as-middleware/src/business_log/sessions.rs)：复用[共用 Gateway 文档](middleware/as-middleware/src/gateway.rs)，[原观察读取物化](middleware/as-middleware/src/business_log/sessions/observations.rs)只借同一树构造已选 DTO；原件与派生索引仍由[唯一数据库](middleware/as-middleware/src/database/sessions.rs)保管。
- [本地 Agent 调用图谱](middleware/as-middleware/src/bin/as-agent-graph/main.rs)：读取校验后的原生报告，计算已证实的工具归属和时序候选。
