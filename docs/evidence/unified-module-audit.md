# 代码／文档／模块边界统一审计

日期：2026-11-08。基线2f0b6b4e2。覆盖Main／消息、配置、规则、进程树、驱动适配及内核接口、Flow／凭证和公共类型。按当前真实生产者、消费者及文档唯一出处核对；已明示未实现的容器、IPv6、数据库等不变成本轮额外交付门槛。

本轮确认的问题已有限修复；没有为清理增设总线、服务定位器、Owner服务、线程或另一份可写状态。旧规范草案归档，历史实测原件保留。

## 1. 授权和消息职责

- **配置发布后撤权可被凭证采用失败跳过。** Reload已发布ReadOnly快照，旧连接却仍保ManageService并可改回权限。原因是先adopt_credentials、失败提前返回，未执行adopt_authorization。现所有快照已发布路径都先独立收敛授权，再采用其他模块；失败仍附真实保存／读取facts，不称Applied。真实Owner＋Unix peer软件反例修前旧会话能写，修后Unauthenticated，磁盘权限保持ReadOnly。没有声称突破Native文件自保。
- **普通日志挤掉完成／失效通知。** MainMessage::RuleLog与Completed／DriverUnavailable共用单个mpsc，容量1时日志先占位。现删除旧混合variant，两个成熟有界mpsc分别接纳；同一消息职责同时就绪优先通知，两边都关闭才退出。实际producer保管两发送端至工作结束，外部Ingress仍Weak。只有日志满时rejected_logs增长，不占通知预算；关键通知自身满仍如实计拒绝，不承诺已实现远程可靠传输。

代码：[management](../../crates/backend/as-main/src/messages/management.rs)、[dispatch](../../crates/backend/as-main/src/dispatch.rs)、[共享消息接口](../../crates/contracts/as-types/src/main_ports.rs)。证据/tmp/as-main-auth-reload-audit/result-{host,fixed-host}.log、queue-result-{current,after}.log及/tmp/as-main-queue-final-{unit-host,installed,lsp}.log。

## 2. 原始数据归一化

- **Responses终态被改义。** 原私有Item用字符串state/profile/role与动态JSON重建公开数据，非流式incomplete／failed变complete，status塞入finish_reason，provider_status丢为None；error:null也被当错误。现直接持公共ChatState／ChatRole／ProviderProfile，状态与结束原因分开，nullable error接受，真实provider错误另发ExchangeFailed，不计作JSON语法失败、不造空assistant。
- **原生ID在第二份JSON IR往返。** 已类型化NativeProtocolId先to_value、再from_value。现partial内部只保实际未齐内容，原生ID直接以原类型关联part，删除往返桥接，不重新定义公开schema。
- **凭证修订裸JSON u64。** SecretWriteReceipt.revision未使用既定decimal codec。现大于JavaScript精确整数范围的修订仍按字符串逐位保留，旧裸数字拒绝，不留兼容入口。
- **真实通道种类漏传。** 当前原socket的kind补为Socket；未实现TlsSession不进入当前有效分支。没有为补纸面结构新造IngressId／计数服务，也未删已有provider原生字段来迎合实现。

代码：[protocol](../../crates/backend/as-flow/src/protocol.rs)、[observe](../../crates/backend/as-flow/src/observe.rs)、[公共类型](../../crates/contracts/as-types/src/flow.rs)。原样最小harness及修前后结果/tmp/as-unified-audit-flow-r1/result.log；精度反例/tmp/as-unified-audit-receipt-test.log。原始事实、partial与公有投影的语义仍以[唯一契约](../contracts/flow.md)为准。

## 3. 功能边界及无效抽象

- 删除只作校验、没有运行消费者的capture_limits、tls_runtime_profiles、attach_deadline_ms及CaptureLimits／TlsProfileId；旧键严格拒绝，不为保字段新造采样模块。
- 删除固定传1且无消费者的VerifiedUpstream.trust_revision，以及没有采样来源却固定报0的lost_samples。正常TLS链／主机名验证保留，实际入口与投递／解析状态独立报告。
- Main不再解析route或执行DNS／地址筛选；Flow现有TLS准备接口在spawn前返回运行时及实际IPv4端点，Main只把结果交原DriverSession。没有增加通用网络服务、缓存或新DTO框架。
- DecodeLimits.reservation_bytes为既有解码保留关系的唯一计算，配置校验与真正申请同用；不是一处接纳、另一处使用不同公式。当前8倍计量覆盖并存缓冲额度，不冒称进程RSS硬上限。

代码：[配置校验](../../crates/backend/as-runtime-config/src/flow.rs)、[Main组合](../../crates/backend/as-main/src/flow_runtime.rs)、[TLS准备](../../crates/backend/as-flow/src/tls.rs)。无消费者的旧字段及采样接口原意图保留于[归档配置](../archive/flow-design/configuration-before-unified-audit.md)和[来源草案](../archive/flow-design/tls-capture-interface-draft.md)。

## 4. 生命周期、预算和锁

