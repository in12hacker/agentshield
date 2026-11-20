# 底层后端

[主模块](main/design.md)与其[消息组合](main/messages/technical.md)负责初始化、调度和公开接口接线。

[能力缺口与归属](gaps.md)统一绑定真实缺口、所属模块和已有数据契约；未明确实现不在主模块复制一份。

- [配置](configuration/design.md)、[规则](rules/design.md)、[进程树](process-tree/design.md)。
- [驱动适配](driver-adapter/technical.md)、[Flow](flow/design.md)、[凭证](credentials/technical.md)。
- [安全日志](security-log/design.md)：保存原规则日志及已配置目录的写关闭审计，提供经授权的只读分页。

各模块的 design.md、architecture.md、technical.md 分别维护三类信息；只有已有的职责才建立文档。
