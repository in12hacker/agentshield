# as-policy-engine

- [lib.rs](src/lib.rs)：规则工厂、固定来源与不可变快照、一致读取／版本检查／保存发布／重载与驱动投影契约。
- [model.rs](src/model.rs)：三类主体／操作／目标、范围、动作与日志配置。
- [management.rs](src/management.rs)：普通／记住规则的单条查询与组／规则编辑，复用唯一原子保存发布接口。
- [maintenance.rs](src/maintenance.rs)：同一规则文档内可读回的三池维护规则规范化，复用配置原根与普通编译导出。
- [compile.rs](src/compile.rs)：校验、条件预编译、有序匹配与缺失事实处理。
- [interaction.rs](src/interaction.rs)：有界交互请求、连接寿命、原截止时间与一次性回复。
- [tests.rs](src/tests.rs)：顺序、身份、询问寿命和管理临界区的行为验证。

依赖as-types与as-runtime-config；通过RulesFactory接as-main，驱动安装和日志持久化归对应平台／媒体模块。
