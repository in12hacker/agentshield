# Flow普通转义参数的可读性反例

本页记录冻结旧实现的公开可读性失败，不定义现行聊天／秘密／规则结构。原“保守脱敏／聊天公开可读性缺口”已退役；现行职责见[Flow本地保真](../backend/flow/protocols.md#本地保真边界)，字段仍归[公共Flow契约](../contracts/flow.md)。文件gate原操作效果、旧聊天字段失败及后续新源分别认定，原结论和 hash 保留。

## 已确认源码路径

冻结源码`661c5c16deda81b36e718e117bc7bd255cbcff81`的`observe::sanitize`对归一化JsonArguments检查反斜线；命中即写入encoded_arguments，不要求凭证租约存在pattern或发生秘密匹配。后续unit把该结果与实际秘密匹配合并为sensitive，并把本单元items及codec私有全部parts标为_redacted。结果是普通转义参数可以遮蔽同单元无关消息，原native ID及协议终态仍可存在。

## 已执行反例与合法正控

从上述git提交另建隔离源码副本`/tmp/as-flow-redaction-repro-20261110`，仅追加一个回归，调用原setup及Observer的feed／finish。当时已核Observer字节与该提交一致，未在原冻结候选中改生产代码或测试；后续实现变更不改这份原反例。普通工具参数只含printf及转义换行，不含假secret，参数也不以其待匹配前缀结尾。

同一回归得到三个实际结果：空租约、普通无转义参数时，原Observation和Finished parts遮蔽数均为0；仅加入普通转义后，空租约下1个原单元及3个Finished parts被遮蔽；租约含不匹配假secret时同样为1和3。测试成功编译，最终在公开可读性断言处失败，退出101；不是初始化失败或真实OpenClaw验收结果。

首轮失败日志`/tmp/as-flow-redaction-repro-20261110.log`保留；带合法正控的日志`/tmp/as-flow-redaction-repro-r2-20261110.log`，结构化记录及源码／日志hash见`/tmp/as-flow-redaction-repro-evidence-20261110.json`。只运行一条有具体目的的本地回归，不以测试数量计进度。

## 真实场景与修复边界

原OpenClaw聊天R22已完成：10项文件Gate有限范围PASS，新增场景聊天明文字段完整性FAIL。首项mkdir拒绝已取得同DecisionId的Ask、完成与SQL及真实EPERM，后续协议请求却整段Redacted；明文等待超时原件保留在`/tmp/as-openclaw-evidence-20261110/r22`。最终实际清点33条脱敏协议记录、10条整段client请求和63条带遮蔽消息；结果SHA256 `99a12f41f1816fc7860ab6bc0aa940fc090783223aa9967fd5c80411f36dfd89`。不从有限Gate通过、原生ID或完成状态补造被遮蔽原文及工具到内核的关系。

用户随后明确要求“禁止脱敏！我们是本地的，就是要保真”和“不要随意增加我没有认可的功能”。后续脱敏匹配器、元数据隐藏及内容中断方案已取消，现行目标是删除主动脱敏链路、原样传递和保存实际数据，见[当前契约](../backend/flow/technical.md#原数据保真)。旧FAIL、原件与冻结源码继续保留；本页不再作为继续开发脱敏方案的依据，新链路结果另记。
