# 2026-11-14 VirtualBox 实际导入与启动记录

本页只记录下述a518产物在嵌套模拟环境中的历史实验，不是最新镜像或用户VirtualBox的状态。当前分发入口及后续输入／保存问题见[交付状态](../delivery/conference-vm.md#当前分发状态)与[封装证据](conference-vm-build.md#幻影key输入与保存链的历史边界)。

目标仅为实际导入交付 OVA、启动并验证 AS／OpenClaw 图形界面及自行配置权限；未扩展模型／工具测试，未修改 AS、OpenClaw 或交付 OVA。

输入：`$HOME/Work/AgentShield-Conference-x86_64/AgentShield-Conference-x86_64.ova`，SHA256 `a518aaa85760cf880f9a0b7782243397ef9e672d03af8606047061af150efc8d`。验证副本核对同一 SHA。

实际环境为 ARM64 物理主机 → QEMU 8.2.2 TCG x86_64 测试宿主 → 官方 VirtualBox 7.2.18。测试宿主独占、使用新 COW 磁盘；宿主 ARM 内核及 Root broker 未改。VirtualBox 宿主模块实际编译加载，空闲 KVM 仅在这个隔离宿主内退出。

## 结果

- **实际导入 PASS**：VirtualBox 输出 `Successfully imported the appliance`，注册 UUID `414d14db-c714-441b-93ec-bccf54d0de03`。
- **启动未完成**：模拟 AMD-V 引擎在进入 Linux 前触发 `VINF_EM_TRIPLE_FAULT`。修正合成 CPU 的 AMD family/model 声明后该现象仍在。
- VirtualBox 自带 recompiler 引擎实际运行 BIOS 并读取硬盘，画面停留 `GRUB loading.`；观察期间未进入 Linux 桌面。
- **AS GUI、OpenClaw GUI、配置写入：本轮均 NOT_RUN**，不以先前 QEMU 的通过结论代替 VirtualBox 结果，也不能据此确认镜像在原生 x86 VirtualBox 同样失败。

因此该轮没有完成完整启动／图形验收，也没有原生Intel／AMD主机上的同产物通过结果。该轮未新建付费资源或运行远端CI；当时的机器请求与runner库存属于历史准备记录，不构成当前工作范围或新验证前置。

原件位于 `/tmp/as-virtualbox-actual-20261114/evidence`，逐文件 SHA 和分项结果在同级 `result.json`。原始导入、安装、AMD-V／软件引擎日志与最后画面已保存；验证虚拟机和隔离宿主已关闭，分发镜像未改。
