# as-types

当前源码入口：

- [context.rs](src/context.rs)：只读MainContext和配置提供的路径数据。
- [configuration.rs](src/configuration.rs)：持久配置与保存回执。
- [log_storage.rs](src/log_storage.rs)：两个原日志库的共同保留预算与所有者回收端口，无写入线程或内容缓存。
- [driver_control.rs](src/driver_control.rs)：临时控制及真实状态接口。
- [dispatch_event.rs](src/dispatch_event.rs)
- [hold_budget.rs](src/hold_budget.rs)
- [lib.rs](src/lib.rs)
- [main_ports.rs](src/main_ports.rs)
- [operation.rs](src/operation.rs)
- [process_source.rs](src/process_source.rs)：窄原生事实端口、真实绑定与消费栅栏。
- [process_tree.rs](src/process_tree.rs)：实例身份、设置和严格查询输入。
- [decimal.rs](src/decimal.rs)：跨JSON边界的精确u64字符串编码。

- [workload.rs](src/workload.rs)：原发起者的宿主机／容器实例事实。

- [driver.rs](src/driver.rs)：分阶段DriverAdapter／DriverSession、真实原生事件及会话寿命端口。
- [driver_image.rs](src/driver_image.rs)：配置与规则不可变镜像、持久代和停止世代校验。
- [driver_wire.rs](src/driver_wire.rs)：驱动唯一64字节封套及单帧片段边界校验。

- [rule_execution.rs](src/rule_execution.rs)：显式生效位置与底层阶段。
- [event_operation.rs](src/event_operation.rs)：统一操作编号；观察事实与可裁决操作分开。

- driver_tools.rs：无状态驱动工具、精确实例目标与信号接纳结果；与只读事实端口分开。

- [flow.rs](src/flow.rs)：当前Flow／凭证事实、精确wire DTO及受限能力端口。
