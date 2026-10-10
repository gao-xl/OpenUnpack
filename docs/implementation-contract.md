# 首版实现契约
用户2026-10-08已授权恢复开发，采用现代架构。Gemini插件并行编写模块；主agent复核集成和实际验证。仅处理用户提供有限密码，原包保留，无云/账户/穷举。
共享类型：crates/core/src/types.rs。前端桥：apps/desktop-ui/src/api.ts 和 types.ts，由主agent维护。子任务不得改共享文件、Cargo/package配置或其他模块。
## 后端模块端口
vault.rs: struct Vault。open(&Path)->Result<Self,String>；views()->Result<Vec<PasswordView>,String>；groups()->Result<Vec<String>,String>；upsert(PasswordInput)->Result<PasswordView,String>；delete(&str)->Result<(),String>；candidates(Option<&str>,Option<&str>)->Result<Vec<Candidate>,String>；record_success(&SuccessEvent)->Result<(),String>；preview_import(&str,&str,&str)->Result<ImportPreview,String>；apply_import(&str,&str,&str,&str)->Result<ImportResult,String>；export_backup(&Path,&str)->Result<(),String>；restore_backup(&Path,&str)->Result<ImportResult,String>；reveal(&str)->Result<String,String>。
Vault所有方法&self，内部短锁；Mutex只在同步用例内，绝不跨await，文件提交单写入者原子替换。口令版本仅改变口令时增长。错误仅固定脱敏说明。成功事件幂等且匹配口令版本。DPAPI当前用户加密全库，失败不能空库覆盖。导入无明文预览。备份AES256GCM+PBKDF2SHA256，参数有上限防DoS。
repository.rs: struct Repository。open(&Path)->Result<Self,String>；save_job(&JobView)->Result<(),String>；jobs()->Result<Vec<JobView>,String>；remove_job(&str)->Result<(),String>；clear_history()->Result<(),String>；save_publication(&Publication)->Result<(),String>；publications()->Result<Vec<Publication>,String>；commit_publication(&Publication,&JobView,&[SuccessEvent])->Result<(),String>（一个SQLite事务）；pending_success()->Result<Vec<SuccessEvent>,String>；ack_success(&str)->Result<(),String>。短事务，busy有限，JSON无口令；操作幂等。
scanner.rs: pub fn scan(paths:&[String],recursive:bool,exclusions:&[String])->Result<ScanResult,String>；pub fn output_name(path:&str)->String；pub fn matches_exclusion(relative:&str,patterns:&[String])->bool。递归不跟链接，Windows路径去重，分卷首卷归组+缺卷检查，DOCX/XLSX/JAR/APK不当归档；通配*?有界实现。
## Worker协议（主agent共享 types.rs 为准）
.NET10 C# Worker复用现有官方7z.dll适配，隔离主Rust进程；可框架依赖开发、最终自包含win-x64发布。
启动参数仅 --pipe-prefix 随机前缀 --parent-pid 数字 --engine 非敏感DLL路径。Worker建立两条NamedPipeServerStream: 前缀-control 和 前缀-events；CurrentUserOnly，Async，Byte。验证客户端PID=parent-pid。Rust连接后发换行JSON WorkerRequest，最大单消息8MiB。事件换行WorkerEvent同字段camelCase、protocolVersion=1。
Control操作 extract/copy/readback/cancel/ping/exit。事件 ready/progress/heartbeat/completed/error/cancelled。requestId一致；只有一项活跃作业；独立控制读取，事件单写入者；进度合并，不因进度阻塞取消。completed/error终结该request；exit结束进程。不得在stdout或stderr输出任何请求或口令。
extract: 准确批准allowedVolumes与destinationDirectory；仅提取一次，不先完整test；所有条目预检；流式SHA256，files包含相对路径/size/sha256/isDirectory；passwordUsed只在引擎实际索取密码时true。exclusions输出过滤，省略条目需反馈排除数/消息，不将CRC失败标成功。
copy: files源映射写destinationDirectory，CreateNew不覆盖；流式SHA与期望对比，差异失败；保留空目录。若输入缓存文件sha256为空，返回该复制内容新摘要；nonempty则验证期望。maxWrittenBytes与minFreeBytes检查。任何工作路径不跟reparse link，限制父路径，不执行归档文件。
readback: files来源为目标临时目录下文件，逐文件摘要比对；返回verificationScope=targetReadback（不宣称绕过缓存或物理断电持久性）。
ErrorCode: NeedsPassword/PasswordMismatchOrCorrupt/Corrupt/MissingVolume/Unsupported/UnsafePath/ResourceLimit/IoError/ProtocolError/Cancelled。不得输出异常原文/密码。
## UI用例
四页批量解压/密码库/处理记录/设置与备份。AntDesign Sidebar+虚拟Table+详情Drawer，任务阶段明确。原包永保留；默认不隐式排除，不自动联网，首版单根顺序。当前设置冻结；等待节点补密码通过新尝试，不误改变已发布成品。
源码目录各自独占；Gemini创建文件必须使用write_file，父目录由主agent预先创建。插件不能运行编译/测试，主agent或集成代理负责。不要修改gemini-subagent源项目或任何个人数据库。
