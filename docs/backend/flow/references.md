# LLM TLS采集、解析与修改的成熟实现调研

调研日期：2026-11-08。本文只维护来源证据，不反向定义消费者架构或接口；跨模块值以[公共契约](../../contracts/flow.md)为准；全部是源码／规范审查，未在AS中运行TLS探针、中介或真实聊天。MCP调研[暂缓](../../archive/mcp-approval-connections.md)，不继续以其排他闸门为本轮前提。

## 2026-11-08选型结论（历史）

以下保留当时的源码调研取向，不作为当前接线方案或验收前置。现行入口是[MediateTls受控出口](tls-egress.md)，被动TLS采样未实现；第三方构件的要求只约束其自身，不转成AS全局门槛。

聊天解析优先采用受支持TLS运行时的明文采样；真实消息修改采用用户态TLS中介模型，二者显式区分、共用HTTP/provider解析和推送。驱动不自研TLS解密，不导出会话密钥，不通过写目标buffer兑现改写。以下项目证明对应构件存在，不证明AS已经集成或覆盖全部应用。

## eCapture：明文采样参考，不能整包当聊天系统

固定提交[bc89a0ed0ea5da82f430e2e0dc2560ad8089b345](https://github.com/gojue/ecapture/tree/bc89a0ed0ea5da82f430e2e0dc2560ad8089b345)；许可证Apache-2.0，已读LICENSE及以下实际源码：

- [kern/openssl.h](https://github.com/gojue/ecapture/blob/bc89a0ed0ea5da82f430e2e0dc2560ad8089b345/kern/openssl.h)：入口保存参数、返回采样SSL_read／SSL_write明文，并把事件交用户态。它没有在内核执行TLS握手解密，也没有取得任意正文修改权。
- [openssl_probe.go](https://github.com/gojue/ecapture/blob/bc89a0ed0ea5da82f430e2e0dc2560ad8089b345/internal/probe/openssl/openssl_probe.go)：text模式的实际附加清单、库适配与不同输出模式。只参考明文路径，不迁入keylog／pcap导出、Go服务和CLI。
- [http2_response.go](https://github.com/gojue/ecapture/blob/bc89a0ed0ea5da82f430e2e0dc2560ad8089b345/pkg/event_processor/http2_response.go)：复用Go http2.Framer和hpack.Decoder；源码明确提示CONTINUATION不支持及HPACK上下文错误。这是有用的采集显示参考，不能整段复制为“每条LLM消息完整解析”的保证。

入口参数／出口结果分离的方式可复用；AS仍须补齐真实实例代、_ex API的长度语义、嵌套去重、采样丢失与有界分片。不是按一个函数名泛化所有TLS ABI。[OpenSSL读](https://docs.openssl.org/3.0/man3/SSL_read/)和[写](https://docs.openssl.org/3.0/man3/SSL_write/)是结果解释依据。

## mitmproxy：成熟的TLS终止与修改模型

核心固定提交[992052ee92d7ff4d240eafbe1afdf87c94b3f080](https://github.com/mitmproxy/mitmproxy/tree/992052ee92d7ff4d240eafbe1afdf87c94b3f080)，LICENSE为MIT；已读：

- [HTTP层](https://github.com/mitmproxy/mitmproxy/blob/992052ee92d7ff4d240eafbe1afdf87c94b3f080/mitmproxy/proxy/layers/http/__init__.py)：stream独立状态、正文缓冲、request／response hooks及实际发送，说明修改发生在传输所有者内部。
- [HTTP2层](https://github.com/mitmproxy/mitmproxy/blob/992052ee92d7ff4d240eafbe1afdf87c94b3f080/mitmproxy/proxy/layers/http/_http2.py)：使用成熟hyper-h2管理会话，不自行把TLS采样片段当消息。
- [流式修改示例](https://github.com/mitmproxy/mitmproxy/blob/992052ee92d7ff4d240eafbe1afdf87c94b3f080/examples/addons/http-stream-modify.py)：支持stream回调，但明确警告跨块匹配和长度问题。由此要求AS按完整SSE／provider语义改写，不能照搬单chunk replace。
- [tlsconfig.py](https://github.com/mitmproxy/mitmproxy/blob/992052ee92d7ff4d240eafbe1afdf87c94b3f080/mitmproxy/addons/tlsconfig.py)：默认上游证书验证与显式不安全选项分开；AS不引入关闭验证路径。

[Local Capture](https://docs.mitmproxy.org/stable/concepts/modes/#local-capture)提供系统透明导流；[证书文档](https://docs.mitmproxy.org/stable/concepts/certificates/)仍要求客户端信任CA，不能称零信任配置。固定mitmproxy_rs提交cddbee77659bae2e8cc210f73b6c3eaa9c34cafe的[sock_create代码](https://github.com/mitmproxy/mitmproxy_rs/blob/cddbee77659bae2e8cc210f73b6c3eaa9c34cafe/mitmproxy-linux-ebpf/src/main.rs)及[Linux用户态入口](https://github.com/mitmproxy/mitmproxy_rs/blob/cddbee77659bae2e8cc210f73b6c3eaa9c34cafe/src/packet_sources/linux.rs)已读，证明新socket导流构件；不证明已有TLS会话迁移，不照搬独立sudo进程管理。

选用这种架构不等于现在把Python代理整个塞进AS。Rust落地优先研究成熟[rustls](https://docs.rs/rustls/0.23.46/rustls/)、[h2](https://docs.rs/h2/0.4.20/h2/)、[httparse](https://docs.rs/httparse/1.10.1/httparse/)等基础库，现有serde_json／tokio继续复用；本轮未新增Cargo依赖、未编译验证这些库。h2是主动客户端／服务端库，不能直接视为被动HTTP2抓包解析器；被动HPACK库选型仍需单独核实，不为此重写整套TLS／HTTP协议栈。

## 官方LLM SDK：只复用增量和终态语义

[OpenAI Python固定提交a1175a35ba6aa2d6bf98d26088a572ab0c5f4453的SSE解码](https://github.com/openai/openai-python/blob/a1175a35ba6aa2d6bf98d26088a572ab0c5f4453/src/openai/_streaming.py)维护跨块行、多个data字段和事件边界；[Responses聚合器](https://github.com/openai/openai-python/blob/a1175a35ba6aa2d6bf98d26088a572ab0c5f4453/src/openai/lib/streaming/responses/_responses.py)分离delta与最终快照。以[官方Responses事件](https://developers.openai.com/api/reference/resources/responses/streaming-events)和[Chat Completions事件](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events)为schema依据。

[Anthropic Python固定提交4c7b598466b88559583f98216139be8e9b34c1fc的_messages.py](https://github.com/anthropics/anthropic-sdk-python/blob/4c7b598466b88559583f98216139be8e9b34c1fc/src/anthropic/lib/streaming/_messages.py)按index聚合块、拼工具JSON、更新累计usage；[官方规范](https://platform.claude.com/docs/en/build-with-claude/streaming)区分message_stop、ping和流内error。两套SDK LICENSE均已读为Apache-2.0。

不引入SDK发请求的网络客户端、自动重试、凭证配置或模型调用；AS在既有流量上解码。API版本／provider schema属于识别事实，不为AS新增旧接口兼容层。未来直接移植代码须保留相应许可与署名，本轮仅设计参考。

## 不选用的“简化”路径

- [Linux kTLS](https://docs.kernel.org/networking/tls.html)要求应用先完成握手并设置密钥／连接，只处理已配置的数据路径，不是任意HTTPS透明解密器。
- 获取keylog配合抓包可用于其他诊断，但带来秘密提取和落盘面，也不自动解决实时变长修改；不进入本方案。
- bpf_probe_write_user或调试器改buffer不提供TLS／HTTP提交所有权，无法自然处理长度、重试、并发和签名；不作为生产修改路径。
- 仅socket网络密文、只看SNI／域名或SSL_read成功，不证明整段聊天完整；TLS探针也不保证涵盖Rustls、GoTLS、QUIC或静态裁剪库。
- [HTTP/2标准](https://httpwg.org/specs/rfc9113.html)要求连接级压缩状态与stream语义，[SSE标准](https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation)要求按事件边界解析；样例字符串截取不能替代这两层。

## 请求级凭证注入的实际代码

[Secretless Broker固定提交23b436dc098a4ba2641882cdcad3879dbce54d10](https://github.com/cyberark/secretless-broker/tree/23b436dc098a4ba2641882cdcad3879dbce54d10)的[generic connector](https://github.com/cyberark/secretless-broker/blob/23b436dc098a4ba2641882cdcad3879dbce54d10/internal/plugin/connectors/http/generic/connector.go)接收HTTP请求及凭证，由Connect对每个请求设置认证头；[config.go](https://github.com/cyberark/secretless-broker/blob/23b436dc098a4ba2641882cdcad3879dbce54d10/internal/plugin/connectors/http/generic/config.go)默认强制TLS。其[CHANGELOG](https://github.com/cyberark/secretless-broker/blob/23b436dc098a4ba2641882cdcad3879dbce54d10/CHANGELOG.md)记录未匹配URL直通导致开放代理、后改为拒绝。AS据此只采纳请求级注入和明确目标范围，不复制任意模板／query注入、宽松正则或不匹配直通。

[Envoy固定提交450984ae504349a188da091a2d7513f451b90c9e的filter](https://github.com/envoyproxy/envoy/blob/450984ae504349a188da091a2d7513f451b90c9e/source/extensions/filters/http/credential_injector/credential_injector_filter.cc)分离取凭证与decodeHeaders；[generic实现](https://github.com/envoyproxy/envoy/blob/450984ae504349a188da091a2d7513f451b90c9e/source/extensions/http/injected_credentials/generic/generic_impl.cc)用header set替换而非追加。[官方说明](https://www.envoyproxy.io/docs/envoy/latest/api-v3/extensions/filters/http/credential_injector/v3/credential_injector.proto)明确该扩展尚缺充分生产验证，因此这里只引用接口模式，不把它称为已证明安全的现成AS执行栈。AS不采用allow_request_without_credential，也不静默trim导入的真实key。

[rustls签名句柄](https://docs.rs/rustls/latest/rustls/sign/trait.SigningKey.html)与[CertifiedKey](https://docs.rs/rustls/latest/rustls/sign/struct.CertifiedKey.html)支持由TLS使用证书链及抽象签名key；[rcgen](https://docs.rs/rcgen/latest/rcgen/)提供标准证书生成；[secrecy](https://docs.rs/secrecy/latest/secrecy/)提供显式秘密访问、Debug／序列化约束及释放清理。它们分别支撑证书和私有租约边界，不是跨模块全能凭证框架。本轮没有添加依赖或运行这些库。

额外已读OpenClaw的[provider-transport-secret-egress.ts](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/agents/provider-transport-secret-egress.ts)：它能将应用内部sentinel在URL／headers替换并拒绝未注册sentinel。此代码位于目标应用内部，只证明替身到真实值的出口模式；AS的方案不依赖修改或导入OpenClaw实现，也不照搬URL秘密替换。

工具调用解析、exec参数到argv、现有生命周期事件及AS实际字段缺口另有固定源码证据；这些是数据核对依据，不能把命令相同当成唯一因果关联。

## 证据范围

实际下载源码、固定提交和SHA256索引位于/tmp/as-llm-tls-research/sources.json；仓库树查询未截断，仅上述文件内容被审阅。库文档／官方规范的浏览结果与源码证据分开。未运行下载代码、未装依赖、未加载探针／中介、未读取目标聊天或秘密配置；真实能力待对应模块实现和有目的的冒烟验证。

本轮补充源码索引为/tmp/as-tls-egress-design/sources.json。OpenClaw源树查询曾截断，因此只依据实际读取的固定文件；数个猜测旧路径404已改用真实路径，不能计作读取成功。TOMOYO v7.0两文件从Linux官方镜像读取，gup.c从官方GitHub读取；没有以别的版本冒充相同源码。
