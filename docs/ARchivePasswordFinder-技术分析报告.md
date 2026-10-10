# ARchivePasswordFinder（解压密码云）深度技术分析报告

> 分析对象：`C:\Program Files (x86)\ARchivePasswordFinder`
> 分析方式：静态逆向（ILSpy 反编译 + PE/CLR 元数据解析 + 字符串/资源提取 + LiteDB 数据抽样）
> 分析结论定位：面向 OpenUnpack「解压工坊」的可借鉴 / 应规避清单
> 说明：该项目核心方法体受 **DNGuard HVM** 保护（方法体加密、运行时由原生 `APFRuntime*.dll` 解释执行），
> 因此本文对"方法级语义"的判定基于**元数据骨架 + 字段/属性命名 + 第三方依赖 + 配置模型 + 数据产物**的交叉推断，
> 凡属推断项均已标注 `[推断]`；结构、依赖、配置、数据层结论均为 `[已验证]`。

---

## 0. 结论摘要（TL;DR）

| 维度 | 结论 |
|---|---|
| 产品定位 | 单机 WinForms 压缩包密码恢复工具，主打"**本地密码库 + 云密码查询**"而非纯算力穷举 |
| 核心技术栈 | .NET Framework 4.6.2 / C# / WinForms + Telerik UI；LiteDB 本地库；7z-ZS + UnRAR 解压内核 |
| 密码验证模型 | **以"真实解压"为验证手段**（进程级 7z.exe/UnRAR.exe 或库调用），而非仅校验加密头/首块 |
| 破解粒度 | 字典攻击（主推）> 掩码/正则 > 穷举；另有"强力引擎"外挂 hashcat / John the Ripper |
| 代码保护 | 全解决方案 **DNGuard HVM**（方法体虚拟化），附带 x86/x64 原生 Runtime |
| 主要瓶颈 | ① 验证路径重（进程/IO 爆炸）② 并发调度粗（Timer 轮询 + 全局计数）③ 全代码虚拟化导致零可维护性 ④ 本地库明文存储密码 |
| 综合评价 | **业务设计优于工程实现**：密码来源分层、任务状态机、可插拔解压器抽象值得借鉴；执行模型、并发与可维护性是反面教材 |

---

## 1. 项目概况与组成 `[已验证]`

### 1.1 产品身份

| 项 | 值 |
|---|---|
| 品牌/域名 | `jieyamimayun.com`（"解压密码云"）、`www.jieyamimayun.com`，版权标注 2019 |
| 主程序 | `ARchivePasswordFinder.exe`（PE32 / x86，2.24 MB，CLR `v4.0.30319`，入口 `0x0600008b`） |
| 目标框架 | `ARchivePasswordFinder.exe.config` → `.NETFramework,Version=v4.6`；`APF.Common.dll` 元数据为 `v4.6.2` |
| 权限要求 | `app.manifest` → `requestedExecutionLevel level="requireAdministrator" uiAccess="true"`（**始终以管理员 + UI 自动化权限运行**） |
| DPI | `PerMonitorV2` |
| 安装目录 | `Program Files (x86)`（固定路径，非用户态数据） |

### 1.2 模块划分（由 PDB 泄漏的真实源码结构）`[已验证]`

反编译字符串中残留三条编译期 PDB 绝对路径，直接暴露开发机工程布局：

```
J:\Projects\ARchivePasswordFinder\Common\obj\Debug\APF.Common.pdb
J:\Projects\ARchivePasswordFinder\Data\obj\Debug\APF.Data.pdb
J:\Projects\ARchivePasswordFinder\CipherFile\obj\Debug\CipherFile.pdb
```

据此可得工程结构：

| 程序集 | 职责 | 保护状态 |
|---|---|---|
| `ARchivePasswordFinder.exe` | 主程序：UI（frmMain 等 12 个窗体）、任务调度、破解引擎、解压器 | DNGuard（119 文件中 94 个受保护） |
| `APF.Common.dll` | 通用层：配置模型、HTTP、进程、文件、日志、多语言(MLang COM)、CRT32 | DNGuard（88/42） |
| `APF.Data.dll` | 数据层：LiteDB 仓储（`DataContext` + 泛型 `LiteDbHelper<T>`） | DNGuard（7/6） |
| `CipherFile.dll` | 密码学：CryptSharp（PBKDF2 / SCrypt / Salsa20）、Twofish.NET、HMAC-SHA256 | DNGuard（19/17） |
| `APFRuntime.dll` / `APFRuntime64.dll` | **纯原生 Win32 DLL（无 CLR 头）**，DNGuard HVM 运行时引擎 | 原生 |
| `BatchZip.exe` / `SimpleCopy.exe` | 辅助进程（批量压缩/复制） | — |

### 1.3 第三方依赖清单 `[已验证]`

