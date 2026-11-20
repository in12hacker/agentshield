# 业务日志架构

[产品语义](design.md)与[技术模型](technical.md)分别维护。中间层依赖后端公开值；Main不导入业务库、不组装Ask窗口、不推定关联。

## 所有权与数据流

配置模块先派生固定`middleware_database_file()`，中间层初始化只借该ConfigPaths结果；不接外部写入路径、不自行定位自保根。业务库所属模块创建父目录与数据库，安全配置、规则和秘密仍由后端所属模块写入。

`BusinessLog`持唯一SQLite Connection及有限工作保管，单写入路径、WAL、外键和短事务。没有第二个数据库owner、转发服务、聊天历史HashMap或自主关系线程。宿主将Flow唯一ChatReceiver经Main组合直接交同一Media的consume_chat，在Messages职责内独立入库，即使没有外部订阅也继续；不与控制结果共用待发送队列。取得真实OperationResult即保存原EventData和结果，不以已有Ask、工具关联、动作或固定文本筛选；Observed本身不制造执行结果。

```text
Flow唯一实时输出 → Main原消息接线 → 中间层业务消费 → SQLite事务提交
真实Ask／原操作结果／原生交接 ─────────────────┘
真实Main Connection → ContentAccess核权 → SQL只读临时ContextView → 前端业务返回
```

队列出队即移交原Arc；工作持有原数据到事务返回，随后释放。查询从结构化表重建所需原DTO，消费后释放，不安装缓存。第三方SQLite FFI复用rusqlite，项目代码不新增unsafe。

## 关系与反馈

