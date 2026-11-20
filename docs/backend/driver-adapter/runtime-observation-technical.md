# 原生请求观察源

范围见[驱动适配](technical.md#原生请求持续采集与工具边界)，公共原始结构见[原驱动类型](../../../crates/contracts/as-types/src/runtime_observation.rs)。原工具事实契约继续见[Flow](../../contracts/flow.md#模型响应与工具的事实交接)，两者不能互相冒充。

## 正式调用与所有权

进程树既有ProcessFactsPort::inspect取得真实ProcessBinding后，适配在原work任务读取该实例实际ELF并安装可用libuv点。原Execute/Create观察还会在该work上排实际inspect，不在唯一接收泵等待自身答复。目标由真实TGID、kernel birth、self_exec_id与当前executable inode/device约束，不按Agent名或私有栈选择。实际ELF在本次准备任务内按文件真实长度一次读取为自持有字节，object／gimli借用同一buffer；不再以File的随机小读反复扫符号。分配使用try_reserve，读取／分配失败按实际错误返回；不增加固定大小准入条件。buffer在元数据解释结束后释放，不保存目标堆或另建映像缓存；真实process／exec在下发前仍复核。 同一次准备只建立一份实际符号索引，供原libuv探点、v8dbg常量及具名getter读取共同借用；同名冲突、section范围、Text及对齐检查仍由原契约核验，不选最后值。索引与image同寿命，结束即释放，不新增全局缓存。libuv类型仍使用标准DWARF解释，实际布局冲突继续标缺字段；本批不引入编译单元地址过滤。

运行时元数据准备的可选诊断沿现有tracing记录pid／birth／exec_cookie、phase及单调elapsed_ms；不在回调运行时同步写stderr。阶段值metadata_started／metadata_ready／installed／gap分别表示开始读ELF、完成元数据读取、真实cmd17回执成功及实际错误；installed不表示已捕获Tool，不为普通HIPS增加就绪条件。诊断输出和采集原件分开，原callback／NativeFact仍按原接口消费，不因诊断关闭而删去。

同一运行时资源owner按真实PID／birth／exec_cookie去重准备；已完成ELF读取且没有公开探针符号同样是准备终态，不在每次进程查询时重新读取。该标记只记录这一exec实例的静态元数据检查已完成，不保存目标堆、瞬时寄存器、聊天或事件。实际读取／安装失败释放本次标记，后续原查询可重试；旧任务失败不能删除较新exec的标记。真实exec更换代次、exit按出生身份清理，会话退出随owner释放。没有安装探针不能报告installed。

安装资源按实际进程/exec寿命保管；只有原命令确认成功才保留，准备失败释放标记，实际exec/exit或会话退役释放。没有目标堆缓存/聊天回放。事实期即可安装；Initialize保留真实profile，旧source/request交接不能串到新源。停止和清理沿原驱动顺序。

缺DWARF、部分探点或未知工具语义不拒绝已有原始观察，不使inspect或HIPS启动失败；具体缺项报告，不能假成功。当前源在libuv提交入口一次采集原寄存器、API参数及当前TLS／Isolate／Frame技术投影；适配器只解释这份不可变原件。原submit/work/done/cleanup、API返回、实际child task及AS绑定继续沿同一来源交接。R12研究的PC/CB/LR筛选、Promise/Realm/Env和深图不进入该原始源，也不以HTTP身份或0loss作为采集门禁。

已有安全原始事实可先保留，由后续业务按接口取用；不因当前没有对应业务组合就删除采集能力。采集范围、原始来源和有效寿命仍须明确，缺失与实际读取错误分开保留。未经使用的语义解释参数不等于已经取得的原始情报。

## 安装命令17

### 公开共享消息格式的读取边界

用户已允许解释Pi等公开共享消息格式，禁止逐Agent分支。公开格式说明只决定字段语义，不提供目标堆偏移；实际布局仍由本次ELF／已初始化运行时元数据给出。不得使用OpenClaw名称、私有函数、固定承载位置、固定Node版本或llnode默认偏移作为后备。当前读取器已接入原生消息、适配及业务数据库；实际运行验证状态只在[证据](../../evidence/runtime-correlation-research.md#原专项量化点与历史排期)记录，不以代码存在代替验收。

三类操作共用运行时上下文读取，不能把FSReqPromise当成通用入口。已核的静态通用根是实际owner线程TLS→当前V8 Isolate→continuation Frame；TLS位移和Frame成员位置取实际具名getter数据流，禁止硬编码本次映像数值或调用会创建Local handle的getter。内核在原提交uprobe内完成读取和复核；回调后的适配器不再读取目标。R12的req.data→persistent resource→Frame仍是文件研究原件，不充当其他类别的既有生产实现。

Frame之后的最小读取范围是live ALS值→其直接data字段中的对象→公开Assistant.content成员。只解释公开type／role／id／name／arguments／content／responseId／provider／api／model；不按holder属性名选择，不追Promise、Realm、Env、闭包或prototype。工具对象须同时由当前store直接引用及助手content原成员引用，保留同对象依据；这只证明持有公开对象，普通ALS可保存旧工具，不能按唯一pair推定当前工具执行；仅有工具列表不能按唯一项或最近项选择。多组不能确定当前工具的关系保留候选。参数递归只处理已选工具的arguments。

字段结果复用NativeToolContext，不另建工具DTO。工具ID保留实际复合原值，provider／api／model只是公开消息中的标签，不能升级为真实TLS来源；实际ProcessKey仍来自驱动。根链、Map／table／descriptor／背板、选中字段及参数读集须在原回调返回前复核；短读、未知布局或不一致保留实际结果，不晚读目标补字段。当前调用／HandleScope保管和根链复核提供本次有效性，不能把libuv返回后的ClearWeak倒填为入口状态。

原上下文沿既有req/work寿命机械交接，实际EventBound再给出域1原事件。原件只沿MainMessage::RequestHandoff进入原业务日志，CAPI交接不自动生成ToolExecution，当前执行语义可信发布者未实现，origin FK consumer仅保留受控软件验证；独立工具消息和直写旁路已退役。HTTP发行作用域仍独立核验，公开消息解析的授权不允许只凭responseId相等或provider别名自动绑定ChatPartRef。[原件复核](../../evidence/runtime-correlation-research.md#公开共享格式授权后的窄读取复核)

三类原操作的交接须分别核实，共用原NativeRequestOperation及工具事实，不复制DTO／队列：

- 文件Open：owner提交的原req／work到实际worker syscall，不能在没有Isolate的worker补读owner上下文。
- 进程Execute：uv_spawn的原提交到真实fork／clone child task实例，再到子进程exec；裸uv_process_t.pid或父子PID不足以承担这段证明。
- 网络Connect：uv_tcp_connect在owner调用线程执行connect，完成通知异步；实际未发生的connect不能生成EventBound。
- 网络Send直接路径：公开uv_try_write／uv_try_write2在本次调用内尝试原buffers，不排队；由实际调用scope到实际Network.Send EventBound机械交接；它只证明原生调用，不证明现场公共工具对象正在执行，不按同socket或参数推定。其来源状态只活到实际API返回，不进入uv_req哈希，避免以stream地址冒请求对象。管道／TTY同API不被命名为网络，仍以实际LSM操作为准。[libuv公开同步尝试写契约](https://docs.libuv.org/en/v1.x/stream.html#c.uv_try_write)
- 网络Send排队路径：uv_write／uv_udp_send可能立即、排队、部分写及批量发送；按原请求／队列项／缓冲索引／实际批次位置到每次syscall交接，不以同socket、同TID或字节相同补绑。原生返回探点现覆盖sendto／sendmsg／write／writev／sendmmsg，只有实际接纳Network.Send等待才有返回事实。该外层scope不等于已取得libuv队列到原工具的交接，不能拿LSM保护或原返回通过代替ToolExecution来源。

沿原鉴权会话与64字节封套，不分片、不接受前端路径／任意地址读取请求。固定头144字节，小端：

- 0：host_tgid:u32、elf_machine:u16（ELF标准EM值）、probe_present:u16（仅bit0—11）。
- 8、16：原process birth、self_exec_id，各u64。
- 24：实际executable inode:u64；32、36：device major/minor，各u32。
- 40：十二个实际文件offset，各u64，顺序uv_fs_open、uv__work_submit、uv__fs_work、uv__fs_done、uv_fs_req_cleanup、uv_spawn、uv_tcp_connect、uv_write2、uv_udp_send、uv_write、uv_try_write、uv_try_write2。对应present位不置则不安装；0本身不是缺失。uv_write与uv_write2是独立入口，不能假设必然委派。
- 136：字段数:u32、零保留:u32。

随后ceil(count/64)个u64为字段可用bitmap，再count个u64值。唯一顺序由[as_runtime_fields.h](../../../drivers/linux/as_runtime_fields.h)声明：libuv result／work来自真实DWARF；V8具名常量来自该ELF的v8dbg；TLS／Frame及容器辅助布局来自该ELF真实getter的受支持AArch64数据流。具名符号／控制流／结果冲突只使对应字段不可用并报告，不选最后值，不调用目标getter，不使用版本或偏移后备。各bitmap字均按实际字段数校验，offset0由可用位区分。缺布局保留寄存器原件和实际GAP，不阻断普通HIPS。

内核核birth/exec/inode/device、实际exe/mm/file pin和offset<i_size。原work及公开API的return handler关闭对应线程活动scope；不能等主线程done才关闭。文件原req/work只按已观察活寿命保管；spawn由sched_process_fork将已证原request EventKey交给实际child task载体，到该子实例的exec／exit释放；不复制工具上下文，不继续借父profile／request裸指针。父进程exec／exit不撤销已完成的交接，也不向后代自动泛传。全runtime退役清全部child载体，source切换显式使旧key失效。作用域同时核原uretprobe consumer、原线程实例及SP；API返回不表示原I/O完成。排队write／UDP作用域只提供enclosing_call，不因此声明刷出的队列项属于当前工具。原安装失败沿真实errno返回；未安装不能上报成功。

## 原始交接事件0x10c

复用EventKey、NativeSyncThread、可选NativeRequestOperation与NativeRequestHandoff，不把uv回调当LSM File.Open或ToolExecution。observation使用同驱动source、独立runtime_sequence和域2；LSM原事件仍域1。request只在已取得原活寿命时填写，原地址仅为当次现场值，不能由时间或候选唯一补关联。

固定头72字节：observation EventKey(source16/domainu64/sequenceu64)；actor(tgidu32/tidu32/birthu64)；observed_boottime_ns:u64；presence:u32、phase:u32、operation:u32、reserved:u32=0。presence仅bit0=request、bit1=operation；request置位才随后带32字节原EventKey，不存在则无此段。operation未置位时固定槽是零保留，不能当实际类型。其后phase正文：

1. Submission：api:u32、projection_bytes:u32、projection_failure:u32、register_presence:u32、group_birth:u64、exec_cookie:u64、IP:u64、SP:u64，固定48字节。register_presence仅bit0=LR、bit1=FP，置位才按LR／FP顺序各带原u64。group birth属于进程，固定头actor birth属于线程，不混用。api=1 FileOpen、2 Spawn、3 TcpConnect、4 Write2、5 UdpSend、6 Write、7 TryWrite、8 TryWrite2。随后为对应API参数及projection_bytes个原投影字节。projection_failure=1仅表示原输出分配失败（此时无投影），0表示有投影；解释失败不改原始字节。
   - FileOpen：req／loop／path_address三个u64、flags／mode两个i32、callback:u64、path_status:u32、native_error:i32、path_len:u32、实际path前缀。status0 Complete、1 ReadFault（实际负errno）、2 Capacity。其余error槽为零保留，空路径保留[]。
   - Spawn：loop／process_handle／options三个u64；handle地址不是PID。
   - TryWrite／TryWrite2：stream／buffers两个u64、buffer_count:u32、reserved:u32=0；只有TryWrite2随后带send_handle:u64。没有req／callback参数，不补0冒充取得。
   - TcpConnect：req／stream／sockaddr／callback四个u64。
   - Write2／UdpSend：req／stream／buffers三个u64、buffer_count:u32、reserved:u32=0、send_handle或sockaddr:u64、callback:u64。Write没有send_handle槽，其他相同；Write2实得0仍是Some(0)，Write没有该参数为None。
2. Submit：原loop/work/work_cb/done_cb四个u64。
3. WorkEnter：原work:u64。
4. WorkLeave：原work:u64，不编造void返回值。
5. Done：work:u64、presence:u32、reserved:u32=0。presence仅bit0=result、bit1=result_read_error且互斥；置位才随后带原result:i64或实际负read errno:i32。0是实得值。
6. Cleanup：原req:u64。
7. EventBound：域1实际EventKey32、flags:u32（bit0原controller wait，bit1实际enclosing API）、reserved:u32=0；bit0置位随后wait_id:u64，未置位为DirectCheck且无wait段；bit1置位再带enclosing_call EventKey32。request仅表示已证原请求所有权；enclosing_call是外层调用现场，不能充作ToolExecution或反馈授权。
8. ApiReturned：实际C int result:i32、reserved:u32=0；0不是已完成I/O。
9. ChildCreated：实际child tgid:u32、reserved:u32=0、group birth:u64。boot身份由已鉴权驱动会话提供，不从PID再查询补身份。

### 一份技术投影与一次解释

技术字节的唯一TLV定义及字段单位见[as_runtime_capture.h](../../../drivers/linux/as_runtime_capture.h)。64字节头保存实得进程所有者、起止BOOTTIME、TP／Isolate／Frame及各自presence、实际errno和Complete／Partial／Unavailable／Inconsistent状态。字符串保留UTF-16实际单元（含未配对代理项），double保留原位模式；短读、容量、布局、变化、分配失败分别记录，分配失败不伪装成读取errno。已取得的PROPERTY元组先保存，公开字段选择只限制继续读取对象正文；字典的已读非data值保留原元组及property_kind，不调用访问器。描述符访问器尚未取得的value不补读、不填零。PROPERTY的representation与property_kind各自独立，0值不充作缺失。所有临时readset／索引仅本回调存活，复核后释放；直接写原消息的已有slice，不引入输出owner／引用计数或缓存。

适配器以Bytes接管原wire allocation，路径与投影是共享slice；不另深拷贝。原消息只解析一次，得到公共[CaptureAnalysis／SharedMessageCandidate／SharedMessageProof](../../../crates/contracts/as-types/src/runtime_observation.rs)。语义解释失败保留projection与Malformed；真实owner不一致保留原件与OwnerMismatch。局部容量／未知字段不会把已证一致的独立原子结构改成成功的完整参数。没有HTTP身份也保留所有已得事实。

中间层按实际request保留文件req/work、spawn的真实child实例、Connect／TryWrite原调用线程到事件的交接。公开消息形状及同对象依据始终为候选：普通ALS也可保留上一工具／助手，不能因一个候选或原生交接准确就升为ToolExecutionFacts。缺的是可信“当前工具执行”发布及原交接，不是要求采集更多字段或拒收原事实。排队Write／UDP在原队列项／批次交接尚未取得时只给出NativeCallScope，不选择当前调用，也不自动绑定ChatPartRef。SQL只保存一份projection BLOB及原analysis；已证工具事实的查询表示沿原Submission引用，当前无正式确认生产者，详见[业务日志](../../middleware/business-log/technical.md#原生请求交接原件)。

已取得多少就保留多少；缺Tool、metadata、HTTP、丢样或未完成重组不作为这些原事实的拒收条件。确认关系另核原身份和交接，不从原始记录存在就推定ToolCall归属、风险、成功或任务完成。适配沿DriverEvent::RequestHandoff到原MainMessage与业务消费者，权限、原操作等待及现AS scope/EventKey后半保持原机制。

RequestHandoff、RequestReturn、FutexSelection自适配已读取时起，按[唯一原件接纳契约](technical.md#必要结果接纳与普通观测)保管，不在events暂满时主动丢弃。Main从events取出后，在原Messages消费future中直接调用consume_native_fact，等待唯一业务消费者接纳；不转投完成／状态notices队列，也不另建搬运队列。消息接收端真实关闭明确返回失败。原子回调此前的采集损失仍各自报告；本项不声称补回已丢原件或保证全链零丢样。
