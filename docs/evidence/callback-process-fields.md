# 回调与进程树字段补全验收

日期：2026-11-08。基线e4e069020。TLS出口／phantom本轮完成设计；本页只记录已实现的Native回调、事件解码、树与查询字段链，不宣称LLM／凭证运行时已经交付。

## 完成的代码范围

复用原有事件和规则链，新增或补齐目标argv、cwd、成功exec新argv、kind／exec_cookie、调用线程birth、PID／NET／USER namespace、TGID层级、保存／FS凭据、fork真实child属性与组退出码。mount namespace由真实/proc扫描供给，不硬编码内核私有布局。Self事件保留已有file flags／FMODE，umount保留真实mount root；网络增加公开API产生的socket cookie及socket自身netns，和actor的netns分开。

kind／cookie统一归ProcessInfo，删除树顶层重复属性；初始／最新视图及查询沿同一结构。父属性保留原样本stamp。线程属性补全同时核TID与task birth，不能跨线程或复用TID；成功空cmdline保留取得的空值；proc来源空值不用于推定原事件argc=0。固定格式同批更新，未加接口版本或兼容分支。

同步get_cached_process只读缓存；异步get_process／query(Get)仅对已有存活实例的缺失且有真实来源字段尝试一次补取，并写回同一进程树。复用现有扫描锁与任务保管，并发请求共用结果；失败及调用方取消后的读取结果同样记住，不随全局缺口反复读取。真实exec清除旧补取标记，显式刷新允许重新读取。列表、父链及快速匹配不增加I/O。成功空内容不触发补取，未增加推测字段或第二份事实缓存。

## 独立审核发现与修正

- Native审核发现长规则ID可通过采用校验却放不下Direct的64字节结果尾部；现校验完整封套及固定共同／类别预留，删除容量clamp。11452字节Process/File ID总长和15503字节Network ID总长为当前边界，边界加1及旧16084反例已在本地算术控制中拒绝；这项算术控制不是Root采用试验。
- Rust审核发现/proc空cmdline不足以证明原事件argc=0。用户随后明确成功空读必须保留空值：当前argv字段保存Some([])，字符串语义为""，单NUL保存一个真实空参数；规则补全不会把proc空值推成原事件完整零参数向量。目标／主体argv不混用，post-exec补新镜像仅限成功Observation。
- 最新Native与Rust只读代码审核均无剩余实质阻断。原报告保留发现及修复记录：/tmp/as-callback-native-review.md、/tmp/as-callback-rust-review.md。
- 单次补取审核修正了全局epoch变化使不同失败节点相互开放重试的问题，改为每节点Pending／Completed；并核对取消、exec竞争及旧扫描结果不能完成新镜像的补取。最终源码与r6原始回执经独立交叉审核合格：/tmp/as-process-detail-review.md、/tmp/as-r5-family-observer-review.md。

## 最终真实联调

最终原Root代理任务67c3567b0ea44aca9aa85abd5744a314完成，aarch64／7.0.0-1019-nvidia隔离QEMU。使用正式配置／PolicyDocument／Rules.Update／原生匹配和Main事件链，未在宿主加载模块或改安全配置。

同一独立inode的/suite/callback_probe按target.arguments[2]选择Allow与Deny。Deny实际exec返回EPERM且witness不变；Allow实际执行并写witness。该副本不是受自保bootstrap豁免的/as/bin/as-service，不能借豁免跑绿。

同一child两次真实exec，直接核原始Create／exec／Exit事件和树：kind、三个Native namespace、NStgid、real/effective/saved/fs UID/GID、name、exe、argv、cwd对独立/proc读取；cookie对独立Inspect及实际exec增量；组退出码对真实wait原值23<<8=5888。原始fork argv必须None，不用creator填它。随后通过异步详情接口补取活child的mount namespace，独立/proc核值，第二次查询复用同一Arc缓存，initial Arc不变；原始回调mount仍明确为空，不能靠扫描掩盖回调漏字段。

原有进程／网络32项真实允许／阻止／终止、文件问询／超时／无前端、自保效果、DriverTools、配置采用、冷恢复和模块卸载同轮通过。QEMU、subject、scope、supervisor均退出0，子树及监督进程均回收；Root代理保留运行。

## 失败、修正与保留证据

