# SQLite业务日志接口与表示

## 聊天历史分页

模型 HTTP 聊天使用原 `messages`、`message_revisions`、`deltas` 与 `chat_inputs`。交换页从原消息身份按 ExchangeKey 分组，取首次提交位置排序。模型消息页通过现有 `read_message` 重建当前提交 revision，原始部件及来源保真。直播和续读使用原 ChatCursor 与 `read_next_chat`，按原标签逐条返回实际修订。历史页与直播按原消息 ID 和 revision 对账。同一消息随后提交的用量等新事实进入下一 revision。NativeGateway 的会话／聊天采用本页[来源化读取](#聊天读取类型与分页)，不制造模型交换或实时 Envelope。每页各自使用短事务，读完即释放。[业务契约](../technical.md#llm-聊天工作台)

本页定义中间层存储表示，不复制后端事实语义。原类型继续引用[公共Flow契约](../../contracts/flow.md)、[Main消息](../../backend/main/messages/technical.md)及[中间层引用](../technical.md)。产品与职责见[设计](design.md)、[架构](architecture.md)。

## 唯一入口与回执

原 Ask／完成记录复用 event_headers、file_payloads、rule_hits 和 operation_reports。写打开的 `File.Open`、原 access_mask、EventKey 及 DecisionId 按事件契约保存。写关闭记录由底层安全日志提供。[事件契约](../../contracts/events.md) · [写关闭审计](../../backend/security-log/technical.md#写关闭审计)

`BusinessLog::initialize(&ConfigPaths, Arc<LogStorageBudget>).await`只读取配置已派生的固定数据库位置，创建／打开同一库；生产无Path override。测试可用私有临时路径构造。record_chat消费原Arc<ChatEnvelope>；record_up消费真正收到的Ask／OperationResult；record_completion借用原MainNotice判别内部来源，并接收消息入口唯一投影的Arc<OperationResult>。两个结果入口取得真实OperationResult即保存原EventData和结果，复用write_operation原有幂等与冲突核验；不按Question、关联是否已经提交、交互来源、固定反馈文本或Allowed／Denied筛掉原件。Observed不是执行结果，仍按原类型返回NotApplicable，不制造OperationResult。原结果保存不产生Question／ToolExecution，不把错误码推定为根因或来源失效；噪声分流归正式Driver规则，不在已取得结果的入库入口丢弃。

生产只接纳原RequestHandoff及其候选和CAPI事实交接，不从公开消息pair自动推定当前工具执行。`native_tool_executions(event_ref,delivery,origin_handoff,origin_candidate)`和既有ToolExecutionFacts／NativeToolContext／SourceRecord::ToolExecution读取shape保留给明确受控的软件source验证；其link_capture只在cfg(test)存在，生产无工具执行发布者。record_tool_fixture在测试中显式声明已证执行来源后写origin引用，不以候选唯一作证明。原正文只归native_request_handoffs，没有独立ToolExecution消息、直接writer或重复正文。

`bind_tool_execution(call, event)` 归中间层，输入是其已按权威作用域核实的原引用。同一事务校验候选自身的原生事实、原事件、消息和协议引用，缺事实返回 NotProvided，明确矛盾返回 Conflict 并回滚候选，两者属于来源校验结果。两条各自有效而互相竞争的绑定均保留，活动查询报告冲突。原生响应标识按实际 provider profile 选择原协议命名空间。固定反馈领取在原事务内复核已持久化的原生冲突。Ask 窗口和反馈使用已确认关系。[事实契约](../../contracts/flow.md#模型响应与工具的事实交接)

原事件的执行现场直接复用[唯一字段契约](../../contracts/events.md#原操作执行现场)。业务库按原 event 行结构化保存系统调用字段、六项参数、内核栈和用户栈，u64 使用现有无损整数存储。栈共用一套有序帧存储，分别标明内核与用户来源。None 无子记录，空栈保留采集记录和零条帧。用户栈成功返回 0 和负 errno 分别保留原结果。读取由 EventData 统一校验，不另存 JSON 副本、不解引用地址或推断工具关系。原 Ask 与完成及安全日志沿同一原事件保真。

Network 的可选 DNS 发送快照直接复用事件契约的 `DnsSendSnapshot`。`network_dns_snapshots` 以原 `network_payloads` 行为唯一外键，在同一事件事务内保存 requested_bytes（U64）、attempted_bytes／send_flags（INTEGER）、原 data（BLOB）与一次解析状态及各状态实际字段。无快照无子行；已取得零字节保留空 BLOB，analysis=None 保留 NULL，不与 not_dns／short_read 混淆。域名仍只在原 network target_domain 一处保存，不另存域名映射或消息正文 JSON。读取重建同一类型并由现行 EventData 契约核验，错误或部分读取仍保存原前缀；这些事实不证明后续数据包内容或网站连接归属。

File 的可选 `mount_effect` 同样直接复用事件契约枚举，在 `file_mount_effects` 中以原文件载荷行为唯一外键保存。None 无子行；`unknown` 是已取得但无法确认影响边界的原分类，不能改为 None 或 independent_view。业务库只保存和读取原值，不在查询时用路径、当前 namespace 或进程树重算挂载影响，也不执行自保规则。

`native_request_returns` 保存[原生系统调用请求返回](../../contracts/events.md#原生系统调用请求返回)。原 `EventKey` 使用既有事实索引，操作类别、等待身份、返回时间和返回值按列存储。`ActivityContext` 从同一事务读取观察和已有原操作回执，独立返回观察记录。观察可以先于原回执写入，缺失不改变候选查询或裁决。

`native_futex_selections` 按列保存 [private futex 原选择](../../contracts/events.md#private-futex-唤醒选择)，通过现有消息消费者和原 `EventKey` 事实索引进入 `ActivityContext`。实际 waker／waiter 的线程字段照原值返回。未取得配对等待时，两个等待列均为 NULL，查询继续返回候选选择。`native_futex_selections` 随当前业务库初始化创建，供同一库继续接纳观察。

### 原生请求交接原件

公开同对象解释复用原CaptureAnalysis，不另建业务DTO。受控软件验证的原工具值以origin_handoff／origin_candidate引用指定Submission及候选位置；生产尚缺当前执行语义来源，普通公开消息候选不产生该引用。对外查询恢复原值；不保留另一组工具正文列或旧格式读取分支。相异原件保留并在context_gaps报告，不能择一。每次接纳原件先按完整observation核一致；晚到矛盾传播到新旧request和bound_event已关联Event的context_gaps，原事实仍保留。Submission／实际ChildCreated和EventBound按显式原request保存并可读；相异原件和晚到冲突不覆盖，均不生成当前工具执行权威。该引用不是新增工具缓存。原投影读取继续遵守既有内容权限。

`record_request_handoff`沿现Main消息消费者接纳原[`NativeRequestHandoff`](../../../crates/contracts/as-types/src/runtime_observation.rs)，无ToolCall/HTTP/零丢失/完整字段准入条件。已取得的路径原字节、空数组、0、空值与None均保持原值；不据此推定成功或身份。每次实际消息接纳保存一份原类型；Submission的技术投影只在projection BLOB保存，JSON元数据使用私有projection_ref标记，读取时复原同一公共类型。不设observation唯一性门禁或覆盖旧原件。

`native_request_handoffs(id, delivery, observation, request, source, phase, bound_event, fact, actor_tgid, actor_tid, thread_birth, observed_boottime_ns, projection, enclosing_call)`属于同一业务库。`id`是本库原件行号，`delivery`引用实际提交记录；`fact`是除投影BLOB外的原类型正文；projection_ref明确指示有无对应原BLOB，矛盾报告Conflict，不能以null或空数组替代实得投影。`projection`只在Submission保存原字节；`enclosing_call`只索引EventBound实际外层API，不是原请求归属。其余仅为查询索引：observation/request保存各自完整源域身份的规范JSON，request未取得为SQL NULL；source是observation的16字节来源；phase是原枚举标签；bound_event仅在原EventBound阶段保存真实EventKey的既有FactReference编码，其余阶段为SQL NULL。actor_tgid/actor_tid直接索引原actor的u32；thread_birth和observed_boottime_ns直接索引原线程出生及原观察BOOTTIME纳秒，u64使用8字节大端BLOB，0保持原值。索引按observation、request、source/phase、非空bound_event，以及(source,phase,actor_tgid,actor_tid,thread_birth,observed_boottime_ns)建立；数值排序不经过SQLite浮点/CAST，不另存上下文或路径正文。

每条raw可按原observation索引通过native_observation读取；不为每条raw重复写facts/fact_records。只有EventBound原行在既有fact_records登记真实event引用。活动查询按该event取得其明确request key，再从相同SQL读事务读取该寿命已保存的所有前后原件。原Submission的observation就是后续request所显式引用的寿命ID，而Submission自身request仍为None；查询同时按该明确引用取得observation相等的原Submission，不倒填原字段，不按地址或时间补边；无request的EventBound只带自身，不把其他request=NULL记录连入。缺开头、缺工具或阶段不完整不删原件、不拒绝入库，也不把地址或时间相近作为另一request。返回顺序只是入库行号顺序，原观察序号和时间仍在原类型中保持，不由此推定完成。没有EventBound的原件继续保存，不创建虚构EventData/Question/Tool关系；实际EventBound与已保存Main完成按同一EventKey联查，不要求两类原件按先后顺序提交。

补充时序查询先从原event及NativeRequestReturn取实际来源/线程身份和BOOTTIME，复合索引只读返回之后该线程的第一已保留WorkLeave时点及同点全部行，再按request索引每寿命读取一次原件。同次查询解码出的原记录直接移交唯一插入入口，不再次序列化或解析；解码成功后才登记本次查询的已见行，不能因解码失败假称已消费。一批读取当前明示request的目标原Opening，不沿错误阶段或冲突目标递归扩图。查询中保留同observation的所有相异原行并报告缺口；raw行按本库id保持实际入库顺序。native_request_reference及same_thread_return_sequence只在同一次ActivityContext读事务派生，不保存第二关系缓存，不改变Ask或固定反馈。模式唯一含义见[审计契约](../audit/technical.md#先连接已证原件与补充时序)。

原事件启动范围由 `event_boot_scopes(event PRIMARY KEY, boot_id BLOB(16))` 与同一事件事务保存；未取得时没有子行，不回填历史。读取复用 EventData 校验。`ToolAttribution` 与 Ask 的可选候选窗口复用本模块的原消息／进程身份表和同一读事务，不写关系表、不另存候选缓存。按最后观测时间定位响应时使用 `message_revisions` 的时间索引；唯一业务结构及匹配含义见[审计契约](../audit/technical.md#通用操作系统模式)。

`BusinessLog::activity_context(anchor, attribution)` 是正式只读组合接口，返回既有ContextView与独立可选ToolAttribution；与WS业务查询共用。原协议／工具交接和问询完成继续由原活动判定验证，候选不写确认关系。它复用原SQL事务与原消息／事件结构，不建立行为图状态副本。

已接纳写入实际失败时，数据库owner在原SQL任务仍受TaskTracker保管时，登记一次异步`AS_BUSINESS_WRITE_FAILED`诊断，持有原StoreError及其Debug源；实际失败计数、通知退出等待者和返回原错误不等待stdio。诊断沿[公共异步I/O原语](../../backend/main/technical.md#异步诊断输出与控制活性)保管到实际写出或明确错误，不能把登记诊断称为已经写出。Media的`AS_BUSINESS_STORE_FAILED`另标明chat／notices／native_facts消费方向并保留同一原源。对外错误类别、协议及真实失败结果不变；本地错误源不变成业务字段或原始载荷日志。只有泛化错误而没有这些原源的历史原件不能反推容量、锁或约束原因。

每个写入用一个短事务；返回StoredReceipt时事务已commit。接纳、在处理、已提交分别表达；没有将Accepted写成Stored的接口。查询及关闭沿同一Connection串行执行，已启动工作保留原数据直到实际返回，不提供无限内部待写队列。

`BusinessLog::close()` 在原接纳锁内启动唯一退役任务，再返回观察 future。内部接纳状态从允许工作切换为共享关闭观察值。一次关闭结果从等待中变为实际成功或实际错误，供取消、重接和并发的等待者读取。退役任务复用原 TaskTracker 等待全部已接纳 SQL，然后在阻塞池关闭原 SQLite Connection，并发布真实结果。关闭任务不计入自己等待的工作集合。无运行时可启动或失去关闭结果时返回 Unconfirmed。原物理关闭错误保留为错误来源，关闭观测成功只在 SQLite 实际关闭后发布。[锁定 TaskTracker](https://docs.rs/tokio-util/0.7.18/tokio_util/task/task_tracker/struct.TaskTracker.html)

record_up在函数调用时就经原接纳锁和TaskTracker登记SQL工作，同步返回Result：Err是未接纳的Closed／Unconfirmed，Ok内仅用于观察真实结果的owned future不决定SQL生存。真实Ask创建者立即处理接纳结果，不等待其commit才展示或接下个Ask。关闭在同一接纳临界区封口并等待全部已接纳SQL；已开始的校验／事务失败仍沿原write_result记录并唤醒宿主，不伪造Stored。普通record_chat／record_completion继续由各自消费者等待数据库确认，前端Peer接纳及socket写出不参与其完成条件。

## 表结构

所有源ID保持原域。u64列采用固定8字节大端BLOB，覆盖0到u64::MAX且可排序；UUID、digest、原字节用BLOB，其他有符号值用INTEGER。没有将u64转成i64或浮点的路径。

| 表组 | 内容与边界 |
|---|---|
| deliveries | 实际消费的源cursor、事件种类和库内提交次序；不是因果时间 |
| protocol_sources、process_info／argv | profile、真实actor／channel／认证域与原字段；选项和空集合独立表示 |
| protocol_records、protocol_native_ids、evidence_sets／evidence_segments、record_quality | 原协议记录、原生ID的namespace／字段路径／JSON scalar类型及词法值、分段原始字节及缺口 |
| messages、message_revisions、message_record_refs、message_quality、usage | 原消息身份、首次消费次序、全部revision及原来源引用；usage未取得与已取得空列表分开 |
| parts、part_native_ids | 实际part顺序／ID、类型、原text、工具名、参数／结果字段、原生引用；不存整个ChatMessage封套 |
| deltas、delta_changes | 原base／revision和逐项append／put；查询只重建合法base链，缺base明确NotProvided，不造完整消息 |
| events、event_headers、event_cgroup_lineage、event_actor_namespace_tids、process_info、process_payloads／file_payloads／network_payloads、operation_reports、rule_hits | 原操作／命中／提案和实际结果分开。两条原链以事件头为外键按 ordinal 保存。cgroup ID 用原 u64 存储，actor namespace TID 用 INTEGER，沿[原事件契约](../../contracts/events.md)读回。没有原链时无子行。 |
| event_syscalls、event_stacks、event_stack_frames | 系统调用原字段与参数，内核和用户栈各自的采集记录及有序 IP。用户栈的原返回值用 INTEGER 保存。每个事件的每种栈最多一条采集记录。 |
| questions、question_choices、ask_windows／ask_window_items | 实际接纳的 Ask、原 EventKey／RuleHit／choices／timeout／剩余时间及 SQL 知识截止下的窗口引用。完成含 DecisionId 不生成 Ask |

| relations、relation_evidence、context_gaps | 原事实引用、明确边和逐项证据；不存相似度／猜测边 |
| feedback、feedback_bindings | 原拒绝／工具引用、真实反馈阶段及唯一业务claim；无结果claim只叫Unconfirmed，不推定仍在运行，也不重放 |
| credential_facts、source_gaps、flow_status | 已采集的非秘密凭证阶段、源缺口和原运行观察 |

`questions.timeout_type` 与 `questions.timeout_terminate` 保存原 [Ask 默认动作](../../backend/main/messages/technical.md#主模块代码接口)。重读及同 DecisionId 重投都核对该值。当前数据库只接受含这两列的格式，不回填历史原件。

文件原事件的`file_payloads`在原列外增加`rename_flags INTEGER`与`creation_mode INTEGER`，只保存[事件唯一定义](../../contracts/events.md)的原值。实际0为SQL整数0，未取得为NULL；不是从路径、inode或操作结果推导。两列与原事件／命中／结果在同一事务写入、按原FileData读回，不增加第二份JSON字段或flags／mode规则DSL。映射原参数按同一行的 `map_requested_prot`、`map_effective_prot`、`map_flags` 三列保真存储，沿现有 U64 列类型，全部为值或全部为 NULL。截断参数以同一行的 `truncate_size`（U64）和 `truncate_attributes`（INTEGER）保存。打开数据库时校验当前完整列布局；旧布局立即返回Invalid，不等首次读写才失败，不自动迁移或补默认值。

SQL NULL只表示原Option::None；空字符串、空BLOB、0、false保持原值。可选集合有独立presence字段，零子行分别表示已取得[]或未取得None。工具参数／结果的ChatContent::Json可在该字段保存原JSON值，包括null／[]／空字符串；写入与读回共用原内容owner及唯一JSON值解码，不递归创建通用JSON节点表。JSON数字型原ID保存类型标记及已取得的字符串表示，不经SQLite数值转换；数字表示与原文的边界只在[Flow公共契约](../../contracts/flow.md)定义。

原生交接的JSON metadata仍按原typed结构校验，候选的`context.arguments`复用同一ChatContent。原已解析JSON payload按本行原candidate序号移动保管，在typed metadata验证后移回，避免通用Serde再次把用户对象当私有数字／raw载体；不克隆参数、不择取候选、不改原Projection检查或新增存储DTO。None、JSON null、Text与空容器分别按原结构保存，错误如实传播。

原执行结果只引用[原操作契约](../../drivers/linux/operations.md)，不复制或推定错误语义。operation_reports的outcome列保存原变体名；Failed使用outcome="failed"及outcome_native_error INTEGER原负码，其余原变体的该列为NULL。读取按这两列精确还原同一ExecutionOutcome，不用JSON列或第二DTO；actor_error／parent_error仍只属于各自终止信号。已有结构缺少当前列时初始化明确失败，不迁移旧库、不补默认值、不改历史证据库。

## 持久化聊天流的索引与读取契约

record_chat分解保存载荷，并在同一事务显式保存输入事件标签与确切载荷行，供读取端重建原ChatEnvelope；不按消息state猜Started或Finished。

同库chat_inputs索引不保存envelope JSON或第二份正文：

```sql
chat_inputs(
  delivery INTEGER PRIMARY KEY REFERENCES deliveries(id),
  event_kind TEXT NOT NULL,
  payload_row INTEGER NOT NULL CHECK(payload_row > 0)
)
```

delivery是本库原消费提交位置，不是源序号或因果时间。event_kind由原ChatEvent变体显式写入；payload_row是该标签唯一确定的原载荷表主键。原ChatCursor只复用deliveries.cursor_runtime／cursor_sequence，不再复制到索引。未知标签、缺载荷行或错误列类型明确失败，不猜另一张表、不返回空成功。

- observation引用protocol_records，复用原source、evidence、native_ids与quality读写。
- started与finished分别保存标签，均引用确切message_revisions行；复用read_revision，不调用选择latest或应用后续Delta的read_message。写入helper必须返回该次实际revision行，而不是message owner行。
- delta引用deltas及delta_changes，按ordinal读取原AppendText／PutPart，保原message_ref、base_revision与revision；与已有消息组装共用同一changes解码，不另做协议解析。
- credential_use引用credential_facts，复用原actor与类型字段，空值、完整u64和实际空字符串保持。
- rule_feedback引用该次feedback行，复用read_feedback；不使用feedback_bindings.last_fact替代输入的历史阶段。
- exchange_failed引用exchange_failures，gap引用source_gaps，status引用flow_status及按ordinal排列的status_profiles。保原Option、范围与列表，不从当前状态补字段或去重。

九种标签对应八类载荷表。即使原载荷行相同，两个真实输入仍分别保留原cursor和标签，不按正文合并。父行rowid在原insert后立即取得，不能在写子表后用last_insert_rowid猜回父行。

同一原SQL事务写deliveries、载荷、chat_inputs及原关系，tx.commit成功后才唤醒聊天读者。唤醒由实际SQL closure执行，不依赖等待future仍存在；失败不发布“已提交”。只增加原owner上的合并Notify，不携正文或维护另一份tail缓存。没有索引的旧库不能从已丢标签回填；当前结构不匹配明确失败，不修改历史证据库或增加迁移兼容。

读取端口继续只属于BusinessLog，返回原公共类型：

```rust
async fn chat_tail(&self) -> Result<Option<ChatCursor>, StoreError>;
async fn read_next_chat(&self, after: Option<ChatCursor>)
    -> Result<Option<Arc<ChatEnvelope>>, StoreError>;
async fn validate_chat_cursor(&self, cursor: ChatCursor) -> Result<(), StoreError>;
fn chat_changed(&self) -> impl Future<Output = ()> + Send + 'static;
```

chat_tail取chat_inputs最后提交位置的原cursor；空库None。read_next_chat的after=None表示尚无已读记录，从首条已提交输入开始；Some先按原cursor唯一索引定位delivery，再按chat_inputs.delivery递增取一条。validate_chat_cursor复用同一实际lookup、不加载正文；续读订阅成功前先完成它。原cursor不存在返回NotProvided，不能猜成已过期或默默改读tail。当前无下一行返回None，只表示没有新的已提交输入，不表示Flow或数据库EOF。

源cursor的sequence是身份，不能据数值比较推定实际入队／提交顺序；遍历以SQL原消费位置为准。一个查询只重建一条原事件并释放SQL锁，不能拉全量历史或在SQL锁内等待socket。chat_changed在调用时创建owned等待者；消费者先登记变更等待再查询，查空后才等待，避免提交发生在查空与等待之间而失唤醒。关闭唤醒等待者观察真实Closed，不捏造源Gap。

默认Live订阅先在原有限pending中异步取得tail，授权复核后才回复Subscribed；这个已提交边界是其起点，不以墙钟或模型时间划分。writer仅持一个查询future和一条当前帧，跨control选择保管，不反复取消并重建已登记SQL。完整本地写出原帧后才推进cursor；入队不等于写出，更不等于前端已读。连接及Content授权仍在读取前后和实际写出时复核，业务库不保存运行时capability。

## 查询与授权

### 共享容量与滚动保留

业务库与底层安全审计库合计使用[配置的日志总预算](../../backend/configuration/technical.md#2-可写configdocument)，出厂 1GiB。共同协调只比较实际文件占用并选择较大库先淘汰，不保存正文、不建立线程或第二写入队列。SQL 写入、清理和 checkpoint 都由原库所有者执行，且不进入 Ask／驱动裁决调用链。

本库的 `retention_records` 只索引一次原写入单元创建的根行；登记与原记录同事务，不复制封套或正文。淘汰按本库已登记的写入次序，删除完整单元以及依赖它的派生关系／冻结窗口，不留下悬空外键或多态原行引用。共享 source、进程、parts 与 evidence 仅在其他保留记录不再引用后回收。当前进程实际在途的 Ask／反馈原引用不提前淘汰；已物化的一次查询上下文随消费者保管，不成为历史缓存。初始化前已有原行按各原表自身顺序纳入保留索引，不推定跨表事件时间。

物理计量包含主文件及 WAL／SHM／journal，删除后由 [SQLite auto-vacuum](https://sqlite.org/pragma.html#pragma_auto_vacuum) 和原连接 [WAL checkpoint](https://sqlite.org/wal.html#checkpointing) 回收，不以 freelist 或 page_count 冒充文件已缩小。一次真实事务可产生暂态文件增长；提交后的保留量重新收敛。checkpoint Busy、尚在途引用阻止清理或实际文件回收失败只增加独立的 deferred_maintenance 计数，不改已提交 Stored、不映射成 SourceUnavailable；后续写入继续尝试收敛，正式宿主输出容量维护与停机实际文件统计。真正 SQL 未提交的写入仍返回原错误、不假 Stored。单条本身超过预算或所有可淘汰单元仍在途时，保留真实已提交事实并明确超额，不能通过截断正文或清空在途状态伪造满足预算。已淘汰的历史 cursor／原引用返回既有 NotProvided，不顺延到另一条事实。

保留索引以根表的稳定主键登记，Gateway 连接使用原 capture BLOB；不把其隐含 rowid 当跨 VACUUM 身份。删除闭包中的 rowid 只在同一 SQL 事务内临时使用并清空，不持久保存非 INTEGER PRIMARY KEY 表的 rowid。

初始化的幂等回填按每个根表遍历缺漏，以原 `(kind, object)` 唯一索引点查；已有项不重写原 generation。`object BLOB` 无类型亲和，保留实际 INTEGER／BLOB 主键。比较使用 `r.object=+p.<原主键>`；一元 `+` 仅移除表达式亲和、不转换原值，使 SQLite 可同时用 kind 与 object 定位，避免 INTEGER PRIMARY KEY 的亲和传播导致每行只按 kind 扫描。重启仍复用同一回填入口核对根表；仅存在保留索引表不能证明之前的多语句初始化已全部完成，因此不另建完成标记或猜测跳过漏项。这里的遍历不是跨表时间推定或正文缓存。

固定反馈使用本次原 Event／Part 的同一运行时引用 guard；准备调用保管到反馈结果 SQL 提交，同时通过 `RuleFeedbackPrepareInput.retention_guard` 移交同一 Flow Transaction 保管到真实终态。两个必要持有者共享同一个 guard，最后释放才解除保留；不从历史 Prepared 重建 pin。[Flow 生命周期](../../backend/flow/technical.md)

ActivityContext、AskContext只从同库结构化原事实构建一次性ContextView。ContentAccess在读取前及敏感值返回前核同一原会话授权，不从Flow取snapshot／replay。20条窗口限定一个明确exchange，request_context与response_output分别按首次实际消息消费次序排列，取尾部20条；Delta／revision使用同逻辑消息，重复原消息不按内容去重。

同EventKey／message identity的来源冲突保留错误，不覆盖另一原事实。缺执行桥／Agent根绑定／命令字段映射分别返回既有Gap；EqualBytes不是关系。固定反馈准备消费DB中的原值与真实来源，反馈不创建前端会话permit，也不设置执行deadline／Expired状态；消费源的显式取消与Flow真实退役沿原owner收尾。Flow不回查业务库，普通客户端不能登记执行事实或边。

## 依赖与验证依据

采用固定rusqlite 0.39.0与bundled SQLite，禁用默认statement cache，复用其标准事务／WAL／外键实现。系统缺sqlite3.pc不回退另一路数据库；项目以Rust1.89实际编译确定兼容范围。官方机制见[rusqlite](https://github.com/rusqlite/rusqlite/tree/v0.39.0)、[SQLite WAL](https://sqlite.org/wal.html)、[事务](https://sqlite.org/lang_transaction.html)。第三方FFI不代表项目新增unsafe；SQLite机制或编译结果本身也不能证明安全审计账本或正式WS运行效果，现有WS接线仍沿[接入契约](../technical.md)。

应用会话选择及查询契约见 [中间层技术文档](../technical.md#应用会话目录)。
`protocol_session_fields(owner,kind,field,issue)` 和
`protocol_session_values(owner,ordinal,kind,bytes)` 在 write_protocol 的同一事务保存原请求选择及
所有已取得值。Header bytes 不转码，JSON string 以 UTF-8 存实际字符串，JSON number 保留数字词法；
kind 区分三者。没有另存正文，原证据仍由 evidence_sets 持有。没有选择不生成字段行；选择但未获取
保留字段行和 issue、空 values。查询从原 source 域联表，不建独立 session owner、映射缓存或深拷贝聊天。

## Session 与原生聊天业务模型

本节唯一维护 Session／聊天的中间层业务类型和判定。代码职责见[架构](architecture.md#session聊天的纯业务子模块)。Gateway 共用 connection、document、reference 与字段引用直接使用[中间层来源契约](../technical.md#nativegateway-共用来源与解析入口)，不复制为 HTTP source；原 `ChatMessage`、`ChatRole`、`ChatContent`、`NativeIdValue`、`SessionObservation` 和 `ExchangeKey` 仍引用 [Flow](../../contracts/flow.md)。本节的业务投影不下沉驱动或 Flow。

### 对象、身份与范围

- **sessionId**：原应用会话实例 ID，取明确会话对象的 `sessionId` 字符串。真实会话键为 `(GatewaySessionNamespace, sessionId)`，不是 `(capture, sessionId)`。非空原 ID 才能选择真实会话组，空字符串仍作为原观察保存。同一已核 Gateway 发行域明确发布的同一原 sessionId，在重连或多个观察连接中仍属于同一会话，各原件引用独立保留。只有 sessionId 文本相同、只有物理来源相同或只有一个候选均不足以构成该身份。
- **sessionKey／key**：原协议在指定对象上给出的会话引用或别名。分别保留字段路径和原字符串，不解析 `agent:…` 拼写为身份，不改称 sessionId。别名的值与它在某一原对象中指向哪一个实例是两项事实；reset 或复用时不建立全历史可变的 key→ID 映射。
- **runId／lastRunId**：前者是该原执行／公开流的运行引用；后者只表示会话投影声明的最近运行。原字符串不剥复合前后缀，不将父 announce、初始 run 与后续 run 混为一项。作用域为已核 GatewaySessionNamespace 及明确会话／运行交接；同域跨观察连接可机械传递已证明的运行引用，不从相同 run 文本跨发行域连边。
- **agentId**：原协议明确的 Agent 标识，作用域限同一已核 GatewaySessionNamespace。不是 ProcessKey、Agent 进程名或会话 ID；多个会话可以同属一个原 agentId。缺失不妨碍保存会话或聊天，不从 key、任务提示或模型名称填入。
- **RPC id**：公开 `req/res` 封套中的原字符串 ID，只在同一 capture 内配对。保留原值，空值留原件但不作配对键；非字符串原类型不转字符串。它不是 runId、idempotencyKey、消息 ID 或 AS 查询的 CommandId。
- **AS 观察引用／容器与 HTTP**：`ChatSessionRef` 只引用 AS 已保存的原观察，既不签发应用身份，也不使未关联交换成为成员。AS 观察对话容器是[产品定义的后备概念](design.md#应用会话与原生聊天)，不进入本节真实身份字段或归组算法。`ExchangeKey` 只指一次实际模型 HTTP 交换，provider Conversation 和原应用 Session 另有来源。

`GatewaySessionNamespace { server: ProcessKey, server_port: u16 }` 是当前旁路范围内的 Gateway 运行时发行域。server 直接取已核 GatewayConnection.server 的完整进程实例，包含原 boot_id、host_tgid、birth_boottime_ns；server_port 直接取该连接实际服务器端口，范围 1..65535。本类型限当前契约指定的宿主初始网络命名空间、IPv4 回环公开 Gateway 协议端点，不能移用于任意 IP／网络命名空间，也不从缺少地址的原记录补写某个具体 IP。客户端进程、客户端临时端口、socket cookie、capture、采样 source／generation 和观察者人数不属于应用发行域。

该命名空间与该公开协议明确发布的 sessionId 共同构成原身份：各连接先沿原字节／I/O／Upgrade 核实实际发行端及原公开消息，再比较同一完整服务器实例、上述实际端点和原 sessionId。这里不是用裸 PID／端口猜会话，亦不要求额外取得一个与该范围无关的永久发行者字段才能归并重连后的明确身份。不同 Gateway 进程寿命、不同端点或实际公开身份冲突分别保留；若原件明确显示同一进程／端点下存在尚不能区分的多个逻辑发行域，则本类型不足，保留原观察并标 `session_namespace_unavailable`，不把 capture 固化成替代应用命名空间。跨 Gateway 重启的持久身份、跨端点共享发行域或该范围外的多租户发行域，仅在所属公开协议的实际字段能说明时另行明确；缺这些事实不切断当前已核运行时发行域内的完整会话。

ModelHTTP 的字段分组复用原选择：仅接受 `ClientRequest`、完整无 quality 缺口、`issue=None`、恰一个非空标量，并有实际 ProcessKey 与 actual_auth_scope。域为完整进程寿命、origin、profile、route_label、config_version、实际凭证域、字段种类／位置和值种类／原值。重复头、重复 JSON 键、空值、未解析、Missing 原样保存而不确认归组。此分组不自动宣称所选值就是 Gateway 的 sessionId，不从工具输出、模型正文或 schema 同名字段生成当前请求的 Session。

### 纯投影的输入与原字段

`business_log::sessions::project_gateway` 借 `GatewayDocument`，返回会话／聊天的原引用、必要索引及逐项 `SessionGap`，不持有正文、role JSON 或转录位置 JSON 的副本，也不写库。原观察 DTO 只在选定读取项时由私有 `sessions/observations.rs` 借同一树物化；SQL 不解析应用对象或重判成员。需要已保存候选时，输入是同一读写事务定位的原引用和值；绑定函数只输出已证明成员或缺口。原对象字段不从另一对象补造，借请求取得的 agentId 仍引用请求，不填进回执自身的缺失字段。

`GatewaySessionObservation { reference, session_id, session_key, run_id, last_run_id, agent_id, request }`：reference 为指向原会话对象的 `GatewayFieldReference`；五个标识均为 `Option<String>`，分别取该对象的 sessionId、指定 key 字段、runId、lastRunId、agentId，不把 runId 与 lastRunId 归一为同字段。None 表示没有取得支持的字符串值，`Some("")` 保持原空串。request 为 `Option<GatewayReference>`，仅在同连接 RPC 配对成立时指其原请求；事件本身没有请求时为 None。字段路径及类型不符的实际原值仍在原件，通过下述 gap 指出，不以 None 丢掉读取结果。

投影只解释已指定的原公共形状：

- 成功 `sessions.create` 回执的 `/payload` 取 `sessionId/key/runId`；`/payload/entry` 是另一个原对象，只取它自身字段。原请求 `/params` 的 `key/agentId` 独立记录。回执根与 entry 已取得的同义会话字段冲突时保留两份原对象并报告，不选择其一。`parentSessionKey` 只是父引用，不是被创建对象的 key。
- `sessions.describe` 成功回执的 `/payload/session`，以及 `sessions.list` 成功回执的 `/payload/sessions/{index}`，各为独立会话对象；分别取原 `sessionId`、`key`、`lastRunId`、`agentId`。原 `session:null` 表示该回执明确返回 null，不反证更早创建对象不存在。
- `sessions.changed` 支持两个明确公开对象：有对象 `/payload/session` 时取该对象的 `sessionId/key/lastRunId/agentId`；平铺形状取 `/payload` 的 `sessionId/sessionKey/runId/agentId`。两种对象有各自原字段路径，不是旧协议回退；两种同时出现时各自保留并核冲突，不先取一种掩盖另一种。缺字段的部分对象仍保存。
- `chat`／`agent` 事件只从自己的 `/payload` 取实际 `sessionId/sessionKey/runId/agentId` 引用；`lastRunId` 不凭名字套入事件。Group 另外借同一文档消费自己的工具／终态条件；Group 命中本身不确立 Session，Session 投影仍按实际明示身份及下文成员证明判定。
- 直接输入形状为 `sessions.create` 和 `chat.send` 原请求的 `/params/message` 字符串。它本身就是正文，直接投影为 `ChatContent::Text`，包括 `""`；不读取不存在的 `.content/.role`，不因未配对、失败或没有消息对象而丢弃。依这两个公开 method 的消息输入语义投影协议 `ChatRole::User`，证据是本原请求 `/method` 和 `/params/message` 的引用，不是网络方向、真实人类身份或管理授权。请求只证明提交输入，不证明会话已创建、消息已接纳／送达或模型已执行。
- `chat.startup` 与 `chat.history` 的已配对成功完整回执从 `/payload` 取实际 `sessionId/sessionKey`，作为独立会话观察；`/payload/messages/{index}` 是该原历史窗口的消息对象。每条消息经该回执明示会话对象证明归属，不从请求别名倒填回执缺值。`hasMore/totalMessages/offset` 只保留原应用窗口声明，不等于 AS 已采全或本库条数；数组内部顺序及原位置保持，不主动拉取缺页。
- `chat.history` 的 `kind=delta` 与 `kind=reset` 是当前协议的不同消息种类。delta 的 `/payload/messages/{index}` 可为明示 `sessionId/sessionKey/messageId/messageSeq` 的包装对象，原聊天位于它的 `/message`；包装对象和消息对象分别保存字段引用，不把包装缺少 role 当成聊天为空。空数组仅表示该原回执没有新增消息观察。只有已有完整回执／原游标交接或包装对象自己的明确会话身份才能确认其成员；不透明 `deltaCursor` 保持实际 JSON 类型与原引用，不是应用 Session ID。reset 仅是原历史窗口重置，不删除会话／消息、不生成新 Session，也不主动驱动 Agent 补取。
- 其他对象消息取 `chat` 事件的 `/payload/message`。上述各消息对象按该来源的实际 `role/content` 和明示元数据投影；没有 role 保留缺失，不借直接输入 method 补 User。直接输入出现非字符串、对象消息出现非对象时，保留实际值及 `unsupported_shape`，不互当兼容路径。其他消息不在任意嵌套对象中搜索替代，原件照常保留。

这里列出的公开形状按字段位置区分来源，消息或 RPC 名称相同不够。未核结构不会因字段看似齐全而获权威；未取得的发布或请求不补造。聊天流中无消息 ID 的修订证明边界见下文，不假定每个 delta 都是一个新消息。

`GatewayHistoryWindow { reference, request, kind, session, messages, input_cursor, output_cursor }` 是上述原历史回执的窄投影，不是聊天缓存。reference 指原回执 `/payload`；request 为同连接 RPC 原请求 `GatewayReference`；kind 为 full／delta／reset。full 只是在原 kind 未声明且符合该方法正常完整窗口形状时的业务分类，不伪造一个原 kind 字段；明确 delta／reset 各按其协议种类解释，未知原 kind 不回退到 full。session 为 `Option<GatewaySessionObservation>`，只承载本窗口原对象明确给出的身份；messages 为实际 `/payload/messages` 的 `Option<GatewayFieldReference>`，Some 指到空数组与缺字段 None 分开，类型不符保引用及缺口。input_cursor／output_cursor 分别引用实际原请求 `/params/cursor` 与原回执 `/payload/deltaCursor`，未提供为 None；不解码不透明游标内部字段、不把它当 Session 或修订号。`hasMore/totalMessages/offset/windowReset` 的实际值仍由 reference 定位，不反推未见消息。

完整窗口本身给出的 sessionId 足以证明其数组消息归属，不需要每条消息再给 runId；历史中的工具结果也不等待模型 HTTP 绑定。delta 包装本身的 `/sessionId`、`/sessionKey` 和实际 `/session` 对象各保原引用并核已得值冲突，真正消息为包装 `/message`。通过原请求复用已保存窗口 output_cursor 的同类型同值，且原公开方法确实表示该窗口续读时，可沿同一已核 Gateway 发行域机械保留原窗口的会话引用；只有请求别名或同值游标候选唯一不构成证明，有竞争会话引用时保留冲突。空 delta 不增加消息也不表示全会话结束。reset 中断的是旧窗口续读依据；AS 保存原 reset，既不删旧消息、不改变真实会话身份，也不主动发起 full 请求。随后被动观察到的新 full 回执按其自身明示身份建立窗口。

### RPC、成员证明与冲突

RPC 索引只保原请求／回执行及本连接的原 ID、方向、方法和实际成功／失败状态。请求来自 client，回执来自 server，原封套 `ok=true` 才用于成功交接；`sessions.create` 另核 `/payload/ok=true`。请求 key／agentId 是选择值，成功回执是应用处理后的实际对象；各保原值和引用，不要求选择值与生效 key 字面相等，也不在 AS 重做规范化。根和 entry 的 sessionId 均取得时核一致，不以可选 entry 缺失拒收根对象的真实身份。describe／list／startup／history 各按上述明确原形状消费；成功 reset 是窗口结果，不当空 full 或失败请求，不把不支持的载荷推成空对象。相同 capture 与原 RPC ID 是配对必要条件；重复请求／竞争回执保留全部引用和 `rpc_conflict`，不按最近时间或文本挑选。只有一侧时保留 `rpc_unpaired`，不因同连接只有一个请求就补配对。晚到另一侧从原索引重核，在同一事务补充派生，原请求和回执均不改写。

`sessions.create /params/message` 在收到回执前已经是保留的原输入。原成功回执按上述同连接原 RPC ID 配对后，以它实际提供的 sessionId 和 runId 证明这次输入的会话／初始运行归属；身份域取该原已核 Gateway 发行端。这表示原应用的成功回执声明，不表示模型执行完成、任务完成或用户已读。正文仍指原请求，回执中的 runId/sessionId 仍指原回执，不倒填为请求本来就有的字段。回执晚到时在同一业务事务补这份交接，沿原输入引用继续读取，不新增一条相同首问；已配对失败标 `rpc_failed` 并引用实际失败字段，未配对标 `rpc_unpaired`，均不伪造成功归属，也不删除正文或方法角色。后续运行的同域精确 run 引用继续使用这份交接，缺可选 runId 时只保实际已证会话归属。

聊天归到真实 Session 只接受以下原交接：聊天自身的明确 sessionId；同一原请求／成功回执实际给出的会话 ID；或该原运行的明确身份观察同时给出 sessionId 与 runId，后续聊天精确引用该 run。身份矛盾核原 Session ID：同 run 指向不同 ID 仍保留全部冲突引用；同一已证 ID 的不同原观察出现别名或 Agent 元数据差异不撤销身份，也不据这些差异改写原值。同一次发布中明确重复表达同一生效对象的字段仍核一致（如 create 根／entry、changed 平铺／session、delta wrapper／session）；history 根的请求 key 与 wrapper 生效 key 不视为同义生效字段。全部交接核同一 GatewaySessionNamespace，RPC 配对另外核同一 capture；投影行的 lastRunId 只证明它明确指向的那次 run。仅有别名、相同 Agent、同 PID、时间接近、相似文本或一个候选均不成立。不能可靠绑定的 HTTP 交换或原生聊天保留未关联，不将其他来源值填入原 Missing SessionObservation。

Session reset 后，上述明确会话对象若提供新 sessionId，按新实例保存；旧实例及其成员不重命名。reset／create 名称本身不生成新 ID，也不增加未经原公共结构核实的 reset 载荷分支。相同 key 先后指向不同 ID 时分别保留原关联证据；key-only 或缺少可核 run／RPC 交接的后续聊天保持未关联，不取“当前最新映射”。并发会话按发行域与真实 ID 分线，RPC 按各 capture 分线；同域同 ID 的不同观察连接不另立应用会话。相同 run 指向不同会话、相同原消息身份给出互斥字段等情况保存原引用和 `identity_conflict`，不挑较晚／较全对象覆盖。缺失本身不是冲突，原 null 不是另一个确定身份。

晚到冲突使相关派生成员证明不再成立时，在同一事务重核相关引用；原会话对象、消息和旧证明原件仍保留。读指定锚点时带回其受争议原引用和 gap，不把它们继续标成确定成员，也不顺延到更早会话或另一个唯一候选。目录的确认计数不含这些未定成员。

`SessionGap { reason, references }`：reason 为 `unsupported_shape`、`field_type_unsupported`、`session_namespace_unavailable`、`session_identity_unavailable`、`alias_binding_unproven`、`rpc_unpaired`、`rpc_failed`、`rpc_conflict`、`identity_conflict`、`message_identity_unavailable`、`message_order_unproven`、`message_order_conflict` 或 `revision_order_unproven`；references 为非空 `GatewayFieldReference` 列表，逐项定位实际受影响原对象／字段。未取得的字段用所在原对象引用，不造一个不存在字段的“已读值”。未核 TCP／WS 来源的缺口仍用原采集缺口，不能伪造此处已核 Gateway 引用。这些是观察缺口，不是驱动失效或 HIPS 裁决。

### 聊天读取类型与分页

所有新枚举以 `kind` 判别，值在具名字段承载；没有未标记联合、默认分支或旧形状兼容。以下类型归业务日志的 Session／聊天子模块；同进程借原类型，不进行 JSON 往返。

- `ChatSessionRef`：`model_http { record: ProtocolRecordId }` 或 `native_gateway { object: GatewayFieldReference }`。record 指原字段组首个请求；object 指 `(GatewaySessionNamespace, sessionId)` 首次确认该组时采用的原会话对象。这个对象的 capture 是证据位置，不是分组键；后续重连／其他观察连接取得同一原身份时引用同一目录锚点，各自原对象引用仍保留。两者都是库内原件锚点，不能由客户端自报字段创建身份；后续更多事实或更早原件的晚到配对不替换锚点。
- `ChatSession`：`model_http { reference, source: Arc<ProtocolSource>, session: SessionObservation, exchanges: u64 }`，直接复用原 source／观察及不同真实交换计数；或 `native_gateway { reference, namespace: GatewaySessionNamespace, source: Arc<GatewayConnection>, session: GatewaySessionObservation, chat_observations: u64, gaps: Vec<SessionGap> }`。原生 source 只指目录锚点的实际连接，session 只指锚点原观察，不代表该会话所有连接都相同或是最新快照；namespace 才是发行域。计数只统计实际归属的原聊天观察，不称消息／run／任务数；多观察者的重复原观察不冒称多条应用消息。reference 与变体来源相符；u64 JSON 为十进制字符串，不倒灌其他原对象的字段。
- `ChatMessageScope`：`model_http { exchange: ExchangeKey }` 或 `native_gateway { session: Option<ChatSessionRef> }`。原生的 Some 只接受 native_gateway 会话锚点；None 为已保留原生聊天观察的来源化列表，不宣称它们属于共同 Session。它不触发虚拟归组。
- `ChatMessageRef`：`model_http { message: ChatMessageId }` 或 `native_gateway { object: GatewayFieldReference }`。后者是该消息／未定身份观察首个已提交原对象的引用，具有原 connection／方向／帧／字段，不生成 ExchangeKey。
- `ChatMessageEntry`：`model_http { message: Arc<ChatMessage> }` 原样包住现有模型类型；或 `native_gateway { reference: ChatMessageRef, native_message_id: Option<NativeIdValue>, order: GatewayChatOrder, observations: Vec<GatewayChatObservation>, gaps: Vec<SessionGap> }`。observations 非空；它们属于该逻辑消息的各原观察，只有真实修订依据才按修订先后展示，其余陈列为原观察而不选“当前最新正文”。order 只表达下面定义的消息位置，不是修订。原生来源下移到每条 observation，不用一个 source 代替跨观察连接的各原件来源。模型公共 ChatMessage、活动与 Ask 的原 FactReference 不因历史封套扩展而改称原生类型。
- `GatewayChatObservation { source, reference, wrapper, history, session, handoff, run_id, message_run_id, message_sequence, wrapper_sequence, record_timestamp_ms, transcript_position, native_fields, role, content, state, revision }`：source 为本观察实际 `Arc<GatewayConnection>`，与 reference.message.capture 一致；reference 指真实消息对象／直接输入。wrapper 为 `Option<GatewayFieldReference>`，仅 delta 包装形状指 `/payload/messages/{index}`，此时 reference 指其 `/message`；history 为 `Option<GatewayFieldReference>`，仅窗口消息指该回执 `/payload`。两者非消息正文副本，不能用 wrapper 的 model／lastRunId 等会话投影覆盖 nested message 的值。session 为 `Option<ChatSessionRef>`，只保已证归属。
- observation.handoff 为 `Option<GatewaySessionObservation>`，只在原请求输入存在同连接成功 RPC 的明确会话／运行对象时填写；其 reference 指原回执对象，request 为 Some(本观察 reference.message)。run_id 是原封套 `/payload/runId` 的 `Option<String>`，message_run_id 是原消息相对 reference 的 `/__openclaw/runId` 的 `Option<String>`，两者不互填。直接请求无自己的 run_id 时仍为 None，原创建回执的 runId 只在 handoff 的实际字段／原引用中读取。wrapper 的 session／lastRunId、inFlightRun 中的 run 和这两个 run 字段各有对象，不能套入当前消息运行。
- observation.message_sequence／wrapper_sequence 均为 `Option<NativeIdValue>`，分别取消息 `/__openclaw/seq` 和实际包装 `/messageSeq`；不是一个按 full/delta 二选一的 native_sequence。每值分别由 reference 或 wrapper 加该固定路径定位原字段；同时取得时保留双方原类型和值并核对应消息 ID 与序号是否一致。record_timestamp_ms 为消息 `/__openclaw/recordTimestampMs` 的 `Option<JSON值>`，保原记录声明的 Unix 毫秒，不当 BOOTTIME、AS 首次观测时间或消息顺序。transcript_position 为消息 `/__openclaw/transcriptPosition` 的 `Option<JSON值>`，保留实际 source、rawSeq、其他属性及原类型；显式 null 为 Some(null)，没有属性才为 None。source 是该转录位置的原不透明域，不是驱动 source 或 Session 发行域；rawSeq 只属于该原位置域，不与消息序号无条件等同。原位置改变不单独证明换消息、换会话或正文修订。
- observation.native_fields 是 `Vec<GatewayFieldReference>`，仅索引该消息自身实际出现的 `/__openclaw/id`、`/idempotencyKey`、`/__openclaw/idempotencyKey`、`/timestamp`、`/responseId`、`/api`、`/provider`、`/model`、`/responseModel`、`/toolCallId`、`/toolName`、`/isError`、`/stopReason`、`/usage`，及实际 `/content/{index}` 中 `type=toolCall` 对象的 `/id`、`/name`。列表引用原值，不扫描任意嵌套同名字段，不取 wrapper 的会话 model 充当消息 model，不剥工具复合 ID 后缀。响应／工具引用只有在同原公开协议作用域下解释；这些字段并存不证明模型 HTTP 交换成员，缺真实模型来源域时 HTTP→Session 继续未关联。工具 isError、模型 stopReason／usage 不改称系统调用结果或任务完成。消息全部 content 仍用原 ChatContent，不生成假 HTTP ChatPartRef 或执行关系。
- observation.role 为 `Option<GatewayChatRole>`，后者有三种来源化表示：`string { value: ChatRole, original: String, field: GatewayFieldReference }`、`json { value: JSON值, field: GatewayFieldReference }` 和 `protocol_input { value: ChatRole, method: GatewayFieldReference }`。对象 role 的字符串原词汇由 original 与 field 保真，value 是该协议业务投影：user／assistant 等沿 ChatRole，toolResult 映射 Tool，未知／空串用 Other；不把原 toolResult 改写为原件中的 tool。非字符串仍保 null／false 等实际值，无属性为 None。protocol_input 仍只对上述直接输入方法给出协议 User，method 与原输入位置为依据，不是原对象存在 role 或已证明人类身份。
- observation.content 为 `Option<ChatContent>`：直接输入字符串取 observation.reference 所指原字段并构造 Some(Text(原字符串))，空串同样为 Some；对象消息取实际 content，字符串用 Text、其他实际 JSON 值用 Json，缺属性为 None，原 null 为 Some(Json(null))。形状不支持时仍按实际原值保留内容及 gap，不造一个 role/content 消息对象。state 只取原 chat 事件 `/payload/state` 的 `Option<JSON值>`，其他来源为 None，不把 RPC 成功／失败当模型 Complete／Failed；revision 为 `Option<NativeIdValue>`，只取原聊天对象的 revision 字符串／数字，不用入库号、lifecycleRevision、RPC id、runId 或外层 seq 充当消息修订。原生 ID 不从封套 id 取得。原协议时间不转为 AS 的 first_seen／last_seen。

消息原生 ID 按当前公开形状取 full 消息 `/__openclaw/id` 或 delta 包装 `/messageId`；nested `/message/__openclaw/id` 同样保留原引用。包装明确承载这条 nested 消息时分别核两份 ID 及两份消息序号，不能因数值恰同就把无关对象认成同一条；冲突保双方原值与缺口，不挑一个覆盖。身份域仍为已核 GatewaySessionNamespace、已证原会话及原消息命名空间，不是 RPC id，也不要求消息根有 id。空 ID、缺会话、类型不符均保原件与相应结果，不使用正文相似或唯一候选归组。

同域同会话的明确消息 ID 归入同一消息项，各窗口／连接观察引用仍独立保留。消息序号表达消息在会话中的位置，不是该消息的修订号；相同消息值在 full／delta 重现不自动成为一次新修订，也不因没有源 revision 就丢消息。只有原对象明确修订字段或原协议已证明的同一消息更新交接才建立修订先后；观察值实际有差异但先后未证时保各观察及 `revision_order_unproven`，不以 RPC id、消息 seq、rawSeq、观测时间或较晚 SQL 到达挑“最新正文”。窗口限长／投影差异不直接宣称原消息被编辑；同消息／明确修订给出互斥角色或正文时保冲突。transcriptPosition.source 的变化本身不是身份或正文冲突。逻辑消息聚合与修订呈现是业务规范化，原观察的值／类型／空值不被规范化结果覆盖。

直接输入的观察身份仍是原请求字段，创建回执只补 handoff。若同会话的原成功回执与原历史元数据明确交接原幂等消息键或消息序号，并实际给出该原消息 ID，可补充同一逻辑消息的来源；不以两数相等、文本相似或仅一条历史记录代替交接。未证明此事时保原输入／历史观察各自来源，不重复入库正文或伪造消息 ID。外层事件 seq、payload.seq、窗口 deltaCursor 与消息 id／消息序号／消息 revision 各保原义。

### 原生对话顺序与分页变更

消息位置由业务日志纯投影／查询提供，SQL 只索引已给定的原字段，前端只显示其已定序段、原窗口段和未定序观察，不重做排序归因。`GatewayChatOrder` 仅三种：

- `session_sequence { sequence: NativeIdValue, references: Vec<GatewayFieldReference> }`：同一已证真实 Session 内，以该公开形状明示的消息 `__openclaw.seq`／wrapper.messageSeq 为依据。references 非空，分别引用实际字段；两者同在时按原对应消息核一致，冲突为 message_order_conflict 而不是任选一值。只有原 JSON 非负整数且能由既有 u64 无损表示时按精确整数升序比较；原数字字符串／不支持数字仍保原值和缺口，不转码、截断或浮点比较。不假定序号连续、不据缺号制造消息或断言采样丢失。
- `history_window { window: GatewayFieldReference, index: u32 }`：没有可比较会话消息序号时，保留同一实际 full 历史窗口 `/payload/messages` 的数组正序，index 是实际从零开始下标。只证明该窗口内部顺序；独立窗口之间、窗口与直接输入之间没有明示位置时，不按 SQL 到达、时间或文本猜相对先后。重复取得相同已证消息 ID 时保原窗口位置，不克隆成另一逻辑消息。
- `unresolved { references: Vec<GatewayFieldReference> }`：非空原引用保未取得顺序或有冲突的观察，并带相应 SessionGap；不附加到已定序消息尾部后称最新对话。无全序的窗口／来源分别成段显示；段的陈列位置可以用提交位置定位，但明确不是跨段对话顺序。`scope.native_gateway.session=None` 是来源化观察列表，也不承诺不同会话间的共同对话序。

transcriptPosition.source/rawSeq 独立保留，不作为全会话排序键；source 不同的 rawSeq 不比较，和消息序号同数不推等价。late full 中的旧消息有已证消息序号时按原序归位，没有序号时留在原窗口内部，不能排在最新消息之后冒称完整对话。原消息序号、数组位置、入库位置与 revision 是四件事；hasMore／totalMessages 仍只说明该原窗口，不能由有序列表声称全会话已采全。

`ChatMessageCursor` 是查询游标，与不变的 ChatMessageRef 分开：`model_http { message: ChatMessageId }` 沿模型原交换与首次提交顺序；`native_gateway { reference: ChatMessageRef, read_through: u64 }` 仅允许原生 reference。read_through 是该读事务已知的 SQLite deliveries 截止正行号（JSON 十进制字符串，1..i64::MAX），不是源 seq／修订／时间；原消息及成员／位置证据均能按该截止定位。cursor 不授予内容权限，也不指定写入路径，不保存服务端页面快照。

首次读取原生消息页时取当前已提交的 delivery 截止；续页保持同一 read_through。消息身份、成员、顺序及修订只使用截至该位置的原事实／证据重建，不将后来取得的成员或旧消息插进已经开始的分页。原引用及同一 scope 每页仍重核；原引用不存在为 NotProvided，游标非法或不属于该读取范围为 Invalid，不换候选或默读另一原件。后续新事实在用户从首页刷新时进入新的读取截止。这里复用同库已提交位置和追加证据，不建立页面缓存、长读事务或分页变化追踪服务。

Session 目录仍按两类固定锚点的登记提交顺序 newest-first 分页；before／next_before 的既定语义不变。模型锚点取原 deliveries；原生事务使用 `deliveries.kind=native_gateway` 且源 cursor 为 NULL，不写 chat_inputs。原生消息页按本节原对话序／窗口段／未定序来源读取，模型消息页仍按原交换首次提交与消息序号正序。after／next_after 使用 ChatMessageCursor，next 只在该读取截止内确有下一页时返回本页末项原引用及同一读依据；limit 计消息项／明确观察项，不计同项修订或把同一 ID 的重复窗口观察算多条消息。一次项的修订不拆成新消息分页；过大沿原 output_too_large，不删内容或扩大限制。目录提交顺序、模型消息顺序、原生对话顺序不互相套用。

所有 None／空值遵守原契约：读取成功的 `""`、`[]`、数组空项、0、false 和 JSON null 均保留；字段存在但类型不支持时保留原字段引用及 gap。Header 原字节不转码，JSON 字符串保留实际字符串，原 JSON 数字通过既有精确 codec／NativeIdValue 保持字面量，不经浮点或 SQL CAST。原 wire 才是字节原件，解码值不重编码冒充原 wire。

### 事务、索引与唯一内容存储

本节只增加同库的派生索引，不另设会话库、writer 或聊天日志。原连接／帧继续按下节唯一原件表示保存；未核原件不能直接生成下列业务索引。初始化在安装 DDL 前核本轮 Session／RPC／window／成员表及其共用原件依赖的全部表名和列名布局；version0 已含 schema 对象即拒绝，version2 校验成功直接返回，不经 `CREATE IF NOT EXISTS` 修补缺件。

- `gateway_rpc_observations`：外键指正常原 message，索引 req／res、原 RPC 字符串、请求 method 和实际响应成功状态；保留双方原引用，不存 params／payload 正文或内存 pending map。配对读取限同 capture，重复及晚到竞争重核所有相关派生，不择最近一行。
- `gateway_history_windows`：一条原历史回执对应一份 GatewayHistoryWindow 索引，保存原 message／request 外键、kind、明确会话观察及 messages／输入输出 cursor 的字段位置。不透明 cursor 的实际 JSON 类型及精确值表示只作等值索引，不解析内部身份；输入 cursor 另用于保留 reset 对原窗口续读依据的中断。`gateway_history_sessions` 仅追加已经原 full／窗口续读交接证明的会话观察引用，沿链复用同一原引用，不填进窗口自身缺值。full、空 delta、reset 均保留原结果；不保存窗口正文或第二份消息列表。窗口内序引用实际数组下标；通过已明确的会话与原消息 ID 查询已有消息，不按文本合并。
- `gateway_session_observations`：以 `(message, object_pointer)` 指唯一原对象，分别索引实际 sessionId／key／runId／lastRunId／agentId 字符串及确已配对的请求引用。None 为 SQL NULL，空串为原 TEXT；每个字段的原路径按上述公开形状核对，不存一份对象 JSON。发行域经本消息原连接联表真实 server 完整 ProcessKey 及 server_port 取得，比较实际进程字段而不是只比较 process 表行号。显式缺口引用与该原件同事务保存。
- `gateway_chat_observations`：以 `(message, object_pointer)` 指真实聊天对象或直接输入，delta 另保 wrapper／history 原引用。分别索引 nested 原消息 ID／seq 与 wrapper messageId／messageSeq 的类型／值及原路径，不覆盖为单列事实；message ID 归组、原生顺序、窗口下标、原录入位置、转录 source/rawSeq、原修订及封套／消息 run 各自保语义。`gateway_chat_memberships` 追加原成员／交接、逻辑消息固定原件锚点、缺口及本次 delivery，读取按截止过滤，不覆盖以前的证据；迟到的较早原观察归到已存在逻辑消息时复用其锚点，不改签原 ChatMessageRef。顺序与修订由截止内的原索引及原字段重建；索引不缓存页面或维护“最新正文”。角色仍区分原字段与 method 证据，原响应／工具字段只引用消息自身原值；不复制正文或 ChatMessage 封套。晚到回执补同一原输入交接，失败／未配对照常保留。
- `chat_session_entries` 只索引固定锚点及其登记位置：`delivery` 为现有 deliveries 正行号外键，`kind` 为 model_http／native_gateway，`protocol` 或 `gateway_message/object_pointer` 按来源互斥引用上述原对象，无 Session 值或正文列。原生锚点按 `(GatewaySessionNamespace, sessionId)` 解析到同一项；新 capture 取得同一明确原身份时追加原观察引用，不另插一个 Session 锚点。模型选择值仍只在原 `protocol_session_fields/values` 保存，原生值只在对应观察中索引。计数及成员查询在读事务由结构化索引组合，不写第二份会话快照、全局发行者注册表或可变“当前 key 映射”。

写入先在事务前完成一次公共 JSON 解码，SQL 工作保管原件及该树；事务分配原引用、显式读取原候选、纯业务判定，原 wire 与结构化索引共同 commit。缺失、未支持或业务冲突是已保留原件的结果，不回滚原件当作从未采到；SQL／结构完整性错误按原 StoreError 回滚并报告实际失败。原件成功提交后才发布原提交通知；这不制造 Flow 聊天源序号。已接纳 SQL 不因前端取消而丢弃；等待 work future 不是新的数据 owner。

查询按索引定位本页所需原帧，复用同一 WS／JSON 解码及纯投影函数，临时解码一次后借出角色／ChatContent 与修订；释放事务后只保本次返回值，不保存重放缓存。原帧只存一次，Group 与 Session 都只回指其原 reference。原正文不为目录、聊天页、Group 或修订列表重复入库；不从不同原消息相同字节猜内容去重。库布局以唯一现行 schema 为准，不双读双写、不保留旧 ChatSession 形状、默认兼容值或旧格式回退，历史证据库保持原件不被此设计修改。

### Gateway 原件存储

`group_gateway_connections` 按 capture 保存原连接与进程字段、Upgrade 请求及回执的 evidence 引用；`group_gateway_socket_identity` 以同 capture 为唯一主键／外键，`client_cookie/server_cookie` 保存各端非零 u64 的固定 8 字节大端 BLOB。两表及 Upgrade 原件同一事务保存，不复制连接或字节。类型含义直接引用[共用来源](../technical.md#共用来源与原引用)。

`group_gateway_messages` 保存 capture、from_client、实际 sender／receiver 进程外键及可选采集 reason。正常消息 reason=NULL 且双方身份非空；Gap reason 非空且双方身份 NULL，不借握手身份替未核区间背书。`group_gateway_frames` 按 `(message, ordinal)` 保存原帧 stream_offset 和唯一 evidence_sets 外键；wire 含原头、mask 和负载，消息不另存解码正文。正常帧正长度、有序且不重叠；Gap 原片保留实际重叠、倒序或相同 offset，不套正常 WS 消息约束，也不派生身份。

同一原消息的 `decode_issue` 可空列保存 facade 给出的 GatewayDecodeIssue：非文本为 non_text，JSON 解码失败为 invalid_json，成功为 NULL。该列不复用采集 reason，不把已核 WS 的 JSON／控制帧解释结果改写成传输缺口或抹掉实际双方进程。Gap 不做 JSON 解码，decode_issue 为 NULL，原 reason 表明此时不是解码成功；读取先核原 reason。未知业务形状仍是成功 JSON，其字段缺口按 SessionGap 保存。当前布局包含此列，缺列不走旧格式回退。

`group_unverified_streams` 保存 UnverifiedGatewaySource 五个原来源字段、from_client 和 reason；`group_unverified_frames` 保存各片 ordinal、stream_offset 及唯一 EvidenceBody 引用。不填写已核 capture／ProcessKey 或业务身份，不伪造为另一 NativeGroupTcp。原 `group_unframed` 保存未能接纳到原流的实际驱动记录及原字节。source 改代、断流和关闭的原件保留规则仍由原消费者负责，不在 SQL 猜边界。上述名称只是既有存储位置，Session／Group 共用同一原件，不各建一份同义表。
