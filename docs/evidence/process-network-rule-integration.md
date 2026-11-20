# 进程与网络规则真实冒烟

日期：2026-11-07；基线d79b4d3b9。本轮完成已有Execute、Connect、Send入口的真实规则验收和字段说明，未增加fork前控制、普通文件Read／Write或MCP管道能力。

## 真实规则消费链

探针和媒体复用上轮FamilyProbe、原生hook、DriverTools及TerminationReport；没有另外实现规则解释器或测试helper kill。测试规则写入同一正式规则文件，经PolicyDocument读取、完整校验／编译、MainAssembly采用和DriverPort下发。

Driver项直接在原生匹配；Backend Fast项先由同类Driver ToUser保管原操作，再进入Process／Network派生规则工厂，沿正式决定和原操作回执收尾。观察器由原来的文件路径过滤改为同一PID＋EventKind＋operation，并检查最终实际rule_id，不能用其他规则的拒绝顶替本项。既有文件问询与自保用例保持同链运行。

## 最终真实结果

Root任务`62ce6f9497454a40ab5f23dee69efe02`通过，aarch64、7.0.0-1019-nvidia隔离QEMU。每种操作分别在Driver和Backend Fast正式规则中，执行Allow正控及None／Actor／ActorAndParent：

- **Execute／exec**：Allow真正切换镜像，目标程序写入marker；三种拒绝中marker保持原值。None返回EPERM，actor正常退出1、parent正常0；Actor为actor signal9、parent正常0；ActorAndParent为两者signal9。
- **TCP Connect**：Allow在真实回环listener接受连接；三种拒绝无连接到达。原始syscall／wait及分项终止结果均与选择一致。
- **已连接TCP Send**：所有场景都先建立真实连接；Allow接收端收到`changed`，三种拒绝接收端没有字节。不是用提前阻止Connect替代Send验证。
- **UDP SendTo**：Allow接收端收到实际数据报`changed`，三种拒绝无数据报；覆盖显式msg_name分支，TCP Send覆盖socket peer分支。

以上32条结果全部独立检查实际命中规则ID、原Allowed／Denied、两项信号报告以及真实wait；actor已降至uid65534，parent保留uid0。Both探针保持父进程存活，直到真实AS信号到达。网络只用guest回环；接口在guest内明确启用，无宿主服务、外网或宿主信号。

原有文件8阶段、controller父保护的部分失败、DriverTools、树镜像／生命周期、自保11项副作用、普通裁决／问询／超时／headless、配置、健康重启、strict冷恢复和卸载均通过。QEMU／subject／scope／supervisor退出0，子树与监督进程全部回收，Root代理保持运行。

## 字段契约与校验修正

