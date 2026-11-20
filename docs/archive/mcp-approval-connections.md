# MCP连接方式与系统层审批闭环调研

状态：2026-11-08用户已明确暂缓MCP方向，当前工作见[LLM设计](../backend/flow/design.md)。本页仅保留调查结论，不是当前实施任务；提到的DenyNotice等旧候选接口见[暂缓记录](flow-design/runtime-feedback-paused.md)。

调研日期：2026-11-08。本文记录当时的目标：不修改OpenClaw等目标源码，尽可能系统层runtime拦截一次MCP调用，交AS用户审批；允许才送达server，拒绝向原client返回唯一、明确的用户禁止结果。现有HIPS三类规则／问询继续复用。本轮为源审和一个自建Node载体检查，没有BPF闸门实验或真实OpenClaw验收。

## 闭环的必要语义

审批对象默认是一次tools/call，不以“批准连接／批准server启动”代替后续每次调用。审批期间server尚不能读取这次请求；AS保留完整不可变请求和typed id。client可以把分片写入AS真实保管区，这只代表字节被接纳；不能为等待完整JSON堵住第一个分片write，使client无法再发后续片段。MCP调用的等待对象是对应响应，而非必须冻结每个write系统调用。Allow只转交一次，Deny完全不转交并在原响应方向返回该id的错误结果。

这比事后解释某次syscall Denied更强，也不能用pidfd_getfd单独追加响应替代。未经前置保管，server可能已经执行且仍会自行回复；即使追加错误被client接受也不构成本闭环。旧“借写端补注”只保留为回传构件的研究，不再作为此需求的完整方案候选。

## 先按真实连接分型

官方[MCP transports](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)定义stdio和Streamable HTTP。stdio是应用协议，不代表内核对象一定是匿名pipe。必须辨别：

- **匿名pipe／FIFO**：普通write/writev经file_permission，可即时拒绝操作；没有完整消息buffer／可等待的BPF-LSM消息闸门。一次write可能半条／多条RPC，不能按syscall数审批MCP调用。SOCKMAP不接管pipe。
- **AF_UNIX SOCK_STREAM socketpair**：常用于Node/libuv子进程stdio。它属于socket，有额外的BPF sockmap机制可研究，不能沿用“匿名pipe无socket接口”的结论。
- **Streamable HTTP明文TCP**：按POST请求完整保管、GET/SSE反向事件处理。有成熟协议网关构件；系统层透明导流仍要解决原进程／连接绑定及激活时点。
- **Streamable HTTPS**：协议层闭环还需要合法明文和写回权；透明导流本身不解TLS，CA信任、pinning、mTLS和已有连接不能隐去。被动TLS探针不授予修改或保管能力。
- **旧HTTP+SSE及自定义传输**：OpenClaw当前公开源码存在此连接类型，不意味着AS应保留旧协议兼容。应先确认实际部署协议，再明确支持／Unsupported，不能静默将旧端点当Streamable HTTP。

## OpenClaw／Node的已核事实

只读固定公开OpenClaw提交`058133da525b6a26ede908902565e519f74a2a47`：

