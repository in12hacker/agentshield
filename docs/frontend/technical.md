# Dashboard 技术与控件验证

[产品](design.md) · [架构](architecture.md)

## 唯一接口出处

传输、权限、业务封套和查询见[中间层 WS](../middleware/technical.md)。规则/版本/保存见[规则](../backend/rules/technical.md)；配置/保存/采用见[配置](../backend/configuration/technical.md)；临时开关与状态见[驱动通信](../contracts/driver-communication/technical.md)；进程完整身份/查询见[进程树](../backend/process-tree/technical.md)；安全日志见[日志分页](../backend/security-log/technical.md#只读分页)；原 Ask/Answer/OperationResult 见[Main 消息](../backend/main/messages/technical.md)；private 见[凭证](../backend/credentials/technical.md)；证据/Ask 上下文见[审计](../middleware/audit/technical.md)；Group 见[调用图谱](../middleware/agent-graph/technical.md)；原聊天字段见[Flow](../contracts/flow.md)。本页只追踪消费者和操作名，不复制 DTO。

## 控件覆盖设计

下列每项是用途、功能 owner、具体浏览器操作及预期。`RawFacts` 的每一个 summary 都检查展开原文、折叠并且不发请求；动态嵌套 summary 也要展开盘点。只有最终确认触发写命令。本地交互前后对照接口事件，页面进入/明确查询产生的读取单独核对。DOM 清单只表示观察到，不自动表示执行或通过。

### 全局/鉴权（main.tsx、dashboard.tsx、i18n.tsx）

- language select：选择三种语言，检查 html lang、标签和本地偏好；无 WS。
- 连接及首次认证等待真实传输／协议结果，不设置额外前端截止；用户可显式取消，原身份与权限校验保持。
- token input、unlock、connecting cancel：空/非法值不发送，合法合成值仅首帧 authenticate；取消后 gate 清空且晚认证不能解锁。连接详情 summary 展开端点/Origin。
- 部署端点由`wire.endpoint`校验显式端口的literal loopback WS根地址；末尾`/`可省略，接纳后统一为根`/`，其余传输与身份范围沿原WS契约。浏览器验证使用无末尾`/`的实际构建值核真实握手及Origin。
- 八个导航 link：逐个点击，核 hash、aria-current、标题焦点；窄屏 menu 开/关及 Escape 还焦。skip link 用键盘进入并聚焦标题；返回/前进恢复路由。导航页正常按 owner 读取，不额外发送写入。
- theme：切换 html class 与 aria-pressed；本地。header refresh：protection_overview 原 business ID 与可见返回。lock：回 gate、清内容，不自动重连。

### overview（overview.tsx、overview-model.ts）

- banner refresh、receipt refresh：分别点击，核 protection_overview、替换读槽及分项失败/完成；不把 accepted 当完成。
- details link、五个模块 tile：逐个定位 receipts，核标题焦点与滚动；browse processes 进入进程页。
- command ID、driver/Flow 原回执 summary：展开/折叠，核原值、0/false/空字符串/大整数；异步刷新不重置滚动。 概况重新读取期间只保留概况内容区已测得的像素高度，防止暂时缩短内容导致浏览器钳制滚动位置；旧防护事实仍释放，新回执返回后按实际内容布局，不恢复旧结果、不强制滚动。

### processes（processes.tsx、processes-state.ts）

- load/first page、lifecycle select、page size select、next：核 processes/query/list 的原 filter/page/cursor；返回页号和内容。游标失效显示原错误并允许显式首屏读取。
- processes/containers 两 tab：本地改变组显示不发请求。host/container filter、clear filter：核完整 workload，不从标签推身份。
- 每组 process row：点击本地详情、核完整 key/原值和焦点；return 恢复触发器。coverage/initial/original/reply summary 核字段并折叠。
- Group Get 直达的 reread、两处 return：分别核完整 ProcessKey Get 与 List，不把 PID 当实例。

coverage在完整workload筛选前后均逐字段对同次ProcessPage的status/tree_revision；进程行在原页、Host筛选页及容器tab选择后，完整ProcessKey与原ProcessRecord分别逐值核对，返回还焦。Host TGID标签只表示PID范围，不表示Host workload；不用动态名称或单个出生时间代替完整原记录。

### rules（rules.tsx、rule-inputs.tsx、rules-state.ts）

- refresh：rules/get；stage select、三 category summary、group summary、view group、rule row、return：本地筛选/详情/还焦，不发命令。
- create/edit group：kind、id、number、enabled、execution 五个表单控件；已有身份只读。preview 只校验草稿，back 保留草稿，cancel/close/Escape 不写；confirm 才 rules/edit，核 expected_version 和 create/replace_group。
- group enable/disable/delete 和 rule enable/disable/delete：确认前不写，取消无写；提交核各 RuleChange 及保存/采用分离。
- create/edit rule：id/number/enabled、各类可编辑 operation checkbox、log select、deny_message textarea。唯一作者操作集合在 `rule-fields.ts`，编辑器及浏览器测试共用：Process只有`execute`，File选项直接引用下述规则契约和rule-fields.ts（包含Driver独立Mount影响规则），Network为`connect`/`send`；不从wire完整事件枚举生成作者选项。scope select 的 all/containers、add/remove container 及 runtime/namespace/id 输入逐项操作。作者namespace留空提交null，仅匹配未提供命名空间的实例，不表示通配；非空字符串不trim、不改写。不提供作者空字符串的保存正控，通用ContainerInstance事实codec的空字符串正控独立保留。
- source/target 的 condition field select、add/remove field：按现有 actorFields/targetFields 遍历字段。match operator/value、普通数值、u64、device major/minor、access select、ports first/last、arguments add/index/operator/value/remove 使用同一表单组件；动态添加后逐个操作，不只断言存在。
- action select：allow/to_user/deny/ask；deny termination，ask timeout、timeout default 和 fallback termination 均明确填写。预览展开 preview/diagnostics/before/exactChange，再返回或确认。写回执 accepted、saved(applied:null)、adopted 与 conflict 分别显示，未确认不得自动重发。

可编辑操作依据[规则操作契约](../backend/rules/technical.md#文档组和规则)及唯一能力出处[DriverCapabilities::controlled_operation_bit](../../crates/contracts/as-types/src/driver_control.rs)，规则编译消费见[compile.rs](../../crates/backend/as-policy-engine/src/compile.rs)。这里只维护小范围输入选项，不复制能力查询框架或编译器；所有剩余选项均有合法规则用途，字段/操作及组/动作的复杂组合仍交后端编译。

合法CRUD正控使用非零access_mask及Open操作；编辑原非空namespace后分别清空提交null、填写非空字符串原样提交，核对原scope.instances。三类参数化最小创建使用AllProtected、一个可编辑操作、空source/target及所属组合法动作：process Ask、file remembered Deny、network Driver ToUser。动态全字段场景只证明草稿及出站保真，负控选择明确矛盾：Ask组选Deny为Invalid，文件new_path/access_mask操作要求互斥为Invalid，Network Driver选Ask为Invalid；不再用已删除的观察操作或作者空namespace作失败理由。shape codec独立保留0值反例、通用事实空字符串及全部已知协议操作的读取正控，其目的不是业务规则合法性验证，也不改变生产decoder的职责。

已有规则编辑须在实际动态位置核条件：UID单独时先输入、移除并核其余source不变，再重新添加UID并在添加name后输入、移除；device、inode、access_mask移除前后核同一表单的hidden source/target草稿。移除字段须确实省略，其余字段保留；预览返回仍为该草稿，最终出站用现有typed RuleChange逐值核对。供给回执与真实保存/采用分别认定，不能用其他布局的同名控件动作代替本条件。

文件长度操作在作者选项中区分「已打开文件」与「按路径」两个入口；不能以相同的泛化标签隐藏不同操作身份。

### activity（activity.tsx、activity-state.ts）

- refresh/page size/older：security_log 的 before/limit，显示页码/空页。无 ManageService 时无日志请求。
- 原日志各变体 row：本地选择，return 保留列表滚动/焦点；原件/anchor/因果链及嵌套 summary 展开/折叠。
- read/reread：activity_context 按原 event anchor；loading cancel 不发接口，晚回执不恢复；离页晚回执也不恢复内容。
- 选中原活动记录／实际完成回执后，在内容授权下沿同一证据读槽初次读取，因果链默认展开；Ask同样在原选中读槽初次读取。取消、失败不自动重试，不轮询；原答复和期限不等待内容读取。
- 晚到内容沿现有 `stream` Subscribe／Unsubscribe、`stream_reply` 及 `message.type=chat`；封套版本始终引用唯一 VERSION。`filter:{},after:null` 只从当前 tail 接收，订阅 ack 后补一次原上下文读取以覆盖初次 SQL 与 tail 安装间隔。消息只触发当前可读 SQL 读槽，pending 合并为一次 dirty 重读；原显式 attributionRequest 保持原值，不自动填写条件。原 ChatEvent 不进入前端回放缓存，不修改 Question 冻结窗口或 Answer 通路。正文／part仍用既有 decoder，原协议观察等未投影对象只校验已知事件标签和对象封套。
- 原话 expand/collapse：长文本本地保真切换；download 检查真实浏览器下载的文件名和 JSON 原事件/选择/回执，不发请求。无内容授权时不给读取。

现CausalChain及原件视图消费[局部引用与补充时序](../middleware/audit/technical.md#先连接已证原件与补充时序)：native_request_reference标为「原始引用」，same_thread_return_sequence标为「时序模式」，不套已确认ToolExecution标签。只展示后端返回的边、原raw及原缺口，缺Opening／NativeReturn仍保留已有部分，不由前端重关联或生成边。native_observation点击复用Activity页面与唯一在途证据读槽，发送同一activity_context，完整核对原引用和空attribution；不冒充LSM事件、不进入工具归因／反馈或原事件导出。Ask可显式转到该原件读取，取消／离页释放原读槽，不保存回放历史或自动查询引用。

`tool_reference_candidate`只沿后台明确的原引用投影，在同一操作链内列出全部候选；其原件缺失保留缺口。跨exchange/source的候选结果不再套确认关系的同域校验，原ToolResult的ChatContent独立显示，未取得的call不阻止展示已取得的result。确认执行及反馈仍用原强校验；候选窗口只按后端原message_refs顺序读取，不将后来上报结果添入冻结窗口。现有wire／状态／静态组件用例验证这些消费边界。

活动与Ask的显式候选比较复用现行AttributionRequest／ToolAttribution，普通原事实读取仍使用空归因。具体参数、单位、分页和查询身份由[审计技术](../middleware/audit/technical.md#前端消费参数与在途状态)唯一维护；新控件不另写条件算法。

### group（group.tsx、group-state.ts）

- load/first page/next：group_delegations 的 before/limit，核候选与缺口，空页禁 next。
- A2A parent/child 和 receipt actor node：各自本地详情、return 还焦、original summary。
- detail open process：processes/query/get 使用点击节点的完整 key；Get reread/return 由进程页场景核对。
- card/detail open receipt exchange：chat_messages 按 `scope.model_http.exchange` 传原 receipt.exchange、after:null；不查交换列表猜目标。return、离页/无内容授权/晚回执不恢复正文。

节点三项ProcessKey逐值核对，同PID不同birth的child Get与actor Get分别核原完整key；详情只含本页同完整key的原行。卡片及详情的“完整原观察”逐值对照同次GroupRecord，原回执展开对照解码后的命令ID、页及内容；Gateway的原引用、独立工具ID、null、空字符串、数组空项、0、false和大整数沿lossless JSON保留。该展开不读取SQLite中的TCP／WS原字节，receipt入口也只定位原HTTP交换；原wire及特定工具定位的能力边界仍由Group唯一契约定义。软件供给场景不能代替真实服务节点或跳转验收。

`tests/live/group.spec.ts`沿现有live fixture只操作当前真实Group回执：逐行／节点核原字段、相关原行和详情返回，按原完整key Get并保留not_found，卡片／详情各按原receipt交换读取聊天；离页返回只读取一次当前页，原行不在页内即记NOT_OBSERVED，不找替代对象。显式刷新一次，只有原游标存在才读下一页，无游标核禁用；空样本及未出现的类型／阶段／内容分支各记NOT_OBSERVED，不轮询取绿。语言与宽度由现有live六个project提供；环境就绪和唯一原业务由Root持有，spec不启动／提交／停止VM或Agent业务。

### chat（chat-history.tsx、chat-session.tsx、chat-state.ts、chat-message.tsx）

- all exchanges：session:null；exchange 首屏/page size/next、每个 exchange row：核 chat_exchanges 与 `chat_messages.scope.model_http` 的原引用／游标／limit。
- Session directory summary：首次展开 chat_sessions；refresh/next、source/original source summary、每个可选 session row：按来源化 ChatSessionRef 选择。模型字段的字符串／数值原类型分开；原生 Session 行直接读取该真实会话的 NativeGateway 消息页，不构造模型交换。另有显式未关联原生观察入口。
- messages 首屏/page size/next、return：核来源化 ChatMessageCursor；原生续页保原 reference 和 read_through，首页刷新才读新截止。关闭只本地清资格；旧回复不得重开。`gateway-wire.ts` 共用原件／连接引用校验；`native-chat-wire.ts` 与 `chat-wire.ts` 严格消费[业务日志唯一类型](../middleware/business-log/technical.md#聊天读取类型与分页)，不解释原 Agent JSON 或重算归属／顺序。
- `ChatHistoryMessage` 只拆来源化 header：模型仍用 OriginalChatMessage，原生按后端给定的顺序段显示各观察来源／角色、原窗口及缺口，正文共用 ChatContentValue。空串、JSON null、精确数字与工具原角色保真，不选较晚观察当最新正文。 同一后端消息项的可读角色与正文全部相同时只显示一份正文，全部原观察保留在默认折叠的来源中；不同正文分别显示，不在前端认定哪个是最新修订，也不改变消息身份或入库关系。
- 页离开/换 session/目录翻页/unauthorized 清下游正文；无 chat_content 时所有内容查询禁用，目录仍可展开查看无授权说明。

### settings（settings.tsx、settings-state.ts）

- read configuration、refresh diagnostics：configuration/get、protection_overview；saved document/paths/reply summary 保真。
- edit、四个 checkbox、advanced edit/textarea/discard：本地草稿；invalid JSON 显错误。Flow及凭证绑定的结构校验复用egress-wire同一解码器，ID按原16字节数组传输，hex仅作展示；不将错误类型发给后端，也不复制后端业务合法性校验。preview/back/cancel/Escape 不写。middleware 变更 acknowledgement checkbox 解禁 confirm。
- confirm save：configuration/update 原 expected_version/document；saved 版本与 applied:null/required effects 分开显示；reload/cancel/confirm reload：configuration/reload，非偷偷保存草稿。
- 三类 runtime enable/disable/resume：各自本地确认/取消、最终 control 的 feature/enabled；回执才显示实际状态。ReadOnly/ManageRules 不显示写入口。
- 写入未确认、断线/离页后回执：不自动重放，显式读取恢复观察。

高级编辑尚未预览时取消，重开完整JSON须等于原快照且无update；已有操作回执在编辑器旁的独立section展开，pre逐值等于本次供给的原result，折叠不请求。

### egress（egress.tsx、credentials-state.ts）

- read/diagnostics/edit bindings link：共用 configuration/get、overview、settings 路由；原 flow scope summary 本地。
- read CA、ensure/cancel/confirm：private public_ca/ensure_ca，核原 ca。download 仅当前配置绑定的 CA，核 DER 文件和文件名。
- secret input、prepare import、cancel/confirm：本地检查后 import_secret；rotate checkbox 动态增加 ref/revision、use saved receipt、prepare/confirm rotate：replace_secret 原身份/修订。非法/空输入不发送，真实秘密不进入公共事件附件。
- binding select/review issue/cancel/confirm：核 issue_phantom 的原 binding 与 expected_config；receipt scope summary 展示当次范围。
- once value input/readOnly、reveal/hide/copy/close：仅本地；核剪贴板成功/失败提示、关闭/离页清值。只用合成供给，报告不保存该值。
- token ID input、use issued receipt/review revoke/cancel/confirm：核 revoke_phantom 原 token。无 ManageService/撤权后无 private 请求；失联不重发写入。

合成场景在配置读取中及完整配置已取得两种现有布局分别操作本地输入/预览/取消/已签ID；不为布局新增产品条件。签发范围按发命令时的typed binding/routes逐字段核对，包括完整ProcessKey、容器原namespace及目标路由；读取新配置期间仍核原签发范围。普通控件/接口附件只存身份及动作元信息，不存秘密或一次值；合成供给不代表真实private效果。

## Ask完整窗口与候选控件

Ask沿原choices、timeout、DecisionId/EventKey答复；各页覆盖、规则对话框让焦/恢复、原期限和一次答复继续复用原Ask组件。普通上下文阅读、取消本次等待和重读分别核原命令；取消不取消后端SQL或原操作，晚回执不恢复旧读槽。

完整原窗口按后端message_refs展示全部原角色、parts、修订、空值和缺口，复用OriginalChatMessage。原短摘要的选择器与不完整消息供给不再作为此功能正控；窗口供给复用共享完整ChatMessage，再按本场景保留原ID和源方向。

显式候选身份、时长和字段映射由用户选择，不补默认窗口或参数名。现行候选→20条原窗口→原交换→返回原Ask→答复工作流沿同一个Ask spec和Peer检查，不另建测试框架。测试同时核原查询、多个候选不升级确认、工具结果/空内容保真、展示收起不取消原Ask及原选择仍可答。原接口/结构与生产消费边界仍归[审计三文档](../middleware/audit/architecture.md#界面消费职责)。

该20条供给逐位置展开消息引用、完整消息及各tool_call/tool_result的提供方引用，以生产JSON解析对照同次供给的实际值，再折叠并核无新请求；窗口整体引用另核。此合成场景不证明真实窗口具有相同长度、角色、part或DOM位置，文本非空也不能代替原值相等。

当前原操作节点的单份原件按data-stage=operation定位，pre逐值核同次question SourceRecord及Event/Question引用；不借同名stage或confirmed工具关系。逐路径对照只使用本轮成功场景中的实际DOM身份、路径与动作，其他布局单列；原hidden/disabled/readOnly观察不由可用态动作抵消，全部live状态独立保留。

## 同连接内容授权撤销

当前 Main 的 ContentAccess 只检查同连接 Authorization.chat()，没有按 Session、Group、Ask 或 Event 的资源级 ACL；唯一授权出处见[Main 消息](../backend/main/messages/technical.md)和[WS 首次鉴权](../middleware/technical.md#首次鉴权与真实底层授权)。ReviewSession 的现有 chatContent 表示本连接当前有效的内容读取资格。

只有本连接、仍登记在当前内容读槽的原回执返回 unauthorized，才将该资格改为 false。聊天目录/交换/消息、Group、Ask 上下文和活动证据同时清除已读正文与在途消费资格，保留实际收到的拒绝回执。离页只清页面状态，不恢复该资格；重新鉴权建立新资格。旧连接、取消或离页读槽的晚回执不能撤销新资格。各页面不另设跨页授权门闩。

此转换不关闭 WS，不修改 Access、review_availability 或原 Ask 答复能力；管理查询、private 或规则拒绝以及 not_found/conflict/invalid/unavailable 不触发内容撤权。原 authenticated 帧不改写。控件测试应逐入口验证跨页清理、拒绝保留、晚回执无效和重新鉴权恢复。

软件供给沿`tests/fixtures.ts`唯一Peer记录现行stream命令，场景显式返回订阅／退订ack并核原命令ID及subscription_id；未知封套不吞掉，Peer不自动确认stream。内容撤权交叉场景中的Ask初次选中和Activity日志选择直接核原上下文查询，不再将选择当成纯本地或要求另点读取。

## 运行与证据

标准入口 `npm run test:browser`，Playwright `webServer` 启动实际 Vite，`page.routeWebSocket` 在导航前截获合成回环 WS。官方方式见 [webServer](https://playwright.dev/docs/test-webserver)、[WebSocket mock](https://playwright.dev/docs/mock#mock-websockets)。锁定 `@playwright/test` **1.60.0**，仅 devDependency，Apache-2.0，复用其同版本 playwright/playwright-core；依赖审计以安装时 `npm audit` 实际输出为准。

六个 project 复用同一组功能场景：ja-JP/zh-CN/en-US × 1280/320 CSS px，Chromium，高度900。不是六份复制测试。失败保留 trace/screenshot；每个用例附接口事件和从实际 DOM 取得的 button/link/select/input/textarea/summary 清单（隐藏控件也标明），不记录输入值。清单记录观察状态与实际交互，未交互为 `NOT_RUN`，禁用为 `blocked`；清单不是通过率。新动态控件必须在所属场景加入操作和可见结果断言。

JSON语言目录沿[Node 24标准import attributes](https://nodejs.org/docs/latest-v24.x/api/esm.html#import-attributes)使用`with { type: 'json' }`；测试共用生产translate/idLabel，不另建shim或语言目录读取器。

测试 TypeScript 的 Node 24 API 声明锁定 `@types/node` 24.19.2（MIT，仅 dev），用于 fixtures 的事件附件与下载文件核对，不引入服务运行依赖。`npx tsc --noEmit -p tests/tsconfig.json` 独立检查浏览器测试和共享供给；Playwright 负责执行场景。

每个实际执行用例保留 `dom-controls.json` 和 `interface-events.json` 文件并附在 HTML 报告；DOM盘点按最近的模块/dialog/header/nav标出scope，页面从真实main标题引用对应唯一导航表取得，不用控件创建时的route冒充owner。字段组、动态条件、当前label、owner内实际DOM路径及readOnly/hidden/disabled和实际DOM原因分别保留，每种观察状态有自己的动作和焦点记录。`node tests/summarize-controls.mjs test-results` 按每个project及模块/页面汇成 `control-inventory.json`，同标签不跨模块或project抵消；固定shell/gate/overview控件按实际DOM位置保留语言/状态文字变化，业务控件按模块、区域和字段上下文独立对照，全部原观察状态可回溯。`OPERATED`只说明DOM输入/激活事件；焦点观察、只读、隐藏和禁用分别记录，不代替spec中的参数/可见结果断言，不输出单一百分比。summary 的业务读取单独显式操作；共用 disclosure 步骤只处理纯事实展开。

### 真实服务验证边界

`npm run test:browser` 的合成操作与 `npm run test:live` 的真实功能证明分别记账；入口和参数见 [Dashboard README](../../ui/dashboard/README.md#真实服务控件联调)。live 复用锁定 Playwright、生产导航／翻译／严格 decoder 和共享浏览器步骤，只观察真实 WS，不安装 Peer、不注帧、不导入历史库。当前源码 hash、git HEAD 和编译产物随运行自动记录，外部冻结 manifest 仅供监督比对。控件按 project、模块、区域、fieldset、角色／part、条件和路径核对；disabled、readOnly、hidden／closed 分开保留，同标签和其他状态的动作不能抵消未观察分支。

live启动前读取显式fixture内唯一selected-delivery.json，与owner_status的选定交付整体逐值核验，并检查service摘要格式和实际kernel绑定；不保留固定旧摘要、候选列表、自动定位最近包或失败回退。owner总体启动phase只记原值；AS管理沿同实例真实WS、鉴权及DriverStatus核实，Gateway整体ready不作为其前置条件，实际VM／通道退出沿原owner报告。

真实认证材料从本次0600文件由程序直接使用，模型不读取。认证／private 原帧只在内存核验，落盘仅操作／ID／status等非秘密元信息；不采集 gate／egress DOM 清单，关闭 trace、HAR、video 和自动截图，异常不含参数原文。egress本地操作只记固定控件身份、DOM路径／flags、动作及内存比较结果，不记输入／一次值；DOM本身没有的可选属性不制造undefined字段。业务原帧、步骤、DOM清单和派生原件统一存入本轮`live/private-evidence/`（目录0700／文件0600），可能含业务原文及受限材料，不作为普通报告附件或输出到模型；不按字面内容拒收／改写原件。无法安全分类的无效帧仅记解码失败及未保留，不能声称已有完整原件。采集器失败独立记录计数、目标artifact及错误类型，不能阻断答复或把未落盘引用记成已保存，不记录可能含参数的异常原文。

执行输入清单同时记录生产src与实际live测试源码。长任务开始后保持其执行输入不变；修正测试器时保留原运行、已加载源码／转换产物及中断事实，新源码与续验另绑输出，不能以当前文件反称先前进程已执行修正。仅从原帧取得的历史Question可作为正式只读查询参数，不向新浏览器重放成活动Ask；已关闭弹窗的原路径缺口如实保留。

准备、工作负载和 VM 退役复用[既有 owner](../../scripts/mvp-smoke/README.md)，UI 命令由控件触发，测试端仅作准备与独立复核，不放宽规则、原期限、Origin 或自保。按原连接／CommandId／DecisionId／EventKey 分别核保存、采用、显示和实际效果；若已有原`kernel_wait`见证，则独立校验等待、syscall字段及首return，不能用AS decision或文件未变替代，也不能为可选见证阻塞Ask答复。先及时答复每个实际自有Ask，正常业务终态后再读深层原件；不猜测模型文案、工具错误形状或Ask数量。ToolCall编号／角色样本缺失记NOT_OBSERVED，正式确认桥缺失记LIVE_BLOCKED，候选不升级确认，缺桥不阻止普通HIPS/UI。浏览器／HTTP／Actor由各自owner关闭并核退出，root沿原retire.py实际wait VM／AS／Gateway；运行原件留忽略／外部证据目录，不是产品状态来源。

live不设置启动、控制RPC、业务、UI准备或退役测试截止；等待观察实际page/socket/HTTP/Actor退出、诊断和原owner错误，测试器失败／中断与产品原回执／效果分别记录。HTTP沿原`npm run build`及锁定Vite的`preview()/close()`管理真实寿命；Playwright1.60的webServer把timeout:0当默认60秒，不能用其伪称无限等待。实际端口／Origin由当前owner显式交付；不接管旧实例。原AS规则期限及超时动作保持不变。

Group、Session等正式查询成功但返回空数组时，只记录本次样本NOT_OBSERVED，不能称功能未实现或正式来源缺失。具体交付与原环境证据仍按各模块owner及实际产物绑定核对。

运行前 source `$HOME/.local/share/agentshield-ci/env.sh`。本地安装先确认并移除副本 node_modules 复用 symlink，再在副本 `npm ci`。不把外部 `/tmp` 原帧或真实服务设为运行依赖；合成供给只引用本仓库现行 decoder/type。现有单元与i18n用例持续保留，包含后续归因和Ask完整窗口回归；浏览器检查不能替代它们。Playwright报告、test-results、browser-results均忽略入库，测试源码保留。执行记录只归监督verification/交接，不在tests下另立验收状态文件；设计不能推定实际批次通过。

上述合成浏览器检查只证明UI消费契约，不是普通MVP真实模型／Agent工作流的前置矩阵；AS本身网络等待不增加截止，反馈无Expiry状态，用户原Ask期限独立保留。当前候选先完成已授权正常实跑，未执行／未取得分别记录，不当作失败或补0。

既有控件整合与其实际执行范围见[控件整合证据](../evidence/dashboard-controls-integration.md)，不以设计条目、DOM数或历史通过记录替代当前运行。

## 开箱凭证设置与稳定展示

出站页BindSavedSecret只消费同一ConfigSnapshot中已明确配置的processes／container workloads及现有路由、实际SecretRef回执。AllProtected或Host范围不生成猜测进程／容器身份。表单组合原CredentialBinding，调用原ServiceSettings配置更新；实际持久化、校验和采用仍归原后端owner。没有第二配置缓存、Agent API或秘密副本。签发／撤销仍走原private接口；幻影Key持久可重复使用，清除本页展示不等于撤销。

聊天订阅只标记当前证据读槽失效。后台重读同一选择及归因条件时，保留该槽现有成功结果引用，匹配新回执后替换，不先清空已显示内容；不另存回放快照。显式重读、选择变化、取消、离页、错身份及权限撤销保持原清理和晚回执规则。源数据未取得仍如实显示缺口，允许／拒绝不等待上下文SQL。