| 类别 | 组件 | 用途 |
|---|---|---|
| 解压内核 | `7z.dll`/`7z.exe`（**7-Zip ZS**，Igor Pavlov 1999-2022 + Tino Reichardt 2016-2023） | 7z/ZIP/TAR/GZ/WIM 等 |
| 压缩编解码 | `brotli / zstd / lz4 / lz5 / lizard / flzma2`（x86+x64 各一套） | 7z-ZS 扩展编解码器 |
| RAR 内核 | `UnRAR.dll`/`UnRAR.exe`（WinRAR 官方源，Alexander Roshal 1993-2025） | RAR/RAR5/分卷 |
| 托管库 | `SevenZipSharp`、`Rebex.Zip`、`Xceed.Zip/Compression v6.7`（含 LicenseKey 硬编码于静态构造） | 多后端解压策略 |
| 文件类型识别 | `TrID.dll`（16 MB 特征库） | 魔数/格式识别 |
| 正则生成 | `generex-0.0.4.dll` + `IKVM.OpenJDK.Core` + `IKVM.Runtime`（**用 IKVM 在 .NET 上跑 Java 的 Generex**） | 正则 → 候选密码枚举 |
| UI | `Telerik.WinControls.*`（十余个主题包，合计 ~40 MB） | Ribbon / 网格 / 富文本 |
| 序列化 | `Newtonsoft.Json`、`AutoMapper` | 配置与 DTO 映射 |
| 存储 | `LiteDB` | 本地密码库 |
| 网络 | `RestSharp` + `System.Net.Http` | 云端 API |
| 崩溃上报 | `CrashReporter.NET` | 遥测 |
| 系统 | `System.Buffers/Memory/Numerics.Vectors/Text.Encoding.CodePages` | 兼容包 |

---

## 2. 核心功能与业务逻辑 `[已验证 + 部分推断]`

### 2.1 支持的归档格式 `[已验证]`

`ArchiveFormat` 枚举给出了完整的格式识别面：

```
FakePic, FakeVid, Unknown, Rar, SevenZip, Zip, BZip2, Tar,
Gzip, Wim, Lzh, Lzw, Xz, EXE, LZ4, DZIP
```

其中 `FakePic`/`FakeVid` 说明产品明确面向**伪装成图片/视频的压缩包**这一灰色场景（中文资源站常见做法），
`EXE` 覆盖自解压包（SFX）。运行时再经 `ArchiveItem.DetectByExtension()` → `IsRarFile()`/`Is7z()`/`IsTar()`/`IsZip()` 二次确认，
并有 `RealType`（TrID 结果）与 `Method`（压缩方法）字段。

### 2.2 任务模型 `[已验证]`

```
TaskItem                                   TaskStatus (12 态)
├─ Id: Guid                                ├─ Open / Working / Done
├─ Status / StatusMessage                  ├─ Error / Warning
├─ Archive: ArchiveItem                    └─ CloudPwdNotFund / CloudPwdFunded / CloudPwdStart / CloudPwdNotEnough
├─ Password / Hash                          → 强力引擎族：WorkingWithForceCrack / ForceCracking /
├─ TargetPath / ExtracorType                   ForceCrackDone
├─ FileList / ExtractingError / Extend
├─ IsForceCracking / UsingCloudPwd
└─ IsResumeable / IsFailure / IsSpecialFile
```

`TaskStatus` 中 **7 个状态与"云密码"和"强力破解"直接相关**，印证产品主线是"先查云端密码库、查不到再本地算力"。

### 2.3 密码来源分层与优先级 `[已验证 + 推断]`

由 `CrackPasswordMethods` 枚举 + `ApplicationSettings` 开关 + `CrackManager` 的三个迭代器可知执行顺序：

```
CrackPasswordMethods = { DictionaryAttack, MaskAttack, BruteAttack, CloudAttack }

执行流水线（[推断]，依据状态机 + 配置开关 + 方法命名）：
  1) 计算归档 Hash（MD5/CRC/HMAC-SHA256）
  2) 本地库 FileEntitys 命中？ → 直接取密码          （UsingEmbeddedPasswords / StoreUnzipRecord）
  3) 云密码查询（按 Hash 分页拉取）                   （UsingCloudPwd / CloudPasswordCrack）
  4) 内置常见密码语料（MyData.db 中 PasswordEntitys） （UsingEmbeddedPasswords）
  5) 用户字典文件（Wordlists，多文件有序）            （DictionaryAttack）
  6) 掩码 / 正则（MaskAttackSetting + Generex）        （MaskAttack）
  7) 穷举（Charset 位标志 × 长度区间）                 （BruteAttack）
  8) 强力引擎：hashcat / John the Ripper              （UsingForceCrackEngine）
```

**密码迭代器抽象**（可组合，设计亮点）：

```csharp
public interface IPasswordIterator {
    CharsetCasing PasswordCasing { get; set; }
    bool RemoveWhitespace { get; set; }
    bool Initialize(CharsetEncoding passwordEncoding, int maxPasswordLength, ref string errorMsg);
    string GetNextPassword();          // 拉取式，天然适配流式验证
}
```

四个实现：`Dictionary` / `Dictionaries`（文件字典，`ObservableCollection<Dictionary>` 组合）、
`RegularExpression` / `RegularExpressions`（Generex 正则枚举）、`CloudPasswordIterator`（Cloud 分页队列）、
以及穷举用的字符集生成器。**组合模式让"字典 + 正则 + 云队列"可以串成一条候选流**，是本项目最值得继承的设计。

字符集定义：

```csharp
[Flags] enum Charset { None=0, Upper=1, Lower=2, Punctuation=4, Digits=8, Space=0x10, All=... }
enum CharsetEncoding { None, Ascii, Unicode }
enum CharsetCasing   { None, LowerCase, UpperCase, TitleCase }
struct EncryptionRecord { bool isEncrypted; CharsetEncoding PasswordCharset; int MaxPasswordSize; }
```

### 2.4 云密码服务 `[已验证]`

