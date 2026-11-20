# 进程树

本模块只管理进程事实，公开接口在[lib.rs](src/lib.rs)，公开只读结果在[model.rs](src/model.rs)。规则执行、鉴权、配置写入和日志落盘均由对应模块负责。

- [scan.rs](src/scan.rs)：固定procfs读取与真实身份前后核对。
- [store.rs](src/store.rs)：初始／最新视图、父子索引、失效和有界退役。
- [query.rs](src/query.rs)：内存查询及命中归集。
- [技术契约](../../../docs/backend/process-tree/technical.md)维护唯一字段、语义和接口说明。

采用标准集合、Arc／RwLock和已有Tokio TaskTracker，不建立另一系统缓存或通用管理总线。
