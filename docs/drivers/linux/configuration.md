# 驱动配置投影

所属大模块：内核驱动。产品边界见[驱动设计](../design.md)，组合和职责见[驱动架构](../architecture.md)，唯一入口见[驱动总契约](../technical.md)。驱动内部只读上下文见[DriverContext](context.md)，跨模块协议见[通信契约](../../contracts/driver-communication/technical.md)。本页定义本职责的唯一技术细节；不导入底层实现。

## 输入、校验和输出

DriverConfiguration只接收底层配置模块导出的DriverConfigImage及DriverContext，核验持久默认、原请求限制和固定文件自保投影，输出经验证的只读原生视图。ConfigDocument、as.yaml读取／写入、固定路径派生、ConfigVersion生成均属于底层配置模块，驱动不重复实现。

配置子类把已验证输入交[DriverSelfProtection](self-protection.md)绑定原生自保对象，不重新发现后端位置；绑定使用实际root、静态后端和加载器对象、已验证exec实例及受保护inode集合，不以路径字符串或裸PID假授权。文件自保规则的匹配统一交FileDriverRules，配置子类不拥有第二个文件池。

Group的三类来源端口由底层配置唯一读取、验证并在初始化固定，驱动只核[48字节原生投影](../../contracts/driver-communication/technical.md#持久镜像内容编码)。0只表示本来源未启用；启用值为互异的有效u16端口。任一来源启用都要求实际Group TCP采集可用。范围只约束宿主初始网络命名空间的IPv4回环字节采集，不表示协议已解析或关系已确认；协议解释归[Group模块](../../middleware/agent-graph/technical.md#公开-gateway-旁路来源)。热替换不得改变任一端口，不另建配置缓存或DriverContext字段。

## 临时覆盖结构

```text
DriverRuntimeOverrides {
    process: RuntimeOverride,
    file: RuntimeOverride,
    network: RuntimeOverride,
}
```

RuntimeOverride只复用现行Default／Enabled／Disabled含义；无自保关闭项。该记录由DriverCore的唯一实际状态保管，配置子类不存另一份。Set／Resume输入在这里验证，DriverCore在唯一提交入口调用结果并更新实际状态；消息子类不能直接改值。

普通持久替换保留提交时的实际临时覆盖；完整Initialize恢复持久默认，重连／查询／普通reload不清除。版本递增、恢复默认的含义及自保顺序只按[运行状态契约](../../contracts/driver-communication/technical.md#4-运行时与持久状态分开)，不在此再定义。

## 与规则一起采用

配置验证和普通规则视图准备都不发布候选。DriverCore核对完整候选、预期持久代、真实来源状态和两份输入后，短临界区一次替换有效快照；配置子类不独立更新active或发Applied。

DriverStatus由DriverCore从实际有效状态生成。配置类不读配置true填“已生效”，不拥有状态广播、通讯、日志或安装升级，也不持匹配过程中的临时事件。

## 当前原生采用

持久镜像的固定LE正文与尺寸只在[通信契约](../../contracts/driver-communication/technical.md)及其[唯一原生声明](../../../drivers/linux/as_protocol.h)定义。配置模块提供根、后端、加载器原路径；驱动不拼接这些安装路径。所需能力、默认开关、待决／时间上限及日志选择先整体核验，再建立临时自保栅栏和实际绑定，最后与普通规则一次采用。

Initialize／Replace携带expected_lifecycle_epoch；首片与提交均核对真实停止世代。每次有效Quiesce（含Dormant与重复停止）及Failure使旧候选失效，不能在停止回复后由迟发首片恢复Running。普通临时Set／Resume不推进这个栅栏，也不清除持久状态。

写关闭目录来自同一配置镜像，不另开控制口或文件源。真实目录绑定用内核fsnotify mark管理销毁／卸载寿命，关闭事实仍只由file_release生产；没有FS_CLOSE_WRITE第二采集器或用户事件fd。普通替换复用未变配置的绑定，实际对象退役后不按同名路径自动重绑；完整初始化才重新绑定。引用与编码直接见[Hooks](hooks.md#写打开与写关闭)及[驱动通信](../../contracts/driver-communication/technical.md#写关闭审计事实)。
