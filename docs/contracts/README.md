# 公共契约

- [MainContext](main-context.md)：只读共享路径及明确公共输入。
- [事件](events.md)：真实来源、公共进程信息和差异载荷。
- [安全审计记录](security-audit.md)：规则日志与写关闭事实的共享值；持久化与分页归底层安全日志模块。
- [驱动通信](driver-communication/technical.md)：共享命令与回执。
- [Flow](flow.md)：原协议／聊天、来源、消费队列及窄反馈接口。

跨模块字段与接口唯一出处由[文档索引](../README.md#跨模块契约的唯一出处)指定，公共层不导入实现或循环定义消费者字段。

JSON基础解码复用 `as_types::json::parse`：在Serde原单次解码中拒绝任意深度的重复对象键，保留serde_json原值/类型及大数表示。它只负责JSON语法与键唯一性；Flow和Group各自解释所属协议，不导入彼此实现、不另存解码正文。原字节归既有EvidenceBody/原帧所有者。
