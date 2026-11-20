# ARM 原版 OpenClaw 进程问询真实验收

2026-11-16，Root亲自操作已交付ARM包的独立QEMU副本；AS服务SHA d1fea29a01c548bf6e619ae65d384d15c1a722188f7ffacd378f5a06e7e6fc55，Native SHA 43787d8ca323f28ab996a809d4d8df2237b458565fcd481f745ed0848eab822f。真实GLM Responses／AS幻影Key，目标Agent源码未改，无SDK／观察器注入。

AS全局模态在`#egress-trust`页面显示真实Execute `/usr/bin/printf`。人工选择后原提案为Allow、reason=user，原操作结果Allowed。原stock transcript session 2ef4aad6-e4c2-4715-be3b-afe7b19a27ea、run a99d8536-19b9-47a4-bbdf-34cd78d612c3中seq41／49的原toolResult，实际进程会话kind-dune返回AS_ARM_MANUAL_ALLOW、exitCode0、isError=false、timedOut=false；seq49是同一进程日志再次读取，不是第二次执行。该独立验收观察不导入为生产ToolExecution精确关系。

原件：`/tmp/as-arm-process-20261116/original-tool-results.json`、`manual-process-acceptance.json`、`manual-allow-click.txt`、`manual-allow-receipt.txt`和`manual-allow-result.jpg`。首次自动化从密码DOM读到遮蔽值导致真实认证失败；改用页面原生复制取得真实49字节幻影值。两个较早进程请求未及时人工答复，真实timeout Deny／工具126均保留，不重标为Allow。

来宾重启暴露Node home/cache与/tmp权限错误（EACCES），按标准目录权限修正后原版程序恢复；没有取消权限校验或改Agent。后续首次设置表单、Session代码和后台更新防闪是更新候选，不能把以上运行当作其完整真实验收；x86/Docker/VirtualBox亦分别记录，不套用ARM宿主网络域结果。

后续独立人工Deny补验未通过：原GLM任务实际发起新`printf`，actor TGID/TID1225，Native sequence695710，内核记录`unbound_wait`、elapsed30279044640ns、max_hold_ms30000；未观察到对应Ask，原工具返回126及`Connection timed out`。这是未绑定裁决前的来源等待超时，不能记为用户Deny。同期新GetStatus原回执Running、pending0、dropped9927468；只能证明控制查询仍响应，不能证明原操作抵达快速匹配。原独立SQL尾部仅有此前3条操作回执（最后294046的Allow），未取得本次完成记录。

源码核实原事件消费者把受控请求、辅助原件和关闭日志串在同一出口；必要辅助原件背压暂停ordinary，而未交付Controlled也在ordinary，存在阻断新请求进入统一Ask入口的明确可达链。本批收敛为原端点PRI承载Controlled／Reply／AS_RESULT、两个职责出口复用Main既有Fast／Messages线程，并将辅助事实从控制通知中继移到原辅助消费接口；后续真实新候选结果另记，不能覆盖以上失败原件。
