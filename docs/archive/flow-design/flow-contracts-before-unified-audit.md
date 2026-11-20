# 归档：统一审计前的Flow合同草案

基线2f0b6b4e2，2026-11-08。保留原设计及未实现来源／编辑／存档，链接仅按归档位置调整；不是当前有效接口。当前见[公共契约](../../contracts/flow.md)。

# 流量公共数据与窄端口

状态：共享契约已落Rust声明；受控中介使用当前实现部分，被动采样和持久存档仍未实现。本文是驱动适配、Flow、Main消息和未来存档共同使用的数据／端口唯一出处，只依赖既有基础值类型，不引用这些消费者的实现、初始化对象或私有状态。模块实现不得反向放进本文的类型。

## 基础类型和数据约束

复用[ProcessInfo](../../../crates/contracts/as-types/src/dispatch_event.rs)、[ProcessKey／EventKey](../../../crates/contracts/as-types/src/process_tree.rs)、[Workload／ContainerInstance](../../../crates/contracts/as-types/src/workload.rs)、[ConfigVersion](../../../crates/contracts/as-types/src/configuration.rs)。这些是已有公共值，不导入配置、进程树或Main的实现。已实现类型位于公共契约层；当前Rust声明以[flow.rs](../../../crates/contracts/as-types/src/flow.rs)为准，实际输出沿同一中间层JSON封套；来源私有句柄不反序列化，未实现的被动采样／存档逻辑不因本契约存在而生效。

- InstanceId为128位来源代；Sequence／Revision为u64，在所属代内递增，溢出不能复用旧键。字段出现的ns均为宿主CLOCK_BOOTTIME纳秒，utc_ms为实际采集时钟映射的Unix毫秒；不能拿provider生成时间代替收到时间。
- BoundedList／BoundedBytes／BoundedText分别为有预算的列表、字节串和UTF-8文本；具体预算由调用者传入已验证值，不藏无限集合。Optional缺失为None，零／空字符串若真实存在则保留。
- PrivateBytes只在底层私有数据面流转，无Debug、普通日志或中间层序列化能力。PublicText／PublicJson为已完成必要去密且有界的值；PublicJson不代替原始JSON证据。
- 所有正文、原生ID、证据片段只向已授权内容读取者输出。API key、认证token、cookie及已知秘密不成为“原始事实”对外公开；删改和缺失必须显式标明。

现有基础类型沿现行codec，不改它们的表示。新增JSON中的64位整数／offset按项目十进制字符串口径传输，避免前端Number精度损失；JsonNumber保存原JSON数值字面量，不能先转浮点再输出。新增InstanceId固定为128位值；实际wire编码落地时与唯一codec同批定义，不用Rust内存布局直接跨进程。

## 来源事实

```text
CaptureId { source_instance: InstanceId, sequence: u64 }
ChannelRef { source_instance: InstanceId, object_id: u64,
             generation: u64, kind: Socket | TlsSession }
ActorFacts {
  process: Option<ProcessKey>, info: ProcessInfo,
  tid: Option<u32>, thread_birth_boottime_ns: Option<u64>,
  cgroup_id: Option<u64>, observed_boottime_ns: u64
}
CaptureIssue = Truncated | Gap | AttachMidStream | OrderUnknown | IdentityUncertain
CaptureSemantics = AttemptSample | CompletedIoSample | PlaintextFunctionSample
CaptureChunk {
  id: CaptureId, actor: ActorFacts, channel: Option<ChannelRef>,
  direction: Read | Write, semantics: CaptureSemantics,
  call_sequence: u64, part_index: u32, part_count: Option<u32>,
  requested_bytes: Option<u64>, completed_bytes: Option<u64>,
  reported_result: Option<i64>, stream_offset: Option<u64>, loss_epoch: u64,
  quality: BoundedList<CaptureIssue>, payload: PrivateBytes
}
TlsProfileId { name: BoundedText }
CaptureCapabilities {
  source_kind: BoundedText, library_builds: BoundedList<BoundedText>,
  abis: BoundedList<TlsProfileId>, max_chunk_bytes: u32, max_call_bytes: u32,
  attach_existing: bool, semantics: BoundedList<CaptureSemantics>
}
SourceStatus {
  source_instance: InstanceId, revision: u64,
  state: Starting | Ready | Unavailable | Stopped,
  capabilities: CaptureCapabilities, reason: Option<PublicText>,
  lost_samples: u64, lost_bytes: Option<u64>
}
CaptureEvent = Data(CaptureChunk) | Status(SourceStatus)
```

