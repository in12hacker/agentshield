# Dashboard 控件批次整合

本批以当前归因／Ask完整窗口提交为基线，整合独立控件提交 `1ff50f94f` 的实际差异。使用交付清单校验的两份精确patch，不整文件覆盖旧分支；独立`docs/roadmap.md`修改保留。

## 代码与职责

- 共享focus守卫只封装原生dialog存在检查和DOM focus，页面继续拥有目标、时机、滚动和还焦关系；未增加焦点状态机。
- overview读取时释放旧事实，只保留实际像素高度，防止空读槽暂时缩短布局；新结果按真实内容布局。
- 规则作者操作选择与现行Compiler/DriverCapabilities一致：Process Execute、File九项、Network两项；完整事件枚举继续保真读取。可选namespace作者空白为None、非空不trim；不把通用事实的空字符串域扩成非法可写规则域。两种截断入口三语文案分开。
- 测试供给迁到一处纯contract-supply，浏览器沿标准Playwright入口复用原编解码、翻译与导航；Peer不实现规则编译、权限或数据库。

合入前后归因、Ask20、Inspect、状态和wire的受保护实现逐项摘要无变化。原164个静态单测声明全部保留，AST核对163个函数体一致，另一个仅增加原事实域的空字符串断言；归因与完整窗口的独立用例仍存在。证据及逐文件交叉审核为 `/tmp/as-controls-integration-cross-review.md`。

## 当前契约对齐

旧批次Ask spec还依赖已删除的短摘要控件，原assistant供给也缺完整ChatMessage字段。整合时在同一供给owner复用完整消息，再按该场景保留原ID/源方向；spec读取全部原窗口和长原文，没有恢复旧三条路径。

现有候选／20条窗口／原交换／返回Ask工作流沿同一Ask spec和Peer纳入回归。正控的响应消息含实际tool_call parts，quality、修订、观测时间、原事件目标和process身份彼此一致；没有用不可能由正式owner产生的候选供给冒称通过。该供给仍为合成契约，不宣称真实Agent事实。

## 本轮适用本地验证

实际入口先source `$HOME/.local/share/agentshield-ci/env.sh`。Node24环境下，隔离副本执行当前lock的`npm ci --offline`及依赖预检；随后运行既有`npm test`、`npm run build`、浏览器测试TypeScript与`npm run test:browser`。

- 303项Vitest、4项i18n检查、生产构建及浏览器测试类型检查通过。
- 标准浏览器脚本174个场景一次运行通过，覆盖三语及1280／320宽度，保留各场景接口和DOM原件。这是场景断言结果，不是所有控件覆盖率；未操作仍标NOT_RUN，不把观察状态改成成功。
- TypeScript原生语言服务核对共享焦点、可编辑操作、原归因匹配和完整窗口的引用，语义诊断为空。未安装独立TS LSP程序，不冒称该程序运行。
- 当前副本源码与工作树一致，受保护归因/Ask20实现摘要无变化，文档相对引用及Git差异检查通过。独立逐文件交叉审核无未修复阻断。

原件唯一索引 `/tmp/as-controls-integration.9trf1gg6/verification.json`。首次类型准备失败后误启动的浏览器批次已精确停止，原日志保留且不计入最终通过；本次最终批次在类型/单元/构建成功后才启动。

本批没有改Rust、驱动、事实源或IPC接口，没有重跑OpenClaw或远端CI；前轮R83只证明其冻结状态，不能推定覆盖当前整合bundle。新的浏览器结果只证明消费契约、控件行为和参数／回执，不证明真实Main授权或内核效果。runtime依赖未变，新增库均为锁定的测试dev依赖。
