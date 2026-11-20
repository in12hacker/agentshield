# 实时HIPS与协议内存边界核查

历史限定：本页“当前AS实现／AS当前真实状态／后续约束”均指 2026-11-09 调研和 a1fca61ec 时点。下面整交换倍数预约、retained 池、去密与未实施项不作为当前设计或前置；它们的原判断保留。现行所有权与本地保真要求只见[Flow技术](../backend/flow/technical.md)，优化只针对 AS 自身实际额外工作，不推定 Agent 速度。

本页核查成熟源码和当前AS实现，支持[审计分层](../middleware/audit/architecture.md)；不以“实时”宣称无需内存，也不把保守预留数称实际RSS。

## 成熟实现的实际做法

[Linux TOMOYO supervisor](https://github.com/torvalds/linux/blob/028ef9c96e96197026887c0f092424679298aae8/security/tomoyo/common.c#L2236)使用实际query长度加终止符，经分配器尺寸取整后计入待询问内存，出队后扣除。这说明HIPS也管理有限在途请求，但此额度属于实际问询对象，不是LLM聊天解析额度。TOMOYO的具体超限处理不直接搬为AS规则动作。

[Suricata重组分配器](https://github.com/OISF/suricata/blob/8dbc5cc811cd9dff751a9f27481c1f1e40b4d0b6/src/stream-tcp-reassemble.c#L211)在实际calloc／realloc时核并增减字节占用。它是流式IPS的协议缓冲参考；异常策略中的丢包／旁路等行为不因此成为AS允许的默认动作。

[Envoy流控设计](https://github.com/envoyproxy/envoy/blob/70a60076aaa1fb8eb9444c45ee3ad5e5fb740193/source/docs/flow_control.md)与[WatermarkBuffer](https://github.com/envoyproxy/envoy/blob/70a60076aaa1fb8eb9444c45ee3ad5e5fb740193/source/common/buffer/watermark_buffer.cc)在写入／排空真实buffer时检查水位，高水位暂停读取或HTTP/2窗口更新，低水位恢复。需要完整body的过滤器与可持续消费的流式过滤器分开处理。它是代理传输参考，不是HIPS规则实现。

固定提交、实际源码及SHA位于`/tmp/as-memory-boundaries-research-20261109/sources.json`。这些资料支持有限缓冲、实际占用记账、消费后释放及背压，不支持“每个实时交换必须预留32MiB”这个通用结论。

## AS当前真实状态

驱动／规则／主调度的内部对象本来是EventData、RuleHit、OriginalOperation等类型化值；Flow公开队列已经共享Arc<ChatEnvelope>。没有把跨进程JSONL解码误称为同进程业务模块之间的重复解析。

已定位的冗余是：as-types/flow.rs的PreparedChatEvent::new先序列化完整JSON只保留长度；as-flow/lib.rs的remember再次序列化消息只取长度；observe.rs按累计parts逐项to_string仅为投影计费。真正发送JSONL时还会编码。这些是具体可清理的编码／分配开销，不应被当成底层事实生成必需步骤。

必要工作是：HTTP框架拆分、压缩解码、JSON／SSE协议解析、原生字段取得和跨片脱敏。半条JSON／SSE必须暂存；原字节与结构化投影的意义不同，不能为了少一个buffer丢弃出处。工具参数任意JSON的原值不得为了“结构化”改成猜测字段或默认值。

R14是解析预约与长期状态混用导致observer_capacity，见[原件与修复](capture-admission.md)。a1fca61ec已分池并原子预约双向额度，避免静默关闭和双向互等；它仍按最大值16倍计费，在默认32MiB下只有一个完整交换同时解析，过于保守。此实现是已验证的当前缺陷修复，精细在途记账／中间层JSON收口尚未实现，不能称为最终高吞吐设计。

## 后续收敛约束

JSON对外表达由中间层接入层负责；底层传不可变结构与源字节，移交所有权或共享Arc，不按订阅者重复复制。中间层不重复解析Flow已经提供的provider事实，不建另一份聊天缓存。

计费应跟随实际拥有的在途载荷、消息／原始证据保留及输出队列，明确单位和寿命，不用先生成JSON求长度，也不把逻辑载荷字节冒称精确堆占用。新的计费须在所属数据结构上实现，不增通用内存服务或另一份事实模型。

不能直接将当前倍数改小，也不能简单替成所有流都逐块等待同一硬池：如果每条流都持有半个不可消费的单元，会造成互等。完整单元需求、解压峰值、局部流控、取消和释放必须由同一解析所有者收敛。具体实现前须以实际结构证明边界，不为优化加入不明身份字段或改变HIPS动作。

本轮已完成边界与模型设计；上述编码清理、精细流式记账、正式中间层内存接线尚未实施。原操作决策和Ask答复不能等待这些业务处理。
