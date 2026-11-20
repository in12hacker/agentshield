# Linux内核驱动源码

内核部分采用BPF-LSM＋一个可加载原生支撑模块，保留用户发行版内核。原生模块名称为as_wait.ko，内核设备为/dev/as_wait。整体职责见[驱动架构](../../docs/drivers/architecture.md)，当前接口见[技术契约](../../docs/drivers/technical.md)。

- [as_core.c](as_core.c)：唯一状态／持久快照、三类快规则、当前设备协议和原操作保管。
- [as_self_protection.c](as_self_protection.c)与[私有接口](as_self_protection.h)：实际自保对象、精确exec实例和初始化栅栏。
- [as_hooks.bpf.c](as_hooks.bpf.c)：12个普通可等待LSM入口、16个自保入口及1个写关闭事实入口，完整集合见[hook 契约](../../docs/drivers/linux/hooks.md)。
- [as_runtime.c](as_runtime.c)与[私有接口](as_runtime.h)：实际libuv原始请求观察和在途交接；共享字段只列在[as_runtime_fields.h](as_runtime_fields.h)，不解释ToolCall或HTTP归属。[契约](../../docs/backend/driver-adapter/runtime-observation-technical.md)。
- [as_runtime_capture.c](as_runtime_capture.c)与[私有接口](as_runtime_capture.h)：在同一次有效回调内读取运行时元数据和实际值，保留部分结果及读取错误；不在回调结束后补读目标内存。
- [as_futex.c](as_futex.c)与[私有接口](as_futex.h)：原private futex等待／唤醒及真实队列条目的在途事实；不按地址或时间推定工具归因。
- [as_group_tcp.c](as_group_tcp.c)与[私有接口](as_group_tcp.h)：配置范围内实际TCP字节、序号及套接字事实；不在内核解析聊天或决定Agent关系。
- [as_protocol.h](as_protocol.h)：当前固定LE帧／正文的唯一原生声明和尺寸断言；旧原生头、旧解码和旧接口已经删除。
- [as_hook_facts.h](as_hook_facts.h)：BPF到Native的同hook私有标量输入，包含实际mount namespace及用户栈读取结果；局部缓冲不跨调用保留，不是第二条传输协议或状态缓存。
- [as_flow.bpf.c](as_flow.bpf.c) 和 [as_flow.h](as_flow.h)：受控TLS新连接的内核BPF入口及共享值布局。见[连接来源](../../docs/drivers/linux/flow-binding.md#当前受控入口实现)。

底层用户态加载器as_loader.c、TLS接纳器as_flow_loader.c及其私有FD报文位于[驱动适配native目录](../../crates/backend/as-driver-adapter/native/README.md)。原build.sh all统一编排构建，不把用户态代码列为内核子类。内核中的生命周期、TCP、futex及运行时原始信息采集不因当前业务消费者未接入而删除。

进程创建、exec 成功及最终组退出由原生 tracepoint 提供。具名普通文件 CreateFile 使用目标内核的 Fprobe 调用作用域保管实际原路径，在可睡眠的 inode_create 内复用 File gate 和原操作等待。所有已接操作、加载条件和实际限制见[驱动契约](../../docs/drivers/linux/hooks.md)。规则终止继续复用 [DriverTools](../../docs/drivers/linux/tools.md)。

自保从启用后的新修改／新共享可写映射开始，不追溯撤销启用前已有可写共享映射。Overlay根或嵌套Overlay当前明确Unsupported；详细对象／外部硬链接／冷恢复边界见[自保技术文档](../../docs/drivers/linux/self-protection.md)。

## 构建

需要目标内核的匹配 headers、BTF、Fprobe 创建作用域及 Kbuild 的 gen-btf.sh／resolve_btfids。构建使用 gcc、pahole、bpftool、clang BPF 后端、libbpf 头文件与静态库，以及 libelf、zlib、zstd 静态链接输入。

```sh
source /home/leo/.local/share/agentshield-ci/env.sh
CLANG=clang-18 drivers/linux/build.sh all /tmp/as-driver-build-output
```

`build.sh module OUTPUT` 仅构建原生模块，`all` 另生成 BPF 对象和静态加载器。`KERNEL_RELEASE` 默认取运行内核，DKMS 显式传入目标发布号。`KERNEL_BUILD` 默认取该发布号的 `/lib/modules/<release>/build`，且 headers 的 `UTS_RELEASE` 必须吻合。构建使用本机架构。

有目标构建树的 `vmlinux` 时由 Kbuild 生成模块 BTF。仅运行内核的 headers 缺 `vmlinux` 时，使用本机 `/sys/kernel/btf/vmlinux` 和同版 `gen-btf.sh`。此路径每次从 Kbuild 原始链接对象重链模块后解析 BTF ID，避免同目录重建重复改写已排序的 ID 与标志。未启动的目标需要自身构建树的 ELF BTF，缺失时明确失败。输出模块须包含 `.BTF` 和 `.BTF_ids`。工具输入保留 `PAHOLE`、`LIBBPF_INCLUDE` 和 `LIBBPF_STATIC_LIB`。

产物须满足目标内核签名／信任策略。构建成功不证明真实加载、规则生效、原操作等待或冷恢复；这些必须由当前产物联调取得。加载器意外EOF保留内核pins，只有显式正常排空后的U命令解除核验过的当前实例links。

## 冒烟边界

[驱动适配与正式主模块guest冒烟](../../scripts/native-smoke/README.md)使用真实适配接口与当前产物，在隔离guest中联调；主机Root操作由已授权代理统一调度，不并行加载同一主机模块。基础夹具、旧原始证据及编译结果不能当成当前真实保护效果。
