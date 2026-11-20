# 受控TLS与幻影key验证记录

版本边界：本页各节只证明其所列 R11／R51 等冻结源码、产物与环境；初始去密租约、未实现清单和后续追加的实际范围分别按各自时点解释，不是最新工作树交付声明。现行网络等待、显式取消与限定反馈以[Flow技术](../backend/flow/technical.md)为准，旧候选通过不覆盖后改源码。

日期：2026-11-08，基线787118361。字段含义只依赖[公共契约](../contracts/flow.md)及同页Rust声明；错误原因见[契约核对](tls-contract-audit.md)。本页独占实际执行范围和证据，不重定义接口。

## 实现范围

新增独立凭证所有者与Flow，复用rustls、hyper、rcgen、成熟JSON和压缩解码库。凭证模块保管私有store、CA、256位随机phantom摘要映射、逐请求作用域／目标授权、轮换和撤销；单次敏感认证头租约与旧秘密去密范围沿同一请求保管。

驱动适配通过cgroup connect／sockops真实进程实例与socket事实、SO_MARK及nft packet DNAT接纳连接；保留原socket目的地供现有HIPS规则使用。Helper与底层用已核进程凭证和拥有的FD通信，重新核SO_COOKIE；没有匿名CONNECT入口、客户端自报PID或/proc五元组猜身份。

同一受控HTTP正文同时产生原始ProtocolObservation与ChatMessage，三个已声明provider profile按各自协议解析JSON／SSE；业务会话／工具因果关联归中间层。Main组合固定配置、实际模块端口、首次peer鉴权、独立内容权限、有界订阅与私有管理；没有第二份聊天库或秘密配置。

## 最终同内核真实联合运行

最终r11使用当前7.0.0-1019-nvidia内核二进制、Native模块和BPF，普通宿主UID1000启动QEMU TCG；guest内实际root加载模块、附着BPF并执行正式Main／Native接口。此次不是原Root broker套件的四退出回执。

- 真正独立PID的TLS客户端只持phantom及公有CA；两端TLS正常验证，上游收到准确真实认证头，实际HTTP200与完整SSE返回。
- 正式驱动入口提供匹配的ProcessKey、源配置和原目标，实际接纳1条连接；同一链路输出11条原始／聊天记录，上游仅收到1次请求。
- 正式已鉴权MainMessages／ChatStream接收实际请求及响应记录，公开观察中无真实key。订阅满额明确Busy，同时管理调用仍能接纳，长订阅不占普通命令额度。
- 原目标Send规则在Driver与Backend Fast分别实际Allow／Deny：Allow收到7字节，Deny无字节，真实rule_id、actor结果及ProcessKey均核对。未靠代理新目标绕过旧网络规则。
- 原Main、进程树字段、进程／网络终止、自保、问询／无前端、配置采用、DriverTools、冷恢复和模块卸载通过，出现AS_MAIN_COLD_RECOVERY_PASS、AS_MAIN_NATIVE_MODULE_UNLOADED、AS_MAIN_NATIVE_GUEST_EXIT=0。
- QEMU退出0、guest退出0、未超时；71.71秒执行并实际join回收。宿主没有加载模块或修改nft／CA信任，原Root代理保留运行。

联合媒体是隔离guest中的软件provider和正式端口组合；不称作真实OpenClaw。该fixture在已启动Main下组合Flow读端口，并未专项执行正式宿主的Flow-enabled启动支路／远程CS监听；其源代码组合已检查，普通真实安装布局软件冒烟另行通过。

## 精确产物

- 镜像/tmp/as-tls-native-main-r11/initramfs.cpio.gz，41,823,261字节，SHA256 ba192eed2e65edbd17f9983c6fa75ad1a0af4fd09412a2ffb1f1ddbf25768ea2。
- 104项源码／43项产物在构建前后、最终运行后及当前源码逐项匹配；清单SHA256 25c7f1594ef3c1143aaf383e95e1c151dc597714e8a4109d94b0506899213af9。
- 原Native模块未改，SHA256 256336a0a038aba71032fdf3025f5936143415c96eab812f8069f4ff8774c2cf；入口BPF SHA256 68f5d5fd138e7db2892b98c3954ffc1c68ae745a911059f59fdb8ebc55f226ab；入口loader SHA256 d4b5d342aa45744ff3062ae6a0bcd719bdaa4203d5c44403c6950189feea1688。
- 当前压缩内核由原代理只读导出，SHA256 ddd5a25c2326fbfdf7e03809c614fafe5a953416bbb1b80f9c2258f7260e44e3；实际QEMU使用同内容解压Image，SHA256 6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6。
- 完整原件/tmp/as-tls-native-main-r11/qemu-full.log、result.json；产物记录/tmp/as-tls-native-main-r11-package-record.json，聚合/tmp/as-tls-final-validation.json。

## 本地检查与交叉审核

