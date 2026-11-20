# 无源码修改的运行时关联调研

**已退役的历史方案，不是当前开发指令。** 本页保存2026-11-09的来源核查、独立原型及当时接入建议。用户之后明确要求纯进程外旁路；提交 `73c1fd5da` 已记录退役运行时注入和逐Agent适配。下文的“采用”“接入所需工作”仅是当时提案，不得恢复为当前SDK、预加载或Inspector产品路线。当前边界见[运行时调研](../evidence/runtime-correlation-research.md#当前约束与结论)，历史原件与实验结论保留。

## 结论与采用方向

采用APM的启动时运行时插桩方式：AS提供Node预加载适配器，读取OpenClaw实际工具上下文，沿原生I/O请求对象传递执行标识；驱动只采集该实际传递事实，中间层据原证据产生关系。保留现有Flow作为唯一TLS协议解析／修改出口，不在插件再建规则池或另一条提示词写入路径。

这条组合方案根据以下成熟实现和AS代码提出，并非安装OpenTelemetry就有完整HIPS工具桥。需要修改AS的来源适配、驱动事件扩展和Flow接入；不改OpenClaw磁盘源码。部署需要增加目标Node服务的预加载启动参数并重新启动该服务，不需要因此重启电脑。首版不同时增加Inspector热注入路径。

## 已读成熟实现

- [OpenTelemetry JS零源码接入](https://opentelemetry.io/docs/zero-code/js/)通过预加载在应用启动前安装运行时插桩。它是接入方式的依据，不提供OpenClaw工具语义。
- [import-in-the-middle](https://github.com/nodejs/import-in-the-middle/blob/5a57b9143bc78f6b723c84800c2f0bf245e06750/README.md)提供ESM导出观察／包装及按模块限定加载。当前研究使用锁定3.6.0；未把该依赖加入AS生产工作区。打包进同一私有函数体且不导出的边界不能仅凭普通导出拦截器读取。
- OpenTelemetry OBI的[Node代理](https://github.com/open-telemetry/opentelemetry-ebpf-instrumentation/blob/3d083233bc9dda6856a5ee3cc1b64002ebae61e6/pkg/internal/nodejs/fdextractor.js)与[uprobe消费者](https://github.com/open-telemetry/opentelemetry-ebpf-instrumentation/blob/3d083233bc9dda6856a5ee3cc1b64002ebae61e6/bpf/generictracer/nodejs.c#L662)实际使用异步上下文及`uv_fs_access`侧通道。它的HTTP上下文不等于每个文件工作请求；其基于同线程或连接的通用关联策略不能直接搬成AS的精确工具关系。
- [Node24.22.1随附libuv文件实现](https://github.com/nodejs/node/blob/ce7aaf1cee78b8cf0ce17364260535c418d3ed26/deps/uv/src/unix/fs.c#L139)将实际`req->work_req`交给[uv__work_submit](https://github.com/nodejs/node/blob/ce7aaf1cee78b8cf0ce17364260535c418d3ed26/deps/uv/src/threadpool.c#L276)，工作线程的`uv__fs_work`接回同一个对象。这是跨线程传递的明确桥，不是按线程运行时间猜请求。
- OpenTelemetry [fs插桩](https://github.com/open-telemetry/opentelemetry-js-contrib/blob/fc3b13eb41e9512fb0d769b431583f90b5ef8326/packages/instrumentation-fs/src/instrumentation.ts)已有同步、回调、Promise接口包装；[undici插桩](https://github.com/open-telemetry/opentelemetry-js-contrib/blob/fc3b13eb41e9512fb0d769b431583f90b5ef8326/packages/instrumentation-undici/src/undici.ts)在真实请求对象创建时提取上下文并注入传播头。这两部分可以借鉴方法；普通观测SDK的错误忽略和兼容回退不能搬入AS裁决链。

固定上游提交、文件和SHA256共101项保存在`/tmp/as-runtime-link-research.s2x8o7uy/sources.json`；该清单的实际条目数以文件为准。未下载或执行整个上游产品。

## 当前OpenClaw真实接入点

依据实际安装的OpenClaw2026.10.9／build8b18b55。先前审计的14个源码文件SHA已逐一核对仍相同，原件为`/tmp/as-runtime-link-research.s2x8o7uy/openclaw-source-verification.json`。

1. `dist/tool-execution-context-BwYF5HLu.mjs`持有真实`AsyncLocalStorage`，导出`getAgentToolExecutionContext`和`runWithAgentToolExecutionContext`。`agent-core-jCWPqNtz.mjs:1139`把原`assistantMessage`、原`toolCall`交入，1397／1404／1430处围绕真实准备和执行调用使用该上下文。适配器可以在实际I/O调用点读取该已有上下文，不用在before钩子里设置一个全局“当前调用”。
2. `agent-tools.before-tool-call-BXRZ_7-l.mjs:3373`的插件事件明确传`runId`／`toolCallId`；[官方钩子说明](https://docs.openclaw.ai/plugins/hooks/tool-policy)与安装源码都表明after是观察、tool_result_persist同步。生命周期钩子可以补应用事实，不能仅靠before结束后的时间窗口给后续所有I/O贴标签，也不能在同步persist钩子里临时等待AS回执。
3. `tool-call-id-CBojAlOj.mjs`导出的`sanitizeToolCallIdsForCloudCodeAssist`按原消息／内容位置映射输入输出，同时维护碰撞和出现次序；`embedded-agent-helpers-Cbqwx6fd.mjs:461`实际调用它。应观察这次真实转换的输入和返回值，保留来源、原位置及old／new值；不在AS复制它的算法或按字符串相似建立别名。
4. 实际`@openclaw/ai`包也为2026.10.9，`dist/openai-completions-transport-D1cruFUq.mjs`导出真实`streamOpenAICompletionsRequest`；1677处把实际`chunk.id`写入`output.responseId`，3164处进入请求链，3185处处理原payload。Responses共享实现2951处也直接保存原`event.response.id`。这些是取得SDK响应对象与原生响应ID的真实边界，不能把`turnId`或墙钟当它们的替代值。

内部文件名和压缩导出名不是稳定公共SDK。正式适配必须校验当前安装产物及精确导出契约；不认识的构建明确不接纳该适配，不能按名称猜入口或回落到最近调用。底层HIPS规则仍照常独立工作。

## 已执行的独立原型

原件目录`/tmp/as-runtime-link-research.s2x8o7uy/probe`含未改动的自建应用、ESM上下文模块、AS研究预加载器、锁文件、GDB探针、原生跟踪及`result.json`。这不是OpenClaw实例，也没有加载AS模块或BPF程序。

环境是实际Node24.22.1／libuv1.52.1、aarch64及普通UID。成熟ESM拦截库取得实际上下文；侧通道给出显式set／clear；GDB仅跟踪自己的Node子进程，观察`uv_fs_open → uv__work_submit → uv__fs_work → openat`真实参数和系统调用返回。

12个并发调用故意使用同一个原工具ID和同一路径，但各有独立上下文对象。每个上下文在真实I/O提交点取得单独执行标识；12个标识分别唯一追到4个工作线程的实际openat。随后4次无上下文打开及先前1次建文件打开均不继承旧标识；总计17次实际打开成功，未余留请求。应用两个源码文件运行前后SHA一致。原型控制报文的0表示显式清除命令，不是公开事实字段的缺失哨兵。

这证明了当前Node二进制上显式请求传播和工作线程切换可观测；没有证明AS内核事件已带该标识、OpenClaw插件已接入、x86已测试或完整自动反馈已通过。GDB是研究观察器，正式产品不依赖调试器运行。没有用调试器下的耗时声称产品性能。

## 接入AS所需的最小工作

第一部分归底层来源适配：预加载器从原工具上下文读取原身份，给每个实际工具执行上下文对象分配标识；同一对象在准备／执行阶段复用该标识，不同对象即使call_id相同也不合并。标识由本次已核适配器实例及实例内单调序号组成，表示来源采集实例内的一次执行上下文，不是模型ID、PID或时间戳。原I/O提交时携带它；模型call_id原值另行保留。适配器不重解析LLM正文、不判断风险、不决定允许或拒绝。

第二部分归驱动：uprobe在真实提交／执行边界记录原生请求的生命周期和代次，工作线程只在执行该请求期间携带其标识；原LSM采集点将它和原EventKey、真实task身份一并保留。请求指针只作进程实例内、在途生命周期内的私有键，不作公共永久ID。同步路径、取消、提交失败、返回、地址复用、退出都要清除；缺任一环就没有该操作的关联事实，不继承旧值。

当前`as_hooks.bpf.c`只有LSM入口，`as_core.c:1536`只给LSM注册现有kfunc，不能声称uprobe可以直接调用它。建议在同一受控BPF对象中增加有界来源状态，由uprobe写、原LSM程序读，再通过当前LSM→Native调用参数保留原始来源信息。Native原操作等待及三类规则不另实现。该共享状态及事件扩展尚未开发／验证。

第三部分归Flow接入：在真实模型请求创建点保留每请求标识，并通过受控TLS本地元数据与原ExchangeKey绑定；对外上游不携带AS私有元数据。SDK响应对象和其实际ToolCall位置由该请求来源保留。这个原请求绑定不能由相同responseId、同进程或相同工具ID替代；重复／缺失时不能择一。其通信鉴权及与真实连接主体的核验必须沿所属底层端口完成，不增加匿名自报关系入口。

第四部分归中间层：只消费上述原始事实，组合原[EventKey／ProcessKey](../backend/process-tree/technical.md)、[ProtocolRecordId／ChatPartRef](../contracts/flow.md)和实际ID转换记录，建立既定`tool_execution`。Main只组合公开接口；进程树不保存每次工具／I/O的可变当前标记，不引入第二份进程树或通用关联总线。

第五部分才接现有反馈消费者。Flow保留原规则文本、原管理会话权限及真实写出见证。Chat Completions若要支持精确反馈，须先取得上述完整请求／转换来源，再补其正式定位与写入实现；当前Unsupported不能因安装了适配器就删除。已有Responses也不能跳过完整原事实检查。

## 覆盖边界与复杂度控制

先接已证明能沿请求对象传递的文件Open，再逐个接当前Process Execute和Network Connect／Send的实际原生执行边界，不一次铺全Node模块。`uv_spawn`的真实启动参数可以形成启动操作的来源；仅有进程父子关系仍不能把所有后代操作记为同一工具。OpenClaw的exec环境钩子会过滤NODE_OPTIONS等危险变量，不能指望插件自动向所有工具子进程传递预加载器。跨已有进程／远端任务队列、共享socket、合并发送、多调用共同产生一个syscall等情况，需要其实际消息／请求传递证据；没有就保留缺口，不选“最后一个调用”。本轮没有声称这些分支已经实测。

性能采用I/O边界的窄探针及私有短生命周期映射，不照搬OBI每个macrotask都做同步信号的配置，不逐事件查procfs或数据库。工具名、参数和路径不作关联键。实际开销及其他架构只有实现后才能量化，本轮不报猜测的工期、行数或性能数字。

运行时标记证明的是观测边界内的显式传递，不是被攻陷应用不可伪造的安全身份；标记不能授予规则放行或AS管理权限。内核原事实、应用运行时来源及中间层关系分开保留，缺来源只影响精确反馈，不替代或降级原HIPS裁决。

本轮只完成调研、独立机制原型及方案记录。没有改OpenClaw源码／配置、没有直接操作其验收、没有增加AS生产接口或更改现行规则语义。
