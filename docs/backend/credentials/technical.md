# 凭证模块与幻影key

状态：凭证模块已实现于`crates/backend/as-credentials`，公开契约位于`as-types::flow`；端到端接入和验收结果另记，不以组件通过代替真实导流效果。模块独占秘密材料、phantom映射、作用域判定、CA签发材料、轮换和撤销；只消费[公共值与端口](../../contracts/flow.md)和配置模块提供的已验证设置／固定路径，不导入Flow、Main或驱动实现。

## 使用模型

OpenClaw只持AS生成的随机phantom值；AS在受控TLS出口验证真实连接主体和目标后，将其替换成provider真实认证值。phantom不是OAuth JWT、不是驱动授权票据，也不能单凭它访问任意目标。持有phantom仍须满足绑定的ProcessKey或ContainerInstance及route。

参考Secretless Broker的“客户端不持真实秘密，按每个HTTP请求取凭证并注入”模式；参考Envoy将取凭证和header注入分离的接口。AS只支持明确AuthSlot，不采用任意模板、任意query参数注入、未匹配URL直通或secret缺失时继续转发。

## 模块初始化与私有存储

```text
CredentialInit {
  protected_root: NativePath,
  store_file: NativePath,
  ca_public_file: NativePath,
  bindings: Arc<CredentialSettings>,
  routes: Vec<ProviderRoute>,
  config_version: ConfigVersion
}
CredentialModule::initialize(input: CredentialInit) -> async Result<CredentialModule, CredentialError>
CredentialModule::use_port() -> Arc<dyn CredentialPort>
CredentialModule::certificate_port() -> Arc<dyn CertificatePort>
CredentialModule::admin_port() -> Arc<dyn CredentialAdminPort>
CredentialModule::apply_settings(bindings, routes, config_version)
    -> Result<ConfigVersion, CredentialError>
CredentialModule::shutdown() -> async Result<(), CredentialError>
```

所有路径只借配置模块同一ConfigPaths。以protected_root通过openat2(NO_SYMLINKS)打开根，逐层mkdirat/openat建立或检查固定子目录并保留credentials目录FD；后续读取、临时文件、rename与目录fsync均相对该FD，不重新通过路径跟随祖先符号链接。根和中间目录拒绝非预期所有者／组或其他用户可写，credentials目录0700、秘密文件0600，文件拒绝符号链接／额外硬链接；同目录.lock排他flock拒绝第二个存储所有者。临时文件同目录fsync后rename，原子可见后发布内存快照；目录fsync失败返回PersistenceUncertain，同时保持内存与已经rename的文件一致，不能返回旧快照。真实值不进入普通as.yaml、规则文件、环境、argv或通用日志。该选择复用成熟文件型secret provider和项目既有原子保存方式，不增加Vault／数据库／新后台服务；不宣称离线磁盘加密或抵抗已控制宿主root。

私有快照至少包含：store_instance、持久revision、SecretRef到秘密修订／PrivateBytes、PhantomTokenId到随机token摘要／BindingId／启用状态，以及CA标识／证书／签名材料。文件编码属于本模块，不作为中间层DTO；写入代码必须显式处理私有字段，不给秘密类型派生Debug或普通Serialize。

读取型接口只返回非秘密元数据。内存使用zeroize::Zeroizing管理显式暴露与释放；这不等于mlock、不可换页或任意内存攻击防护。TLS签名使用rustls SigningKey／CertifiedKey与成熟证书生成库，不自行实现密码算法。

## phantom签发和匹配

phantom为可重复使用的持久凭证映射，不是一次性口令；没有按请求次数消费或自动轮转。用户按需签发新值并撤销旧值。前端清除本次签发返回值的展示不等于撤销，真实秘密轮换保留未撤销的phantom映射。

签发使用既有安全随机源生成256位随机材料，编码为固定前缀as_ph_加base64url，不从真实key、PID或时间派生。只有token摘要和对应绑定持久保存，原值在成功创建时向已鉴权管理调用者返回一次；目标配置使用该值，真实key仍在本模块。token是敏感凭证，普通列表／日志不回传它。

