# AgentShield Dashboard

浏览器经现有回环 WebSocket 鉴权。防护概况、规则、进程与容器、活动与证据、Group、聊天记录、服务设置和凭证与出站信任共用同一连接。同步 Ask 在这些页面上沿原操作生命周期显示。

[前端设计](../../docs/frontend/design.md) · [架构](../../docs/frontend/architecture.md) · [控件/接口覆盖与测试边界](../../docs/frontend/technical.md)

## 本地运行

使用 Node 24 和锁定依赖。在本目录执行：

```sh
npm ci
npm test
npx playwright install chromium
npm run test:browser
npx tsc --noEmit -p tests/tsconfig.json
node tests/summarize-controls.mjs test-results
VITE_AS_WS_URL=ws://127.0.0.1:8765 npm run build
```

`test:browser` 由标准 Playwright webServer 启动实际 Vite，WS只供给合成契约帧。逐例附件保留按project与模块/dialog/header/nav归属的DOM控件状态、接口事件和失败trace/screenshot；`control-inventory.json` 是操作对照，不是控件验收百分比。测试直接复用生产translate/idLabel。运行产物由`.gitignore`排除，执行结论归监督verification/交接。

开发时可用同一 `VITE_AS_WS_URL` 运行 `npm run dev`，默认页面地址为 `http://127.0.0.1:5173`。服务端的 WebSocket Origin 使用实际页面地址。管理令牌通过界面输入，环境文件仅配置非敏感连接地址。[鉴权契约](../../docs/middleware/technical.md#首次鉴权与真实底层授权)

## 真实服务控件联调

先由[既有 native／WS owner](../../scripts/mvp-smoke/README.md)准备匹配产物及全新隔离实例，并提供当前冻结UI源码、`call-vm.py`、0600 `vm-access.json`、实际ready状态和本轮HTTP／WS端口。HTTP入口由本fixture使用已分配的127.0.0.1端口启动，服务端Origin必须严格一致；Gateway地址不能代替Dashboard Origin。未确认的服务包或旧UI不能冒称同源。

```sh
source "${AS_CI_ENV:-${XDG_DATA_HOME:-$HOME/.local/share}/agentshield-ci/env.sh}"
AS_LIVE_FIXTURE=/absolute/owned-fixture AS_LIVE_OUTPUT=/absolute/NEW-output \
  AS_LIVE_HTTP_URL=http://127.0.0.1:OWNER_HTTP_PORT/ AS_LIVE_WS_URL=ws://127.0.0.1:OWNER_WS_PORT/ \
  npm run test:live -- --project=en-US-320 baseline.spec.ts
```

[live配置](playwright.live.config.ts)顺序执行、无重试；fixture复用原`npm run build`和Vite原生`preview()/close()`提供HTTP并核实际关闭，不继承测试启动／业务／RPC／退役截止。缺显式资料直接报NOT_RUN，不回mock，默认`test:browser`不包含live。运行自动记录本工作树`src/`、`index.html`、`vite.config.mjs` hash、git HEAD和编译资产；仍须root核实其与冻结UI的关系。身份／协议字段归[技术契约](../../docs/frontend/technical.md#真实服务验证边界)。

- [共享浏览器步骤／观察](tests/browser-controls.ts)与[真实观察fixture](tests/live/fixtures.ts)不供给业务回执；业务原帧、step/ref、DOM及结束记录均保存在本次`live/private-evidence/`的0700／0600私有边界，不作普通附件。认证／private值只在内存核验，private元信息不含输入／一次值；字面内容不决定业务原件接纳，采集错误独立记账。
- [当前写操作场景](tests/live/review-workflow.spec.ts)：显式设置`AS_LIVE_GENERATE_WORKLOAD=1`及`AS_LIVE_BUSINESS_CONTEXT`，由本轮owner的公开业务上下文给出原策略快照、自有资源前缀及`cases`中的正常任务原prompt。只执行该对象明确列出的`file-allow`／`file-deny`，按键顺序运行；可仅列一个全新`file-deny`。单个workflow经真实UI创建自有write-open规则（mask2涵盖WRITE和READ|WRITE），逐项及时答复、等待真实终态并核原件；本批草稿／private专属回执检查在业务结束后执行，不依赖Allow。最后经UI删除自有规则／组并独立get恢复完整原策略。创建、草稿、替换恢复及退役步骤共用[现有规则helper](tests/live/review-policy.ts)，不安装测试Allow、不修改原维护／产品组。当前fixture沿既有Actor及case_baseline/case_finish供给；每项已提交业务拒绝重放，按原typed choices及实际按钮精确答复每个Ask，原20秒期限不变。写效果分别核原FD和当前路径的字节／对象身份，Allow核请求内容，Deny核原拒绝回执及实际保持，不用read不变代替。正常终态后同页核日中英320及必要1280，不为语言再发模型；实际工具／终态和AS聊天交换分别保留，不猜文案或数量。可选wait／栈缺失不抹去已有回执与效果。
- [持久原件读取](tests/live/persisted-originals.spec.ts)用显式`AS_LIVE_ORIGINAL_FRAMES`与同实例`AS_LIVE_BUSINESS_CONTEXT`定位真实Question／Completion，原对象只用于正式activity查询参数，不重放／注入Ask、不生成业务；活动、聊天消费后经同一UI规则helper恢复原policy及独立get核原configuration。[最终配置场景](tests/live/configuration-save.spec.ts)的权限写入需`AS_LIVE_FINAL_CONFIG=1`，保存未确认先独立读回，最后恢复；无变更reload只接受确切Completed／Configuration。[单独策略清理](tests/live/rule-retirement.spec.ts)复用同一[规则owner步骤](tests/live/review-policy.ts)，只删自己新增规则／组并核原文档。
- HTTP、浏览器和Actor分别正常关闭；VM／AS／Gateway仍归原唯一 owner，最终由 root 正常退役并核 SQL 原件。合成、真实接口、实际原操作效果和未出现样本分别报告，不给全覆盖结论。

## 数据来源

- Ask 沿原 `choices`、`timeout` 和期限快照显示。答复及完成使用原 `DecisionId` 和 `EventKey`。[问询闭环](../../docs/middleware/technical.md#原问询闭环)
- Group 展示已观察的调用和回执候选。进程详情按原完整 `ProcessKey` 查询。[Group 查询](../../docs/middleware/agent-graph/technical.md#正式查询)
- 聊天记录浏览已保存的原 HTTP 交换及其消息。应用 Session 按路由显式字段和真实来源筛选，Agent 身份仍需原证据。活动详情的请求原话由业务层核实。[历史分页](../../docs/middleware/technical.md#llm-聊天工作台) · [活动详情契约](../../docs/middleware/audit/technical.md#原驱动事件为锚的行为链)
- 活动详情从已授权的 Event 锚点读取原 `rule_feedback` 事实。仅展示与原事件及已确认工具调用一致的阶段。上游写出与原操作拒绝是独立事实。无记录时留空，原件沿现有证据下载保留。

### Group 接纳候选到聊天记录

OpenClaw 接纳候选的 `receipt: { kind: 'protocol', value: { exchange, sequence } }` 已引用原协议记录。用户明确点击「在聊天记录中打开此 HTTP 交换」后，页面使用其中完整 `exchange: ExchangeKey`（`flow_instance` 与十进制字符串 `sequence`）打开聊天记录，并沿 `chat_messages { scope: { kind: 'model_http', exchange }, after: null, limit }` 读取第一页。协议记录的 `receipt.value.sequence`、`call_index` 和 `output_index` 不作为消息游标或工具位置。原生 Session 则沿 `scope.native_gateway` 读取同库原生观察，复用内容组件；类型与固定读取截止只引用[业务日志契约](../../docs/middleware/business-log/technical.md#聊天读取类型与分页)。

跳转复用当前已鉴权的单一 WS、独立的 `chat_content` 检查、现有 ChatMessages 分页和唯一消息读槽，不读取或扫描交换列表来寻找目标。目标及读取状态只保存在原聊天状态；切页、关闭详情、退出及权限失败沿该状态清理，已失去读槽的晚回执不恢复目标或内容。之后可由用户明确读取交换列表。原完整 `ProcessKey` 的 Get 跳转保持原接口。

此入口只定位 HTTP 交换，不表示 Session 归属、具体消息／工具位置、子任务终态或父端回投成功。界面保留 ja-JP、zh-CN、en-US 及 320px 窄屏布局。

## 应用会话筛选

会话列表通过现行业务连接发送 chat_sessions {before,limit}，引用使用来源化 ChatSessionRef：model_http 指向原 ProtocolRecordId，native_gateway 指向原 GatewayFieldReference。目录分别展示模型字段分组的来源／交换数，或原生 Session 的实际发行域、会话字段／观察数及缺口；不互换两类计数或把 Gateway 连接当应用会话。Header 原字节与 JSON 原标量分别保留，空值、数字词法与采集问题不互相转换。

用户展开会话目录才读取；选择模型字段分组后，chat_exchanges {session:reference,after,limit} 筛选原交换，再沿原 chat_messages 读正文；选择原生 Session 直接沿 scope.native_gateway 读取同库原生观察，不构造 HTTP 交换。全部模型交换使用 session:null，Group 的原 ExchangeKey 直达清除会话筛选。目录、交换和消息各保留一页，切会话及目录翻页释放下游读取；页离开、连接结束和撤权清正文与在途资格。显示进程来源和应用会话字段，Agent 身份需单独的原证据。
