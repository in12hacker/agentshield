# Dashboard 架构

[产品](design.md) · [技术](technical.md)

## 所有者与边界

[main.tsx](../../ui/dashboard/src/main.tsx) 组合单一 `ReviewSession`、鉴权 gate、全局 shell、当前工作区和 Ask。页面身份/锚点唯一取 [navigation.ts](../../ui/dashboard/src/global/navigation.ts)。`session.ts` 组合现有各业务 state 的读槽和连接资格；`wire.ts` 与各 `*-wire.ts` 严格消费现行封套。底层 MainContext 边界只在 [MainContext](../contracts/main-context.md) 定义。

各 `*-state.ts` 拥有本连接/本页必要状态；React 只持本地展开、选中触发器、未提交草稿和确认步骤。配置唯一由 `settings-state.ts` 保管，settings 与 egress 借用同一快照；凭证一次值由 `credentials-state.ts` 专属保管。聊天目录、交换、消息各只保一页及原读取资格。前端不增加业务数据库、规则引擎、协议别名或第二状态 owner。

## 公共组件

- `DashboardShell`/`LanguageSelect`：导航、主题、语言和全局控制。
- `ScrollPane`、`RawFacts`、`DetailField`、`ProcessKeyFields`、byte/json/download 工具：保真展示、滚动、原件和浏览器下载。共享显示不复制后端 DTO。
- `ScopeInput`、`ConditionsInput`、`ActionInput` 和 `RuleDetails`：组/规则表单、动态条件、容器、参数、动作和同一预览。
- `CausalChain` 展示业务层已提供的原事实和关系；活动原话预览使用 `ConversationExcerpt`。Ask完整原窗口复用 `AskAuditWindows`／`OriginalChatMessage`，活动与Ask的显式候选表单复用 `AttributionPanel`；字段与状态直接引用[审计契约](../middleware/audit/technical.md)，不在前端重新计算跨方向归因。
- `ConfigurationReadStatus`/`ConfigurationFailure`：配置读取和失败；egress 不另造配置入口。
- `global/focus.ts::focusOutsideDialog(target, options?)`：只在没有原生 open dialog 时执行一次DOM focus；settings/egress确认标题、group/chat/process/activity详情和同义返回复用该守卫。各页继续拥有触发时机、依赖项、原触发器、isConnected检查及滚动位置。规则详情先滚动再聚焦的组合仍归规则页；Ask/RuleDialog内部焦点和导航的延后聚焦不套用这个守卫，不建立焦点状态机。

## 生命周期

WS 打开后先鉴权，服务端 grant 决定管理/内容资格；问询可用性只在 UI 已挂载后声明。页离开撤销其读槽；取消读取只释放前端等待，不发送业务取消命令。回执必须属于原连接、读槽和原身份。规则/配置写入提交后的真实状态由后端拥有；未确认/断线不能自动重发或冒称回滚，重新鉴权后显式查询。Ask 答复/完成沿原期限与双身份。

## 测试结构

`test-support/contract-supply.ts` 是 Vitest 与浏览器共享的纯合成事实供给 owner，从既有 wire 测试迁移；不导入 React 测试、不启动服务、不改变生产解码。`tests/fixtures.ts` 只建标准 Playwright fixture：真实 page、WS 截获、事件附件、DOM 控件盘点和少量共享浏览器步骤。每个 `tests/*.spec.ts` 按功能走页面；场景直接用 Playwright locator/assertion，不增加 DSL、路由注册表或通用总线。

测试定位文案直接调用生产 `messages.ts::translate`，原ID显示直接调用 `wire.ts::idLabel`。语言目录使用标准JSON import attributes，Node 24/Playwright与Vite共用同一入口，不另有测试语言解析或ID格式化副本。

WS 回执由场景显式供给。fixture 不根据命令计算规则、配置或持久状态；只记录请求并将指定回执发回。所有测试只用合成 token/秘密/一次值；接口事件附件不收认证载荷和 private 值，DOM 清单不记录表单值。失败 trace/screenshot 来自隔离合成浏览器，不能指向真实服务使用。
