# 底层安全日志技术边界

规则日志复用 [main_ports.rs](../../../crates/contracts/as-types/src/main_ports.rs) 的 `RuleLogRequest`、`RuleLogDecision`、`RuleHit` 和原事件及结果，保留 Transferred、Pending、Resolved 及原 Result。入队只表示接纳。真实日志写入失败与生产队列拒收分别报告。

`RuleLogRequest.event` 共享原操作的 `Arc<EventData>`。日志模块在消费期间持有该引用，沿现有持久化边界写入原字段，消费结束后释放引用。进程信息已经由主模块入口补全，日志模块不重复查询或维护进程状态。

`SecurityLog::open(&ConfigPaths, Arc<LogStorageBudget>)`只打开配置已派生的`log/security.sqlite`；Main须先完成真实驱动控制身份绑定，自保写不能靠只登记exec身份或豁免绕过。写关闭事实由驱动提供，本模块不读取目录配置或创建通知来源。安装根及日志目录由运行 UID 持有，目录私有，数据库文件权限为 `0600`，拒绝符号链接、非普通文件及硬链接。初始化失败在驱动接纳前返回。没有可配置的数据库目标路径。

Main 将原 `SecurityLogSink` 和现有 `Receiver<SecurityLogEntry>` 组合，`MessageModule::run` 只处理通知。日志模块在阻塞池内逐条执行独立提交，SQLite 使用回滚日志与 `synchronous=EXTRA`。`rule_logs` 表的 `id INTEGER PRIMARY KEY` 只表示本地存储顺序，`record TEXT NOT NULL` 序列化一条原规则记录或写关闭审计事实。序列化仅在持久化边界发生，业务层和原操作仍传类型化数据。提交成功才算已写入，失败报告实际错误并停止日志消费。验证可在退役后以只读 SQLite 连接读回当前记录。

## 只读分页

安全审计与业务库共用[配置的日志总预算](../configuration/technical.md#2-可写configdocument)。本库按原 `rule_logs.id` 淘汰最旧已提交记录，不删除其他目录或业务库内容。原所有者在数据库锁外参加共同预算协调，避免两个连接互等；真实删除、文件回收与写入仍在自己的阻塞工作内完成。实际提交才确认保存，容量不足不改变已经确定的防护决定。被淘汰记录不再保证分页可读。

- `SecurityLogQuery { before: Option<u64>, limit: u16 }`。`before` 是本库的排他行号上界，范围 `1..=i64::MAX`，编码为十进制字符串。省略或 null 从最新提交开始。`limit` 为整数 `1..=64`。
- `SecurityLogPage { records: Vec<SecurityLogRecord>, next_before: Option<u64> }`。`SecurityLogRecord { id: u64, record: SecurityLogEntry }` 的 `id` 是该原记录的本地行号，编码同 `before`。按 `id` 倒序。还有更旧的记录时，`next_before` 为最后返回的行号，否则为 null。空页也返回原确定结果。`SecurityLogEntry` 承载原规则日志或本页定义的写关闭审计。

日志模块在固定连接上用主键范围查询已提交记录。查询参数限行号与页大小。每行按实际 `SecurityLogEntry` 还原，保留规则日志或写关闭审计的原字段。行号只用于同一日志文件的分页。第一页之后的新写入不会插入本次较旧页面。[SQLite 行号表](https://www.sqlite.org/rowidtable.html)

读端口与写端口共享日志所有者的一条连接。读写在阻塞池串行使用连接，写端等待队列和编码发生在连接锁外。读请求争用时返回 Busy，输入无效返回 Invalid，持久化所有者退役后返回 Stopped，实际读取或还原失败返回 Unavailable。页内 JSON 还原在释放连接后进行。退出按原日志消费者收尾并关闭连接，读端口不延长所有者生命周期。完整记录经 Main 仅向 ManageService 提供。[正式 WS 运行审计](../../middleware/technical.md#运行审计)

事务和恢复复用 [SQLite 原子提交](https://sqlite.org/atomiccommit.html)及[同步模式](https://sqlite.org/pragma.html#pragma_synchronous)。中间层数据库契约单列于[业务日志](../../middleware/business-log/technical.md)。

## 写关闭审计

记录与字段只在[安全审计公共契约](../../contracts/security-audit.md)定义，所有生产者／消费者直接引用as-types同一类型。DriverEvent::FileClose由Main绕过进程事实补全、规则匹配及Ask，通过原日志队列等待容量后移交；原队列真实关闭才报告日志拒收。日志模块不创建fanotify／inotify来源，不另有unmark或通知drain循环。最终Source结束后，原生产者寿命关闭，日志消费者排空同一队列并提交后退出。

目录配置的唯一读取及导出见[配置](../configuration/technical.md#写关闭审计范围)，回调、目录对象、路径及消息引用寿命见[驱动](../../drivers/linux/hooks.md#写打开与写关闭)。路径读取实际失败保存原负errno；不丢原因，不因单条路径缺失宣称整个来源失败。通用驱动丢失、日志队列拒收、SQLite失败分别报告，不映射成已退役fanotify的专属状态。

历史[R65](../../evidence/file-wait-extension.md#r65-原版-openclaw)只证明当时fanotify来源，不证明当前Native替换。当前源码／加载／真实日志结果按本次冻结候选单独认定。