| 组件 | 方法 | 语义 |
|---|---|---|
| `TokenProxy` | `LoginToken(u,p)` / `TokenValidate(t)` / `HideLogin()` / `PasswordRecover(email)` | 账号体系（注册需充值） |
| `WorkerProxy` | `PreSearch(hash)` / `Search(hash, out list)` / `GetAsyncPassword(hash,start,count=100)` / `Report(FileEntity)` / `ReportFix(FileEntityDto)` / `CanUseCloudPassword()` | 按 **文件 Hash** 检索密码；**分页 100 条**；支持**用户上报密码回馈云端** |
| `CreditsProxy` | `GetCredit()` / `GetCredits()` / `Recharge(sn)` | 点数计费 + 卡密充值 |
| `InfoProxy` | `GetUpdateInfo()` / `GetLastVersion()` / `GetAds()` / `Buylink()` / `PostAdvice(msg,addr)` | 更新、广告、反馈 |
| `WebInterface` | `PostToWebAPI(url,data)` / `PostToWebAPIWithAuth(url,data,token)` | 统一 REST 出口 |

协议契约：请求 `RequestDTO { Token, dynamic Info }`，响应 `ResultDTO { int Code, string Info, dynamic Extend }`；
业务实体 `FileEntityDto { VolumeFormat, VolumeCounter, Password, FileHash, FileName, Extend, FileList, TotalPhysicalSize, Method, Version }`。

> ⚠️ **隐私与合规提示**：`Report()` / `ReportFix()` 表明客户端会把"归档指纹 + 密码"上报云端；
> `PreSearch` 会把本地文件 Hash 发送出去。对于内网/涉密归档，这构成**数据外泄面**。
> 反编译与安全评估时应把"云开关"视为敏感行为，而非普通遥测。

---

## 3. 底层运行机制与关键流程 `[已验证 + 推断]`

### 3.1 总体执行链路 `[推断，依据类骨架]`

```
Program.Main
  └─ SystemCheck.CheckCompoent()        // 校验 7z/unrar/Trid 组件存在性
  └─ GlobalVal (静态单例容器)
        ├─ Settings: ApplicationSettings   // Config/Setting.json
        ├─ Passwords: PasswordList         // 内存密码池（含计数排序）
        ├─ GlobleDataContext: DataContext  // LiteDB
        └─ ExtractorList: List<BaseExtractor>
  └─ frmMain (Telerik Ribbon UI)
        └─ TaskManager
              ├─ Timer → TimeTask_ExecuteTask → ProcessTasks()   // 轮询调度
              ├─ _Tasks: List<TaskItem>
              ├─ RuningTaskHost: List<TaskExecutor>              // 并发宿主
              ├─ ManualResetEvent PauseEvent / ResumeEvent        // 暂停/恢复
              └─ DiskFullEvent                                    // 磁盘满熔断
        └─ TaskExecutor(manager, task).Work()
              ├─ GetPasswordFromServer(hash)      // 云查
              ├─ 选择 BaseExtractor（ExtractorComparer 排序）
              ├─ DetectPwd(password) → UnZipPasswordValidity(...)
              └─ ExtractByUsingPassword(pwd, isCloud)
```

### 3.2 关键流程：密码验证 `[推断 + 已验证支撑]`

证据链：
- `CrackManager.UnZipPasswordValidity(zippedFilePath, outputFolder, password)` 返回 `PasswordValidity { Invalid, UserPasswordIsValid, OwnerPasswordIsValid }`；
- 方法内含 `IsPasswordRequired`、`dllNotSupport`、`DecByList`、`IsPasswordMatched` 等字段；
- `BaseExtractor` 有 `fullTestMode`、`IsDecByList`、`DetectPwd(password)`、`Extract(password)`、`GetFileList(password)`；
- `ApplicationSettings.TrySmallFileFirst = true`（默认）。

结论：**验证即"真解压"**。产品承认密码校验必须落到解压层面（zip 传统加密只有校验位、AES/RAR5 必须解首块），
并通过 `TrySmallFileFirst`（优先试小文件）、`IsDecByList`（按已知密码表精简）、`fullTestMode`（全量校验开关）做优化，
但没有实现"解出第一个数据块即停"的**惰性验证**——这是最大性能缺口（见 §5）。

### 3.3 解压器策略层（设计亮点）`[已验证]`

```csharp
public abstract class BaseExtractor {
    public abstract string InfoText { get; }
    public abstract int Priority { get; }                       // 排序权重
    public abstract bool IsArchiveFormatSupported(ArchiveItem);
    public abstract bool Initialize(ArchiveItem);
    public virtual  Task<bool> Extract(string password);
    internal         Task<bool> DetectPwd(string password);
    public virtual   void GetFileHash();
    internal         ProcessPriorityClass GetProcessPriority();
    // 持有：HashProcess / ExProcess（外部进程句柄）、Canceled、IsHashing、Warning、IsDecByList
}
```

优先级实测值（`Priority` 属性字面量）：

| 解压器 | Priority | 后端类型 |
|---|---|---|
| `XceedZipExtactor` | **1** | 托管库（最快，首选） |
| `SevenZipSharpExtractor` | **2** | 托管 P/Invoke 7z.dll |
| `UnrarExtractor` | **5** | Schematrix.Unrar 托管封装 |
| `GzipExtactor` | 10 | 托管 |
| `RebexZIPExtractor` | 10 | 托管 |
| `SevenZipExeExtractor` | 11 | **外部进程** |
| `UnrarExeExtractor` | 动态 | **外部进程** |

设计意图清晰：**优先走 in-process 托管库（省进程开销），失败/不支持才降级到外部 exe**。
`ExtractorComparer : IComparer<BaseExtractor>` 负责按 Priority 排序。

### 3.4 外部引擎集成 `[已验证]`