- [mcp-transport.ts](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/agents/mcp-transport.ts)区分stdio、SSE和Streamable HTTP。
- [mcp-stdio-transport.ts](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/agents/mcp-stdio-transport.ts)的send先serializeMessage再写stdin，接收端有实际分帧器。不能由一次库write推定一个系统write对应一条完整RPC。
- 该路径调用[owned-stdio](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/process/owned-stdio.ts)和[child adapter](https://github.com/openclaw/openclaw/blob/058133da525b6a26ede908902565e519f74a2a47/src/process/supervisor/adapters/child.ts)，后者使用node:child_process的pipe stdio；容器／其他launch路径须各自核实。
- [libuv v1.52.1 process.c](https://github.com/libuv/libuv/blob/v1.52.1/src/unix/process.c)的UV_CREATE_PIPE调用uv_socketpair(SOCK_STREAM)。本机仅自建Node v24.22.1／libuv1.52.1子进程，fstat(0/1/2)均socket=true、fifo=false，正常退出0。没有启动／检查真实OpenClaw实例；此证据只证明该Node标准spawn路径，不能覆盖本机OpenClaw实际版本／所有MCP server。

因此连接枚举必须记录实际内核对象类型、进程强身份、两段stdio关系和协议，而不是看到“stdio／pipe”字样就决定技术栈。实际OpenClaw载体仍只能由指定验收聊天按已有资源归属确认。

## 扩大范围后的Unix socket候选

Linux v7.0 [unix_bpf.c](https://github.com/torvalds/linux/blob/v7.0/net/unix/unix_bpf.c)包含Unix stream的psock协议适配；[sock_map.c](https://github.com/torvalds/linux/blob/v7.0/net/core/sock_map.c)接受已建立的Unix stream对象，允许研究runtime加入SOCKMAP/SOCKHASH。pidfd_getfd可在有权限时取得已有socket FD引用；它只是对象取得机制，不是排他接管。

拟议数据路径：原接收端→SK_SKB_VERDICT重定向→AS有界缓冲／JSON-RPC分帧→现有规则和单Ask→允许后回注原接收端，拒绝则原server不收请求并向client返回同id错误。BPF不解析业务语义，不睡眠等待人；AS保管请求。目标源码和原启动命令不必改，这是系统层runtime中介，与让用户包装MCP命令启动不同。

但**stock Unix sockmap不能据此宣称已经构成排他审批闸门**。源审发现：

1. unix_stream_sendmsg先把skb放入普通sk_receive_queue，释放queue/state锁后才调用sk_data_ready。
2. unix_bpf_recvmsg在普通queue非空、psock队列空时回退__unix_recvmsg。并发reader可在上述回调前读走原字节；verdict还没有机会裁决。
3. start_verdict只更换回调，不主动排空旧queue；map更新没有等待已经进入普通read路径的线程退出的统一屏障。一次查看queue为空不证明未来排他。
4. unix_stream_splice_read直接走unix_stream_read_generic，不能把recvmsg适配当成所有读取路径覆盖。
5. 非TCP的stream_parser＋stream_verdict组合在sock_map_link返回EOPNOTSUPP；Unix候选应研究无stream_parser的SK_SKB_VERDICT，把MCP分帧留底层，不谎称内核已识别整条MCP。

依据为v7.0的[af_unix.c](https://github.com/torvalds/linux/blob/v7.0/net/unix/af_unix.c)、[unix_bpf.c](https://github.com/torvalds/linux/blob/v7.0/net/unix/unix_bpf.c)、[sock_map.c](https://github.com/torvalds/linux/blob/v7.0/net/core/sock_map.c)及[skmsg.c](https://github.com/torvalds/linux/blob/v7.0/net/core/skmsg.c)。这是上游v7.0源码时序反例及未覆盖路径，不是已在本机复现的内核漏洞；发行版补丁内核还需核相同路径，不能据此宣称本机或现有AS已出现该问题。

INGRESS回注有独立正面依据：sk_psock_backlog→ingress_enqueue将已重定向数据放入目标psock ingress_msg，不再执行目标原verdict；可研究专用AS注入端避免循环。但它不修复原字节旁路。所读sockmap_listen自测实际入口只运行INET/INET6，不能当作Unix验收。

继续此候选必须先找到并证明合法的原生接收／队列屏障和完整读取覆盖。当前没有已确认的最小实现，不默认通过改内核函数表、内核补丁或让应用加配合代码解决。失败必须保留已有HIPS拒绝及明确能力状态，不能把“能redirect一些字节”发布为MCP保护。

## 其他路线的比较结论

- [MCP Guard固定源码](https://github.com/permission-protocol/mcp-guard/blob/91930537639319c9bbe27121ed3775526136da41/src/proxy.ts)保留待审批原请求、批准后forward、拒绝回原id，证明闭环业务逻辑；它通过spawn被代理server取得传输所有权，不能当作本需求的系统runtime附加方案，也不移植其observe直通／倒计时自动释放等策略。
- Docker／Microsoft MCP Gateway展示协议网关／会话路由，前提仍是流量经过网关。官方OpenClaw插件／自身审批属于应用集成；用户要求系统层，因此不作为默认答案。
- 系统层透明TCP导流＋协议中介是HTTP候选，目标源码不必改；已有连接的安全迁移、TLS条件及主体绑定必须分别证明，不把新连接方案外推为任意存量会话。
- ptrace或seccomp完全I/O模拟可研究，但不是把返回值改一下：需要线程、FD别名、read/writev、poll/epoll、部分传输、信号及绕行路径。seccomp不能无条件装到任意已有进程，CONTINUE还存在buffer TOCTOU。当前不选为低复杂度默认栈。
- [bpftime官方attach说明](https://eunomia.dev/bpftime/documents/attach/)提供Frida函数插桩和syscall text transformer；[syscall_intercept](https://github.com/pmem/syscall_intercept)提供另一拦截构件。它们可不改源码但会注入运行时／修改执行代码，不能冒称纯内核BPF。可研究按明确runtime/ABI支持的I/O适配器，加系统访问约束防直接绕行；仍需真实请求保管、read/poll语义及返回绑定，不是现成通用MCP闸门。此外bpftime所读usage文档明确标早期且不适合生产，syscall_intercept官方README注明停止维护；仅作为介入机制参考，不推荐直接变成当前生产依赖。用户未批准把运行时注入列为默认主线，本轮不安装／运行。
- ftrace／livepatch／file_operations私有替换属于另一层原生适配研究，版本／锁／资源寿命没有闭环证明前不宣称可行。file_permission、SIGSTOP、只复制管道写端都不足以单独实现本审批目标。

## 拒绝结果与避免重试

在具备排他CallGate后，对tools/call的用户拒绝计划返回合法MCP工具错误结果（isError=true），保持原typed id；明确文字为“用户禁止本次MCP工具调用，未交给server执行；未经新授权不要重试”。JSON-RPC协议错误与工具错误分开，不用临时网络故障／断线冒充用户拒绝。格式依据[官方工具错误规范](https://modelcontextprotocol.io/specification/2025-11-25/server/tools#error-handling)。具体OpenClaw版本怎样传给LLM必须由真实消费者验收，公开源码不是本机实际行为证明。

retryable=false可作为明确的AS扩展说明，但不是标准客户端必然遵守的控制位。不能保证仅一句话让任意LLM永久不重试。实际安全靠规则／审批状态：同一已保管请求只放行一次，重复帧／断线恢复不能复用授权；新请求重新匹配规则。若要后续同工具／同server不再弹窗，需要用户明确选择记住该范围，不能把一次拒绝暗中变永久全server禁止。

一次拒绝、工具级禁止、server级禁止须在UI和规则中有唯一含义；不能把不透明id或参数相近猜作相同请求。新id不绕过当前有效Deny规则。原client明确取消或传输已失效时，尚未转交的请求不能再放行；client取消与审批渠道断开是不同原因。原client仍有效的审批超时只用该规则原timeout动作，deadline固定，不能发虚假进度消息无限续命。取消／回答竞争同一次状态转移，开始转交后的部分结果如实报告。

## AS应有的最小闭环接口（未实现）

```text
McpCallKey { connection_generation, session_scope, request_direction, typed_rpc_id, as_call_sequence }
HeldMcpCall { key, immutable_request, digest, requester: ProcessKey,
              server_identity, rule_version, original_deadline, exclusive_gate_lease }
CallGateState = Capturing | Held | AwaitingReview | Forwarding | Denied | Finished
McpCallReceipt { key, approved_digest, forwarded_payload_bytes: Option<u64>,
                 outcome: NotForwardedUserDenied | ForwardedOnce | PartialOrUnknown,
                 reply_state: NotSent | Submitted | ClientConsumptionObserved }
```

server_identity必须是本地强进程／通道代，或远端已核origin／endpoint／认证上下文／会话代，不能只用serverInfo自报名。审批视图包含实际server、tool、去密参数及明确的隐藏字段标记，并绑定同一digest和规则版本。

exclusive_gate_lease只能由已证明的连接执行者产生，观察CaptureChunk不能构造它。Main组合端口，规则模块匹配现有类别，单Ask循环审批；底层Flow保管并写出，驱动只执行对象／路径约束。stdio不自动等于File：真正pipe归文件目标，Unix socket按实际socket类别映射，不能一条规则跨两池或偷偷增加第四池。

请求审批不是事后DenyNotice解释。OnlyObserve／无排他gate必须明确不具备本能力，不能依靠后续kill充数。无前端的既有快规则／跳过Ask语义不能被偷偷改掉；产品若要求某类调用必须人工批准，须在对应规则／profile明确无人可审时的终局处理，不把本设计示例当作已批准的全局变更。

下一步最有价值的工作是确认实际载体与排他截获点；Unix候选先证明读旁路能否被合法原生机制封住，再验证完整消息保管／唯一错误回复。原型不是大测试矩阵，也不改OpenClaw源码。真实OpenClaw验收仍由指定聊天负责，本聊天未直接操作该实例。

## 本轮证据

只读源码与hash索引：/tmp/as-mcp-connection-research/sources.json；自建Node检查：node-stdio-observation.json；独立内核源审：/tmp/as-runtime-sockmap-review/source-assessment.md。OpenClaw公开树查询有truncated=true，未声称遍历全仓；本报告只引用实际读取的固定提交文件。未执行SOCKMAP加载／重定向／竞态测试，未改生产代码或内核。
