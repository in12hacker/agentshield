# 运行中容器的动态接入

本页定义容器动态接入的产品目标。目标是由运行在容器 Linux 宿主机上的 AS 发现已有 Docker／OpenClaw，并从实际接入时刻处理后续行为。当前能力与剩余缺口统一见[底层清单](../gaps.md)，实测依据见[动态接入与重连](../../evidence/container-dynamic-reconnect.md)。

## 使用条件

动态系统监控采用目标原有的启动方式、源码、镜像和运行进程。AS 早于 Docker 启动时，容器入口在运行时和实例出现后接入。AS 晚于 Docker 和 OpenClaw 启动时，先核验已有对象，再跟随真实生命周期更新。已取得的对象引用在实际退出后退役，重建的容器使用新的完整对象身份。之前发生的系统行为没有当前采集证据。

当前 AS 的进程树已支持存量扫描与后续驱动观察。文件、进程和网络行为按已覆盖的实际 hook、原字段和现行规则处理。把这些行为精确归到 Docker 容器实例，使用默认运行时的真实元数据与当前内核对象绑定。

## 实例数据与所有者

复用 ContainerInstance、ProcessKey、Workload、原 cgroup／namespace 字段及 IngressFacts。运行时元数据负责完整对象 ID 和生命周期事实，内核描述符负责活对象与当前网络域。驱动适配持有和验证这些引用，Main 组合接口，进程树只保存经证实的成员关系，规则和凭证消费该实际关系。

发现通知只触发重新核对。具体接入须取得并核对运行时对象、当前进程寿命、实际 cgroup 及网络命名空间，处理读取期间的退出、重启和重建。PID、时间接近、目录名和容器名称各自只保留原含义。源不一致时保留未绑定状态，规则不使用未经确认的 Workload。

真实 OpenClaw 验收状态统一见缺口清单。此前隔离 runc 证明了运行后获取对象引用的 Linux 机制，其私有 state 文件不是当前采用的 Docker 生产接口。

绑定字段、验证顺序和出口交接由[驱动适配技术契约](../driver-adapter/technical.md#容器实例绑定)维护。

## 网络行为与 TLS 内容

系统网络事件可以从接入后的真实 hook 取得。原 socket、原进程和原命名空间分别保留其身份。容器受控出口还需在实际 netns 完成导流、接纳和上游连接，具体技术归驱动适配，TLS 协议解析归 Flow。

受控 TLS 采集只接纳入口准备完成并启用后的新连接。目标进程可以先运行，原连接的后续数据不因 AS 启动而自动变成可解密数据。客户端实际信任 AS CA 与 phantom 授权是独立条件，具体支持范围由[受控出口](tls-egress.md#支持范围与成熟依据)维护。

已有连接可以自然结束，后续新连接按当时有效的规则与已证入口处理。强断重连未纳入自动接入流程，原始试验和失败结论见[动态接入证据](../../evidence/container-dynamic-reconnect.md)。

## 对照与验收

Tetragon 对存量对象和运行时回调分别采用其来源映射。Docker Inspect、runc state 和内核引用已在[默认 Docker 隔离试验](../../evidence/container-dynamic-reconnect.md#docker-默认-runc-链的运行后绑定)核对 init 活对象。原 OpenClaw 聊天负责真实验收，必须保持同一目标进程运行，并分别核对对象绑定、规则效果、TLS 条件及退出。[Tetragon 存量关联](https://github.com/cilium/tetragon/issues/3381) · [Docker Inspect 实现](https://github.com/moby/moby/blob/master/daemon/inspect.go#L103-L153)

专用 runtime 的创建时交接实验只保留为已有机制证据。当前动态接入不把它作为默认启动前提。