[规则技术文档](../backend/rules/technical.md#当前配置字段字典)按实际JSON键列明顶层、组、规则、scope、action、文本／参数／端口对象、12主体字段及三类target。明确原actor旧镜像／argv和请求目标的区别、task comm、本地连接前采样值、IANA协议号、FMODE访问位，以及Driver／Backend树补全／当前无供给字段。

字段核对发现TextMatch额外键被静默忽略。实际完整YAML负例先失败，增加现有serde的deny_unknown_fields后，同一未知字段被拒绝且合法规则继续通过。没有加入兼容解析、忽略未知字段开关或新规则模型。独立复现记录`/tmp/as-rule-field-audit/{before,after}.log`，主回归`/tmp/as-process-network-field-{before,after}.log`。

## 检查与产物

- source本机CI环境后，Rust1.89 workspace／all-targets测试、严格文档与安装布局软件冒烟通过；本机1.92 Clippy全target `-D warnings`、fmt及差异检查通过。没有借远端CI补工具或运行GitHub Actions。
- Rust Analyzer检查私有completion的迁移、FamilyProbe及TextMatch实际引用，受影响文件诊断为空；没有使用xtaskx。本轮未改Native源码，复用已经真实加载并审过的同一Native模块。
- 最终包`/tmp/as-process-network-main-20261107/initramfs.cpio.gz`，SHA256 `f08d5ac59221b015d0635142d207a0e14cdc0e0683036bcef93778143a438990`。
- Native模块SHA256 `d19d7ac88c9129f4672d174f955f08b6ced1250be0457c01cb51e8c3a809ede1`；82项源码清单全部与最终源匹配，无Root后源码例外。
- 原始Root记录`/tmp/as-process-network-root-result.jsonl`；源码／产物清单`/tmp/as-process-network-main-20261107/root/suite/source-main-sha256.json`；本地`/tmp/as-process-network-{tests,clippy,fmt,doc,installed,lsp-before,lsp-after,package}.log`。

## 范围

本轮真实验证IPv4的TCP／UDP和普通ELF exec；IPv6、解释器链、其他网络协议、真实PID轮转／重新挂父／非主线程exec并发、容器PIDNS init、其他内核／发行版和x86实机未执行。32项是实际验证范围，不是新增的开发准入门槛，也不按数量计算产品进度。

SignalSent仍只表示信号接纳；所测退出由实际wait另证。未在宿主加载模块、修改Lockdown／签名信任或重启；真实OpenClaw仍由指定聊天执行，MCP管道、精准容器绑定和正式分发欠账保持不变。

独立交叉审核合格，可合并，无剩余实质阻断。报告`/tmp/as-independent-process-network-review/final-review.md`独立核对规则字段、未知字段修复、32条原始结果和82项源码／完整产物SHA。摘要`/tmp/as-process-network-validation.json`。

## 2026-11-15三类默认WS与发送原返回

正式frontend.py默认问询已统一File.Open、Process.Execute、Network.Connect及Network.Send，复用原subject／rule_kill_probe和同一问询、回执、SQL核对。新增Send外层fprobe复用原request_enter／request_leave，覆盖socket write、writev及sendmmsg；只有真实Network.Send等待才发布原返回，没有新增普通文件Write Ask。没有Actor PID／来源探针白名单或测试Allow。

R3包`/tmp/as-coverage-ws-r3-20261115`，镜像SHA256 7936e9297062aa3e26c20730a5db211ebf1bcf660100755cb47506c99fe9d032。as-service SHA256 a61d53822e3ff11131e997dc0ba475e84882f5e84eb286ac478e95f7a614b039，as_wait.ko SHA256 77098b2d1a378414d87a39458237ebdd3dcdff37e77919bc9929233fa8157fff。内核aarch64 7.0.0-1019-nvidia，Image SHA256 6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6。其余BPF／loader复用原构建的未变资产，未复制旧source manifest冒充新构建。

真实自有guest共16条原Ask：openat、execve、connect、sendto、sendmsg、socket write、writev、单消息sendmmsg各Allow／Deny。答复前核实际actor／线程、同syscall和as_wait等待，文件／目标映像标记、TCP连接／字节及UDP数据报分别核实际效果。全部16条原EventKey／DecisionId／规则／回执及原生返回在SQLite核对，允许原返回依次为FD3、0、0、7、7、7、7、1，拒绝均为-EPERM；sendmmsg的1是消息数，不是7字节。普通协议、ReadOnly、聊天、原日志和正常退出工作流同批通过；AS／QEMU实际退出0、模块卸载、无强制退出。

原件`/tmp/as-coverage-ws-r3-run-20261115/report.json` SHA256 4658aa2c830e03b747f9185924cb79ef8e09871adda986e77e4868680189f18b；middleware-report.json SHA256 cfffe9fd7b9713738a6dcb77f2c91e9899db9999ea7670e73140ea3351f16edd。该真实运行后只加强脚本的原返回标量断言；当前verify_native_return已对这16条未改原WS／SQL报告实际重放，并拒绝7项错误单位／状态替换，原件`/tmp/as-public-message-20261115.RrKEdtIr/native-unit-replay.json`。没有把旧>=0检查称为严格单位验证，也未为此新跑模型或回写原报告。

本地workspace／all-targets、严格Clippy、fmt及Python语法通过。Rust Analyzer原枚举引用及诊断通过；clangd实际LSP取得request_probes六处引用、零错误，保留12条既有内核宏sizeof提示。clangd的GCC命令适配只在临时CDB处理lp64／aapcs及不支持的bounds-strict选项，实际GCC Kbuild参数未改。首次本地构建因缺pahole失败，使用Ubuntu Noble官方pahole1.25包解到本轮临时目录后原build.sh module成功，实核BTF／BTF.base／BTF_ids并实际guest加载；没有安装宿主软件、加载宿主模块或调用远端CI。

最终严格断言脚本另经原打包／guest入口真实复验通过：`/tmp/as-coverage-ws-strict-20261115`镜像SHA256 389f0cbb0697712ab9e2b29f1b012c754842a1b339d20d67ddcf777080cc3b71，服务及驱动与R3逐字节相同。`/tmp/as-coverage-ws-strict-run-20261115/report.json` SHA256 23959856b793173b85dc6530e52623b90495b9a8c6de182b65b123147f31c827，middleware-report.json SHA256 5931216148ae219f5df9bdd387def6c4bf776a3d52545893c33892f6a3c12e80。16条原问询、16条原返回逐值通过，文件保留FD范围核对，Execute／Connect明确0，五种Send明确7字节或1消息，拒绝明确-EPERM；30条聊天游标、20条安全日志对账通过，真实退出0且卸载。此前各批原件未覆盖或重写。

这是[量化完成点C](runtime-correlation-research.md#原专项量化点与历史排期)，不是工具精确归因N／H或真实GLM固定提示F。原stock R67本地provider三类结果和R95真实GLM仅文件的边界继续保留；本批不增加Agent改造、注入或SDK。

## 进程执行与网络 WS Ask 验收

同一正式鉴权 WS 在真实 aarch64 `7.0.0-1019-nvidia` 隔离 guest 执行六项。每次答复前原 `execve`、`connect` 或 `sendto` 分别在 `as_gate_exec`、`as_gate_connect`、`as_gate_sendmsg` 的 `finish+0xaa0 [as_wait]` 等待。Ask 与回执的原 EventKey、DecisionId、EventData、RuleHit 及规则版本一致，六次实际结果均为用户答复。

| 操作 | Allow 实际效果 | Deny 实际效果 |
| --- | --- | --- |
| Execute | 目标 ELF 执行，标记改为 `changed` | 原调用返回 EPERM，标记保持 `original` |
| IPv4 TCP Connect | 回环监听端接受连接 | 原调用返回 EPERM，监听端无连接 |
| 已建立 IPv4 TCP Send | 对端收到 `changed`，共 7 字节 | 原调用返回 EPERM，对端收到 0 字节 |

基线 Send 的目标操作与 WS 答复发送同时在 `as_gate_sendmsg` 等待。审核客户端的原 WS FD 在 `sendto` 内耗时 8150.3 毫秒，超过原 Ask 期限。规则分流修订后，真实答复发送均以原 `NoMatch` 快速收尾，其目标端口与 Ask 规则不同。Send Allow／Deny 的答复发送耗时分别为 14.777／10.762 毫秒，实际目标操作仍在原期限内接受用户答复。第一次夹具运行收到下一探针启动产生的正常 `NoMatch` 回执，后续每项通过正式规则接口清空上一项测试规则再准备探针。

最终包 `/tmp/as-ws-exec-network.MaUl7ens/final-candidate/initramfs.cpio.gz` SHA256 `fdb30dbf01d42bfe5bb2257c265ffc12a44d652ad91af51663023d9a80ad1baa`，实际 guest 包 SHA256 `42e93f9deed93d3e9b8a72cecbff781be5d426f6f32a020aaf5c8e9682fa8327`。原生驱动产物沿 R56。服务经原 RequestStop 退出 0，WS 双方 Close，guest 正常退出并卸载模块。六条 Ask 的 ID 和剩余时间与 SQLite 一致，库中有六条对应执行结果。受影响 Rust 测试及严格 Clippy 通过。

原件为 `/tmp/as-ws-exec-network.MaUl7ens/{diagnostic-guest.log,final-guest.log,result.json}`，结果 SHA256 `f8370e52cf6a35420a6b2cebd716ad3d4af3318715a5471107afd78b4f6ebced`，独立复算脚本为同目录的 `verify.py`。范围为原生 ELF exec 和本机 IPv4 TCP。fork 沿进程创建事实链，精确归因沿现有底层缺口。

### 工具执行交接候选复验

复用上述专项场景，在 [工具执行交接候选](middleware-business-store.md#工具执行交接消费)的同一 as-service 和原生驱动产物上复验。测试镜像只叠加原专项启动入口和脚本。六个 WS Allow／Deny 场景均通过，原系统调用分别在 Execute、Connect 和 Send gate 等待。进程允许后执行目标映像并写标记，拒绝返回 EPERM 且标记不变。Connect 允许时对端接受连接，拒绝时无连接。Send 允许时对端收到 7 字节，拒绝时收到 0 字节。每个场景匹配实际规则版本、原 DecisionId、EventKey、EventData 与回执。规则按目标程序目录或回环 TCP 目的地址及端口匹配。SQLite 六个原 Ask 身份及期限与 WS 一致，共有六个执行结果。服务和 QEMU 退出 0，原生模块卸载。

结果 `/tmp/as-tool-handoff.TKXfoWwW/process-network/result.json` SHA256 `8459793fee955846d99c78ef57b2f38cf46f881abf3969b83bb2f180049c13bb`，测试镜像 SHA256 `e8985003883a88a9e1534e19dc91cbb565f63ea7581b0658cc3796355839bfa7`。对照原件和只读复算脚本在同目录。上述六项为真实内核及正式 WS 实测，原版 OpenClaw 本轮验收范围为文件写打开。

### 原生请求返回复验

2026-11-12 将 Execute、Connect 和 Send 接入 [原生请求返回](../contracts/events.md#原生系统调用请求返回)，与 FileOpen 共用驱动在途观察、适配、业务库和活动查询。在 aarch64 7.0.0-1019-nvidia 客体复用上面的六项原 WS 场景。专项包只替换测试入口，正式服务和驱动与基础客体逐字节相同。

Execute 的 EventKey 为 37／75，真实包装函数返回 0／-EPERM，目标文件标记分别改变／保持。Connect 的 EventKey 为 108／141，返回 0／-EPERM，监听端分别有／无连接。Send 的 EventKey 为 175／210，返回 7／-EPERM，对端实际收到 7／0 字节。每项按原 DecisionId 核对同一 EventKey、操作类别、独立等待编号及返回值。两个客体与原生模块均正常退役。

基础报告为 `/tmp/as-request-families.lUkaQ15B/base-guest/report.json`，SHA256 `f8dc08a84c37deeeb75ec3dee89c4726997f7db1236bca917b6cbcac2b25a5e5`。专项报告为同目录下 `families-guest/report.json`，SHA256 `8e981386b6f4225d8e1dc4af3b2df6b2d007f68f13e6d9b4f8e6934678c5c938`。候选清单 SHA256 为 `d8b0aba35f1b532bccd7e09b3748585f4c9e4fd66d14d942ea3b5de7988cb576`。工作区 200 项测试、严格 Clippy、格式及 Rust LSP 检查通过。