r1任务84236c6d2f404e11b14e6572f8b6b8ce在“fork callback fabricated foreign argv”断言失败。r2任务9c754e9b9fb144408b3242d9782d42a1仅加诊断，原断言保留，实际显示原始Create seq33的child.argv=null；initial argv却来自同源seq35、child自身TID99及真实task birth，值为执行前已有argv。

原因是既定initial记录首次接纳样本：Observation先排fast队列，Direct在桥中先处理，不能要求initial必定来自最早fork。r3仅修正这一过强断言：initial argv如存在必须匹配独立父进程/proc样本，原始fork argv为空、initial不变、后续真实argv/cwd、cookie、witness及Deny效果断言全部保留。三轮生产代码相同，没有为了通过测试修改保护行为。失败原件和r2来源记录均保留。

r3任务0b79716c7ec947a780c1ac419c7d3efa已通过。后续普通UID安装布局冒烟发现本机部分namespace readlink实际返回空字符串，原读取器误将其变为整个Tree Invalid；r4仅将这个空返回记为可选字段Missing，其他身份／覆盖／非空格式校验不变。独立源审、安装布局冒烟及组件检查通过后，r4任务794b21bd019d43f4bbecd9c25d847577重新完成全部真实联调，r4仅对应当时精确源码。

用户随后新增单次详情补取与成功空值语义。r5任务a3a2cab18c094574af621357ce972ef5的新详情case通过，但整体在既有connect-fast-parent的family observer失败；原错误未保存退出码，不能声称实测某个退出码。源码确认hold-parent先回收actor、再报告结果，两步之间父进程可能被真实终止，外层subreaper因此既拿不到报告也无法回收已回收的actor。修正仅涉及两个冒烟文件：hold-parent发布actor PID后直接等待信号，由外层subreaper唯一回收actor，并在失败时保留真实ExitStatus。没有helper补杀，也没有放宽两进程真实信号、witness或scope断言。

r6使用修正后的精确产物重新完成全部真实联调；相对r5仅上述两个夹具文件变化，生产代码及Native模块不变。r1／r2／r5失败原件保留，最终成功不覆盖失败记录。

## 产物与本地检查

- 最终镜像：/tmp/as-callback-main-20261108-r6/initramfs.cpio.gz；SHA256 5ea03f39a32faa26abaa97c4ec7f44d5f60befb36226ce73072f373697eaa6a4。
- Native模块：/tmp/as-native-exec-facts-20261108/module/as_wait.ko；SHA256 256336a0a038aba71032fdf3025f5936143415c96eab812f8069f4ff8774c2cf；as_core.c SHA256 827b0513af40a63613a7d523939e780d33308e474c7e50df04f083c202d2e19a。
- 最终82项源码及39项产物清单逐项匹配，独立probe inode与静态ELF已核：/tmp/as-callback-main-20261108-r6-package-record.json。源码清单SHA256 f2904fcffa5ae8420be8eb1f3956eb7a615a5551365596bbb2ac7731e1bb5083。
- Root原件：/tmp/as-callback-root-submit-r6.jsonl；公开回执：/tmp/as-callback-root-result-r6.json；摘要：/tmp/as-callback-validation.json。失败原件未重写。
- Rust 1.89 workspace/all-targets通过；沙箱最初因SO_PEERCRED被拒而失败，普通宿主UID运行同一测试通过，未删鉴权检查。相关组件回归、Rust 1.92 Clippy -D warnings、fmt、普通UID安装布局软件冒烟、strict rustdoc、Rust Analyzer引用／诊断和Native clangd通过。原生构建保留编译器／pahole差异及Kbuild BTF提示，未隐去日志。
- 单次补取最终本地日志：/tmp/as-detail-workspace-host-final.log、/tmp/as-detail-installed-host-final.log、/tmp/as-detail-final-tests.log、/tmp/as-detail-final-msrv-tests.log、/tmp/as-detail-final-clippy.log、/tmp/as-detail-final-lsp.log、/tmp/as-detail-doc-final.log。软件冒烟仅证明软件夹具；真实内核效果由上述r6单独证明。

Root覆盖当前ARM64内核和上述场景；未运行x86实机、真实Docker／OpenClaw、凭据各ID差异化切换矩阵或任意线程／namespace变更组合。网络新增cookie／socket namespace有公开API与代码审核依据，本轮未为它们另外执行SO_COOKIE独立值对照；不把已有网络行为回归扩大成该值专项验收。上述限制不改变已经执行的字段与规则结果。