`PathConfig` 暴露的路径常量直接揭示了"强力引擎"实现方式：

```
Sevenz2hashcat | JohnripperjumboZip | JohnripperjumboRar | JohnPath | HashcatPath
SevenZipPath   | UnrarExePath | TempPath | ImagePath | BasePath
```

配合 `APF.Common` 中泄漏的成员名 `get_HashcatPath` / `get_Sevenz2hashcat` / `_johnripperjumboRar` / `_johnripperjumboZip`：

- **7z2hashcat**：把 7z 归档头转成 hashcat 可识别的哈希；
- **hashcat**：GPU 大规模爆破；
- **John the Ripper jumbo**：CPU/SIMD 爆破，分别有 RAR / ZIP 变体。

即 `UsingForceCrackEngine = true` 时，程序不自己算，而是**调度外部专业工具**。这是明智的性能分工，
但代价是：路径硬编码、需用户自备、版本耦合、无法统一进度/取消语义。

### 3.5 数据持久化 `[已验证]`

LiteDB 单文件 `MyData.db`（1.38 MB），两个集合：

| 集合 | 实体 | 字段 |
|---|---|---|
| `PasswordEntitys` | `PasswordEntity` | `Id(Guid,BsonId)`, `Text`, `Count` |
| `FileEntitys` | `FileEntity` | `Id`, `VolumeFormat`, `VolumeCounter`, `Password`, `FileHash`, `IsSync`, `FileName`, `Extend`, `FileList`, `TotalPhysicalSize`, `Method` |

**实测：数据库中的密码为明文存储**（直接 dump 出 `tiaokan.vip`、`www.1342050.com`、`123456`、`www.91xiezhen.top`、
`ASDFGHJKLzxcvbnm`、`N@1a%9bN(mo5)b` 等大量站点口令），且该库同时承担两个角色：

1. **内置常见密码语料**（数百条中文资源站口令，对应 `UsingEmbeddedPasswords`）；
2. **用户解压记录 + 保存的密码**（对应 `StoreUnzipRecord`、`FileEntitys`）。

> 虽然 `CipherFile.dll` 提供了 `EncryptHelper`（AES-256-CBC + PBKDF2，`DerivationIterations = 1000`），
> 但**数据库文件本身未加密**，`PasswordEntity.Text` / `FileEntity.Password` 无任何字段级保护。

数据访问层（`APF.Data`）：

```csharp
public class DataContext {                       // 门面
    LiteDbHelper<FileEntity>     dbfileHelper;
    LiteDbHelper<PasswordEntity> dbpwdHelper;
    List<PasswordEntity> FindPasswordEntitys(Expression<Func<PasswordEntity,bool>>);
    int  AddPasswordEntitys(IEnumerable<PasswordEntity>);
    ...
}
public class LiteDbHelper<Entity> : LiteDbBase {  // 泛型仓储
    Guid Insert(Entity); int InsertBulk(IEnumerable<Entity>);
    IEnumerable<Entity> Find(Expression<Func<Entity,bool>>);
    int DeleteByLinq(...); ILiteCollection<Entity> GetCollection();
}
public class LiteDbBase : IDisposable { LiteDatabase LiteDb; }  // 长连接单例
```

### 3.6 资源调度机制 `[已验证]`

| 机制 | 实现 |
|---|---|
| 并发度 | `ApplicationSettings.ExtractRuningCount`（默认 **1**）+ `TaskManager.RuningTaskHost` 列表 |
| 调度驱动 | `Timer` 回调 `TimeTask_ExecuteTask` → `ProcessTasks()`（**轮询式**，非事件驱动） |
| CPU 自适应 | `IntelligentBalancedCPU`（默认 true）+ `APF.Common.Helper.ProcessCpuCounter`（**PerformanceCounter 逐进程采样**） |
| 进程优先级 | `BaseExtractor.GetProcessPriority()` → `ProcessPriorityClass` |
| 速度平滑 | `SimpleMovingAverage(CircularList<double>)` → 显示用速率 |
| 暂停/恢复 | `ManualResetEvent` 双事件 |
| 磁盘满 | `DiskFullEvent` + `TaskManager.DiskFullErr()` |
| 休眠抑制 | `PowerUtilities.SetThreadExecutionState(ES_CONTINUOUS\|ES_SYSTEM_REQUIRED)` |
| 进程清理 | `BaseExtractor.EndProcesses()` / `TaskExecutor.Dispose()` / `AppDomain.CurrentDomain.ProcessExit` 钩子 |
| 异步工具 | `APF.Common.Extension.ProcessExtensions.WaitForExitAsync`、`ProcessAsyncHelper.ExecuteShellCommand/RunProcessAsync` |
| 弹性封装 | `TaskExtensions` 提供 `Retry / WithTimeout / Fallback / Catch / OnFailure`（**设计亮点**） |

### 3.7 反分析与代码保护 `[已验证]`

| 层 | 手段 |
|---|---|
| 方法体 | **DNGuard HVM**：IL 方法体被抽空，替换为 `throw new Exception("Error, DNGuard Runtime library not loaded!")`，真实逻辑加密存于资源，运行时由原生 Runtime 解释 |
| 原生层 | `APFRuntime.dll`(x86, 1.81 MB) / `APFRuntime64.dll`(x64, 3.74 MB) — **无 CLR 头的纯 Win32 DLL**，"Hardened Virtual Machine" |
| 符号混淆 | 大量 `_FF00_FF00_FF00_...` 中文/私用区字符方法名，`ZYXDNGuarder.Startup()` 注入静态构造 |
| 授权 | Xceed / Rebex 商业库 LicenseKey 在静态构造中注入（值由 HVM 在运行时解出） |
| 反调试 | `[MethodImpl(MethodImplOptions.NoInlining)]` 全量标注 + `MethodImplOptions.NoInlining` 阻断内联分析 |
| 崩溃遥测 | `CrashReporter.NET` + `Program.SendReport/SendReportSilently` |

