# 驱动工具实现与联调

日期：2026-11-07；基线7755bc503，结果冻结于93c336fc7。后续规则Actor／Parent接通见[新验收记录](rule-termination-integration.md)。仅记录本轮实际实现和验证，不替代[DriverTools契约](../drivers/linux/tools.md)。

## 已实现

- as_tools.c／h统一无状态进程身份查询与指定实例SIGKILL，原核心中的查询实现已迁出，不保留第二实现。核心仍独占鉴权、接纳、容量和会话状态。
- DriverTools为DriverSession的独立能力，ProcessFactsPort保持只读；DriverPort复用现有连接。软件源明确Unsupported。目标由真实驱动instance、宿主TGID和纳秒birth组成，复用实际ProcessBinding；未增版本、兼容分支或第二通讯路径。
- 已引用TGID任务直接提供出生时间，固定struct pid后以kill_pid(pid, SIGKILL, 0)发送，不在校验后重新按数值寻找进程；保留系统凭证和LSM。SIGKILL提交在core_lock外，不等待退出；先预占原Reply容量，既有control_writes保管到完成。
- 成功只报告SignalSent／NotRunning，出生身份不符明确Conflict，不伪造进程退出或规则OperationResult。规则Actor／Parent自动终止能力位仍未开放；此工具不表示此前两项规则动作已经完成。

## 实际验证

source本机CI环境后，Rust1.89 workspace/all-targets测试、严格文档及安装布局软件冒烟通过；本机1.92的Clippy全target `-D warnings`、fmt通过。Rust Analyzer查DriverTools、from_binding和两层kill_process实际引用，三个受影响文件零诊断；两个Native文件clangd检查零错误，未用xtaskx。

Native使用已有Kbuild和匹配当前内核的gen-btf.sh；最终模块具有.BTF、.BTF.base、.BTF_ids。工具从既有/tmp/as-driver-tools复用，不新引入依赖；Kbuild初步跳过BTF的记录与后续显式生成分开，最终可加载性以下述实际联调为准。

Root任务`f17fe38871ed47dc978d705296776df2`通过。aarch64、7.0.0-1019-nvidia隔离QEMU中，实际驱动和Main运行：

- 创建只属于guest的等待子进程，查询真实内核身份。
- 错误birth返回Conflict；错误driver instance返回Conflict；0／越出正pid_t范围输入返回InvalidInput。以上拒绝后目标仍存活。
- 宿主PID1、AS控制者及实际kthreadd请求均Forbidden。
- 正确目标返回SignalSent，父进程实际wait确认signal=9；回收后再请求同一目标返回NotRunning。此处退出证据来自wait，不从发送回执推定。
- 原有进程树、自保11项副作用、直接规则、人工答复／超时、无前端、配置采用、健康重启、strict冷恢复及卸载均通过。

QEMU、subject、scope、supervisor均退出0，测试子树及监督进程已回收。未向宿主机进程发信号，未在宿主机加载模块、修改Lockdown或签名信任，Root代理保持运行。

## 对应产物

- 冻结源码清单81项全部与当前源匹配：`/tmp/as-tools-main-20261107/root/suite/source-main-sha256.json`。
- 镜像`/tmp/as-tools-main-20261107/initramfs.cpio.gz`，SHA256 `466b323da2f679acf41589e57d0b66c22a060ea0e638b537a4ef7e252e7a0ed2`。
- Native模块SHA256 `812f3130d4af6abbcbdac80b5905a400ec2344816fc9ef55628f1e4e45be1e6c`。
- 原始Root结果`/tmp/as-tools-root-result.jsonl`；本地`/tmp/as-tools-{tests,clippy,fmt,installed,doc,native-build,core-lsp,native-lsp,lsp-before,lsp-after,package}.log`；摘要`/tmp/as-tools-validation.json`。

未执行x86实机、其他内核、真实PID轮转／非主线程exec并发压测；上述竞争处理采用内核标准pid／signal实现及源码审查，不宣称已经压测。D状态退出耗时不作立即完成承诺。真实OpenClaw不由本聊天操作。

独立只读审核合格，可合并，无剩余实质阻断。审核独立核对81条源码与完整产物SHA、Root终态、SIGKILL实际wait及拒绝后存活；报告`/tmp/as-independent-driver-tools-review/final-review.md`，不等于完整驱动／MVP交付。
