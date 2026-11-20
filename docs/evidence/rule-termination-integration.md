# 规则阻止与终止接入验收

日期：2026-11-07；代码基线93c336fc7，结果冻结于d79b4d3b9。后续进程／网络专项真实验证见[当前记录](process-network-rule-integration.md)。本页记录规则终止接入及真实证明，不替代[规则](../backend/rules/technical.md)、[原操作](../drivers/linux/operations.md#终止与释放)和[DriverTools](../drivers/linux/tools.md)唯一契约。

## 本轮完成

- None只阻止；Actor阻止并请求终止原actor；ActorAndParent同时请求原actor和捕获时的物理父进程。驱动直接规则、底层Remembered／Fast、Ask回答／期限到期／渠道断开都已沿原操作收尾接通。
- 复用DriverTools唯一kill_bound实现，没有在规则、消息线程或Rust再写一套发送。显式PID工具仍使用当前调用者权限；规则绑定入口使用原快照保管的已鉴权采用者cred，窄范围override／revert并调用kill_pid(SIGKILL,0)，保留系统凭证和LSM。
- actor／parent／controller均保管真实pid引用。parent在初始RCU捕获时固定，不在等人结束后重查当前父进程；源故障或中断不因返回-EPERM误触发终止。Completing只执行一次；锁外两项尝试完成后才发回执，held／capture／快照寿命覆盖整个过程。
- 既有64字节as_result表达scope和两个实际信号结果，经NativeResult、OperationReceipt／Driver直接结果、MainNotice、日志请求和中间层Receipt完整消费。原操作Denied与信号Failed独立；Both逐项尝试，保护controller等目标时不假称全部成功。
- 启用既有Actor／Parent能力位4／8，未增加接口版本、旧解析、用户态补杀、执行Owner或额外业务线程。

## 真实内核结果

最终Root任务`9e7a2af84f854c8eac6697e8e7a9d14f`通过。aarch64、7.0.0-1019-nvidia隔离QEMU，测试actor实际setuid为65534，物理parent保留uid0。目标可写权限0666，排除DAC预先拒绝；原文件内容均不变。

- driver-none：原打开返回EPERM，actor正常退出1、parent正常退出0；没有信号请求。
- driver-actor、remembered、answer：actor实际wait为signal9，parent正常退出0；信号报告Actor Sent、Parent NotRequested。answer的原结果原因为User。
- driver-parent、fast、timeout、disconnect：actor和原parent均实际wait为signal9，两个信号报告Sent；timeout和disconnect原因为Timeout。
- protected-parent：actor实际因SIGKILL退出；controller父目标返回Failed(native_error=-1)，controller仍存活、原操作仍Denied。不存在因为父终止失败而放行。

测试使用专门的guest家族探针和subreaper实际wait，无测试helper kill或父死信号制造结果。Both场景父进程在报告actor结果后保持存活，直到真实AS信号到达，避免把正常过早退出的合法NotRunning误判成产品失败。测试规则明确限制来源为探针／subject，文件内容校验仍真实执行。

原有DriverTools指定实例测试、整机进程树／生命周期、自保11项副作用、普通直接裁决、询问／超时、无前端、配置采用、健康重启、strict冷恢复和卸载均通过。QEMU、subject、scope、supervisor退出0，子树与监督进程全部回收。

## 本地检查与源码对应

- Rust1.89全workspace／all-targets测试、严格文档、安装布局软件冒烟通过；本机1.92全target Clippy `-D warnings`和fmt通过。真实Unix认证测试使用普通本机UID环境，沙盒EPERM不能冒称产品通过。
- 最后仅家族探针的父存活协调和example调用参数调整，受影响example check／Clippy、fmt及上述最终Root通过；生产逻辑未在实测后改动。
- Rust Analyzer实际查共享回执、TerminationReport、原操作结果投影及解码引用，受影响文件诊断为空；Native Core／Tools clangd均零错误。未使用xtaskx。
- 新回归证明已请求信号的-ENODATA仍为Failed，不能当成NotRequested丢弃已经确认的Deny；未请求但报告Sent及Allow携终止都拒绝。
- 冻结包82条源码清单全部与最终源匹配，未留下Root后源码例外。包为`/tmp/as-rule-kill-main-delivery-20261107/initramfs.cpio.gz`，SHA256 `646e09ecb830d4c050cbbc099952e079e70f6f3a312b49cad47ca7fd355c82e5`；Native SHA256 `d19d7ac88c9129f4672d174f955f08b6ced1250be0457c01cb51e8c3a809ede1`。

原始Root结果`/tmp/as-rule-kill-root-delivery-result.jsonl`，清单`/tmp/as-rule-kill-main-delivery-20261107/root/suite/source-main-sha256.json`。本地日志`/tmp/as-rule-kill-{tests,clippy,fmt,doc,installed,core-lsp,tools-lsp,lsp-before,lsp-after,fixture-check,fixture-clippy,package-delivery}.log`。

首轮任务`3ccbbcb4004b4c8d8a28bbe3a8c1552b`失败：当时测试规则未限制来源，AS读取校验文件即被拒，尚未进入规则终止探针；仅修正冒烟规则source后，同Native任务`1c1cdebc7c0540939a663838bf1d4840`全部通过。最终包再消除测试父进程过早退出竞态并复验通过。两份早期原始结果和包均保留，不把失败或旧SHA改写成最终证明。

## 明确边界

SignalSent只表示内核接受信号；本轮以实际wait另行证明所测目标退出，不承诺所有目标立即退出。未执行真实PID轮转／reparent／非主线程exec并发压力、容器PIDNS init专项、其他内核或x86实机，以及Process／Network终止专项。复用标准pid／signal对象机制不等于上述场景已经压测。

未向宿主进程发信号、未在宿主机加载驱动／改Lockdown／改签名信任，Root代理保持运行。真实OpenClaw只由指定聊天验收，本轮未直接操作。MCP管道控制、精确容器绑定、正式安装分发和其他产品欠账不因本功能完成而被取消。

独立交叉审核合格，可合并，无剩余实质阻断。报告`/tmp/as-independent-rule-termination-review/final-review.md`独立核对最终源码／完整SHA／Root终态和真实wait，结论限本轮规则终止接入。证据摘要`/tmp/as-rule-kill-validation.json`。
