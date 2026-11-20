# 脚本化中间层技术入口

产品及架构分别见[设计](design.md)、[架构](architecture.md)。业务DTO只引用[消息契约](../../backend/main/messages/technical.md)及[流量公共契约](../../contracts/flow.md)，不在这里复制字段表。

## 宿主和传输

[as-service](../../../crates/middleware/as-middleware/src/bin/as-service.rs)是正式可执行宿主，必须部署在既有固定bin/as-service布局。它初始化正式Main和实际Linux驱动，再从配置的middleware_endpoint绑定Unix socket。没有软件失败回退，无参运行，SIGTERM／SIGINT实际shutdown；仅清理自己绑定的socket inode。

[transport.rs](../../../crates/middleware/as-middleware/src/transport.rs)唯一维护当前version=2 JSONL封套。每帧在message／stream／private／business 四种载荷中恰选一个；message仍为原Down／Up，不维护第二套普通命令。帧长度和队列有界，错误不包含原始私有载荷。

- stream登记当前连接的SQL实时订阅，事件沿原Up::Chat。Subscribe{filter,after:Option<ChatCursor>}仍只支持默认filter，非空范围Unsupported。未提供after时先异步取得实际已提交tail作为Live起点；提供after时在Subscribed前用同一真实cursor索引校验，不存在返回not_found，不猜过期或重置tail。SQL起点准备在原有限pending中，reader不等待它；授权前后复核，准备完成且真实安装订阅后才回复subscription_id。没有retained_from或Flow回放接口。
- business 只复用已文档化的 BusinessQuery／BusinessResult，按原 CommandId 关联，查询前及敏感发送前复核原 ContentAccess；只读已提交业务事实，不接收客户端事实／关系，也不恢复旧 Flow 历史 RPC。
- private在ManageService检查后才解析，调用CredentialAdminPort／CertificatePort。输入无普通Debug／Serialize，输入和专属输出buffer零化；签发值只写原会话，不进入广播／报告。新增u64修订沿既定decimal codec。
- 无CommandId的拒绝、源失效和订阅结束使用独立diagnostic；有id的普通拒绝保留原Up::Reply分类，不让客户端盲等。诊断不代替执行回执。

合法Up::Reply在尚未写出时无法编码或超过现有帧上限，沿diagnostic报告reply_output_unavailable，id仅引用原CommandId，code为output_encoding_failed／output_too_large。它只表明这次结果未交付，不构造CommandResult::Failed或暗示管理动作未生效；客户端应读回真实状态，不自动重试。合法Chat的未写输出拒绝只结束对应原Arc订阅并报告StreamFailed，原cursor不推进、SQL原件保留，不能关问询身份或伪造Flow Gap。部分帧后的实际IO／授权错误仍沿原传输结果处理，不能再插入另一JSON作替代回执。