ActorFacts只含核实过的原发起者事实；缺失不能改填中介PID或把未知workload当Host。ProcessInfo统一承载已获得的命名空间、保存／FS凭据和其他共同属性，不再另定义同义workload／netns字段。cgroup数值／namespace inode是观察事实，单独不提供永久容器身份；ContainerInstance只能由真实实例提供者绑定。Flow不解析Docker目录名来补它。

ChannelRef绑定真实对象生存期；裸FD、SSL指针或网络四元组不能替代它。call_sequence在同一来源代内定位真实调用，part_index从0起；完整性必须同时满足分片齐全及完成字节范围。失败／缺口不能用后续一个成功样本抹平。source sequence和到达时间不代表业务因果。

CaptureCapabilities仅描述采集能力；不含HTTP解析或修改开关。采样来源只读，不能产生OriginalOperation或未提交正文所有权。丢失计数必须在正文拥塞时仍保留；具体hook、采样和ABI解释由来源实现负责。

## 原始协议观察

```text
FlowMode = ObserveTls | MediateTls
ProviderProfile = OpenAiResponses | OpenAiChatCompletions | AnthropicMessages
ExchangeKey { flow_instance: InstanceId, sequence: u64 }
ProtocolRecordId { exchange: ExchangeKey, sequence: u64 }
CredentialUseId { exchange: ExchangeKey, sequence: u64 }
ProtocolSide = ClientRequest | UpstreamRequest | UpstreamResponse | ClientResponse
ProtocolSource {
  actor: ActorFacts, channel: Option<ChannelRef>, side: ProtocolSide,
  profile: ProviderProfile, origin: PublicText, route_label: BoundedText,
  config_version: ConfigVersion, observed_at_utc_ms: Option<i64>,
  credential_use: Option<CredentialUseId>
}
EvidenceSegment = Bytes { offset: u64, data: BoundedBytes }
                | Redacted { offset: u64, length: u64, reason: PublicText }
                | Missing { offset: u64, length: Option<u64>, reason: PublicText }
EvidenceBody { original_length: Option<u64>, segments: BoundedList<EvidenceSegment> }
NativeProtocolId {
  namespace: Request | Response | Message | Conversation | PreviousResponse
           | ToolCall | ToolResultReference | ProviderEvent,
  field_path: BoundedText, value: JsonString(BoundedText) | JsonNumber(BoundedText)
}
CaptureSlice { id: CaptureId, offset: u32, length: u32 }
ProtocolObservation {
  id: ProtocolRecordId, source: ProtocolSource,
  unit: HttpJsonBody | SseEvent, event_name: Option<BoundedText>,
  capture_refs: BoundedList<CaptureSlice>, body: EvidenceBody,
  native_ids: BoundedList<NativeProtocolId>, quality: BoundedList<CaptureIssue>
}
```

一条记录对应完整或明确不完整的HTTP JSON正文／SSE事件，不对应任意TLS分片。offset按解压后的该原始协议单元字节域计，Bytes保持对应原字节，不重新JSON格式化；Redacted／Missing显式保留原位置。片段不能重叠，无法定位缺口则终止可重组区间并报告质量，不能补字节。传输头仅按明确安全字段提取，不整包上报认证信息。

这里的“原始”是TLS明文经HTTP content decoding后的协议单元，不冒称原始TCP密文；capture_refs指向参与解码的实际采集区间，不承诺压缩前后逐字节一一映射。引用超过预算必须显式标记截断，不编造完整出处。

