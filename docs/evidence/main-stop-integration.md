# Main授权停止闭环

本轮基线6d399be9b7cb80d3161caecf47b02341ea22371f，只补Main控制／生命周期缺口。产品目的、装配、唯一结构分别见[Main设计](../backend/main/design.md)、[Main架构](../backend/main/architecture.md)、[消息技术契约](../backend/main/messages/technical.md#停止请求与真实退出)，不在本页另定义字段。

## 实现

当前RequestStop取代旧不可用停止命令，不保留别名或版本分支。管理权限、原urgent预算和队后短认证临界区通过后，单次宿主端口接纳才返回Accepted；没有监听者、接收者掉线或已消费不假接纳。命令任务不等待退役或自己的tracker排空。

宿主收到请求立即确定1000ms本地回执尝试截止，关闭外部管理和原Ask，继续poll原session；实际完整写出、session结束、外部信号或截止到达后，统一由已有MainThreads.shutdown真实退役。该局部宽限不是Ask续期／驱动／退役期限。运行时StopReplyWrite仅随原CommandReply线性移动，完整JSONL成功才消费；编码、入队、部分写、EOF或丢弃不伪造写出，也不称前端已读／进程已停。实际退出由外部真正拥有的Child.wait观察。

MainMessages构造收敛到原MainThreads.management_entry，重复取得为同一Arc；原TLS冒烟另建第二Main及单独close的旁路已删，专用Flow凭证夹具直接经自身端口核查，原Main由其唯一运行时close／drain。没有新总线、任务队列、全局状态表、Supervisor、任意PID／路径／shell停止接口或MainContext可写字段。

## 已执行

运行前source本机CI环境。Rust1.89完整workspace／all-targets／locked／offline168通过、0失败，原件`/tmp/as-main-stop-workspace-tests-20261109.log`。全workspace严格Clippy、rustdoc、fmt通过：`/tmp/as-main-stop-workspace-clippy-20261109.log`、`/tmp/as-main-stop-rustdoc-20261109.log`、`/tmp/as-main-stop-fmt-20261109.log`。Python当前客户端AST检查通过。

现有软件冒烟通过`/tmp/as-main-stop-software-final-20261109.log`：保留聊天背压／后续Ask／真实EOF及配置撤权验证；新增真实Unix RequestStop／Accepted全帧与本地写出见证、Readonly拒绝及原运行时实际退役。软件平台夹具不称Native／stock保护。

Main26项与transport7项定向检查通过；实际类型／旧接口反控用正确type封套并有当前正控，不把格式错误当退役旧接口证明。Root最终LSP三正式源无错误，bind_stop_listener、CommandReply、written与writer调用有效：`/tmp/as-main-stop-root-lsp-{references,diagnostics}.json`。

Main独审见`/tmp/as-main-stop-independent-review-20261109.md`；宿主／Python／transport独审见`/tmp/as-main-stop-host-transport-review-20261109.md`。审核发现宽限起点会受close耗时后移，已按收到请求立即记录截止修正，再执行最终整合检查；作者审核和父组合范围分开。

## 真实Native

包`/tmp/as-main-stop-delivery-r1-20261109`，镜像SHA256 `5ad392a5a70cc1e671ed8860ff23bd6c2b34d3266bb3c8a221a6165be54204f9`。151源码／18产物逐项匹配0差异，manifest SHA256 `f000026565b2e6698939ee804c6d63205a331753d0fc5bbfb9ae2e154251b3f6`，原件`/tmp/as-main-stop-verification-20261109.json`。

`/tmp/as-main-stop-native-run-r1-20261109/report.json`通过，SHA256 `20c761cf1f24f8eb38c5074221e95c377c09a12bcb727513c06ad5c501b13308`。28必须场景PASS，独立安全日志持久模块仍NOT_IMPLEMENTED。原本因Flow配置需完整重启的同一业务节点改走RequestStop：实际Accepted后没有发送OS停止信号，父进程wait到本实例PID100退出0并核本实例STOPPED／媒体退役记录，随后再启动并继续实际TLS／phantom／聊天及其余原场景。末尾Readonly阶段外部SIGTERM仍为明确独立监督入口，非请求失败后的兼容回退。

宿主UID1000与guest真实Root分别记录；实际Native/BPF与匹配7.0.0-1019-nvidia／aarch64内核沿原验证产物复用，未改原生帧。QEMU实际join、无timeout／failure。宿主模块、Root broker、MOK和原OpenClaw未改。

## 验收边界

原指定聊天已完成10ab7a1c420bdb72ff5ad45070f748721b54a5fc的R21有限验收，结论PASS。原件`/tmp/as-openclaw-evidence-20261110/r21/result.json`，SHA256 `8719e9344803df3749c0a852c9eb2744023613155cbd706f02181e1a9d9f0b29`。151冻结源码／18产物／原包和内核逐项匹配。

`authorized-stop-proof.json`分别记录两个真实AS实例：配置重载前PID150与全部业务结束后PID221，各仅一次RequestStop；各有原Accepted、本实例host-consumed／local written／STOPPED及外部Child.wait exit0，`os_stop_signal_sent=false`。接纳约15／22ms，实际退出约12.686／9.605s；1000ms回执宽限没有被当作总退役期限。OpenClaw原进程沿用，未停止宿主Root／OC。

六个真实请求均final，普通允许／拒绝、两User Ask及同一回答身份上的无人答复Ask原Timeout／Deny均与SQL闭合。公开5632条Chat的原标签、确切payload／revision与delivery位置逐项匹配，SQL5633包含独立真实source_closed；questions／operation_reports／question_completion各3。实际库写失败0，真实退役／卸载／QEMU join，无强制清理。独立软件停止／Readonly拒绝和冻结源168／0分别记录，不当作所有stock故障模式已覆盖。

旧在线脚本两项仍为PARTIAL_HIPS_PASS_TELEMETRY_FAIL，legacy stock-openclaw/result.json仍FAIL：其使用前一exchange的任意finished结束当前等待。初始散列原件与失败快照保留，不改绿、不重发。停止前完整原帧、各原请求的精确协议／工具／终态，以及随后独立SQL比对全通过，根result仅据这些事实裁决本轮有限PASS；差异见`online-observer-discrepancy.json`及`observer-status-resolution.json`。后续可复用脚本的修正与旧冻结输入分开。

Accepted、回执完整本地写出、进程退出三者分别证明，未借先前R20或producer Native结果倒填。正常未读／断开不取消已接纳停止；实际敏感半帧授权失效仍必须截断，不为强求回执延迟撤权。

正式分发宿主、systemd重启策略、驱动升级和其他剩余能力仍见[底层清单](../backend/gaps.md)，本页不声称它们已交付。未明确部分不由Main补任意shell或安装器。
