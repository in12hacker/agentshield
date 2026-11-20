# 内核驱动

[产品设计](design.md)、[架构](architecture.md)、[技术](technical.md)分别维护。驱动私有子类位于 linux/，共享事件与控制协议位于公共契约，不引用后端私有 MainContext。

- 回调与原操作：[Hooks](linux/hooks.md)、[原操作保管与收尾](linux/operations.md)、[无状态工具](linux/tools.md)。
- 已采用状态与策略：[DriverContext](linux/context.md)、[配置](linux/configuration.md)、[规则匹配](linux/rules.md)、[自保](linux/self-protection.md)。
- 传输与来源：[原生端点](linux/transport.md)、[协议消息](linux/messages.md)、[受控 TLS 与原生观察来源](linux/flow-binding.md)。
- [机制参考与取舍](references.md)：外部实现依据，不代替当前接口或运行证据。

原事件字段只在[事件契约](../contracts/events.md)定义，封套、镜像与回执只在[驱动通信契约](../contracts/driver-communication/technical.md)定义。用户态加载和会话资源属于[底层驱动适配](../backend/driver-adapter/technical.md)，不在内核子类中另设所有者。
