# TLS实现与既有归一化契约核对

历史限定：下面错误与修正依据绑定 2026-11-08 及所列源码时点。“当前”链接指向现行唯一契约，不能据此把原去密／Redaction 租约等旧实现恢复为当前要求；本地主动脱敏已经退役，见[现行保真边界](../backend/flow/technical.md#原数据保真)。原错误、反例及运行结果不回写。

日期：2026-11-08。依据为本轮开发前的提交787118361，以及当前[公共契约](../contracts/flow.md)、[协议语义](../backend/flow/protocols.md)、[消息边界](../backend/main/messages/technical.md)和[自保](../drivers/linux/self-protection.md)。本页记录实现错误与修复依据，不重新定义数据结构，也不把实现遗漏改称产品新需求。

## 错误原因

原文已经规定原生ID的路径／类型／值、ToolCall／ToolResult的native_id、缺失与真实空值、TextKind、按provider语义处理增量／终态和有界消息职责。这些定义没有缺失；本轮初版代码没有完整执行它们。

- **按字段名递归，替代按协议路径提取。** 通用JSON递归把工具input中的业务id／call_id也标成provider ID。修复改为按明确profile、方向和协议路径提取，不进入业务参数／schema推定ID；原始可公开报文仍保留。
- **事实提取与聊天投影的字段链断开。** 已提取的tool ID没有传入ChatPart.native_id，投影固定填None。修复复用同一协议事实的实际ID、类型和值及真实JSON Pointer，不能再生成另一份同义ID。
- **默认值掩盖缺失。** partial工具缺name或content时，代码用空字符串或JSON null兜底。修复遵守既有契约：未齐先保留原始记录／partial，不伪造完整ToolCall；真实空名称与明确JSON null仍保留。没有增加一套产品schema。
- **把流式索引规则套给非流式数组。** 非流式tool_calls没有流式index，缺省0使多个工具覆盖同一个part。修复按非流式真实数组位置处理，流式只使用实际index；文本与refusal分别投影为已定义的Plain／Refusal。
- **同一归一化语义没有贯穿全部已支持分支。** TextKind.Refusal初次只在Chat Completions接通，Responses的delta／done／非流式拒绝块仍投影Plain；去密逻辑已有逻辑位置映射，却又维护另一份content白名单，使refusal被遗漏。修复贯穿原有Refusal定义，并以同一明确协议位置映射作为跨片去密入口，删除重复白名单，不增加另一套字段定义。
- **把HTTP库轮询次数当作正文事实边界。** 定长请求发送完最后DATA后，库不一定继续轮询EOF；初版因此漏掉完整请求观察。修复使用Incoming的真实end_stream／长度终态，不补造读取回执或修改目标正文。
- **去密窗口混淆物理JSON位置与协议逻辑位置。** 多个choice轮流出现在每帧数组第0项时，以JSON Pointer存前缀会被另一choice清掉，导致同一个choice的秘密尾片公开。独立审核直接核EvidenceSegment::Bytes的原字节证伪了初版；只搜索事件JSON编码文本无法检查数字数组中的原文。修复按真实provider逻辑item／index维护私有有界去密状态，原始field_path继续保留真实JSON位置，验证直接读取公开Bytes而非重新编码文本。
- **二次协议解析后绕过公有JSON边界。** 工具arguments本身是JSON字符串，内层标准unicode escape在codec二次解码后形成真实秘密。初版只检查外层字符串，随后直接创建PublicJson，违反原文“已去密公有值”的定义。独立反例打印出实际公开arguments中的秘密，修复将已声明参数流归类到同一归一化映射，并在最终公有JSON边界沿同一秘密租约检查已解码键／值；原始证据显式Redacted，不以重编码文本代替原字节。
- **新入口没有贯穿既有管理职责。** 初版敏感管理直接调模块，绕过有限命令接纳、消息线程、排队后核权和实际排空。修复沿Main现有能力执行；真实秘密不加入普通Down／Up序列化。
- **只校验引用形状，漏校验实际所有者。** 配置模块不能知道SecretRef是否真实存在；初版凭证owner也没有核实。修复由凭证模块校验启用绑定的实际秘密／route，Main在保存前调用该窄接口；不存在的秘密不会成为成功采用。
- **忽略跨模块初始化时序。** 原自保要求只有已认证实例能受控写；初版在Native鉴权前打开私有store及写锁文件。修复将凭证初始化放在真实Native Running且自保Enabled之后、业务就绪之前；失败走同一退役路径，未删冷恢复断言或放松自保。
- **把配置模式当来源状态。** 原契约已经要求来源状态与Gap；初版来源队列关闭后仍保留可成功订阅的读端口。修复报告实际入口状态与来源关闭，已接纳exchange按真实结果收尾，不推定全部失败或成功。

这些问题的共同代码原因是：通用JSON辅助逻辑越过provider语义边界；提取、投影和消息消费者没有逐字段贯通；新端口没有复用已有接纳／鉴权／退出职责。修复位置分别归协议模块、凭证所有者和Main组合层，没有由主模块复制协议解析或秘密存储。

## 真正需要补清楚的技术细节

- 活订阅需要总接纳上限，但不能占尽普通有限命令许可而阻止撤权。聊天接纳独立有界，容量由既有queues.messages派生；管理命令和urgent控制保持各自职责，不新增配置门槛／注册框架。
- 文档中的u64字节预算必须与实际byte-permit接口表示范围核对，转换前校验，不能截断为u32。新增64位JSON字段继续使用既定十进制字符串codec。
- 被动采样来源与受控中介入口的状态来自不同实际所有者。中介入口直接读取同一TlsIngressSession，不制造并不存在的探针能力或第二份可写状态。

字段、端口和编码仍只在[公共契约](../contracts/flow.md)及其链接的Rust声明维护；配置、消息、架构文档引用它，不复制后再修改另一套schema。协议未完整时先保留真实原始记录；不能为了投影成功制造值。

## 证据归属

独立审核原件为/tmp/as-tls-independent-review.md及/tmp/as-tls-contract-classification.md。缺失秘密引用的独立consumer反例与修复前后回执保存在/tmp/as-tls-review-missing-secret/。实际测试与最终源码／Root回执见[交付证据](tls-egress-integration.md)，失败原件保留，组件测试不替代真实导流或OpenClaw验收。

完整本地验证还暴露了一个测试夹具的跨格式错误：规则测试先转serde_json::Value再写YAML，启用原始JSON数值精度后，Number的JSON私有序列化形式不是规则YAML整数。修复让正例直接由真实PolicyDocument序列化，负例在同一实际YAML条件中加入未知键；规则生产解析和unknown-field拒绝语义未放松。原失败保存在/tmp/as-tls-msrv-workspace-tests.log，定向修复结果在/tmp/as-tls-policy-yaml-fixture.log。

## 集成环境错误与数据契约错误分开记录

隔离VM原包缺标准localhost解析和RTC模块。r8实际测得补hosts前解析失败、补后解析成功，但TLS仍未验证完成；不能据此把r7失败直接说成HTTP502。r9实际CLOCK_REALTIME为Unix5，入口身份／版本／端点均匹配且已接纳，客户端报证书BadEncoding；同一运行没有保存leaf DER，文档不声称已实测其日期。

源码核对显示CA／leaf使用实际UTC减5分钟，rustls-webpki拒绝1970年前的DER时间。当前内核RTC_HCTOSYS已启用，但PL031是模块且原最小包漏载。r10只给同一guest补原发行版rtc-pl031，RTC实际设置2026-11-08 UTC；生产Rust／C未改，原证书验证、HTTP200、SSE、真实key注入及正文去密断言全部通过。该前后结果支持修复正常VM时钟初始化，没有通过关闭密码校验掩盖实现错误。完整交付结果仍见独立验收页。
