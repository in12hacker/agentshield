# 归档：Flow配置草案

基线2f0b6b4e2，2026-11-08。无消费者的探针／附加字段已退役，不是当前配置；原意图保留供后续模块设计。当前见[配置契约](../../backend/configuration/technical.md#flow运行时配置)。

## Flow运行时配置

当前ConfigDocument接受flow:Option<FlowSettings>与credentials:CredentialSettings；flow:null明确未配置出口。当前唯一类型见[flow.rs](../../../crates/contracts/as-types/src/flow.rs)，未知／旧字段拒绝，不增加接口版本。配置模块统一读取／校验／写入和提供快照，Main只传不可变参数。

计划FlowSettings字段：

本文独占FlowSettings和ProviderRoute等设置结构；FlowMode、ProviderProfile、TlsProfileId及基础数据只引用[公共契约](../../contracts/flow.md)，不能从Flow实现导入。原有ConfigVersion仍为基础内容标识，公共契约不依赖整个ConfigDocument。

```text
FlowScope = AllProtected | Workloads(BoundedList<Workload>) | Processes(BoundedList<ProcessKey>)
ProviderRoute { id: RouteId, origin: BoundedText, method: BoundedText, path: BoundedText, profile: ProviderProfile }
CaptureLimits { max_chunk_bytes: u32, max_call_bytes: u32, max_source_queue_bytes: u64 }
DecodeLimits { max_connection_buffer_bytes: u64, max_message_bytes: u64,
               max_total_buffer_bytes: u64, max_active_exchanges: u32, max_json_depth: u32 }
DeliveryLimits { replay_bytes: u64, replay_retention_ms: u64, subscriber_queue_bytes: u64 }
FlowSettings {
  enabled: bool, scope: FlowScope, mode: FlowMode,
  tls_runtime_profiles: BoundedList<TlsProfileId>, provider_routes: BoundedList<ProviderRoute>,
  capture_content: bool, capture_limits: CaptureLimits, decode_limits: DecodeLimits,
  delivery_limits: DeliveryLimits, attach_deadline_ms: u32,
  egress: Option<TlsEgressSettings>
}
TlsEgressSettings {
  ca: CaRef, connect_timeout_ms: u32, handshake_timeout_ms: u32,
  drain_timeout_ms: u32, max_connections: u32, max_streams_per_connection: u32
}
CredentialBinding {
  id: BindingId, secret: SecretRef, enabled: bool,
  subjects: BoundedList<CredentialSubject>, routes: BoundedList<RouteId>, slot: AuthSlot
}
CredentialSettings { bindings: BoundedList<CredentialBinding> }
```

AllProtected按已有产品语义包含宿主机和容器；它是选择请求，不代表容器实例供给已实现。Workloads只接受真实Workload值，重建实例不可沿用原ContainerInstance；未知归属不猜Host。Main通过实际事实接口把选择转为来源所需的ProcessKey集合，容器选择不能在未核实身份时假采用。

ProviderRoute只按URL解析后的origin、实际method及原始编码path匹配；path不含query、不自行解码后再折叠路径。重复或冲突的同一匹配元组在配置校验时拒绝，不定义另一套优先级。原始请求仍完整留给Flow作协议事实，路由规范化不能改写原文。

- `enabled: bool`、`scope`：启用及受保护对象选择。适配核为真实ProcessKey／来源；尚未实现的容器实例选择不能靠容器名推定支持。
- `mode: observe_tls | mediate_tls`：显式被动观察或用户态TLS中介，默认设计为observe_tls；失败不自动换模式。mode只是选择，不表示能力已就绪。
- `tls_runtime_profiles`：允许使用的受支持库／ABI profile标识。实际ELF路径、build id和附加结果由适配发现／报告，不让前端下发任意探针偏移。
- `provider_routes`：明确的origin、请求method、规范化path与ProviderProfile绑定；仍核实际HTTP／JSON格式。只在此维护协议路由，不成为另一套HIPS放行／阻止规则。
- `capture_content: bool`：是否采集聊天正文。关闭时仅有来源／流量摘要，不声称每条正文仍可见。
- `capture_limits`：max_chunk_bytes、max_call_bytes、max_source_queue_bytes；约束探针采样和数据面。
- `decode_limits`：max_connection_buffer_bytes、max_message_bytes、max_total_buffer_bytes、max_active_exchanges、max_json_depth；SSE事件和解压后正文纳入对应消息及全局预算，解压CPU／倍率使用受限实现预算。
- `delivery_limits`：replay_bytes、replay_retention_ms、subscriber_queue_bytes；明确只是内存回放，不是数据库保留时间。
- `attach_deadline_ms`：本轮来源附加的有限期限，不能作为无限重试的起点。

这些资源字段使用工程默认值并允许维护调整，不要求普通用户逐项填写才能观察；落实现时数值与真实容量校验一起确定，不声称未经测试的吞吐。bytes／ms单位固定，非零／上限／预算包含关系由配置模块校验一次，其他模块使用已验证快照和实际分配结果。

MediateTls必须有egress，ObserveTls禁止携带egress以免字段无效却被接纳。egress.ca只引用凭证模块已有CA；上游验证固定使用正常系统信任和主机名校验，不配置忽略验证开关。各时间／容量有工程默认值和明确单位，不是用户逐条手填的测试门槛。首版mTLS、ECH、QUIC、pinning绕过和在线CA轮换均不支持，采用前明确拒绝，不列“可能支持”。

顶层credentials:CredentialSettings，只保存非秘密BindingId／SecretRef、实际主体和route引用，全部私钥／真实key／phantom值禁止进入ConfigDocument。每个phantom只对应一个BindingId，以token的真实映射选绑定，无允许／拒绝排序歧义；同一个route可服务不同已授权主体的独立绑定。移除绑定或禁用后，新授权立即拒绝。

配置模块校验唯一ID、引用、scope形状和预算，提供同一只读快照；凭证模块只保管传入Arc，不另存一份可写策略。Main按required_effects调用凭证apply_settings和Flow必要重建；未实际采用不得回复Applied。ConfigVersion保留来源，授权检查以当前绑定／秘密修订为准，普通无关配置变化不自动废弃所有TLS连接。

新增固定路径仍由配置独占派生：credential_store_file为data/credentials/store，ca_public_file为data/credentials/ca.crt；均位于同一自保根。凭证模块执行真实私有写入，不由Main、中间层或Flow自行拼根。普通data_directory不是允许外部指定任意秘密写入目标的接口。

MiddlewareAccess具有`chat_content: bool`，控制该已认证peer是否可读取当前采集范围正文；独立于read_only／manage_rules／manage_service，无需每条再次授权。未授权只有摘要；变更后旧订阅撤权，不能让客户端自报true。具体新增字段实施时同批更新结构／文件／校验，不留旧接口别名。

ChatArchive本轮只有未配置端口，不添加假数据库配置。未来固定聊天存档目录由本模块在同一自保根下派生为data/chat并只读导出；数据库文件名／引擎／保留策略待存储实现确定，Flow和Main不自行定位或拼根。当前不创建聊天目录／数据库，不接受中间层写入路径。

资源／mode改变沿现行required_effects报告重建范围；已采集会话持原快照，新会话采用新值。内容读取权限撤销立即作用于订阅，不等待旧会话结束。实际来源Unavailable不会被配置true覆盖，已有HIPS初始化失败停止规则不改变。

受控出口采用当前静态IPv4地址集合，初始化对route解析DNS；无IPv4结果或容器scope在入口采用时明确Unsupported。DNS更新／Flow资源变化需要完整重建。凭证绑定修改独立产生ApplyCredentials，由Main立即调用凭证所有者采用；其他Flow设置变化产生ReinitializeMain，不把保存等同于运行时采用。公有CA与私有store固定路径已由ConfigPaths导出，凭证模块接收同一protected_root以建立并保管目录句柄。
当前受控出口要求DNS SNI，IP-literal origin在配置校验时返回Unsupported；没有隐含直连或禁验选项。原始地址仍由入口事实记录，域名／IP是不同事实字段。