原生ID保留实际字段路径、类型和值，不将MCP id、provider call_id、DecisionId互填；跨记录比较必须包含实际来源／端点／账户隔离上下文。敏感账户标识保留在底层授权范围内，不使用API key作为公开关联键。无法确认同一账户／会话时不得由底层合并。中介来源没有BPF样本时capture_refs为空，其真实会话归属仍由source表达，不伪造CaptureId。

LLM的tool call表示模型提出调用；后续tool result表示应用向模型报告结果；二者都不是某个syscall真实执行的证明。底层只提取这些原生字段和现有EventKey指向的操作事实，不输出推定的工具执行因果边。会话拼接、工具与系统操作关联、归因置信度由中间层负责。

```text
CredentialUseFact {
  id: CredentialUseId, revision: u64, actor: ActorFacts,
  binding_ref: Option<InstanceId>, destination_origin: PublicText,
  state: ScopeRejected | Prepared | AppliedToRequest | Submitted | Failed,
  auth_field: Option<BoundedText>, reason: Option<PublicText>, observed_boottime_ns: u64
}
```

此结构只记录实际凭证执行者的阶段与结果，未执行的阶段不生成。binding_ref是不含秘密的配置绑定标识，不是phantom token或真实key。AppliedToRequest表示认证字段已在私有请求中替换；Submitted表示该请求交发送栈，不代表provider认证成功。实际HTTP响应单独记录；被动探针不能猜出这些状态。底层按真实注入的use id绑定同一请求，不依赖中间层返回关联结论。

## 聊天投影与真实修改回执

```text
ChatMessageId { exchange: ExchangeKey, sequence: u64 }
PartId = u32
ChatRole = System | Developer | User | Assistant | Tool | Other(BoundedText)
ChatContent = Text(PublicText) | Json(PublicJson)
ChatPart = Text { id: PartId, kind: Plain | Refusal | VisibleReasoning, text: PublicText }
         | ToolCall { id: PartId, native_id: Option<NativeProtocolId>, name: BoundedText,
                      arguments: Option<ChatContent> }
         | ToolResult { id: PartId, native_id: Option<NativeProtocolId>, content: ChatContent }
         | MediaRef { id: PartId, kind: BoundedText, safe_label: PublicText }
         | Opaque { id: PartId, provider_type: BoundedText, reason: PublicText }
         | Redacted { id: PartId, reason: PublicText }
UsageCounter { name: BoundedText, value: u64 }
ChatEditTarget = RequestBody | ResponseBody | SseEvent
ChatEditReceipt {
  exchange: ExchangeKey,
  state: NotWritable | Prepared | Submitted | Partial | Failed | Cancelled,
  serialized_bytes: Option<u64>, accepted_bytes: Option<u64>, reason: Option<PublicText>
}
ChatMessage {
  id: ChatMessageId, revision: u64, source: ProtocolSource,
  record_refs: BoundedList<ProtocolRecordId>,
  origin: RequestContext | ResponseOutput, role: ChatRole,
  candidate_index: Option<u32>, model: Option<BoundedText>,
  observed_parts: BoundedList<ChatPart>, submitted_parts: Option<BoundedList<ChatPart>>,
  state: Streaming | Complete | Interrupted | Failed,
  quality: BoundedList<CaptureIssue>, provider_status: Option<BoundedText>,
  finish_reason: Option<BoundedText>, usage: Option<BoundedList<UsageCounter>>,
  edit: Option<ChatEditReceipt>, first_seen_boottime_ns: u64, last_seen_boottime_ns: u64
}
```

ChatMessage是同一真实协议记录的展示投影，不嵌入Flow私有HTTP状态、内核句柄或中间层会话图。请求历史明确标RequestContext；同文本重发不按hash合并。工具参数在JSON未完整时arguments=None，原始片段仍在ProtocolObservation，不能把半段当完整对象。未提供字段保持None，累计usage按provider语义保留，不加和或猜值。

