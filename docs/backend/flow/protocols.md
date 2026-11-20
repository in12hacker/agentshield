# LLM协议解析与TLS内容修改

当前中介正文实现见[ProviderCodec](../../../crates/backend/as-flow/src/protocol.rs)、[原始观察与投影](../../../crates/backend/as-flow/src/observe.rs)和[content decoding](../../../crates/backend/as-flow/src/content.rs)；被动HTTP重组尚未实现。公共数据和端口只依赖[公共契约](../../contracts/flow.md)，源码依据在[调研](references.md)。本页只处理LLM HTTP正文语义；MCP审批／pipe回复不推进，phantom认证字段由独立凭证端口与TLS出口执行，不混入正文解析器。

## 分层处理

处理顺序为TLS明文来源→HTTP连接／stream解码→transfer framing及content decoding→JSON／SSE→provider消息→原值ChatEvent。TLS record、SSL函数调用、TCP片段、HTTP DATA和SSE事件是不同边界，不能直接按换行抓JSON。

HTTP/1复用成熟语法解析，按Content-Length／chunked／合法EOF边界处理正文和trailer；一条持久连接含多个请求响应。HTTP/2维护每方向连接级HPACK、SETTINGS及HEADERS／CONTINUATION，按stream id分正文；RST_STREAM与GOAWAY影响范围按标准处理。抓到JSON片段不能证明知道它属于哪条stream。缺少头压缩状态时标不可解码，不能清空HPACK表后假装恢复。

被动HTTP2解码器不得向目标发SETTINGS／WINDOW_UPDATE；成熟主动HTTP2端点库也不能直接当旁观解析器。中介模式由成熟HTTP栈管理流控、两端连接和重编码，不把原来的H2帧头手工改长度后透传。

gzip／br按实际Content-Encoding由现有解码器处理，输出writer检查实际解码字节上限；JSON深度、单协议单元及当前解码状态使用现有容量。不支持或截断的编码保留明确缺口，未实现独立压缩倍率或CPU计时预算。UTF-8及CR／LF可能跨任意输入块，不能先把每个网络块独立转字符串。

## SSE状态

按[WHATWG SSE](https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation)处理空行派发、多个data字段拼接、注释／心跳、event与id；不能只认以data开头的一条网络块。只在完整事件边界解析JSON。SSE传输结束、provider某个内容块done和整次模型响应结束分开。

provider未知事件保留原ProtocolObservation，已知字段按所选profile归一化，未知状态不补造为消息；底层不保留原始历史缓存，已知终点之外不能自行生成完成。id用于该来源协议关联，不等同于AS游标；订阅重连也不触发AS重发目标模型请求。

## Provider解码

### OpenAI Responses

请求的input字符串或input项、显式instructions分别映射真实输入内容；不能把instructions猜成用户消息。previous_response_id或conversation原样提取为NativeProtocolId及字段出处，由中间层建立关系；服务端历史未在当前请求出现则不补齐。响应按response id、output_index／item_id、content_index维护项与内容块。

