# 受控TLS出口

当前实现位于[as-flow](../../../crates/backend/as-flow/src/lib.rs)及[TLS发送者](../../../crates/backend/as-flow/src/tls.rs)，由主模块组合真实入口和凭证端口；实际验证范围以证据页为准。本文独占Flow内部TLS出口、请求状态机与实际写出行为；只消费[公共契约](../../contracts/flow.md)的入口／凭证端口和[配置字段](../configuration/technical.md#flow运行时配置)，不导入驱动、凭证或中间层实现。

## 支持范围与成熟依据

首版范围确定为新建的TCP TLS1.2／TLS1.3连接、HTTP/1.1和HTTP/2、已配置LLM API route。复用rustls的两端TLS／验证／签名句柄、成熟HTTP栈的分帧与流控；参考mitmproxy的HTTP stream状态机。原生导流参考mitmproxy Linux sock_create机制；当前入口采用cgroup connect／sockops采集真实进程实例及socket，按标记由nft packet DNAT导流，接纳后重新核真实socket cookie。宿主和经过核验的默认 Docker 独立网络域共用 IPv4／IPv6 TCP 入口实现。具体来源归驱动适配维护，真实覆盖见缺口清单。

连接范围从驱动适配实际启用入口后开始。目标进程可以先于 AS 运行，随后新建的连接才进入本链路。入口就绪表示来源接纳已开启，完整请求仍需通过 TLS 信任、route 和凭证校验。此时点只限定受控 TLS／幻影 key，不限制进程树在启动时读取已有进程或普通 HIPS 处理后续原操作。

IPv6 扩展仍在 Flow 启动阶段从相同 DNS origin 解析目标集合，同一公共 `SocketAddr` 与 `IngressFacts` 贯通双栈。原 SNI、route、凭证授权及上游证书校验复用同一实现。原生地址绑定和退役见[驱动入口契约](../../drivers/linux/flow-binding.md#ipv4-与-ipv6-共用入口契约)。

Flow 的私有只读计划为 `HashMap<HttpsOrigin, BTreeSet<SocketAddr>>`，每次启动对每个规范 origin 解析一次。提交给入口的是全部地址的去重并集。Flow 自己保留各 origin 的地址归属，用原 IP、端口与实际 SNI 核对接纳连接。上游拨号只使用通过核对的原 TCP 对端，域名保留给 TLS 证书与 HTTP authority 校验。该计划随 Flow 生命周期释放。

既有TLS连接迁移、HTTP3／QUIC、ECH解密、证书pinning绕过及mTLS不纳入首版；选中这些能力的配置明确Unsupported。不开TLS0-RTT、不关闭证书校验、不自动回退直连。客户端必须实际信任AS CA；AS只导出公有CA，用户／部署系统负责目标信任配置，本轮不修改容器。宿主机有证书不等于容器Node已信任。

AS 后启动时，新连接仍需通过上述证书信任与 phantom 授权。定向关闭旧 TCP 只结束原 socket，重建由客户端负责。强断重连未成为当前自动接入功能，实际连接重建、证书失败和原会话结果见[原测试](../../evidence/container-dynamic-reconnect.md)。

这些限制是本版明确边界，不用“可能支持”承诺。被动ObserveTls模式尚未实现，选择该模式明确拒绝；不能把空采集来源替代本场景的phantom注入。

## 唯一接入和启动顺序

本节规定 AS 内部模块的依赖顺序。目标应用的启动时间独立于该顺序。

Main初始化配置／规则并完成真实Native鉴权与采用，在原自保保护下初始化凭证所有者，验证所启用route／绑定引用、CA及上游信任材料，再创建Flow和有界AdmittedConnection队列，最后调用驱动适配的start_tls_ingress启用新连接导流。Flow准备失败不启用导流；适配失败撤销本轮资源并返回失败，不把已选MediateTls换成ObserveTls。

驱动适配拥有导流、接纳端点和原连接绑定，向队列移动实际连接；Flow一旦接纳就独占该连接的I/O和释放。没有真实IngressFacts的连接立即关闭，不开放通用匿名CONNECT代理。固定端点与句柄由所属适配生成，不接受前端指定地址、FD或目标URL来创建代理。

身份指向连接发起的ProcessKey或真实容器实例，不等于每条消息的生成线程。多个进程共享连接时不得进一步宣称精确请求发起者；凭证授权语义限定在这个已经核验的连接主体范围。连接绑定已失效或缺失时拒绝注入，不能从客户端token反推出PID。

## 一次请求的固定处理顺序

1. 从AdmittedConnection取得原主体、通道、原目标和导流计划代。ClientHello的SNI及原端口确定配置 origin，原对端须属于该 origin 的已采用地址集合。核对后使用CertificatePort取得该origin的叶证书签名句柄，完成客户端TLS。
2. 正式HTTP栈解析请求头，按实际authority／Host、method和原始编码path选唯一route；头重复、authority冲突、认证字段冲突或无route均拒绝。origin匹配以URL库的规范化主机／端口为准，禁止字符串前缀域名匹配。
3. 为该请求分配ExchangeKey，沿原HTTP请求对象保管其头与正文；原正文事实在第6步实际发送消费时由同一观察器取得，不在建连前另行读取整份请求。
4. 向已核对的原 TCP 对端建立独立上游连接，按固定 HTTPS origin 正常校验CA链、有效期及主机名，得到VerifiedUpstream。真实认证头仅在验证成功后申请和发送。
5. 从唯一指定AuthSlot取phantom，按公共CredentialPort做请求级主体／route／method／path和token校验。获得AuthHeaderLease后用set语义替换这个认证头，不能同时发送phantom和真实key，不能附加第二份Authorization。
6. HTTP栈发送一次请求；每个HTTP2 stream单独持凭证租约，不使用全局“当前key”。发送端对实际应用／发送阶段记录CredentialUseFact，原请求正文按实际消费流式进入同一观察器／provider解码器，不另采集代理自身连接。观察器只按正式字段读取content type／encoding及配置选择的会话元数据，不采集或保存认证头。
7. 上游响应正文从同一HTTP栈解码，原始可公开JSON／SSE事件产生ProtocolObservation，单条消息投影产生ChatMessage，然后沿既有消息职责送中间层。正文不因前端显示与否暂停；业务关联不参与底层转发决策。

当前写入范围是phantom认证头替换，以及按[公共目标契约](../../contracts/flow.md#固定规则拒绝反馈目标契约)在原工具完成上报后追加规则固定原文的限定Responses反馈。普通响应原文转交；Flow不执行工具或自动重发模型请求。解析失败、截断、消费者拥塞及反馈未修改分别报告，不把HTTP提交称模型已理解；通用聊天编辑未启用。

## HTTP、并发与秘密生命周期

当前每个exchange建立一条独立上游TLS连接，删除连接池及跨origin的HTTP2 coalescing状态；下游HTTP2各stream仍独立授权。没有自动重试模型请求。上游重定向原样返回客户端，不代其跟随或把认证头复制到新origin；后续请求重新选route。

认证头写入路径使用HTTP库的sensitive／never-index语义，不进入HPACK动态表、普通header日志、ProtocolObservation或异常字符串；该路径的真实key由凭证所有者、一次性发送租约及传输库必要副本持有，归还租约不能宣称清除了所有库内部副本。已采集正文和元数据按实际值保存，不因内容与凭证字节相同而脱敏；此处不扩大认证头采集范围。

HTTP1的chunked、Content-Length以及HTTP2的DATA／HPACK由正式协议栈处理，不手工改TLS记录长度。请求正文可以边收边转发，凭证授权只需已完成的头／目标检查，不为聊天采集强制缓存整段回答；单个stream的流控不持全局连接表锁。

管理写、证书签发缓存与连接表只用短临界区；锁内不等待TLS、网络、磁盘或用户。证书签名材料由CertificatePort保管，Flow不持CA私钥文件或重新定位路径。初始CA生成一次持久保存；正常启动不换CA。首版不提供在线CA替换，不能让客户端信任悄然变化。

## 网络等待与显式取消

启动 DNS 查询、客户端 ClientHello／TLS 完成、原 TCP 对端建立及上游 TLS 验证直接等待原异步操作。Flow 不设置网络、握手、反馈事务或排空超时，也不保留额外截止配置／开关／helper。原生网络与模型访问的实际结果、对端关闭或显式生命周期取消决定收尾；经过的时间不产生另一种 AS 裁决。

原 SNI／来源核对、证书校验、VerifiedUpstream 和凭证授权顺序保持。真实连接／TLS 失败仍按原有限错误码返回，不新增重试、匿名转发或关闭验证。连接与交换许可由原 owner 保管至实际完成或显式取消，不因等待时间推定失败。

显式 Stop 封闭新接纳并取消原网络任务，随后等待原 tracker 中的任务及已移交收尾实际退出；原事实消费者持续处理清理所需输入，最后关闭输出。Stop 成功不等于网络自然成功、某个响应已写出或 Agent 已消费，不用活动交换数快照计算“真正已中断数”。

## 失败、撤销和退出

- 路由／来源不符、token无效、撤销或scope不符：请求未发上游，返回本地401／403及有限错误码；不泄漏token、真实key、完整参数，也不伪造模型assistant消息。
- 凭证材料不可用：请求未发上游，返回本地503；上游TLS／连接失败返回502。没有“失败仍放行原认证头”选项。
- 上游请求已部分发送后失败／取消：记录实际Partial／Failed，结束对应stream或连接，不重放，不以另一个JSON成功响应覆盖。
- 撤销与authorize_header共享同一短授权临界点。撤销完成后不产生新租约；此前已授予并开始发送的请求继续按实际结果收尾，不承诺撤回已发送字节。
- 停止先撤销新的导流接纳，保持现有答复和退出所需资源，显式取消已接纳连接并实际等待收尾；不关闭Root broker、不卸载HIPS、不恢复到原TLS连接假装无影响。

本地生成的错误与上游响应分来源记录；只有实际HTTP状态才能上报，200不证明每个凭证或业务动作成功。首次TLS握手失败、pinning拒绝、主体绑定失败都给明确原因，不能显示“已保护／全量聊天正常”。

## 事实与失败边界

协议事实来自同一HTTP正文观察者；协议／聊天内容不做脱敏，跨网络块和SSE增量只保原解析状态，保留实际字节、字段和原生ID。原始单元、结构投影及credential状态共用exchange，底层不建立业务因果关系。

入口关闭只结束接纳并发布service级来源缺口，已有exchange继续按真实结果收尾。来源关闭后不再接纳新连接；显式stop取消并实际等待自身任务和原事实收尾，再发布Stopped。异常丢弃生命周期所有者只发真实取消信号，不宣称完成join。

当前入口要求ClientHello携带实际DNS SNI，与唯一允许origin匹配；IP-literal origin及无SNI握手未实现。配置采用前明确拒绝IP-literal route，不把配置保存／端口监听称作该请求可中介。底层HttpsOrigin值能表示IP，不等于每一种传输入口已支持它。