修订仅属于这一消息；增量和终态快照不能重复追加全文。Complete表示消息按已知协议结束，不能清除quality或冒称全会话完整。Submitted仅表示中介向自身发送栈交付完预期内容，不等于远端接受／模型理解。幻影凭证替换不是聊天正文修改，不用ChatEditReceipt冒充凭证作用域验证。

## 事件、查询与端口

```text
ChatCursor { service_instance: InstanceId, sequence: u64 }
ChatFilter { processes: Option<BoundedList<ProcessKey>>,
             workloads: Option<BoundedList<Workload>>, exchanges: Option<BoundedList<ExchangeKey>> }
ObservationGap { kind: Capture | Delivery | Retention, exchange: Option<ExchangeKey>,
                 cursor_range: Option<(ChatCursor, ChatCursor)>, reason: PublicText }
ArchiveState = NotConfigured | Ready | Unavailable(PublicText)
FlowStatus { mode: FlowMode, sources: BoundedList<SourceStatus>,
             decoder_profiles: BoundedList<ProviderProfile>,
             edit_capability: Unavailable | ReadOnly | Supported(BoundedList<ChatEditTarget>),
             archive: ArchiveState, retained_from: Option<ChatCursor>,
             active_exchanges: u64, lost_samples: u64, lost_deliveries: u64, parse_failures: u64 }
ChatDelta = AppendText { part_id: PartId, text: PublicText } | PutPart(ChatPart)
ChatEvent = Observation(ProtocolObservation) | CredentialUse(CredentialUseFact) | Started(ChatMessage)
          | Delta { id: ChatMessageId, base_revision: u64, revision: u64,
                    changes: BoundedList<ChatDelta> }
          | Finished(ChatMessage)
          | ExchangeFailed { exchange: ExchangeKey, http_status: Option<u16>, reason: PublicText }
          | Gap(ObservationGap) | Status(FlowStatus)
ChatEnvelope { cursor: ChatCursor, event: ChatEvent }
ChatSubscription { retained_from: Option<ChatCursor>, events: Receiver<ChatEnvelope> }
ChatQuery { filter: ChatFilter, from_utc_ms: Option<i64>, to_utc_ms: Option<i64> }
ArchiveCursor { store_instance: InstanceId, token: BoundedBytes }
ArchiveEntry = Observation(ProtocolObservation) | Message(ChatMessage)
             | CredentialUse(CredentialUseFact) | Gap(ObservationGap)
StoredEntry { source_cursor: ChatCursor, entry: ArchiveEntry }
ArchivePage { entries: BoundedList<StoredEntry>, next: Option<ArchiveCursor> }
ArchiveCommit { committed: BoundedList<ChatCursor> }
FlowError = Unavailable | InvalidInput | NotRetained | Busy | Stopped | Failed(PublicText)

FlowReadPort {
  subscribe(filter: ChatFilter, after: Option<ChatCursor>) -> Result<ChatSubscription, FlowError>;
  snapshot(id: ChatMessageId, filter: ChatFilter) -> Result<ChatMessage, FlowError>;
  status() -> FlowStatus;
}
ChatArchivePort {
  store(entries: BoundedList<StoredEntry>) -> async Result<ArchiveCommit, FlowError>;
  query(query: ChatQuery, cursor: Option<ArchiveCursor>, limit: NonZeroU32)
      -> async Result<ArchivePage, FlowError>;
  status() -> ArchiveState;
}
```

Receiver是有界异步接收端，不是回调注册框架。CaptureEvent、ChatEnvelope和StoredEntry分别是私有来源数据、授权上行事实和持久记录，不能互相当作网络发送能力。消息模块先核实际会话授权，再调用FlowReadPort；subscribe、snapshot和archive.query均使用与当前授权范围相交后的ChatFilter，None不跳过授权。查询时间为[from,to)，游标由实际store绑定查询及权限范围，客户端不能靠修改token扩权。