**保护覆盖率实测**：

| 程序集 | 源文件 | 受保护文件 | 覆盖率 |
|---|---|---|---|
| `ARchivePasswordFinder.exe` | 119 | 94 | **79%** |
| `APF.Common.dll` | 88 | 42 | 48% |
| `APF.Data.dll` | 7 | 6 | 86% |
| `CipherFile.dll` | 19 | 17 | 89% |

未受保护的基本只有：P/Invoke 声明、COM 接口定义（MultiLanguage/MLang）、枚举、`AssemblyInfo`。

---

## 4. 三维度评估

### 4.1 执行效率

| 环节 | 表现 | 评价 |
|---|---|---|
| 密码迭代 | 拉取式 `GetNextPassword()`，`CircularList` 复用缓冲，`TrimAllWithInplaceCharArray` 原地裁剪 | ✅ 良好 |
| 密码验证 | **每次候选触发一次解压路径**（库调用或进程创建） | ❌ 核心瓶颈 |
| 进程开销 | 降级路径 `SevenZipExeExtractor`/`UnrarExeExtractor` 每次 spawn 子进程（Windows 约 10–30 ms/次） | ❌ 严重 |
| 字典加载 | `Dictionary` 流式 `reader` 逐行读取 | ✅ 良好（未全量入内存） |
| 正则枚举 | 经 IKVM 调用 Java Generex | ⚠️ 跨 VM 互操作，单次调用开销高 |
| 哈希计算 | MD5 + CRC32 + HMAC-SHA256 三种并存 | ⚠️ 冗余，未统一 |
| HVM 开销 | 94 个文件的密集路径走虚拟化解释执行，**放弃 JIT** | ❌ 性能与可维护性双重损失 |
| 字典型优先 | 默认推荐"字典模式"（自述"暴力破解请尽量采用字典模式"） | ✅ 策略正确 |

**粗略量级 `[推断]`**：in-process 后端下单候选约 0.1–5 ms（取决于归档大小/压缩方法），
外部进程后端下**每次候选额外 +10–30 ms 固定成本**。若字典 10 万条走进程后端，仅进程创建就消耗 17–50 分钟，
而这与密码正确性无关——**固定开销主导**，是典型"架构决定性能上限"。

### 4.2 资源调度

| 项 | 实现 | 评价 |
|---|---|---|
| 并发粒度 | 全局 `ExtractRuningCount`（默认 1），任务级串行 | ⚠️ 无候选级并行 |
| 调度方式 | `Timer` 轮询 `ProcessTasks()` | ❌ 延迟/空转，非事件驱动 |
| CPU 自适应 | `PerformanceCounter` 逐进程采样 | ❌ **PerformanceCounter 创建成本高且句柄易泄漏** |
| 线程模型 | `BackgroundWorker`（`decryptBackgroundWorker`）+ `AsyncStateMachine` + `ManualResetEvent` | ⚠️ 三代异步范式混杂 |
| 内存 | `SimpleLog._logEntryQueue` 为无界 `Queue<XElement>` | ❌ 高频日志下无界增长 |
| 大文件 | 主进程 **PE32/x86**，地址空间 2 GB（32 位进程） | ❌ 超大归档/超大字典受限 |
| 休眠/磁盘 | 抑制休眠 + 磁盘满熔断 + 暂停/恢复事件 | ✅ 考虑周到 |
| 进程遗留 | 依赖 `ProcessExit` 钩子清理 | ⚠️ 强杀/崩溃时易残留 7z/unrar 子进程 |

### 4.3 架构设计

**优点**

1. **分层清晰**：App（UI/编排）→ Core（任务）→ CrackEngine（迭代器/验证）→ Extractors（格式后端）→ Data/Common（基础设施）。
2. **可组合的密码源抽象**：`IPasswordIterator` + 组合模式，字典/正则/云队列可串联。
3. **可插拔解压器 + 优先级**：策略 + 比较器，后端可替换、可降级。
4. **事件驱动 UI 解耦**：`TaskManager` 暴露 `OnTasksChanged / OnArchiveProgressChanged / OnTotalProgressChanged / TaskMessage`。
5. **状态机完备**：12 态覆盖云密码计费失败、强力破解中、磁盘满等边界。
6. **弹性工具**：`TaskExtensions` 的 `Retry / WithTimeout / Fallback / Catch`。
7. **编码兼容**：`CN2JPHelper` + MLang 处理 GBK/Shift-JIS/BOM 文件名（面向中文/日文老包，务实）。

**缺点**

1. **UI/业务耦合**：`frmMain` 单类含 487 处受保护成员（推测数千行），业务逻辑落进窗体。
2. **基础设施重复实现**：自研 `SimpleLog`（本可用 NLog/Serilog）、自研 `HttpHelper`（且**固定启用 SSL3**）。
3. **配置 JSON 无版本/无迁移**：`Setting.json` 直接增删字段（实测 DB 中无 `IsSync`，说明字段已随版本漂移）。
4. **全代码虚拟化**：反维护、无堆栈、无调试、无法热修 —— 对"已停止维护"的项目尤其致命。
5. **商业 UI 组件锁定**：Telerik 体积庞大且授权受限。

