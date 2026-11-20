# MCP与工具错误反馈暂缓记录

2026-11-08用户明确暂时搁置MCP，只先完成LLM对话消息解析、TLS修改边界、逐条推送和数据库预留的调研与文档。

先前运行时MCP审批、DenyNotice／FeedbackPlan／ReturnPort、向pipe追加拒绝解释、幻影key等联合设计保留在Git提交2814e0c4feeade23c4d447cffc5684e781e776b0对应的docs/design/flow-control.md、docs/architecture/flow-control.md、docs/technical/flow-control.md和flow-protocols.md。它们是未实现的旧方向，不是当前接口、兼容实现或本轮交付门槛。

[MCP连接调查](../mcp-approval-connections.md)继续保留其来源和未证明边界；不能因为暂停而把stock Unix sockmap、复制pipe写端或采样误写成已完成审批。恢复此方向时先明确任务范围，不自动恢复旧的AS托管启动前提。

当前唯一产品、架构和技术分别为[LLM设计](../../backend/flow/design.md)、[LLM联合架构](../../backend/flow/architecture.md)、[Flow契约](../../backend/flow/technical.md)和[provider解析](../../backend/flow/protocols.md)。通用HIPS原操作、三类规则和已交付终止能力均不因本次文档收敛改变。

用户随后确认了新的窄目标：由规则预配置固定拒绝文本，减少LLM无意义重试；当前只设计产品／边界／数据契约，见[执行反馈](../../middleware/execution-feedback/design.md)。这是LLM受控出口上的规则反馈，不恢复本文列出的旧MCP接口、pipe回复或托管启动方案，代码未实现。
