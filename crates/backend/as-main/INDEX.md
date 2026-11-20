# as-main

- [assembly.rs](src/assembly.rs)：配置优先初始化、MainContext和模块接口组合。
- [runtime.rs](src/runtime.rs)：实际线程、一次性宿主停止监听与写出能力、驱动初始化检查及排空退出。
- [dispatch.rs](src/dispatch.rs)：快速／慢速／消息队列。
- [native_events.rs](src/native_events.rs)：唯一原生事件桥、实际选版租约、直接裁决和退役通知。
- [messages](src/messages/mod.rs)：首次鉴权、当前DTO、CommandReply运行时能力、管理路由和驱动封套。
- [lib.rs](src/lib.rs)：公开入口。
- [smoke.rs](src/smoke.rs)、[test_driver.rs](src/test_driver.rs)：仅cfg(test)的软件供给。
- [configuration_smoke.rs](../../middleware/as-middleware/examples/configuration_smoke.rs)：真实安装布局下的配置与消息软件冒烟；[调用脚本](../../../scripts/smoke-main.sh)。

- [flow_runtime.rs](src/flow_runtime.rs)：Flow与真实驱动入口生命周期组合。
- [messages/flow.rs](src/messages/access.rs)：内容授权、订阅与私有凭证管理接纳。