签发前要求绑定存在、启用、SecretRef可用及其route有效。BindingId只决定持久规则选择，实际使用仍逐请求检查：真实IngressFacts满足主体范围，当前route仍匹配，目标peer通过HTTPS校验、method／path匹配，指定认证位置只有一份合法phantom。非phantom真实凭证、重复认证头、错误前缀、错误scope、撤销token都返回明确拒绝。

首版认证位置为Authorization的Bearer值，或x-api-key的单值。秘密值禁止CR／LF／NUL；导入时不静默trim或改写实际key。拒绝在URL、查询、body中搜索替换或将任意字符串模板执行为认证规则。没有额外HIPS匹配池，普通Allow不越过凭证scope。

## 授权、轮换和一次性租约

authorize_header只读取不可变已加载快照；查找、作用域核验、当前revision及撤销状态在短临界点确认，返回单次AuthHeaderLease。秘密只向已核VerifiedUpstream对应的真实发送者借出，用后释放；不授予前端或聊天解析器取密方法。

token撤销／秘密轮换由单一异步writer串行持久保存，磁盘工作在阻塞任务中，授权短锁不等待I/O。rename前失败保持原快照；rename后目录持久性未确认按上文报告。整个写入及发布由TaskTracker保管的owner任务持有，调用者取消不放弃半途事务；shutdown先拒绝新授权／管理，再排空已有owner任务。配置更新复用传入只读策略，短临界点发布；不另建策略源。替换真实key保留SecretRef及未撤销phantom映射，新授权用新revision；已经授予的旧请求按原租约收尾。需要立即停止的连接由主模块调用实际停止接口，不伪造旧请求已撤回。

同一进程／HTTP2连接并发调用各自拥有use id和租约，不共享可覆盖的“当前key”。认证头租约只持本次发送所需材料，不为聊天过滤复制秘密／phantom。无效输入／失败错误只包含非秘密原因码。

## CA职责

CA材料由本模块独占，公有证书可导出，签名私钥不提供读取接口。首次部署经ensure_ca生成并持久化一份CA，取得CaRef再启用中介；重启复用，缺失／损坏返回失败，不自动换CA。首版不提供在线CA轮换。叶证书只为当前已配置route的精确origin签发，证书有效期一天，CA身份／origin缓存最多256项、每项最多复用一小时；签发不持缓存锁。CA材料缺失但公有证书存在或材料损坏均失败，不自动替换。

CertificatePort返回证书链和不透明签名句柄，Flow只用于握手。上游使用正常根信任及主机名校验，与AS给客户端签发的CA分开；不能将AS CA自动提升为所有上游可信根。用户／部署系统把公有CA安装到目标实际信任库，AS不从宿主证书存在推断Docker／Node已信任。

管理入口复用Main首次鉴权和ManageService，不新增开放取密HTTP服务。普通配置更新只改非秘密绑定；真实秘密导入、轮换、phantom签发和撤销通过本模块管理端口受控写入。组件测试只使用临时目录和生成的测试秘密／CA；真实provider key和部署信任由管理入口按用户部署配置提供，不修改目标容器配置。


## 授权与存储边界

授权逐项检查当前binding启用状态、实际ProcessKey／ContainerInstance、route、method、原始编码path、认证位置及token摘要。IngressFacts的ConfigVersion保留原始来源；无关配置更新不会使旧连接自动失效。绑定删除／禁用后新授权立即拒绝。issue_phantom则对expected_config严格检查，并在持久提交后再次复核，配置变化不返回新token。

VerifiedUpstream仅可由已完成的rustls ClientConnection产生；公共构造器用同一正常根信任快照和具体WebPkiServerVerifier，对实际peer链针对指定origin主机再次验证，不能把另一个主机已完成的握手改名作证明。不接受配置true或客户端DTO声明已验证。认证头应用使用set与sensitive语义，租约消费一次；apply完成后不返回脱敏租约。

私有存储上限32MiB、秘密最多1024项、token最多8192项、单秘密最多16KiB。真实key保留字节，不trim；空值、CR／LF／NUL以及HTTP头不合法的字节拒绝。存储JSON只由本模块显式编码，秘密类型不派生Serialize／Debug；临时编码缓存和JSON中的秘密字符串显式清理。不承诺库内部所有副本或离线磁盘已加密。