---

## 5. 技术瓶颈与设计不足（按严重度分级）

### P0 — 决定性瓶颈

| # | 问题 | 位置 | 后果 | 依据 |
|---|---|---|---|---|
| P0-1 | **以"完整解压"为密码验证手段** | `CrackManager.UnZipPasswordValidity` / `BaseExtractor.DetectPwd` | 单候选成本 = 解压成本，与密码长度无关，吞吐量被 IO/解码锁死 | 方法签名 + `fullTestMode` 字段 |
| P0-2 | **缺乏"解出首块即判定"的惰性验证** | 同上 | 正确密码可能在前 1 字节就能判定，却要解完整档；错误密码需遍历到失败点 | `TrySmallFileFirst` 只是缓解，非根治 |
| P0-3 | **外部进程路径的固定开销** | `SevenZipExeExtractor`(P=11) / `UnrarExeExtractor` | 每候选 +10–30 ms 进程创建，10 万级字典直接不可用 | `ProcessAsyncHelper.RunProcessAsync` + Priority 字面量 |
| P0-4 | **无服务化/常驻 Worker 池** | 全项目 | 无法批量喂候选给一个长驻进程，进程生命周期 = 候选生命周期 | `HashProcess`/`ExProcess` 为 **per-extractor 实例**字段 |

### P1 — 严重问题

| # | 问题 | 位置 | 后果 |
|---|---|---|---|
| P1-1 | 并发为 **任务级**，无候选级并行 | `ExtractRuningCount` 默认 1 | 多核 CPU 在单任务下大量闲置 |
| P1-2 | `Timer` 轮询调度 | `TaskManager.TimeTask_ExecuteTask` | 延迟抖动、空转、难以公平调度 |
| P1-3 | `PerformanceCounter` 逐进程采样 | `ProcessCpuCounter` | 每次 new 成本高、句柄泄漏、首次采样恒为 0 |
| P1-4 | **本地密码库明文** | `MyData.db` / `PasswordEntity.Text` / `FileEntity.Password` | 任何进程/用户可读取全部历史口令（实测已 dump 出明文） |
| P1-5 | 日志队列无界 | `SimpleLog._logEntryQueue` | 高频尝试下内存持续增长 |
| P1-6 | 全量 DNGuard HVM | 79% 主程序 + 原生 Runtime | 性能损失 + **零可维护性**（无法修 bug、无法升级 .NET） |
| P1-7 | 主进程 32 位 | PE32 / x86 | 2 GB 地址空间上限，大数据集/大字典受阻 |
| P1-8 | 云上报含文件指纹与密码 | `WorkerProxy.Report/ReportFix/PreSearch` | 隐私外泄；涉密归档不可用 |

### P2 — 改进项

| # | 问题 | 位置 |
|---|---|---|
| P2-1 | 强制启用 SSL3（已废弃协议） | `HttpHelper` 静态构造 |
| P2-2 | 三种哈希（MD5/CRC32/HMAC-SHA256）并存且语义不清 | `EncryptHelper` / `FileEntity.FileHash` |
| P2-3 | 自研日志/HTTP 重复造轮子 | `SimpleLog` / `HttpHelper` |
| P2-4 | 配置无版本号与迁移 | `Config/Setting.json` |
| P2-5 | `AutoFixExtension` 默认开启拖慢批量添加（作者自述） | 向导文案 welPage3 |
| P2-6 | 强杀时子进程残留 | `ProcessExit` 钩子 |
| P2-7 | 依赖 `Program Files` 固定目录 + 要求管理员 + `uiAccess=true` | `app.manifest` |
| P2-8 | Generex 经 IKVM 跨 VM 调用 | `RegularExpression` |
| P2-9 | 正则/掩码无上限保护，`MaxPasswordSize` 仅记录不拦截 | `EncryptionRecord` |

---

## 6. 性能优化与架构改进建议

### 6.1 性能优化（按 ROI 排序）

**① 把"验证"从"解压"中剥离 —— 最高优先级**

- ZIP 传统加密：只需校验 12 字节加密头中的校验位 → **单候选 < 1 μs**；
- ZIP AES / 7z AES / RAR5：必须解出至少一个数据块 + 校验 CRC/HMAC → **单候选 ~ 1–50 μs**；
- 结论：**直接实现格式级"首块验证"**，不落盘、不建目录、不 spawn 进程。仅当验证通过后才走完整解压。

```csharp
// 目标形态（伪代码）
interface IPasswordProbe {           // 替代 BaseExtractor.DetectPwd 的"验证"职责
    ProbeResult Probe(ReadOnlySpan<byte> header, ReadOnlySpan<byte> firstBlock, string pwd);
}
// ProbeResult: Match / NoMatch / NeedFullTest（仅传统 ZIP 才可能 NeedFullTest）
```

**② 常驻 Worker 池，取代 per-candidate 进程**

- 将 7z/unrar 的调用改为**长驻 helper 进程**（`BatchZip.exe` 已存在，可复用其思路）；
- 采用 **stdin 喂候选 / stdout 回结果** 的协议，消除进程创建固定开销；
- 池大小 = `min(物理核数, ExtractRuningCount)`，配合 `Channel<T>` 背压。

**③ 候选级并行 + 分片去重**

- 用 `Parallel.ForEachAsync` / `Channel` + `SemaphoreSlim` 做候选级并行（而非任务级）；
- 字典按字节区间**分片**给多 worker，避免重复尝试；
- 全局去重：`PasswordComparer` / `Password.GetHashCode()` 已具备能力，接一个 `HashSet<string>`（或布隆过滤器）即可。

