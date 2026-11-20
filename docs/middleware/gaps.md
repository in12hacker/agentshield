# 中间层实现范围与来源缺口

本页维护中间层接口实现和来源缺口，不定义当前任务优先级、不重定义字段或复制底层缺口；实际工作范围查唯一HANDOFF。产品目的、模块职责和技术结构分别归[设计](design.md)、[架构](architecture.md)、[技术契约](technical.md)。实现与真实执行分别核对，文档承诺尚未被调用方消费的能力仍算缺口。已实现且符合产品边界的接口补齐文档；尚未消费的已承诺能力保留为缺口，不删除要求来迁就页面，也不依据过时文字重复开发已有采集器。

## 当前可用范围

正式 WS 的问询、防护管理、进程／安全日志和模型聊天沿原接口；聊天读取区分模型 HTTP 字段分组与 NativeGateway 原生会话。前者复用真实 ExchangeKey 和 ChatMessage，后者按已核 Gateway 发行域及原 sessionId 读取有原引用证明的聊天，两者不自动互认。Group 接纳回执仍打开其原模型交换。来源、接口、已执行证据与验收范围各按所属契约及原件认定，不从投影代码或软件供给推定正式采集通过。

当前链路及有限验收范围见[Session 与原交换交付](../evidence/middleware-websocket.md#原应用-session-的正式查询链)。内核与底层已提供的事实直接复用；[底层缺口](../backend/gaps.md)继续由其所属模块维护。

Group子任务终态与父公开回投已通过原版OpenClaw R87正式旁路验收：同进程双同参委派、正式WS/SQL/原wire/原socket身份及并发隔离核对通过。仅按实际公开来源交付，未发布或未取得的阶段仍如实缺失；当前不是未开发接口。[验收与范围](../evidence/group-terminal-passive.md#r87-正式验收通过)

## 审计与 Ask 上下文的消费边界

活动与Ask复用现有审计接口查询候选和原窗口。字段、交互和在途状态分别归[审计技术](audit/technical.md)、[产品](audit/design.md)、[架构](audit/architecture.md)；不在本页复制DTO。

- 前端归因wire完整解析原AttributionRequest／ToolAttribution，活动与Ask表单明确提交身份、时长及参数映射。普通原事实读取仍使用空归因，比较请求才携显式条件。
- ActivityContext与AskContext沿同一原读槽核对CommandId、原事件／问询和pattern；合法结构的错身份只结束本次读取，不断开Ask连接。
- Ask完整窗口按原引用和修订复用原ChatMessage组件，区分已确认、候选和未核窗口；工具结果、空内容、缺件、冲突和源缺口保留。三条摘要路径已移除，不在前端重排或重新查询最新正文覆盖冻结记录。
- 原交换跳转复用ChatHistory。Ask导航只收起展示，保留原期限、答复能力和显式返回入口；新Ask不继承上一项的展示状态。
- 内容资格由ReviewSession既有chatContent统一维护。当前内容请求被拒绝后清各读槽、保留原拒绝，原Ask答复和管理继续各走实际授权。

实现与实际验收范围分别记录于[消费链证据](../evidence/middleware-websocket.md)。通用工具原始交接和固定反馈自动归属不由候选页面补造，仍见下节；Group实际发布范围见上述已交付记录。测试和控件数量不等于功能交付。

## 仍需原始来源的缺口

原工具→EventKey、EventKey→ChatPartRef及真实固定文本写出的逐类要求与历史原件见[专项证据](../evidence/runtime-correlation-research.md#原专项量化点与历史排期)。精确归因和反馈不作为普通HIPS运行的前置；各自仍按原来源及实际写出认定，不用控制效果或候选冒称已交付。

- **工具事实持续生产与协议绑定**：R12已有同次ToolCall→native→EventKey正例且本分支独立复算通过；现Main/消息/业务库/ActivityContext后半已实现。正式采集入口的本轮代码候选已接统一Submission与原候选引用，实际生产归因尚未验收；到原ChatPartRef的接线仍未实现，HTTP前半仍未证明；不能把有限研究升级为当前通用支持，也不能用HTTP缺口挡住原生事实接纳。[底层唯一清单](../backend/gaps.md#精确因果来源)
- **固定拒绝提示自动发送**：规则文本、反馈端口、正式完成消费者调用和阶段查询已接通；实际发送只消费已核实的工具关系。通用工具语义来源仍缺，候选不触发正文改写；实际拒绝不等于提示已发送。[反馈唯一契约](execution-feedback/technical.md#必须如实暴露的缺口)

2026-11-16原件复核已证明26条 stock Session／ToolCall／原上行结果的对应，AS未成帧原字节也确有结构化Gateway工具回执；当前缺口是正式Gateway来源／成帧及特定系统任务绑定，不能再笼统称没有Session或工具字段。原拒绝必须使用命中时文本，不能借后来规则补旧事件。分项证据、代理身份及消费次序边界见[本轮反馈原件复核](../evidence/rule-feedback-integration.md#2026-11-16原-session工具回执与上行报文复核)。

R95 的真实 GLM Responses／幻影 key、原版 OpenClaw 图形流程及正式 WS 的写打开 Allow／Deny 已验收通过。实际 ToolExecution、反馈绑定和反馈事实均为0，固定文本没有进入原工具完成上行；此处仅完成正式消费者与协议写入能力，stock 精确追加正例仍未证明。软件真实 TLS 正例与此分开，见[固定反馈证据](../evidence/rule-feedback-integration.md#r95真实-glm-幻影链与候选不改写)。

该生产桥及固定提示仍是独立功能缺口。原件按实际取得内容保存，中间层不从独立trace、时序或候选补造发送目标。固定拒绝提示已接正式消费入口；缺失工具语义及原来源域时保持未发送。历史运行与复核边界见[生产桥证据](../evidence/runtime-correlation-research.md#真实工具桥生产接入复核2026-11-14)，具体镜像或用户实例结果另见[交付状态](../delivery/conference-vm.md#当前分发状态)。

这些来源缺口按已授权采集范围调查，先取得事实再决定接口；不改 Agent、不注入、不加共享 SDK。用户已认可的时序／行为模式可补充展示，保留原条件与命中值，不升级为确定 ToolExecution、Ask 触发调用或反馈授权。已取得的事实仍正常保存与查询。

## 已明确延期或待定的产品范围

回答时「记住」的窗口／阈值和完整交互、安装升级签名与双架构发行、未来业务侧脱敏均沿[路线图](../roadmap.md)。MCP 仍暂缓；不把这些范围混成本批消费功能的完成条件。
