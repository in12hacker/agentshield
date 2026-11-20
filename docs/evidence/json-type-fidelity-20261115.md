# 公共JSON类型与业务存储读回

Bot的R23隔离方案暴露了一个现有公共边界问题：启用arbitrary_precision时，旧`Value::deserialize(Unique(...))`会把真正的`{"$serde_json::private::Number":"1.0"}`对象解释成数字。它不是R23真实请求中已经观测到的业务失败，也不代表原协议字节丢失；软件比较已对旧冻结代码复现。

## 唯一实现及消费链

修正归已有`as_types::json::parse`，不在transcript另装Strict解析器。保留公开签名、锁定Serde和重复解码键拒绝；先以零Value遍历复用原默认嵌套校验，再借用RawValue的真实片段按对象／数组／数字构造值。顶层直接复用原字节，不再多扫一份整段RawValue；没有自写新深度阈值、f64范围裁值、隐藏Number API、新JSON DTO或缓存。采用锁定库既有数字表示，精确边界只在[公共契约](../contracts/flow.md)定义。借用依据见[RawValue公开说明](https://docs.rs/serde_json/latest/serde_json/value/struct.RawValue.html)；没有升级锁定依赖版本。

数据库聊天内容读回改用同一公共解析。Native交接的typed metadata继续原Serde校验，opaque JSON按原候选ordinal移动保管并恢复；None／JSON null／Text、原Projection检查、行ID去重和顺序保持。原`ChatContent`与`NativeToolContext`直接复用，没有另一组事实DTO或存储格式回退。

原SQLite回归覆盖写入后读回的字面Number／RawValue对象、嵌套值、空值和相异metadata错误；native交接回归先复现旧读回失败，再修正通过。公共parser、SQLite消费者、全工作区测试和严格Clippy分别按其范围记录，不把软件验证称为新模型／Ask验收。

## 实际原件与开销

- 软件比较及程序：`/tmp/as-json-public-compare-20261115-oj8155_w/RECEIPT.md`、`RECEIPT-optimized.md`。两个模块直接读取冻结r4和当前源码，1.92 release、相同锁定Serde；只读R23记录303的147128字节HTTP正文，按原分段位置复原，没有重拆SSE或遍历历史。
- 完整记录的解析值一致；人工反例只证明旧数值伪map错误与新对象类型正确，不冒充真实Agent请求。
- 本次各500次，旧均值约0.492ms、新约0.871ms。没有速度通过阈值，未显示明确提速，不能外推GLM或前端延迟。内部成本仍为输入长度乘嵌套深度；没有持有第二份原文缓存。
- SQLite及LSP原件：`/tmp/as-native-handoff-opaque-before.log`、`/tmp/as-native-handoff-opaque-after.log`、`/tmp/as-native-handoff-tests.log`、`/tmp/as-native-handoff-clippy.log`、`/tmp/as-native-handoff-lsp-evidence.json`、`/tmp/as-json-lsp-evidence.json`；工作区原件在`/tmp/as-write-close-native-replacement-20261115/fidelity-final-workspace.log`及`fidelity-final-clippy.log`。

这一增量不在已冻结r4服务内，不改变正在进行的r4真实验收原件或结论。没有整合R23隔离A/B、候选查询新DTO、全历史扫描或2秒轮询，也未实现生产当前工具发布者。
