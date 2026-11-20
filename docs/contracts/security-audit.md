# 安全审计记录

本页定义驱动、主模块、安全日志及查询消费者共用的事实。数据库、分页和提交归[安全日志](../backend/security-log/technical.md)，驱动来源及原生对象寿命归[写关闭hook](../drivers/linux/hooks.md#写打开与写关闭)。本页不定义匹配、询问或业务归因。

`SecurityLogEntry`只有原`Rule(RuleLogRequest)`与`FileClose(FileCloseAuditRecord)`两类，沿主模块已有日志队列和安全日志消费者流转。已读FileClose等待同一队列容量，真实关闭才报告日志拒收，不将日志拒收改成驱动失效。入队是接纳，SQLite提交成功才是持久化。

`FileCloseAuditRecord::FileWriteClose`使用以下唯一字段：

- `reporter_tid: i32`：释放回调时实际宿主线程号，取内核当前任务；不表示原打开者、写入者或因果actor，不据此查询一个当前同号进程冒充原主体。
- `device_major: u32`、`device_minor: u32`、`inode: u64`：原可写普通file对象在释放回调中的inode及设备值；inode按十进制字符串编码。它们不替代路径或进程身份。
- `path: Option<Vec<u8>>`：AS读取通知时，沿原持有path对象取得的路径原字节，未取得为None。与上述回调标量的采集时点分别保留；不宣称原子全局快照、不按旧名称重新打开。不损失UTF-8之外的字节；成功空字节串保留`[]`。
- `path_error: Option<i32>`：本次路径读取实际返回的Linux负errno；读取成功为None。路径缺失不反推文件不存在、已删除或来源整体失败。

记录表示一个以写访问打开的普通`struct file`最后一个引用释放；未写入内容也可发生。dup／fork增加该对象引用不会令每次closefd都变成一条终结记录；多次独立open是不同file对象。它不表示最后一个inode writer、写入成功、数据落盘、病毒扫描或hash更新。

写关闭不走规则匹配／Ask，不构造RuleHit或OperationResult。旧fanotify专属overflow／source-failed变体随旧来源删除；实际驱动通用来源失效和丢失按原DriverStatus／通道契约表达，不能把全局dropped推成关闭专属丢失。现有日志拒收与数据库失败各自保留真实结果，不吞错、不冒充已刷盘。
