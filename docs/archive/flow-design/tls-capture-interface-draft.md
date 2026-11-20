# 归档：被动TLS来源接口草案

基线2f0b6b4e2；未实现，不是当前DriverSession接口。进入对应来源模块开发时再明确。

## TLS来源窄接口（待实现）

本节是TLS来源的唯一公开初始化入口，事件／状态只依赖[流量公共契约](../../contracts/flow.md)。来源实现仍属于本适配，不导入Flow、消息、配置模块实现，也不让内核引用底层MainContext。

```text
TlsCaptureInput {
  subjects: BoundedList<ProcessKey>, profiles: BoundedList<TlsProfileId>,
  max_chunk_bytes: u32, max_call_bytes: u32, attach_deadline_ms: u32
}
DriverSession::start_tls_capture(input: TlsCaptureInput, output: Sender<CaptureEvent>)
    -> async Result<TlsCaptureSession, FlowError>
TlsCaptureSession::replace_subjects(subjects: BoundedList<ProcessKey>)
    -> async Result<BoundedList<SourceStatus>, FlowError>
TlsCaptureSession::stop() -> async Result<BoundedList<SourceStatus>, FlowError>
```

Main通过真实进程事实组合初始主体集合，后续生命周期变化沿同一session更新集合；不因每个进程新增线程或一套加载器。配置中的容器选择必须先经实例提供者解析为真实主体，绑定未实现时明确Unavailable，不能把Docker名称或裸PID直接作为授权。

input只接已验证数值和实际主体；适配负责查真实映像／ABI、附加及私有调用关联，不重读配置源。SourceStatus逐来源报告真实附加／退役结果，接纳集合不等于全部已可采集。停止仅释放本次TLS来源资源，不关闭DriverSession的HIPS控制链。具体hook私有约束由TLS来源文档维护；该文档不再定义另一套公共attach入口。