**④ 消除 CPU 自适应的昂贵采样**

- 弃用 `PerformanceCounter`，改用 `Process.TotalProcessorTime` 差分或 `Environment.ProcessorCount` + 队列深度自适应；
- 让并发度由"待处理候选队列长度"驱动（背压自适应），而非采样 CPU。

**⑤ 日志改造**

- 换用 NLog/Serilog 异步目标，设置**有界队列 + 丢弃策略（DropOldest）**；
- 候选级日志降为 `Trace` 且默认关闭；`SimpleLog` 的 `_logEntryQueue` 必须加上限。

**⑥ 大内存/大字典**

- 主进程升至 **x64**（现有 x64 原生 Runtime 已具备，说明无技术障碍）；
- 字典流式 + 内存映射（`MemoryMappedFile`）避免整文件入内存。

### 6.2 架构改进建议

| 方向 | 建议 |
|---|---|
| **进程模型** | 主进程（UI/编排，x64）+ Worker 进程池（解压/验证，可 kill）+ 外部引擎适配层（hashcat/John）。三者用**命名管道**（项目已有 `IPC.NamedPipeServer/Client` 雏形）解耦 |
| **验证抽象** | 拆分 `BaseExtractor`：`IArchiveReader`（读结构/取首块）+ `IPasswordProbe`（验证）+ `IExtractor`（解压）。**验证与解压彻底分离** |
| **密码库安全** | 密码字段用 **DPAPI**（`CurrentUser` 范围）或 AES-GCM 加密落库；密钥派生用 PBKDF2 **≥ 200k 迭代**（当前 1000 太低）或 Argon2；导出走独立口令加密 |
| **存储选型** | LiteDB 单文件不擅并发写；改为 **SQLite（WAL）** 或分区 LiteDB（读写分离），或纯只读语料 + 独立用户库 |
| **配置治理** | JSON 增加 `SchemaVersion` + 显式迁移函数；用 `System.Text.Json` 替代 Newtonsoft |
| **日志/HTTP** | 用成熟的 NLog/Serilog + `IHttpClientFactory`；移除 SSL3，只保留 TLS1.2/1.3 |
| **隐私** | 云上报改为**显式可选 + 内容预览 + 脱敏**（默认只上报归档指纹，永不自动上报密码） |
| **可维护性** | 放弃 DNGuard HVM；若必须保护，改用轻量混淆（ConfuserEx 级）+ 关键算法隔离成原生库，保留其余可维护 |
| **编码处理** | 用 `System.Text.Encoding.CodePages` 注册 GBK/Shift-JIS 兜底 + 现代 ICU；`CN2JPHelper`/MLang 仅作最后手段 |
| **弹性** | 保留并强化 `TaskExtensions`（Retry/WithTimeout/Fallback）——这是项目里最现代的代码 |
| **依赖瘦身** | Telerik 十余个主题包（~40 MB）→ 按需保留 1–2 个或改自绘 |

### 6.3 建议的目标架构

```
┌─────────────────────────────────────────────────────────────┐
│  UI 层  (WinForms / Tauri-WebView2)                          │
│  仅绑定 ViewModel，事件订阅，不写业务                          │
└───────────────┬─────────────────────────────────────────────┘
                │ 事件 / 命令
┌───────────────▼─────────────────────────────────────────────┐
│  编排层 Orchestrator                                         │
│  TaskManager（Channel 队列，事件驱动，非 Timer 轮询）          │
│  任务状态机 + 断点续跑 + 暂停/取消（CancellationToken）        │
└───────┬──────────────────────────────────┬──────────────────┘
        │                                  │
┌───────▼────────────────┐        ┌────────▼─────────────────┐
│ 候选密码流水线          │        │ 外部引擎适配层            │
│ IPasswordIterator 组合  │        │ hashcat / John / 7z2hashcat│
│ 字典丨掩码丨云队列丨穷举  │        │ （进程池 + 进度/取消统一）  │
│ + 分片 + 去重 + 背压     │        └──────────────────────────┘
└───────┬────────────────┘
        │ 候选流（Channel<string>）
┌───────▼─────────────────────────────────────────────────────┐
│  验证层 PasswordProbe（常驻 Worker 池，零落盘，首块判定）      │
│  ZIP-Trad │ ZIP-AES │ 7z-AES │ RAR5 │ …  多态实现            │
└───────┬─────────────────────────────────────────────────────┘
        │ 命中
┌───────▼──────────────┐   ┌────────────────────────────────┐
│ 解压层 IExtractor     │   │ 存储层：用户库(加密) + 语料(只读) │
│ 7z-ZS / UnRAR 常驻    │   │ DPAPI/AES-GCM；SQLite WAL       │
└──────────────────────┘   └────────────────────────────────┘
```

---

## 7. 对 OpenUnpack「解压工坊」的借鉴清单

### 建议直接借鉴 `[已验证]`