活动关系的种类、证据强度与候选条件由[审计唯一模型](../audit/technical.md#复用的实体与关系)定义。每条边保留原FactReference证据，引用不全或冲突记录Gap；候选与补充时序不升级为tool_execution。进程父子关系不在本库另算，Agent调用图谱不做成员管理或推定边。

固定反馈由业务层从DB取原完成、原ToolCall及原协议证据，由后端原完成消费者经显式Flow窄端口执行，不借用前端管理／内容会话授权；原文来自原命中版本，不从当前规则重读。业务绑定／阶段保存在同库，不能因重连重发已处理反馈。无桥不发请求；Flow实际写出阶段与入库提交分开。

## 启停与失败

业务库初始化成功后再接纳业务；运行时入库失败向原宿主报告，不静默放弃数据、不影响既有安全裁决。正常停止先封外部管理并保留内部消费；源结束并排空消息后，由业务库所有者发起一次关闭，排空已接纳 SQL 并实际关闭 SQLite。宿主及其他调用方等待同一个关闭结果，等待取消不转移或取消库的退役责任。未提交事务由 SQLite rollback，提交失败不生成 Stored。

Ask转发和答复不等待业务入库；业务查询只沿同一真实ContentAccess核权。正式 WS 查询资格由真实 Main 会话授权。SQLite结构化持久化不宣称已交付远程可靠推送或完整Agent执行桥。

正式 WS 宿主复用这一唯一数据库实例及原写入任务。客户端断开只释放其发送和查询工作，已接纳的 SQL 仍由数据库完成。新增连接不重复登记同一原 Ask 或复制聊天存储，查询返回后释放临时视图。

## Session／聊天的纯业务子模块

在现有业务日志模块内划分纯业务投影与数据库实现，原因是同一公开协议同时服务 Session／聊天和 Group，语义不能由某张 SQL 表或 Group writer 代管。代码落点及依赖方向如下；接口与字段只在[技术模型](technical.md#session-与原生聊天业务模型)和[中间层共用来源契约](../technical.md#nativegateway-共用来源与解析入口)定义。

- `business_log.rs` 保持唯一业务日志 facade，组合原件接纳、纯投影与原数据库事务；Session 的业务公开类型由它所属的子模块定义，不从 `database/sessions.rs` 暴露 SQL 所有权。
- `business_log/sessions.rs` 承担无 I/O 的 Session 身份、RPC 交接和原聊天引用／索引投影。输入是窄 Gateway 文档及本次 SQL 读出的原引用候选；输出是明确结构化投影与缺口。私有 `sessions/observations.rs` 只在读取已选项时物化原观察 DTO；写索引不深拷贝正文。两者不查库、不访问 Group 内部、不回读 Agent、不保留跨调用缓存。
- 私有 `gateway.rs` 从现有 Gateway 适配收敛公共原协议解码及共用来源引用。它只处理一个已经核验的消息，复用公共 JSON codec；不持有 TCP 字节缓存，不登记业务状态。Group 的 `agent_graph/gateway.rs` 保留自己的委派／终态投影，改为借用该窄文档。
- `database/group_gateway.rs` 继续保存共用的原连接／帧及 Group 派生索引；表名前缀不赋予 Group 一般聊天职责。其原件读写 helper 只核结构和外键，不调用 Group／Session 协议解析器。`database/sessions.rs` 保存与查询 Session／RPC／原生聊天的结构化索引；`database/chat.rs` 继续保存 Flow 原事实；`database/query.rs` 在一次读事务内组合相应结果。
- `business.rs` 处理已鉴权的查询选择与结果封装，`transport` 处理原连接和发送。两者不接收外部原件、改写事实或执行会话匹配。以上均在原 `as-middleware` crate 内，无新增 crate 或后台任务。

### 单一接纳与事务

```text
原驱动 NativeGroupTcp
  → Media::consume_group → 原 collect／collect_gateway（TCP／I/O／Upgrade／WS）
  → 唯一业务日志 facade 接纳持有完整原消息的 SQL 工作
  → 事务前共用 Gateway 解码一次 → 原事务分配原件引用并借该树
  → SQL 显式读取原候选 → Session／聊天纯投影 ＋ Group 纯投影
  → 原帧一次保存 ＋ 字段／RPC／成员／修订引用索引 → 同一事务提交
  → ContentAccess → 业务历史页 → 既有内容组件
```

SQL 工作先为本次原消息分配原件引用，再借同一事务读取同连接 RPC／已核 Gateway 发行域内原身份索引，显式传给纯业务函数判定。事务执行封装在 facade 的原工作中；SQL helper 只保存判定结果。创建会话的原输入独立保存，晚到成功回执仅按原连接和原 RPC 引用补充该输入的会话／运行交接，不重复正文或改请求原值。不同观察连接的原对象经同一已核发行域与真实 Session 身份解析到同一目录锚点，连接本身不成为应用身份。解码不支持、缺字段或证据冲突仍提交原件和相应缺口；真实 SQL 错误回滚本次原件与索引并沿原失败链报告，不返回 Stored。

原生聊天内容只引用原帧内明确字段；库内没有第二份聊天正文或解码文档缓存。读事务按索引定位必要原件，复用同一解码和投影入口重建本页字段，返回后释放。模型 HTTP 的原 ChatMessage／Envelope 仍按 Flow 原义存取，不将 Gateway 消息套入其 ExchangeKey。

公开 startup／history 的 full、delta 包装与窗口 reset 都经过同一窄解码和 Session／聊天业务投影；窗口身份、消息原元数据和成员／位置证据由业务模块解释，SQL 只保存原引用与必要索引。原生展示顺序由业务查询使用这些原证据给出，不把 SQL 到达次序当对话顺序。续页带原引用及固定读取知识截止，仅使用该截止内的追加证据；首页刷新才取新截止，不依赖页面快照、聊天缓存或通用归并服务。前端只展示后端给定的顺序段，Group 不参与一般历史窗口或消息排序。

停止先收敛原接纳，原采集消费者保管并排空在途原件，之后业务库排空已接纳 SQL 并实际关闭。未配对 RPC、未完成聊天与未知 Session 留在原库及缺口中，断线不补终态，重新连接不接续旧 capture 或旧 RPC；这不切断已核同发行域内的原 Session。浏览器断开或撤权只终止该连接的读取／写出，已接纳业务事务仍由原 owner 收尾；原 Ask／答复与规则期限继续沿原消息职责推进。

JSON 解码在写事务开始前完成一次；唯一 SQL 工作保管原件及该树，在事务分配原引用后借它构造投影视图、读取所需原索引并写入结果。查询在短事务内取得原引用、成员／顺序证据及必要原件，事务结束后重建本页内容；只复用解码和原字段读取，不在查询时重新裁定或写入会话归属。
