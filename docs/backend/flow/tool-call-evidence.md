# Tool call解析与命令核对的证据

本页记录设计阶段已核源码结论，区分协议支持、可核对数据和AS供给边界。第三方代码观察只证明所列固定版本的事实，不定义当前AS采集实现、逐Agent适配方向或真实正常业务的前置。以下公开OpenClaw引用固定提交058133da525b6a26ede908902565e519f74a2a47，不能冒充后续实际安装包。后续R13真实实例及2026-11-09安装源码审计另见[反馈证据](../../evidence/rule-feedback-integration.md#普通openclaw自动关联)。底层仅提供[公共事实](../../contracts/flow.md)，比较和业务关联由中间层执行。

## 结论一：LLM的tool call可以解析

[OpenAI函数调用规范](https://developers.openai.com/api/docs/guides/function-calling)给出结构化函数名、调用ID与JSON参数；Responses的call_id、Chat Completions的tool_calls[].id及返回tool_call_id分别按实际协议保留。[Anthropic工具规范](https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview)也明确提供结构化tool_use，应用执行后再回tool_result。流式参数必须拼接到完整单元后解析，不能把一段arguments文本当最终对象。

当前AS的[protocol.rs](../../../crates/backend/as-flow/src/protocol.rs)与[observe.rs](../../../crates/backend/as-flow/src/observe.rs)已按已选profile解析工具名、原生调用ID、完整参数及其来源记录，沿[公共事实契约](../../contracts/flow.md)发布原始记录与聊天投影。流式参数尚未完整时保留片段，不伪造完整参数。只从自然语言中猜“模型想执行命令”不属于这项能力；解析tool call也不证明对应系统操作已经执行。

## 结论二：OpenClaw exec参数进入真实执行链

已读以下实际代码链：

- [bash-tools.exec-run.ts](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/agents/bash-tools.exec-run.ts)：execute接收toolCallId和args，经参数处理后把params.command／workdir及可选execCommandOverride传入runExecProcess。
- [bash-tools.exec-runtime.ts](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/agents/bash-tools.exec-runtime.ts)：实际执行文本为execCommand或command，构造spawn输入并交process supervisor；后台更新确有sessionId、pid、startedAt、cwd字段，但不能外推为每种前台结果都携带pid。
- [bash-tools.exec-host-spawn.ts](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/agents/bash-tools.exec-host-spawn.ts)：形成[shell, ...shellArgs, commandWithShellSnapshot]；PATH前置、shell snapshot及指定执行覆盖会改变最终命令包装。
- [shell-utils.ts](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/agents/shell-utils.ts)：Linux bash分支使用--noprofile、--norc、-c，其他shell有自己的参数。不能固定假设所有命令都是sh -c。

因此“对比命令行”可作为具体核对方法：保存LLM原始command，另保存实际exec目标argv及cwd，对明确包装路径比较相应参数。不能先把argv拼成一条字符串再比较，也不能把shell最终派生的每个子进程argv直接等同于最初command。

## 结论三：命令相同不构成唯一调用身份

两个不同call_id可以携带相同command；它们生成相同argv时，字符串比较只能得到“命令一致”，不能区分属于哪次调用。这是信息不唯一的反例，不是本机实测失败记录。

源码还给出两种确定限制：execCommandOverride／shell包装能使最终argv与LLM参数不同；[write工具默认实现](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/agents/sessions/tools/write.ts)直接调用文件写接口，没有“一条write工具调用必然对应一个shell命令”的关系。read／write路径、HTTP请求等应各自按真实操作数据核对，不能强制转换成命令行。

该研究区分Parsed、CommandEqual、CommandDifferent、NotComparable和Ambiguous几种论断；这些术语不是当前业务DTO或新增产品功能要求。没有记录不等于命令未执行，CommandEqual也不能升级成ExactExecution。底层不输出这些比较结论，只提供原生参数、argv事实和来源。

## 结论四：OpenClaw已有额外的工具生命周期事实

[工具开始处理器](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/agents/embedded-agent-subscribe.handlers.tools.start.ts)实际发出runId、toolCallId、工具名、args及phase=start；[结束处理器](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/agents/embedded-agent-subscribe.handlers.tools.completion.ts)发出对应结果／错误。[Gateway转发](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/gateway/server-chat.ts)把工具事件送给具备tool-events能力的已登记接收者，并有session.tool通道。

这些已有事件是中间层可接的应用事实来源，不要求把相关算法放进内核或改OpenClaw源码；本轮仅证明固定源码具备这条通道，未连接实际Gateway。事件存在不等于任何客户端自动获权或无丢失。runtime result、provider线上参数和内核结果保留各自出处；不能把应用自报pid直接当ProcessKey，容器PID和宿主PID须有真实映射。

[tool-call-id.ts](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/agents/tool-call-id.ts)还有ID清洗／重写逻辑，所以各来源原ID必须保留，不得在AS中再归一化并谎称始终同一个字符串。应用提供明确映射时中间层采用映射，否则保留来源差异。

## 结论五：AS提供的系统事实及边界

[as_core.c](../../../drivers/linux/as_core.c)的actor_arguments读取current->mm的arg_start／arg_end，供给的是actor已有argv；[进程树scan.rs](../../../crates/backend/as-process-tree/src/scan.rs)读取已运行进程cmdline并保留参数边界。这两项都不能冒充本次exec的待执行目标argv。

基线e4e069020的缺口已在本轮代码补齐：原Execute采集点填真实bprm目标argv，复用[ProcessData.target_argv](../../../crates/contracts/as-types/src/dispatch_event.rs)，没有增加CommandEvent或命令字符串别名；截断／读取失败不填假完整数组。[driver_image.rs](../../../crates/backend/as-policy-engine/src/driver_image.rs)继续对无对应能力的平台明确拒绝采用。本轮真实隔离QEMU已证明正式target.arguments规则可区分允许／拒绝，具体范围见字段验收记录。

成熟HIPS已有直接实现：[Linux v7.0 TOMOYO domain.c](https://github.com/torvalds/linux/blob/v7.0/security/tomoyo/domain.c)的tomoyo_dump_page读取bprm->mm的参数页，[condition.c](https://github.com/torvalds/linux/blob/v7.0/security/tomoyo/condition.c)从bprm->p、argc逐参数扫描。所读[v7.0 mm/gup.c](https://github.com/torvalds/linux/blob/v7.0/mm/gup.c)导出get_user_pages_remote。该证据支持“从已复制到执行目标mm的参数读取”，不支持把syscall入口用户指针快照无条件当最终执行参数；AS实际供给范围仍以自己的字段验收记录为准。

当前回调／树提供真实字段，Flow提供LLM原生tool字段及消息上报；[脚本化中间层](../../middleware/smoke/technical.md)连接现有消息／聊天端口。本研究讨论过按实际包装核对及读取OpenClaw生命周期的可能来源，尚未把它们接成正式通用工具桥，也不将其设为当前开发或普通业务运行前置；真实实例结果只按其冻结候选原件认定，不由固定公开源码推定。普通文件Write等尚未实现的HIPS操作仍按原能力范围处理，不借这份文档宣称已全面覆盖。

源码文件及SHA索引在/tmp/as-tls-egress-design/sources.json；本页协议／OpenClaw结论来自已读固定源码；本轮AS字段代码另完成隔离QEMU内核验收，未执行真实模型或Docker/OpenClaw验收。