| 项 | 理由 |
|---|---|
| `IPasswordIterator` 组合模式 | 字典/正则/云队列可串联成统一候选流，天然适配流式验证与背压 |
| `BaseExtractor` 策略 + `Priority` 降级 | 托管库优先、外部进程兜底；与 OpenUnpack 的"官方 7-Zip Worker"分层一致 |
| 12 态任务状态机 | 覆盖"缺密码/资源不足/部分交付"等边界，与 `acceptance-checklist.md` 的 pending/失败态契合 |
| `TaskExtensions`（Retry/WithTimeout/Fallback/Catch） | 可直接移植为 Worker 调用的弹性封装 |
| `PowerUtilities` 抑制休眠 + 磁盘满熔断 | 长时间批量任务必备 |
| `SimpleMovingAverage` 速度平滑 | 进度显示更稳 |
| `CN2JPHelper` 文件名编码兜底 | 中文/日文老包（GBK/Shift-JIS）真实需求 |
| `TrySmallFileFirst` | 用最小代价文件探密码，思路正确 |

### 建议规避 `[已验证]`

| 项 | 理由 |
|---|---|
| ❌ 以完整解压做密码验证 | OpenUnpack 已规划"云密码查询/穷举"为候选功能——**务必先做首块验证层**，否则重复 APF 的最大错误 |
| ❌ 每候选 spawn 进程 | OpenUnpack 已是常驻 .NET Worker 架构，**保持常驻**，用 IPC 喂候选 |
| ❌ DNGuard HVM 全量虚拟化 | 与"MIT 开源 + 可构建 + 可验收"的项目定位根本冲突 |
| ❌ 密码明文落库 | OpenUnpack 已用 **DPAPI** 保护密码库 —— 继续保持，并覆盖导出/备份路径 |
| ❌ Timer 轮询调度 | OpenUnpack 已有"顺序队列 + 阶段进度"，改为 `Channel`/事件驱动 |
| ❌ `PerformanceCounter` 采样调 CPU | 改用队列深度背压 |
| ❌ 固定 `Program Files` + 强制管理员 | OpenUnpack 已用 `%LOCALAPPDATA%` —— 正确 |
| ❌ 自动上报文件指纹/密码 | OpenUnpack 的"无须网络即可日常解压"是正确取舍 |

### 落地优先级建议

| 优先级 | 动作 |
|---|---|
| **短期** | 在 OpenUnpack 内置 `IPasswordProbe` 首块验证层；`Channel` 替换轮询；日志有界化 |
| **中期** | 候选级并行 + 分片去重；Worker 常驻池 + 命名管道协议；密码库 DPAPI 全路径覆盖 |
| **长期** | 外部引擎适配层（hashcat/John/7z2hashcat）统一进度与取消语义；配置版本化；编码兜底策略 |

---

## 附录 A：关键证据索引

| 证据 | 来源 |
|---|---|
| 类/方法骨架 | ILSpy 反编译 233 个 `.cs`（输出于 `.workbuddy/apf_analysis/`） |
| 保护覆盖率 | `grep -rl "DNGuard Runtime library not loaded"` × 119/88/7/19 文件 |
| 原生 Runtime 判定 | PE 头解析：`APFRuntime.dll`(0x014c/x86) 与 `APFRuntime64.dll`(0x8664/x64) **COM descriptor RVA = 0** |
| 源码路径泄漏 | PDB 字符串 `J:\Projects\ARchivePasswordFinder\{Common,Data,CipherFile}\obj\Debug\*.pdb` |
| 云服务域名 | 字符串 `www.jieyamimayun.com` / `jieyamimayun.com` |
| 外部引擎 | `PathConfig` 的 `Sevenz2hashcat/JohnripperjumboZip/JohnripperjumboRar/HashcatPath/JohnPath` |
| 解压器优先级 | 各 `Extractor*.cs` 的 `Priority => N;` 字面量（1/2/5/10/10/11） |
| 内置语料/明文密码 | `MyData.db` 直接 dump（`PasswordEntitys` / `FileEntitys`） |
| 7z 内核版本 | `7z.dll` 字符串 `7-Zip ZS` / `Copyright (c) 1999-2022 Igor Pavlov, 2016-2023 Tino Reichardt` |
| UnRAR 内核 | `UnRAR.dll` 字符串 `Alexander Roshal 1993-2025` / `D:\Projects\WinRAR\rar\build\unrardll64\Release\UnRAR64.pdb` |
| 权限/DPI | `app.manifest`、`ARchivePasswordFinder.exe.config` |
| 配置项 | `Config/Setting.json`（33 个开关） |

## 附录 B：`Config/Setting.json` 完整默认值 `[已验证]`

```json
{
  "ExtractToSubWhenOnly1Sub": true,      "IntelligentExtractAndRemoveEmp": true,
  "MinimizeToTray": false,               "AlwaysShowOnTop": false,
  "ExtractArchiveInArchive": true,       "TrimPassword": true,
  "ExtractRuningCount": 1,               "TrySmallFileFirst": true,
  "IntelligentBalancedCPU": true,        "UseGlobalExtractPath": false,
  "DeleteArchivesAfterExtractFinished": true,  "DeleteFile2RecycleBin": false,
  "ReplaceExistingFile": true,           "UpdateCheckOnStartup": true,
  "ThemeName": "VisualStudio2012Light",  "UsingCloudPwd": true,
  "CloudPasswordCrack": true,            "UsingForceCrackEngine": false,
  "UsingEmbeddedPasswords": true,        "PlaySoundWhenDone": false,
  "IsSimpleRibbon": true,                "AutoFixExtension": true,
  "IsWizard": false,                     "StoreUnzipRecord": false,
  "CRCErrorExtract": false,              "ForceCrackMode": 0,
  "CurrentLanguage": "zh"
}
```

> 注：默认 `UsingCloudPwd = true`，即**开箱即联网**（说明文档 welPage2 也提示"不需要云查询请关闭，否则会计算 Hash 联网查询"）。

---

*报告完*