response.output_text.delta追加对应块；output_text.done／output_item.done提供最终值校验或快照，不再次追加全文。response.completed、response.failed、response.incomplete保留各自终态；response中的多output项、工具调用及可见reasoning summary按实际类型处理，不提取未公开思维。引用[官方事件](https://developers.openai.com/api/reference/resources/responses/streaming-events)与[SDK状态聚合](https://github.com/openai/openai-python/blob/a1175a35ba6aa2d6bf98d26088a572ab0c5f4453/src/openai/lib/streaming/responses/_responses.py)。

### OpenAI Chat Completions

请求messages中的每项保留原role和工具关联。响应按choices[].index分候选；delta可能只含role、文本、拒绝或分段tool_calls，不能要求每帧都有content。tool arguments只有拼成完整JSON才标完整对象。finish_reason按候选保留，SSE的[DONE]是流终点；usage-only块可能没有choices，不能丢弃或伪造新消息。只收到终点但前面有Gap仍是不完整观测。[官方结构](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events)。

### Anthropic Messages

请求system独立于messages；tool_use／tool_result保留原关联，不推定MCP执行。按message_start、content_block_start/delta/stop的index维护块，message_delta更新顶层信息，message_stop结束消息。input_json_delta是片段，不能逐片当完整工具参数；usage按协议累计值覆盖，不把每个累计值相加。

ping不是聊天，流内error即使HTTP状态为200也须报告。只保留API真实提供、已授权的内容；thinking signature／redacted thinking不伪装文本，不修改带签名块。[官方流式规范](https://platform.claude.com/docs/en/build-with-claude/streaming)及[官方SDK聚合](https://github.com/anthropics/anthropic-sdk-python/blob/4c7b598466b88559583f98216139be8e9b34c1fc/src/anthropic/lib/streaming/_messages.py)。

### 共同边界

非流式JSON直接产生实际项的Started／Finished；错误响应产生ExchangeFailed，不伪造assistant答复。重试是新exchange；原生provider ID和所属来源分别上报，中间层按原引用、实际端点／来源及已取得的认证域核验，认证域不冒充provider账户或会话身份；未取得的身份保留缺口，底层不生成猜测的业务关联。只有明确已知的完整请求才标RequestContext完整；不能把重复发送的历史压成刚产生的新对话。

首批profile沿上述已有范围；Gemini、Realtime WebSocket、HTTP3／QUIC及私有网页协议不自动套用OpenAI schema。预留ProviderProfile扩展是明确分支，不是动态注册框架或猜协议的分类器。

## 保留的通用消息修改语义

以下是通用编辑尚未启用时保留的发送所有权约束，不是现行采集或正常业务运行的额外前置。当前实现范围单列于下方固定拒绝反馈。

参考[mitmproxy流式改写示例](https://github.com/mitmproxy/mitmproxy/blob/992052ee92d7ff4d240eafbe1afdf87c94b3f080/examples/addons/http-stream-modify.py)和[实际HTTP发送状态机](https://github.com/mitmproxy/mitmproxy/blob/992052ee92d7ff4d240eafbe1afdf87c94b3f080/mitmproxy/proxy/layers/http/__init__.py)，修改必须处在真实发送之前：

- 请求／非流式响应：完整且受限地保管正文，按对应provider类型校验和修改，重新JSON编码，再由HTTP/TLS栈发送。尚未定义的字段变换不先实现。
- 流式响应：只可修改已保管、尚未发送的完整SSE事件／明确内容块。跨事件文本替换须保留必要尾部或整个块，不能对每个TCP chunk做字符串replace。跨整条回答的规则需整答缓冲，否则能力明确不支持；不默认牺牲实时输出。
- 同一内容在delta、done与最终response快照中重复出现时，必须一致更新；某profile做不到时禁止该profile的流式修改。不能改一帧而把原文在done中再次发出。
- 改写后处理Content-Length、transfer framing、压缩和摘要头，HTTP/2由正式栈重建DATA／HPACK并执行流控；不能保留失效正文签名。带应用签名／不可理解的加密字段不改，不删除校验来凑成功。
- 不改tool id、provider关联id、角色及签名思考块，不自行增加system／developer指令或模型调用。正文修改必须有单独明确授权与策略，普通网络Allow不授予改写权。
- 原始观测内容与拟发送内容在底层分别保留真实投影；固定拒绝反馈的原内容／改写／提交状态仅按[公共目标契约](../../contracts/flow.md#固定规则拒绝反馈目标契约)维护；通用ChatEditReceipt不在当前接口。输出usage仍为服务端报告，不能当成改写文本的实测token数。

中介断开／容量不足／格式错误不允许重放已经部分提交的模型请求；尚未提交时按所选操作已明确的失败动作收尾。通用正文修改首版不启用；已设计的phantom认证失败明确拒绝请求，不把“失败自动放原文”作为行为。被动采集无写权，也不采用bpf_probe_write_user／ptrace修改目标buffer来绕过TLS和HTTP状态。

## TLS范围

当前MediateTls在用户态建立两段TLS并正常校验上游。被动ObserveTls尚未实现；TLS运行时明文采样是研究来源，不是当前生产入口。固定信任、pinning、mTLS、ECH对目标识别及已经建立的TLS会话分别核验；中介不声称无条件支持。内核kTLS只对应用已配置的连接提供对称加解密，不是所有HTTPS的通用解密接口；会话密钥导出不作为本次方案。

## 归一化字段的实现约束

公开ID、缺失／真实空值、part位置及归一化字段语义统一遵循[公共契约的执行边界](../../contracts/flow.md#归一化契约在实现中的执行边界)，本页不复制字段定义。

Flow只用已有provider解析器归一化真实字段，按choice／output／content／block索引维护当前协议状态。普通Text、JsonArguments、元数据、字符串与数字ID保持实际值和类型，JSON escape由既有serde_json解析；不执行关键词匹配、前缀过滤、脱敏或用占位文本污染聚合。原始EvidenceBody保持原字节和范围，不用重新编码JSON替代。实际缺失仍Missing，未知协议内容仍按已有Opaque描述，不补造原文。

SSE使用完整空行边界派发，支持分块CR／LF、多data行及UTF-8 BOM；EOF不派发未完成事件。[DONE]和ping原始单元仍保留，ping不创建assistant消息。gzip使用flate2 MultiGzDecoder，br使用brotli DecompressorWriter，并检查真实结束与有界输出；截断或未知编码发布明确缺口，原传输字节照实际转发，不伪造完整消息。

## 规则固定拒绝反馈的目标字段映射

当前仅实现 Responses 的发送前追加：在 Agent→LLM 的工具执行完成上报中，以已核 call_id 定位原 `input[].function_call_output`，只追加 `output` 文本。必须有明确原工具和实际认证域；协议锚定采用实际 `previous_response_id`，或同请求完整携回原 function_call 及对应结果。已有引用冲突不绕过；conversation 仅核一致性，不能单独定位。不是按字符串 ID 猜关联。文本只从规则原命中取得，公共类型以[Flow契约](../../contracts/flow.md#固定规则拒绝反馈目标契约)为准。

Chat Completions 和 Anthropic 的聊天采集已经实现，固定反馈未实现；本轮不扩展这两个 profile 的正文修改能力。

字符串用原字符串、一个换行与配置原文连接；空原字符串直接使用配置原文。Responses 的合法 input_text 文本块数组追加同类文本块；对象／媒体／未知内容不强转字符串，返回unsupported_body。provider的原工具状态不因AS提示而被伪造为成功或失败。ID来源及字段已经由现有Codec::request／bind文档化，具体跨请求执行桥仍缺失；不能因能解析这些字段就宣称已能注入。

## 本地保真边界

已采集内容不由AS主动脱敏，发布、存储和查询复用同一原结构；HTTP／JSON／SSE本身未取得或未提供的内容仍如实标缺失。历史保守遮蔽导致的R22失败及反例见[证据](../../evidence/flow-redaction-readability.md)，旧脱敏和匹配器方案已按用户要求退役。

应用会话字段由[配置](../configuration/technical.md)显式选择，结构归[Flow 公共契约](../../contracts/flow.md)。正文在原 Serde 单次解析中核查所有对象键唯一；重复叶键或父对象键保留原 body、上报 Gap，不择后值生成原生 ID、Session 或聊天投影。严格解析仍沿锁定 serde_json 的任意精度数字实现，保留数字词法，不另建解析预算或正文缓存。
