# 中间层

[产品设计](design.md)、[架构](architecture.md)、[接口](technical.md)定义业务层。

正式回环 WS 已接通首次鉴权、原 Ask／答复／执行结果、聊天工作台和防护管理。模块清单见[架构](architecture.md#内聚模块)，现行范围与来源缺口见[交付契约](technical.md#5-交付顺序与来源缺口)。

[实现范围与来源缺口](gaps.md)区分已接通能力、调用方消费缺口、缺原始来源和产品待定范围；底层缺口仍归所属模块，任务优先级不由历史证据决定。

- [业务日志数据库](business-log/design.md)：结构化保存聊天、工具、关系与交互事实。
- [LLM 聊天工作台](technical.md#llm-聊天工作台)：模型实时订阅与原交换读取、原生 Session／聊天的来源化历史查询；业务模型只由[业务日志](business-log/technical.md#session-与原生聊天业务模型)维护，公共 Gateway 解码复用[窄接入](technical.md#nativegateway-共用来源与解析入口)。
- [因果关系链与 Ask 最近 20 条](audit/design.md)。
- [固定拒绝执行反馈](execution-feedback/design.md)。
- [脚本化联调接入](smoke/design.md)：已使用的本地接入，宿主与软件冒烟共用[原消息消费者](../../crates/middleware/as-middleware/src/media.rs)；Unix和正式WS接线共用该owner，软件夹具不替代真实工作流验收。
- [Agent 调用图谱（Group）](agent-graph/design.md)：持续采集已配置回环端点的 A2A 原件，通过正式 WS 查看委派、任务终态及模型交换。
- [OpenClaw 场景](openclaw-scenario.md)：产品场景与分层事实要求，实际验收另见证据。

原事件、原操作、配置、规则、进程、Flow 和 Main 授权只引用[唯一契约索引](../README.md#跨模块契约的唯一出处)。中间层新增认证封套与业务结构由本目录技术文档定义，各子模块按接口组合。原始值保持保真，同进程共享不可变事实，跨连接发送在媒体边界编码，历史只进原业务库。