- **TLS资源早于真实退出报shutdown成功。** 独立Runner不在Shared.work，停信号又晚于work.wait。现同ipc退役锁登记已有work token，跨首await／资产阻塞任务保管并移交Runner至实际wait完成。复用单一admission_stop；首个TLS退出错误在原Ipc聚合，真实Native退出后进入同一次shutdown结果，不混作整个LSM故障。没有新Owner／tracker／watch。
- **缺事实供给的后端容器规则可激活。** ContainerBinding=false时Backend规则也必须在采用前拒绝；禁用group／rule可保留，软件matcher仍能接收明确提供的软件事实，不因此声称Native具备容器实例供给。
- **叶子子树按全系统N预留。** 100无关节点且空余2KiB时，原Subtree要求10624而失败，Children空页却成功。现沿原children索引，仅按访问／排队键增长同一Charge；16子键批次、分页锁外yield，取消释放临时预算。没有新增索引、MVCC、全树快照或后台Owner；保持原实时弱一致语义。
- **静默TTL不执行及回放预算改义。** 原来只publish才过期，且全局replay取min(subscriber)。现同Delivery的expire在发布／订阅／快照／状态读统一执行，连带淘汰派生快照；replay与单订阅预算分别实施，无新定时器／数据库。
- **全局delivery锁内重复JSON／深复制。** 现锁外一次准备封套计费上界，真实cursor在顺序临界区附加；ring／各订阅共享同一Arc，入队只做轻量try_enqueue。保持多producer次序及回放水位原子性，快照拥有副本在锁外复制，不增总线／发布线程。

代码：[adapter](../../crates/backend/as-driver-adapter/src/lib.rs)、[入口](../../crates/backend/as-driver-adapter/src/tls_ingress.rs)、[规则采用](../../crates/backend/as-policy-engine/src/driver_image.rs)、[树查询](../../crates/backend/as-process-tree/src/query.rs)、[Delivery](../../crates/backend/as-flow/src/lib.rs)。原反例/tmp/as-unified-audit-{driver,rule,tree}-repro.log，修后组件和LSP日志同前缀；树leaf／取消及预算释放回归/tmp/as-unified-tree-tests.log。

## 5. 规范收敛

活动产品／架构页不再称当前已实现部分“仅设计”；只画实际入口和所有者。被动CaptureEvent、正文edit和存档伪schema移入归档，当前合同只保真实生产／消费的数据，完整字段链接唯一Rust声明。配置只列有效参数，MainContext只读路径职责未扩展。

修正driver／smoke导航仍否认进程终止、进程树顶部要求无用MainContext的旧口径。宿主普通UID与guest真实root、broker回执分开记录，不把额外Root监督收据设成同QEMU的强制门槛。归档保留历史意图，不作为当前另一套接口。

当前模块间未发现相互导入对方实现／各自私读配置。规则版本租约、树索引、原操作保管是已有真实消费者所需，未因名称像cache／Owner就删除；下层SDK与正式Main guest有不同反例，不凭文件名认定退役。

## 验证和交叉审核

先source本地CI环境，MSRV1.89 workspace/all-targets、strict Clippy、rustdoc、fmt及文档链接／锚点通过；Rust Analyzer验证实际接口／引用。P1和有限P2使用原反例的修前后证据，不按补测试数量计进度。

最终当前整合在普通宿主UID1000、同kernel7.0.0-1019-nvidia QEMU中执行，guest实际root加载原Native和BPF。原Main、双端TLS／phantom／授权原始聊天、四个原目标Send、旧进程网络行为、自保／字段／DriverTools、冷恢复及卸载均通过。QEMU／guest退出0，71.59秒，未超时，实际join回收。没有调用Root代理或操作真实OpenClaw，也不是broker四退出回执。

镜像/tmp/as-unified-audit-native-final/initramfs.cpio.gz，SHA256 dcd7428625f0e265026f2d0dddf0924f8a9aa2d2a1f29eb67c91d4bc8494b4c3。104当前源码／43产物构建前后及运行后匹配，清单SHA256 ea28646aab2ae79dbfcce16fbb19089e3587c2a95cba39815e96a950f9885b17。C／h与原Native模块未改，未更换宿主内核／修改宿主nft或信任库。

原件/tmp/as-unified-audit-native-final/{qemu-full.log,result.json}，聚合/tmp/as-unified-audit-native-final-validation.json；本地/tmp/as-unified-final-{workspace-tests,clippy,rustdoc,fmt}.log。各作者自检和交叉审核分开：Main／配置、Flow／公共契约、driver／规则审计原件分别为/tmp/as-unified-audit-{main,flow,driver}.md，Root树与Main交叉审核/tmp/as-unified-audit-cross-review.md，Flow交叉审核/tmp/as-unified-audit-flow-cross-review.md明确通过。未以原r11旧源码的通过推定覆盖本轮。

明确未来范围仍未实现，见现有存档；本轮不以新增容器、IPv6、数据库、远程CS宿主或更多provider为审计通过门槛。
