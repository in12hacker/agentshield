# 普通与记住规则的单条管理验证

本轮在f4656c82de1bce96320d90995ad8db8351833bbb之后增加单条管理。范围限规则模块、Main消息组合及原软件冒烟；没有增加驱动hook、第二规则文件、内存授权库或接口版本。[规则技术契约](../backend/rules/technical.md#81-单条与组管理的唯一数据结构)是字段与行为的唯一出处。

## 已实现

普通及Remembered组共用Get／GetRule／Edit／Update／Reload。六种显式组／规则修改复用唯一update的版本复查、固定源绑定、校验、编译、原子保存及发布。读回、重读规则文件及重新初始化可见同一保存内容。维护组／规则的定向修改直接Invalid，不删除后悄悄补回；不存在为NotFound，旧版本为Conflict。Main查询要求ReadOnly，修改要求ManageRules，保存与真实驱动采用仍分别回报。

“回答时记住”仍未实现：Answer只处理当前原操作，没有新增草稿、范围确认或自动生成条件。手工CRUD不能当作这条产品链已完成。

## 已执行

环境为Linux 7.0.0-1019-nvidia／aarch64；运行前source本机CI环境并确认Cargo、rust-analyzer与Python可运行。

- Rust 1.89完整workspace／all-targets／locked／offline测试144通过、0失败，日志`/tmp/as-rule-crud-workspace-tests-20261109.log`。其中规则模块22项包含三类普通／记住项的保存、重读、重新初始化、版本冲突、类型／身份及维护保护。
- 全workspace严格Clippy、fmt、严格rustdoc通过，日志分别为`/tmp/as-rule-crud-workspace-clippy-20261109.log`、`/tmp/as-rule-crud-fmt-20261109.log`、`/tmp/as-rule-crud-rustdoc-20261109.log`。
- 原固定安装布局软件冒烟通过：当前消息编码／解码、记住项创建／查询／停用／删除、旧版本拒绝、只读授权及保存／模拟采用回执，日志`/tmp/as-rule-crud-config-smoke-20261109.log`。该夹具明确不加载内核驱动，不证明本候选的内核效果。
- LSP确认Main调用get_rule／edit、RuleCommand消费者及单条管理接口；正式Main与管理实现诊断为0。原软件冒烟的跨crate路径仍被rust-analyzer报告E0583，实际文件存在且Cargo检查、构建和运行通过；显式打开该文件后工具诊断仍存在，不把此工具结果写成全量LSP通过。记录`/tmp/as-rule-crud-main-lsp-opened-{references,diagnostics}.json`。规则模型的单独检查另有proc-macro未展开提示，不能冒称没有工具限制。
- 独立限定交叉审核未发现必修问题，报告`/tmp/as-rule-crud-review-20261109.md`；15个受审源文件精确SHA见`/tmp/as-rule-crud-review-source-20261109.json`，检查结束时全部一致。

## 剩余风险与未执行

当前新增入口未运行本候选的真实OpenClaw或内核VM验收。驱动镜像与消息写入复用既有正式采用路径，没有改驱动ABI。原OpenClaw聊天的R18使用单独冻结的f465候选及148文件源码，不覆盖这轮CRUD。没有创建PR或触发远端CI；本轮本地验证不依赖远端工具。