StoredEntry使用Flow分配的原source_cursor去重，不为存档重造来源；只选择协议事实、终态消息、凭证事实和真实缺口，不默认存每个delta。同一批store必须全提交才成功并回同批cursor，失败不得回成功的子集；同一消息／凭证事实的物化视图按自身revision前进。数据库具体事务／索引实现后定，不改变这个回执含义。

Flow实现FlowReadPort，驱动适配不依赖它；ChatArchivePort未来由存储实现，Flow和消息只持同一实例的窄端口，不导入存储实现。读／写端口不含对方模块回调或MainContext，不返回内核指针、socket写权或数据库连接。缺真实存档即NotConfigured／Unavailable，无成功空适配器。真实提交才生成ArchiveCommit；存储时必须携消息依赖的协议记录或明确证据已淘汰，不能把悬空record_refs声称为已存档原文。

## 受控TLS出口与凭证窄端口

受控出口、凭证与入口实际消费以下窄端口，Rust字段以同页链接源码为准。带Opaque／Private／Lease的值仅在底层受信模块间传参，没有中间层序列化、Debug或客户端构造入口；不同值不可互换，普通公开DTO不能制造对应能力。

```text
RouteId = InstanceId
SecretRef = InstanceId
BindingId = InstanceId
PhantomTokenId = InstanceId
CredentialRevision = u64
HttpsOrigin { ascii_host: BoundedText, port: u16 }
AuthSlot = AuthorizationBearer | XApiKey
CredentialSubject = Process(ProcessKey) | Container(ContainerInstance)
IngressId { driver_instance: InstanceId, sequence: u64 }
IngressFacts {
  id: IngressId, connection_initiator: ActorFacts, channel: ChannelRef,
  original_remote_ip: IpAddr, original_remote_port: u16,
  plan_generation: u64, config_version: ConfigVersion
}
AdmittedConnection = opaque owned async byte stream + validated IngressFacts
VerifiedUpstream = opaque established TLS connection proof
                   (origin, peer-validation result, trust revision)
CredentialUseRequest {
  exchange: ExchangeKey, route: RouteId, source: IngressFacts,
  method: BoundedText, encoded_path: BoundedText,
  slot: AuthSlot, phantom: PrivateBytes, peer: VerifiedUpstream
}
AuthHeaderLease = opaque single-use value
                 (CredentialUseId, binding, secret revision, slot, private header, redaction lease)
CaRef = InstanceId
TlsServerIdentity = opaque certificate chain + signing-key handle
CaPublicInfo { id: CaRef, certificate_der: BoundedBytes, fingerprint_sha256: [u8;32] }
CredentialError = InvalidToken | Revoked | ScopeDenied | OriginDenied
                | InvalidHeader | SecretUnavailable | Stopped | InternalFailure
CredentialPort {
  authorize_header(request: CredentialUseRequest) -> Result<AuthHeaderLease, CredentialError>;
}
CertificatePort {
  server_identity(ca: CaRef, origin: HttpsOrigin) -> async Result<TlsServerIdentity, CredentialError>;
  public_ca(ca: CaRef) -> Result<CaPublicInfo, CredentialError>;
}
```

AdmittedConnection只由驱动适配在实际导流和原连接绑定成功后提供，任意本地TCP连接、请求头自报PID或来源IP均不能构造。connection_initiator只声明连接发起实例，不声明每个HTTP请求的当前写线程或工具调用。归属证据失效时不给凭证；容器身份必须是已核ContainerInstance，不能从netns数值猜名称。

VerifiedUpstream由正式TLS传输完成服务端证书／主机名校验后产生，不能由配置true或中间层字段构造；其连接只能供对应已验证origin使用；HTTP请求的route由实际method／path逐请求确定，不能把整条HTTP2连接固定为某个path。CredentialPort只消费这些基础事实，不回调Flow／Main／中间层，也不依赖其业务关联结论。

