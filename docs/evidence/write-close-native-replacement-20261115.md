# 写关闭来源替换与实际联调

本轮按用户明确要求退役fanotify，统一到现有驱动、主模块与安全日志链。当前源码／产物绑定和实际结果见 `/tmp/as-write-close-native-replacement-20261115/result.json`；最终原件为 `guest-main-r4.log`，源码快照为同目录 `source/`，包内清单为 `guest-main-r4/root/suite/source-main-sha256.json`。

## 实际替换

- 删除安全日志的 `close_write.rs`、fanotify fd／标记／读取／退出循环，以及不再使用的nix、async-io依赖。日志模块只消费既有队列、提交SQLite并分页查询。
- 唯一事实来源是同一BPF对象的 `file_release → as_observe_file_close → 原Native FIFO`。写关闭不生成RuleHit、Ask或操作结果，不把释放线程冒充原写入者。
- 目录绑定复用内核fsnotify mark的销毁／卸载寿命及inode/group索引；它不监听FS_CLOSE_WRITE、不产生第二条关闭流、不建立用户事件fd。避免裸持inode破坏superblock关闭，也不永久pin配置mount；普通规则替换复用原实例，不按旧名称重绑。
- 关闭消息为一个原生持有上下文，保留原path引用。路径在AS实际设备读取上下文取得，读取／清空／错误及物理释放统一处理；原release_queue完成path_put后才退还在途寿命。没有复活file，也没有重新按路径打开目标。
- 原 `FileCloseAuditRecord` 提升为as-types唯一定义，实际路径错误以path_error保留；原FAN专属状态及旧reexport删除。主模块让FileClose事实直接进入原 `SecurityLogEntry` 队列，规则记录包装Rule，普通拒收／写入失败如实报告。字段及采集时点只在[公共契约](../contracts/security-audit.md)定义。
- 配置模块由同一启动快照导出48字节头及目录描述，主模块不解析目录或派生另一份路径。新0x10b正文和feature256见[唯一线格式](../contracts/driver-communication/technical.md#写关闭审计事实)，未加旧格式解析、接口别名或版本回退。

安全日志实现由原531行降至286行；Native Core新增对象保管、读取与目录寿命逻辑约279行，另有纯公共记录及窄解码／导出。不能把跨模块迁移算删除，不能以单一文件行数冒称整个项目已减量。

## 同批清理的真实问题

首次正式Main在写关闭前遇到旧argv测试：由AS自己spawn，却期待后面的普通Deny越过前面的维护Allow。测试改为同一个BusyBox独立执行链，保留真实Child输出、见证及同PID原生回执；按实际两次成功exec核cookie，不修改生产规则或增加测试Allow。原 `guest-main.log` 保留，该轮关闭场景为未执行。

随后 `guest-main-r2.log`、`guest-main-r3.log` 已完成三类和关闭主线，却在最后冷恢复报告EPERM。真实阶段记录表明配置／规则初始化已完成，失败位于Main.start；源码又表明自保写需要实际控制者身份，而旧装配先开日志、后连接驱动。主模块就地调整为先建立并绑定真实驱动控制身份，再打开固定日志库，再采用镜像和启动业务；失败等待原退出并保留SecurityLog I/O上下文。未放宽Self权限或另加豁免。最终同一完整流程通过，旧失败没有删除或回写。

## 实际完成的验证

原 `drivers/linux/build.sh all`、当前源打包和有效终端LSP检查通过。实际guest加载29个BPF入口，包括新增file_release；原SDK加载诊断通过仅证明对应加载和原操作范围，不代替本次正式配置／存储闭环。

最终正式Main使用真实配置／规则模块导出，依次执行普通业务后才核关闭边界：

- 文件普通规则、写打开／询问及真实自保；进程Execute、网络Connect／Send／UDP SendTo均覆盖Driver和Backend Fast的Allow、仅Deny、Actor、ActorAndParent，实际见证／连接效果／wait／原回执分别保存；受控TLS入口另有实际原目标与发送允许／拒绝结果。
- 写关闭核对只读不产生事实、无内容写打开仍产生事实、非UTF8路径原字节、dup最后引用、释放线程／设备／inode与独立系统API值。原查询与退出后SQLite逐字段一致，不假称只是入队即刷盘。
- 目录改名仍跟随原对象，规则Reload真实采用成功；原路径同名替代不被误绑定。恢复目录名并确认记录已提交后，普通tmpfs卸载实际成功；卸载后底层同名目录不自动换实例。没有Busy inode残留或永久mount钉住。
- 健康退出、真实控制进程退出后来源错误、静态后端冷恢复及最终模块卸载均完成。`AS_MAIN_NATIVE_SMOKE_PASS`、`AS_MAIN_COLD_RECOVERY_PASS`、`AS_MAIN_NATIVE_MODULE_UNLOADED`、guest退出0，以及宿主QEMU实际wait退出0均取得。

驱动实况中实际读到的dropped为0，只代表本次已执行负载，不外推零丢样容量。普通日志仍按原有有界接纳报告拒收，不用全局dropped推定文件关闭专属丢失。

部分串口行有既有诊断前缀与JSON共行，未修补原字节。`exact-embedded-domain-records.json`只在未变原文中按确切起点解码完整JSON子串，保留位置，取得32条进程／网络及4条TLS规则原记录；未按时间猜关联，也不因逐行解码缺失抹掉已取得实际效果。

软件原件包括workspace库测试、叶子严格Clippy、as-types／Main／日志与适配／配置测试、Dashboard严格解码/i18n/tsc及LSP。独立只读审核覆盖跨层接线、目录mark引用、部分初始化、读取／释放和最新启动依赖，结果另见同目录 `review.json`；审核不冒称自己另跑了一次guest。

## 不外推的范围

本轮未重新执行宿主签名／加载、其他内核与架构、所有文件系统完全等价、路径失败故障注入、真实GLM／原版OpenClaw／浏览器实测。历史R65只证明旧fanotify来源，不能覆盖新源码；当前是上述Native/Main实际范围通过。原生max_hold_ms及已有HIPS规则期限未在本轮变更，不把它们称为模型自然超时。

模块规范见[驱动Hooks](../drivers/linux/hooks.md#写打开与写关闭)、[安全日志](../backend/security-log/technical.md)及[启动顺序](../backend/main/technical.md#执行与入口)。历史原件、用户roadmap和原Root broker保留，未修改Agent或恢复旧研究任务。