先source既有CI环境并检查实际工具。MSRV1.89完整workspace/all-targets、受最终修正影响的Flow回归、Clippy -D warnings、strict rustdoc、fmt、Rust Analyzer及C／BPF clangd通过。真实安装布局软件冒烟输出AS_CONFIGURATION_MAIN_SMOKE_PASS；它只证明软件夹具，不替代上述内核结果。

日志：/tmp/as-tls-delivery-msrv-tests.log、as-tls-delivery-flow-last.log、as-tls-delivery-clippy-last.log、as-tls-delivery-rustdoc.log、as-tls-delivery-fmt-last.log、as-tls-installed-main-smoke.log。最后fixture诊断变更另有目标check／Clippy／LSP和精确原生包验证，生产Flow未随环境修复改变。

独立审核报告/tmp/as-tls-independent-review.md、as-tls-flow-independent-review.md、as-tls-ingress-independent-review.md、as-tls-main-independent-review.md；已证问题均修复，未以新增测试数计算进度。交错choice、Refusal和内层JSON转义的独立失败／修复反例保留，直接核公开Bytes及实际PublicJson，没有只搜索JSON数字数组的错误检验。

## 失败和执行通道记录

入口早期verifier／极简环境失败原件保留；独立入口job c24c88c3f5824caf9c0531041dae9d9a随后实际通过。联合r2的四个Sendcase通过但冷恢复失败，已将私有文件初始化移到真实Native鉴权／采用之后，原断言和自保不放松。

r7原代理job 1b3c6da39ec14aeba5f6dd23471c1d5f实际失败于新pipeline客户端，未进入冷恢复；不能把QEMU退出0说成套件通过。r8真实证明缺hosts后补齐解析仍未完成TLS；r9实际时钟为Unix5、客户端BadEncoding；加入当前发行版rtc-pl031后r10最小链及r11完整链通过。r9未保留叶证书DER，日期原因以实际时钟、证书生成／验证源码和RTC修复后的对照结果说明，不声称DER日期已实测。

大包上传收到公开ProtocolError／EPIPE，代理status仍ready；接口不公开细分原因，不能断言配额耗尽。读取代理私有manifest的诊断被自动审批拒绝且未执行，没有绕过拒绝。随后以获准的小任务c3c59e006e5244f6bb4b6cf6894c3aed只读导出当前非秘密内核，普通UID执行相同QEMU及真实guest Root，保留准确退出与回收结果；没有停止／重启原broker或伪造broker套件回执。

## R11 验证边界

已实现入口为宿主网络命名空间IPv4新TCP与DNS SNI route。独立容器网络／精确ContainerInstance、IPv6、被动TLS、既有连接迁移、mTLS／ECH／QUIC／pinning绕过、在线CA轮换、聊天数据库、正式CS宿主与真实Docker／OpenClaw均未因此完成。HTTP1／HTTP2在本地真实双端TLS测试中执行；本次guest pipeline使用配置profile的软件provider。没有外部真实模型请求或真实provider秘密导入，也没有GitHub Actions／PR运行。

## R51 Flow 在途消息所有权验收

原版 OpenClaw 在 aarch64 7.0.0-1019-nvidia 隔离 guest 内实际执行。一次 Gateway 会话取得三次真实 GLM 流式交换和 385 帧 SSE。原版工具 `read` 的实参及结果与实际文件相符，后续模型请求的工具结果及最终回复与原件一致。13 条消息各一次 Started 和 Finished，64 条 Delta 同 ID 且修订连续。487 条实际投递逐条与 SQLite 原值及提交顺序相符，零采集 Gap。OpenClaw、AS、guest、QEMU 和浏览器实际退出，Native 已卸载。

Gateway／原模型响应中的工具 ID 含下划线，后续 HTTP 请求的工具历史和结果引用使用该请求原有的无下划线值。AS 及 SQLite 保留两处各自的原值。最初要求跨来源 ID 字面相等的离线核验原件保留，按现行作用域只读复核后通过。同一 HTTP 请求内的工具引用得到验证，跨来源 HTTP 到实际工具执行的因果桥仍待真实来源。

冻结候选为 `/tmp/as-flow-owner-candidate.RnnE7hUj/package`，initramfs SHA256 `6420bfd17a55f506e816a0d41d5f09d92deb18ff0ca48f65da14f4b87cad8c5a`。162 项源码、18 项候选文件及 14 项受保护路径权限已核实。原报告 `/tmp/as-openclaw-evidence-20261111/r51/result.json` SHA256 `de30c740bb0379308ba9cdcaecf6dee535a57826033e2b51b68e5e7bfcbec075`，35 项原件和 20 项脚本清单均核验通过，`recheck.sh` 的只读复算逐字节一致。

本地 Rust 1.89 的 Flow／Main／公共类型／中间层 95 项通过，覆盖真实回环 HTTP/1.1 与 HTTP/2 TLS、同源引用共享、超限帧回滚、断流收尾、并发和反馈。严格 Clippy 和格式检查通过，LSP 既有诊断无变化。测试日志 `/tmp/as-flow-owner-tests.ZWPBmQzw.log`。R51 实例未执行断流、并发或宿主安装。
