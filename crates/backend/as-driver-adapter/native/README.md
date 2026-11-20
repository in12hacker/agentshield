# 驱动适配的用户态原生组件

这些文件属于底层驱动适配，不属于内核驱动子类。职责及生命周期见[驱动适配契约](../../../../docs/backend/driver-adapter/technical.md)。

- `as_loader.c`：以libbpf加载、核验及持有实际BPF links；不匹配普通规则。
- `as_flow_loader.c`：持有本会话导流、监听、子进程及实际连接；不解析LLM或决定风险。
- `as_tls_ingress.h`：本适配内部FD接纳报文的唯一C声明；共享BPF值从drivers/linux/as_flow.h引用，驱动通用协议不传FD。

继续复用原drivers/linux/build.sh all构建入口，产物名称as-bpf-loader与as-tls-ingress不变。内核/BPF源码保留在drivers/linux，接口行为和原始采集范围不因目录迁移变化。