authorize_header从已加载的不可变凭证快照中完成匹配并产生一次性租约，不在锁内读盘或等待网络。成功产生Prepared事实；真实出口应用认证头后才产生AppliedToRequest，再按实际发送结果产生Submitted／Failed。预期的secret revision和绑定撤销在授权临界点重新检查；授权前已撤销必拒绝，已授予且正在发送的请求不宣称能够追溯撤回。

AuthHeaderLease只允许实际请求发送者使用；不授予解析器、Main、驱动或中间层读真实key的接口。redaction lease保留本次用过的秘密匹配范围到响应处理结束，轮换不会导致旧响应中的秘密被遗漏。它不改变原始事实ID，只为公开EvidenceBody生成Redacted区间。

```text
SecretInput = private bounded bytes
SecretWriteReceipt { secret: SecretRef, revision: CredentialRevision }
PhantomIssue { token_id: PhantomTokenId, binding: BindingId,
               value: PrivateBytes, revision: CredentialRevision }
CredentialAdminPort {
  ensure_ca() -> async Result<CaPublicInfo, CredentialError>;
  import_secret(value: SecretInput) -> async Result<SecretWriteReceipt, CredentialError>;
  replace_secret(secret: SecretRef, expected: CredentialRevision, value: SecretInput)
      -> async Result<SecretWriteReceipt, CredentialError>;
  issue_phantom(binding: BindingId, expected_config: ConfigVersion)
      -> async Result<PhantomIssue, CredentialError>;
  revoke_phantom(token: PhantomTokenId) -> async Result<CredentialRevision, CredentialError>;
}
```

管理端口仅供Main已鉴权且具ManageService的入口调用，导入／返回私有值使用专属敏感载荷路径，不进入ConfigDocument、普通状态／日志或调试输出。配置只保存非秘密引用；实际写盘由凭证模块执行。issue只在创建时返回该phantom值，后续列表不回读；失败／丢失答复不自动循环新建token。PhantomIssue没有真实provider key。

ensure_ca只在全新且没有CA记录的受控存储中创建一次；已有CA返回其公有信息，材料损坏返回失败，不能借此轮换。首版不提供在线CA轮换或自定义关闭上游验证的接口。首次部署先通过此管理动作取得CaRef，再启用引用该CA的MediateTls配置。

这些端口只规定一套当前语义；SecretRef、BindingId、CaRef虽采用同样宽度，逻辑上是不同newtype，不能互相传用。TLS身份、读取端口、credential grant和普通网络Allow也不能互相提升权限。

## 归一化契约在实现中的执行边界

当前Rust声明唯一位于[flow.rs](../../../crates/contracts/as-types/src/flow.rs)，provider解码、原始观察与投影的实现只消费此契约，不在本文定义。此处不建立第二份字段表。

原生ID只来自指定profile和方向的明确协议路径；工具input、arguments、schema中的同名业务字段不成为NativeProtocolId。ChatPart引用同一已取得ID的路径／类型／值；局部PartId、数组位置和provider index各有各自含义，不能用缺省0互相替代。非流式按实际数组项处理，流式沿实际index更新。

缺name或content不生成伪完整工具部分；原始记录／partial继续保留，齐全后才投影。协议中真实空字符串、空数组和明确JSON null原样保留；缺失不转为空值。去密后的占位文本不作为原生ID；原字段本来就等于某个占位文本时，只按实际去密路径判断，不按文本外观删除。

中介来源不伪造被动探针的库／ABI能力。FlowStatus.intake_state只表示实际队列接纳状态Running／SourceClosed／Stopped；同一中介入口的真实generation、accepting、admitted、rejected由Main直接读取TlsIngressSession输出，新增u64仍沿十进制字符串codec。全局来源关闭只报告来源事实，exchange仍依实际结果完成；局部Gap的过滤使用同一原事件携带的真实投递来源。

当前字节许可表示范围为u32，配置和初始化在转换前拒绝越界值，不截断；它是实现宽度约束，不是新的业务规则或测试门槛。数据库、被动TLS探针及未实现协议不因逻辑端口存在而具有运行能力。
