# 开发与验证

当前实现由根 Cargo workspace、`drivers/linux` 和 `ui/dashboard` 组成。模块入口见[代码导航](../crates/README.md)，字段及接口只在[契约索引](README.md#跨模块契约的唯一出处)定义。开发从 `master` 创建工作分支。

## 本地工具

Rust 最低版本由 [Cargo.toml](../Cargo.toml) 的 `rust-version` 声明，依赖固定在 Cargo.lock。Dashboard 使用 Node 24 和 package-lock.json。已安装的工具或历史测试结果不能替代本次环境检查。

项目维护机先执行 `source "${AS_CI_ENV:-${XDG_DATA_HOME:-$HOME/.local/share}/agentshield-ci/env.sh}"`；这只是该机器的工具路径配置，不是分发用户的安装依赖。离线缓存齐全时可加 `--offline`，缺工具应在本地解决，不借 GitHub Actions 补齐。

`AS_CI_ENV`可显式指定已有工具环境脚本；未设置时使用当前调用者的XDG数据目录或`$HOME/.local/share`，不自动安装工具或增加权限。公开文档中的`$WORKSPACE`表示相应源码／证据所属检出根，`$LEGACY_WORKSPACE`表示保管中的旧检出根，`$HOME`表示当前操作者家目录；这些只是公开定位写法，原文件名、内容摘要和运行结果不变，不修改本地原件或运行采集值。

## 现有检查入口

在仓库根运行：

```sh
cargo test --workspace --all-targets --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
bash scripts/smoke-main.sh
```

软件冒烟创建临时安装目录，验证配置、鉴权、消息与正常退出，使用明确的软件驱动供给；不能据此声称内核、真实 Agent 或镜像通过。MSRV 测试与实际用于 fmt／Clippy 的工具版本分别记录，不把缺失组件写成产品错误。

Dashboard 复用 [已有 npm／Playwright 入口](../ui/dashboard/README.md)。单元测试、合成 WS 浏览器测试、生产构建与真实服务控件验证分别记录。真实服务／模型调用由当前实例所有者沿现有流程运行，不为文档或合并重复创建实例。

驱动复用 [build.sh](../drivers/linux/build.sh)，使用目标内核自己的 Kbuild／BTF；[驱动冒烟](../scripts/native-smoke/README.md)与[脚本联调](../scripts/mvp-smoke/README.md)独立记录真实加载、原操作等待和效果。编译通过不代表已加载，不能把另一内核的记录算作当前通过。

## 合入与交付

按变更运行适用检查，保留真实失败及修正结果，不按测试数量计算开发进度。远端 CI 按验证批次和已有授权手动安排，不随每次 commit／PR 重跑；普通提交不应恢复旧主线的自动工作流。

源码、构建资产、运行环境和已分发镜像分别标明。镜像沿[唯一封装入口](../scripts/conference-vm/README.md)构建，按[交付文档](delivery/conference-vm.md)记录实际验证与未解决问题，不能以脚本返回 0、HTTP 200 或 QA 旧实例替代用户工作流通过。
