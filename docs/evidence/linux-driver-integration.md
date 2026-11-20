# Linux驱动与主模块联调记录

日期：2026-11-07。验收对象是本次重构的主模块／配置／规则／Linux驱动接线及文件自保。规范以[文档索引](../README.md)指定的唯一来源为准；本记录不是另一份接口定义，也不代表完整HIPS／MVP或旧WP已交付。

本记录对应提交`14f88e8e2a04d2fbe16861353117aa875999cd0f`及下列固定产物。后续进程树设计和FastFacts／SlowFacts接口修复有自己的本地验证，不能用本记录宣称新进程树已做真实内核验收。

## 冻结产物与执行环境

真实执行环境为aarch64、7.0.0-1019-nvidia的隔离QEMU，复用主机正在运行的内核镜像。由现有Root代理启动，未在宿主机加载本驱动、修改安全策略或重启；外部Root broker保持运行。

- 原生模块SHA-256：`9bb22496679910028bcacd56b5b829cc9b8c1f4fbab08bf4f71f660b6561b202`。
- BPF对象SHA-256：`9c76738d96f3adc3f33d971db24b0c8d47ae5a9d9b2e9cab35da46c8003bdd80`。
- 正式主模块镜像SHA-256：`7f61edc55aef2842cdf4e216a025e4ba69a4127de38c5b856e330b7512a71f7e`。
- SDK镜像SHA-256：`45539422c2df1fc7b5b424c8c42623a664de3b35f179e774e070802a0905b291`。

打包与源／产物清单入口见[冒烟说明](../../scripts/native-smoke/README.md)。两个静态Rust入口统一使用锁定getrandom的官方Linux后端，原生模块和BPF完全相同。实际完整27项附着校验通过；原生fork／exec／退出事实使用正式tracepoint。

## 实际通过的链路

正式Root任务`190bf0eb95c2472d8ea2c6bea53f580a`完成，QEMU、subject、scope及supervisor退出均为0，测试子树及监督进程均已回收。串口标记包含主链通过、冷恢复通过、模块已卸载和guest退出0。

- 正式MainAssembly按固定安装布局初始化，配置与规则分别真实导出并由驱动采用，真实状态为Running／SelfProtection Enabled。
- 独立进程的FileOpen直接Allow／Deny、绑定原操作后答复Allow、绑定超时Deny，以及无前端跳过Ask均取得预期实际errno。
- 启用前已有FD与根外硬链接的后续写、新共享可写映射、只读映射增加写权限、chmod、设置／删除xattr、外链写打开、创建、重命名、删除全部拒绝；探针核对内容、权限、属性与名称未变。
- 认证后的配置所属模块真实保存并完成内核采用；日志只观察到队列请求，不声称持久保存。
- 健康排空后新AS进程再次启动成功。随后通过正式规则更新将宽泛文件Allow改为ToUser，退出控制者，独立主体真实收到EIO；新的静态AS进程通过精确维护通道冷恢复成功，再真实排空并卸载模块。测试依赖只使用已声明的三条精确允许项，不泛放行AS或加载器读访问。

SDK Root任务`c7c9882566dd402dbb8b76e1c384e371`完成，全部退出为0且测试子树已回收。通过同一模块上的真实Held答复、绑定超时、未绑定到期错误、旧原请求跨规则替换及实际退代、临时开关保留／恢复、无状态轮询排空，以及控制者SIGKILL、加载器退出、子进程仍继承FD时保持EIO；恢复后迟到旧FD关闭不破坏新会话。

## 本地工程检查

已显式source本机CI环境并核验实际工具；以下在当前Rust实现和Linux构建配置上通过：fmt、Clippy全部目标且警告作为错误、Rust 1.89完整workspace／all-targets测试、真实安装布局软件冒烟、公共API文档missing_docs／warnings严格构建。Rust生产代码定向扫描未发现unwrap／expect／panic或unsafe块。

LSP已核主模块／配置／规则接口引用及诊断，并复核最终适配和Core；当前无错误。独立审阅核对源码、产物SHA与Root结果，按本记录范围验收。原生Kbuild保留编译器／pahole版本差异警告，不宣称原生零警告；实际匹配内核的BTF生成、模块加载、BPF verifier与联调已通过。

原始本地证据保留在：

- `/tmp/as-main-native-root-result-r5.jsonl`、`/tmp/as-sdk-final-root-result.jsonl`。
- `/tmp/as-delivery-msrv-tests.log`、`/tmp/as-delivery-installed-smoke.log`、`/tmp/as-delivery-clippy.log`、`/tmp/as-delivery-doc.log`。
- `/tmp/as-final-root-lsp.log`、`/tmp/as-delivery-driver-lsp.log`、`/tmp/as-core-devt-clangd.log`、`/tmp/as-native-current-complete.log`。
- `/tmp/as-delivery-validation.json`、`/tmp/as-delivery-production-sha256.json`及两个镜像中的源／产物清单。

提交前diff检查仅去除了as_core.c第530行的一个行末空格，无token或行号变化；测试源SHA与提交源SHA的对应保留于`/tmp/as-delivery-whitespace-normalization.json`。已测模块／BPF和上述镜像未改写，不以格式清理另跑一轮Root。

## 边界与清理

不追溯撤销启用前已有可写共享映射；Overlay根／嵌套Overlay明确Unsupported。文件自保不承诺阻止有内核管理权限的主体主动解绑BPF或卸载模块。具体边界只在[自保契约](../drivers/linux/self-protection.md)维护。

本轮未执行x86内核、deb／rpm安装、真实OpenClaw、普通进程与网络各类Deny／Ask的完整实际场景，也不证明MCP消息拦截、进程终止、容器精确绑定、正式中间层、进程树或日志落盘；未实现项继续按所属模块存档，没有兼容／假成功入口。

旧as_wait.c／as_wait.h和旧启动接口已删除，纯codec迁入共享契约；未被任何当前构建／执行消费的重复C诊断已删除。主模块10个生产源文件共2555物理行（含DTO、注释和空行），测试另计。全仓本轮统计的源／脚本15664物理行含契约、测试及示例，相对9b144d9929的8163行净增7501；新增驱动功能及验证不能冒充净删行。已确认退役的两份SDK构建缓存实际释放1064579072字节，当前源码、目标、镜像、证据和broker目录均保护。