当前薄宿主只支持 peer UID 等于服务 EUID 的本地部署，socket0600。同一宿主可显式启用回环 WS，首条认证经 Main 建立独立授权。普通 Down／Up 和现有 codec 复用，接线边界见[正式接入](../technical.md#首次鉴权与真实底层授权)。

[Media](../../../crates/middleware/as-middleware/src/media.rs)是正式薄宿主与configuration_smoke共用的消息消费者；不增加消息队列、转发worker或事实缓存。Media 组合原数据库、一个 Unix 管理会话和已鉴权的 WS 会话。两个只读计数分别是未进入Peer队列的控制通知及媒体已确认的业务提交；聊天不走Peer队列，不以not_forwarded=0推定聊天完整。数据库全部写入结果仍由其自己的真实计数与查询认定，不提供外部事实登记接口。

Media 在短锁内登记、移除或快照当前连接。状态获取失败向调用方返回实际错误。close_peer 只释放 Unix 管理会话；WS 会话由登记守卫在传输结束时移除。

传输读入与命令完成在同一公平select推进，socket writer独立推进发送。现有命令回复、必要来源事件队列各16项，后台命令最多16项，不扩大配置额度。writer在完整帧边界先取原单Prompt槽，再处理命令回复、必要结果，最后按需读取一条已提交聊天记录；Ask不经control.reserve排在普通回复后。每条JSON必须完整写完，不能在半帧中插入另一条消息。各接收端按真实EOF排空，取消仍独立唤醒。

普通OperationResult／DriverStatus／来源失效只进入原Peer控制事件队列，由writer消费。Peer.notice／source_unavailable可取消地等待原有界队列，Result成功只证明接纳，不证明socket写出或前端收到；容量满不关闭Peer或撤身份。该控制消费者等待时，独立consume_chat继续提交原聊天，底层安全日志独立推进。真正会话关闭才使等待返回outbound_closed，原source失效与媒体可达性分开，不能承诺无限积压无损。

三个接纳层分别认定：内核Direct的产生／容量丢弃及dropped边界归[驱动原操作](../../drivers/linux/operations.md)；[驱动适配](../../backend/driver-adapter/technical.md)的必要帧无法交付时按其来源失败契约收尾；实际结果／状态经[Main当前唯一通知消费者](../../backend/main/technical.md)等待接纳。Peer只是此后的媒体接纳，等待容量不改变原执行结果或授权。三层不共用一个拒收数或送达结论。

MainThreads.rejected_notices／rejected_logs只读取同一MainIngress的实际累计拒收数，未连接返回None。脚本宿主每秒采样，仅数值变化时输出有限AS_SCRIPTED_DELIVERY_REJECTIONS诊断；这是采样时的Main入口事实，不是Native dropped、媒体not_forwarded或远端送达总账。诊断没有路径、正文或秘密，不改变队列接纳、规则与原操作决定。

规则日志由[底层安全日志](../../backend/security-log/technical.md)消费。脚本验收在服务实际退出后以只读连接核对原命中、事件和结果，`log=false` 仍保留独立完成回执。

命令完成先取得真实发送许可，再从原pending future取回执。即时拒绝／stream回复也等待同一有界control容量，不以Full返回fatal或制造离线，不另建ready输出队列。极端满载时普通RPC和后续读入可能延迟；原Ask期限仍独立推进，不声称该单socket具有硬实时Answer时延，也不因拥塞改为NoInteraction。真正接收端关闭返回outbound_closed；消费来源关闭、实际socket错误和撤权分别认定。

调度器仍只有一个活动原问询。Peer只保管一个尚未交给MainMessages.question的原Prompt；新Prompt只能替换reply已关闭或原deadline已到的旧项，不覆盖仍活动原请求。writer取出时复用question的期限／授权检查；close释放待交付Prompt，原规则超时动作不变。短Mutex只保护这一个交接槽，不跨await。问询、Answer、撤权和停止不等待普通通知队列，也不增加线程或问询Owner。洪水只按正式规则处理，不增加自动允许、预算放大或静默吞错。

Ask在尚未写出任何字节时过期，只释放该帧，并通过Main原问讯接口回收同连接、同DecisionId的已过期项，不关闭Peer或修改交互可用性。已经写出JSONL前缀的帧不能跳过：原问讯仍在原期限结束，只保这一条已开始的写上下文继续完成，实际撤权、I/O失败或停机才终止传输；不延长原操作，不重放答复。单socket部分帧存在真实HOL，不能保证不读socket的前端按时收到Ask；晚到帧不表示原操作仍可决定，原OperationResult和Main的Answer期限校验为准。敏感写的每次实际写出前继续核授权，撤权后不续发余下正文。

所有输出授权检查由FrameGrant同一适配方法保留实际ModuleError，服务诊断固定为`output_authorization_failed grant=… stage=… reason=…`；grant只取public／content／review／admin，stage只取before_chat_read、after_chat_read、before_encode、after_encode、before_write、write_ready、revoked_watch，reason是该次检查实际返回的ModuleError变体。授权失败不再复用输入invalid_frame，也不转成输出过大／编码失败。撤权监听future结束时先按原grant实际复查；复查失败保留同一实际错误与revoked_watch阶段，若仍成功只报`output_authorization_watch_ended grant=… stage=revoked_watch`并停止敏感写出，不能从watch就绪推定Forbidden或具体撤权原因。普通容量压力不制造这些诊断；Unavailable等校验错误也不能被文字改称为已证权限撤销。日志只包含上述有限字段，不含正文、秘密或parser错误文本。

聊天流只读取[业务日志](../business-log/technical.md)定义的原已提交输入，不把原Envelope复制进Peer事件队列。writer跨select最多保管一个原SQL读取future，先注册提交唤醒再查询，避免反复取消观察future后又登记SQL工作；空查询不当EOF。取得的一条原记录保持至本帧处理结束，只有完整写出后才推进当前精确Arc订阅的原cursor；换订阅不得推进新订阅或把旧结果改绑过去。不预取一批正文，不在reader内等待数据库或socket，也不把内容背压转成review离线。

内容在写出任何字节前编码失败或编码后超过MAX_FRAME_BYTES，属于该次输出不能表示，不是用户离线、授权撤销或内核操作失败。SQL聊天只结束精确Arc对应的原订阅，以原subscription_id返回Stream Failed，code分别为output_encoding_failed或output_too_large；原cursor不推进，不截断原记录，不制造Flow Gap，不关闭Peer或改变review_ready。业务上下文查询的同类失败只以原request_id返回BusinessResult::Failed（Unavailable及相同有限原因）；不替换已发生的OperationResult。该分类只覆盖未写出内容，真实授权失败、I/O失败以及已经开始的JSONL帧仍按上述授权／部分帧寿命处理。

Up::Reply也可能携带完整PolicyDocument或按条数限制的ProcessPage，其内容没有8M传输上界。该类未写出失败沿原diagnostic返回type=reply_output_unavailable、原CommandId及同一有限code，只说明原回复未交付；已执行管理命令的真实effects仍未知，客户端应读回实际状态，不将其改成CommandResult::Failed、假定未保存或自动重试。无关联ID的原HIPS通知和Ask不转成可丢遥测，不截断原载荷或生成小型假DTO。

Flow唯一ChatReceiver由同一Media.consume_chat接管，原输入无条件交业务日志，是否有前端不控制采集／入库；接口与生命周期只引用[Main](../../backend/main/technical.md)。普通控制通知不承载聊天中继。SQL索引、九变体重建和提交通知只在[业务日志契约](../business-log/technical.md)定义；写库失败、读取失败和采集失败分别报告，不伪称落盘或复制普通无关联安全日志。

真实Ask在MainMessages.question取得后，writer将同一原值立即交BusinessLog.record_up；其同步Result明确是否接纳，成功只表示已交原TaskTracker，不是Stored。原owner负责实际SQL工作和失败记账；丢弃完成观察future不撤销已接纳工作。writer不等待旧SQL提交、不增加观察队列，也不把旧SQL未完成作为下个Prompt的展示门槛。接纳失败单列有限诊断，原期限不续；只有数据库实际commit后才成立Stored，不替原用户答复或修改规则动作。

## 授权停止与本地回执写出

唯一wire仍为当前version=2原Down／Up，RequestStop和Accepted的数据定义只引用[Main消息](../../backend/main/messages/technical.md)。旧StopMain不保留别名或旧解析。Accepted仅确认这一次真实宿主端口接纳，不能写成Completed、Stopped或Exited；没有真实停止接收者时由Main返回明确错误。

Main submit的runtime CommandReply不序列化成第二DTO。transport在原pending中取得它，按值把原Up和唯一StopReplyWrite移入现有Output；Up可继续作为只读Arc共享，但写出能力与其分开，不能Clone、serde、存入Arc广播或业务数据库。普通回复、Ask、SQL Chat均无该停止写出能力。当前整帧在编码前接过该cap，只有同一JSONL的全部字节（含末尾换行）实际写入成功后才调用written；排队、编码成功、部分写出、EOF、过期、停机及编码失败均不产生该见证，释放cap仅使接收方观察到Unconfirmed。完整本地写入也不证明远端已收到或进程已退出。

写出能力只能伴随该次Accepted／MainStopRequested且没有Ask期限；不合形状的内部输出明确失败并释放cap，不能以另一条完整帧抵作停止回执。能力由Main实际接纳创建，transport没有测试专用构造器或第二信号来源；本地I/O单测仅证明完整帧／EOF边界，实际cap创建和宿主停止另由真实Main组合验证。

宿主在首次 Main 消息组合前绑定真实停止接收端。收到 MainStopRequest 后封管理及撤下原 Ask，在原 session 中继续尝试写出 Accepted。原 Unix 订阅与底层退役并行推进，到 SQLite 最后提交游标后结束。实际退出由外部 wait 核验。详见[正式停机顺序](../technical.md#聊天订阅停机收尾)。

Accepted是有限的公共停止接纳回执，可在原管理会话退休后尝试发送；若已有敏感JSONL半帧因撤权终止，仍保原授权／协议边界，不插入Accepted或把其写出能力伪报成功。原CommandReply／MainStopRequest／StopReplyWrite的唯一声明和创建权限由Main模块维护，transport不新增停止Owner、任务队列或另一个退役执行者。

## Python客户端和场景

[middleware.py](../../../scripts/mvp-smoke/middleware.py)仅用Python标准库，既可import MiddlewareClient，也提供三个入口：

```text
python3 middleware.py --socket /as/run/middleware.sock status
python3 middleware.py --socket /as/run/middleware.sock command down.json
python3 middleware.py run --service /as/bin/as-service --fixtures /suite --output /suite/NEW-REPORT
```

command文件是当前Down对象（含id），不是新的DSL。客户端按真实CommandId关联异步回复，保留插入的Ask／状态／聊天事件；公共Byte数组按实际字节检查已知私有材料，不搜索其JSON数字文本。私有回复只记录id／状态，token值仅返回原调用者。

已识别的Byte数组检查一次原字节后结束，不再递归每个整数；普通字符串只编码一次供同一组匹配。没有已知匹配材料时无需遍历。非字节列表、嵌套对象／键及文本仍按原逻辑检查；这些只清除重复工作，不改变匹配材料、授权或真实字节判定；这类合成材料检查不转为真实原件采集、持久化或正常MVP的额外门槛。

核心场景包括原生启动／状态、树查询、Driver与Backend快规则、记住优先、Ask允许／拒绝／超时／断开、无渠道跳过Ask、临时开关／重载／恢复与自保、进程／文件／网络仅阻止及终止范围、受控TLS／phantom／原始聊天／轮换／撤销、持久授权撤销和实际服务退出。具体已执行case只按report认定，不凭此清单声明全部通过。

探针复用既有subject、rule_kill_probe、self_probe；[mvp_tls_fixture](../../../crates/backend/as-main/examples/mvp_tls_fixture.rs)只提供受控独立PID的真实HTTPS客户端／provider，stdin JSONL仅属探针控制，不替代后端wire。provider响应为测试文本及tool call，不执行工具、不调用真实模型。

## 运行和证据

包装及QEMU命令见[运行说明](../../../scripts/mvp-smoke/README.md)。报告目录必须新建，原report不覆盖；服务和所有子进程都有实际wait，成功后仍需强制清理则不能报PASS。主机内核／镜像／源码hash、普通宿主UID、guest退出、模块卸载、超时和join分别保留；编译／transport单元测试不代替Native保护效果。

中间层业务库和底层安全日志均已有持久化实现，各自按原数据和提交结果验收。真实 OpenClaw 由原指定聊天执行。R54 写打开 Ask 的允许与拒绝已由 R56 通过正式 WS 复验。[问询验收](../../evidence/middleware-websocket.md)

## 脚本化前端接口

调研核对了 Python `websockets` 的同步客户端和仓库锁定的 `tokio-tungstenite`。当前隔离包使用系统 Python 标准库，未安装 Python WS 客户端。采用已锁定的 Tungstenite 0.29 客户端，将静态测试用 `mvp_ws_client` 与 Python 场景一同打包。握手、分帧、Ping／Pong 和 Close 由现有库处理，测试客户端不导入服务端传输实现。[Python 客户端参考](https://websockets.readthedocs.io/en/stable/reference/sync/client.html) · [锁定的 Rust WS 库](https://docs.rs/crate/tokio-tungstenite/0.29.0)

`package.sh --frontend-ws` 打包正式 `as-service`、原生资产、现有 Python 夹具和脚本化前端，沿现有 `run-guest.py` 执行并输出同一报告格式。包内单独记录入口和测试资产哈希。前端只启用本次自有 guest 服务的回环监听，生成本次认证材料。应用帧沿现行 [WS 契约](../technical.md)，请求和回执按原 CommandId，Ask 按原 DecisionId，聊天重连用最后完整接收的 ChatCursor。写请求无自动重放。

测试客户端子进程只接受回环地址和配置 Origin。标准输入的内部控制为 `send` 携带完整原应用信封、`receive` 携带本次等待毫秒数，以及 `close`。标准输出逐行返回原应用帧或 `transport` 状态，Close 保留实际收到的码，EOF 只报告连接结束。认证材料及私有内容经进程管道和原 WS 专属载荷传递，测试报告只记录专属回复的 ID 和状态。原 SQLite 仅用于退役后的只读对账。

管理场景核对规则保存与采用、临时控制、ReadOnly 的读取和写入拒绝。原问询场景统一覆盖File.Open、Process.Execute、Network.Connect和Network.Send各自Allow／Deny；共同核实际等待、原身份／规则／答复／执行结果与退役后SQLite，不再把原生返回类别写死为file_open。复用subject及rule_kill_probe：文件以原打开结果为依据，Execute核目标映像标记，Connect核对端是否实际接纳，Send覆盖sendto、sendmsg、socket write／writev及单消息sendmmsg，核实际TCP字节或UDP数据报；sendmmsg原返回按消息数核，不当字节数。规则按实际资源目录或监听端点生效，不以PID／来源探针名缩窄，也不添加测试Allow。主机普通用户不能读取的内核现场不假称已验证。

聊天场景在断线期间产生新的受控HTTPS交换，经原游标续读，再核对分页、活动证据与实际提交游标。这里的进程／网络是自有guest原生探针，不是stock Agent，不提供真实GLM或工具桥证明；原OpenClaw的三类覆盖单独记录，不能借文件PASS补齐其他类别。场景结果区分已执行通过、实际失败、未执行及未证明，精确执行桥的未提供状态沿原活动契约保留。

运行审计场景沿正式 WS 对照原 Ask 的 ProcessKey 和规则命中，验证进程分页的作用域、显式刷新拒绝、完整安全日志的管理权限及行号分页。探针就绪、WS 完成和安全日志提交是三个独立事实；脚本沿原请求观察指定进程键及原DecisionId入库；夹具等待参数不是产品网络截止，也不把未取得或未执行当作产品失败。退役后将实际 WS 日志页按原行号与 SQLite 逐值比对。[运行审计](../technical.md#运行审计)
