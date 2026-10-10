# APF 借鉴：OpenUnpack 升级思路提案

日期：2026-10-10
来源：对 `C:\Program Files (x86)\ARchivePasswordFinder` 的逆向分析（见 [../ARchivePasswordFinder-技术分析报告.md](../ARchivePasswordFinder-技术分析报告.md)）
性质：**设计讨论稿**。不是实现承诺，不改变现有首版范围。
核对基线：`architecture.md`、`architecture-modern.md`、`software-spec.md`、`function-summary.md`、`implementation-contract.md`

## 0. 不打破的约束（升级必须服从）

| 约束 | 出处 |
|---|---|
| 本地优先、离线基线、不上传、无云账户 | function-summary §6、architecture-modern §1 |
| 仅处理用户提供的有限候选密码；穷举/云查询不进首版 | implementation-contract、architecture-modern §13 |
| 不引入后台常驻服务、网络 API、账户体系 | architecture.md §1 |
| 官方 7-Zip 引擎 + 第三方许可/源码材料完备 | README、architecture.md §16 |
| 单写入者 SQLite；口令不落库、不入日志/命令行/诊断包 | architecture.md §9、§8 |
| MIT 许可证 | README |

> 结论：只借鉴**扩展点形状**与**能力缺口**，不照搬 APF 的功能范围。

---

## 1. PasswordResolver：从「查库」升级为「候选流水线」

**APF 借鉴点**：`IPasswordIterator` 组合模式 —— `Initialize(encoding, maxLen, ref errMsg)` + `GetNextPassword()`，
四个实现（`Dictionary` / `RegularExpressions` / `CloudPasswordIterator` / 穷举生成器）可串成一条候选流。

**你的现状**：`PasswordResolver` 职责是"分组范围、启停、关联优先、候选去重"，候选**只来自保险库**；
§16 已提出"去首尾空格只允许明确开启后增加派生候选"，但那是散点规则，不是流水线抽象。

**升级动作**（只改扩展点形状，不改首版范围）：

```
ICandidateSource                       // 候选来源
  └─ VaultSource                       // 首版唯一实现：保险库候选
  └─ (未来) WordlistSource             // 字典文件
  └─ (未来) DerivedSource              // 空格/大小写/文件名/站点名派生
  └─ (未来) CorpusSource               // 内置离线语料（见 §6）

IPasswordResolver.Resolve(node) → IAsyncEnumerable<CandidateLease>
```

**必须现在留的字段**：`CandidateOrigin { Original | TrimmedSpaces | CaseVariant | FromFileName | CorpusId | ... }`
理由：§16 明确要求"实际命中的派生口令不能误当原文成功"，且 outbox 有 `passwordRevision` 幂等规则。
**如果现在不把 origin 放进 lease，未来加派生来源时 outbox 契约要重做。**

**验收抓手**：`candidate_origin_is_recorded_and_never_promoted_to_original`。

---

## 2. 把「验证」与「解压」彻底拆开：PasswordProbe 层

**APF 反面教训**：`BaseExtractor.DetectPwd` 与 `Extract` 共用一套解压路径 ——
单候选成本 = 解压成本，且外部进程后端每候选还要 +10–30 ms。这是它的 P0 瓶颈。

**你的现状**：architecture.md §7 已意识到"格式允许的轻量密码试探只缩小候选，不能代表整包成功"，
但没有独立的探针模块，试错路径仍会落到 `NativeArchiveAdapter.Extract`。

**升级动作**：在 Worker 内拆出 `PasswordProbe`，与 `Extract` 共享 `IArchiveBackend` 但**不共享落盘路径**：

| 加密形态 | 探针成本 | 说明 |
|---|---|---|
| ZIP 传统加密 | 微秒级 | 只校验 12 字节加密头校验位；有 1/256 假阳性 → 仍需最终 `Extract` 确认 |
| ZIP-AES / 7z-AES / RAR5 | 十~百微秒 | 解**首块** + 校验 CRC/MAC |
| 固实归档 | 可能退化 | 首块可能需要大范围解码 → 探针要能返回 `Inconclusive`，交回完整流程 |

**关键设计约束**：
- 探针**零落盘**：不建目录、不写文件，只读源包指定区段；
- 探针结果**不构成成功证据**，只做"候选剪枝"（与你现有验证能力模型一致）；
- 假阳性必须由 `Extract` + `VerificationService` 兜底。

**验收抓手**：`probe_never_publishes_and_false_positive_is_caught_by_extract`。

---

## 3. 引擎后端：从「单体 7z.dll」升级为「能力声明 + 路由」

**APF 借鉴点**：7 个解压后端 + `Priority` 降级（Xceed=1 → SevenZipSharp=2 → Unrar=5 → 7z.exe=11），
**但选后端靠文件扩展名 + 静态优先级**，是它的设计缺陷。

**你的现状**：单一官方 `7z.dll`（`EngineClient: Inspect / Extract / Cancel`）。

**升级动作**：不急着加后端，先把接口形状改成**能力声明**：

```
IArchiveBackend {
    BackendCapabilities Capabilities;   // 支持的格式 / 方法 / 编码 / 是否支持分卷
    bool TryOpen(source, out ArchiveHandle);
}
BackendRouter.Select(InspectResult) → IArchiveBackend   // 按实测能力，不按扩展名
```

**真正值得补的两个边界**（都要单独过许可审查）：

| 缺口 | 后备方案 | 许可风险 |
|---|---|---|
| RAR5 特殊压缩方法 / 恢复记录 | WinRAR 官方 `UnRAR.dll` | LGPL + unRAR 限制，不可用于重打包 |
| ZIP/7z 中的 zstd / lz4 / brotli | 7-Zip ZS fork 或官方 codec 插件 | **与"官方 7-Zip"既有约束冲突**，须重走 README 的许可/源码收集流程 |

**建议**：首版先不引入，但把 `BackendRouter` 与 `Capabilities` 落在接口上——
将来加后端是"注册一个实现"，而不是改 `NativeArchiveAdapter`。

---

## 4. 归档内文件名编码：从「日志里的乱码」升级为一等公民 ⭐ 真实缺口

**APF 借鉴点**：`CN2JPHelper.DetectEncode / ChangeCode / FixErrJISPath` + MLang COM ——
专治中文/日文老包的 GBK / Shift-JIS / Big5 文件名乱码。

**你的现状**：**零覆盖**。文档中 `文件名编码`、`乱码` 均无命中；`编码` 只出现在密码导入（AT-05）
和指纹规范（architecture-modern §128）语境。

**升级动作**：

1. 在 Worker 内注册 `System.Text.Encoding.CodePages`（GBK / GB18030 / Big5 / Shift-JIS / EUC-KR）；
2. 检测策略：ZIP 无 UTF-8 flag（bit 11）时，按候选编码试解 + **合法性评分**（双字节区间合法率、非法序列数），
   择优；不依赖 MLang COM（已过时）；
3. 结果作为 `EntryNameDecoding { Encoding, Confidence }` 进入 `NodeResultManifest`，
   与 `sourceVersion` 元数据模型天然契合（**不改源文件、不改源名**）；
4. 解码后的相对路径必须走既有的"不安全条目"校验（目录穿越 / 保留名 / 非法字符）；
5. 低置信度时在 UI 提示"文件名编码不确定"，并允许用户切换编码**只重跑写回阶段**（复用已封存节点结果）。

**为什么值得做**：这是中文用户最高频的抱怨，且实现成本远低于收益；同时也是 APF 里少数"今天仍然正确"的设计。

**验收抓手**：`gbk_and_shiftjis_entry_names_are_recovered_without_touching_source`。

---

## 5. 磁盘调度：把「保守串行」升级为「可测的拓扑感知」

**APF 反面教训**：完全没有物理盘拓扑概念，只有一个全局 `ExtractRuningCount`（默认 1）+
`PerformanceCounter` 逐进程采样 —— 这是它资源调度的根本失败点。

**你的现状**：architecture.md §12 与 architecture-modern §116 已经写对了原则
（"不同盘符可能属于同一物理盘"、"拓扑不明时保守串行"），但**没有落地模块与可观测性**。

**升级动作**：

```
DeviceTopologyProvider                  // 独立、可单测
  ├─ QueryDosDevice / IOCTL_STORAGE_GET_DEVICE_NUMBER → \Device\HarddiskN
  ├─ 卷 → 物理盘 → 介质类型（HDD / SSD / 可移动）
  └─ 失败 → 保守：全部视为同一物理盘（串行）
```

- 复用你已有的 `ResourceScheduler`"一次申请整组许可"，不改语义；
- **诊断面板显示"盘符 → 物理盘 → 介质类型"映射**与"本次为何串行"，
  让"保守限制"可解释，而不是黑盒等待；
- 探测做成可注入（测试可喂假拓扑），避免 CI 依赖真实硬件。

**验收抓手**：`topology_probe_failure_degrades_to_serial_without_deadlock`。

---

## 6. 离线内置语料：云查询的隐私安全替代（需先解决许可）

**APF 借鉴点**：`MyData.db` 内嵌数百条中文资源站常见口令，`UsingEmbeddedPasswords` 开关。

**你的现状**：云密码查询被明确排除（architecture-modern §13、§16）。
但"**离线、只读、随包分发**的常见口令表"与云查询**是两件事**：不联网、不上报、无账户。

**升级动作**：
- 语料以**只读资源**嵌入，与用户保险库**物理隔离**（不写进 `vault.dat`，不参与 DPAPI 库的原子替换）；
- 作为 §1 的 `CorpusSource` 实现，排在 `VaultSource` 之后；
- 默认关闭，开启时 UI 明示"将尝试随包附带的 N 条常见口令"；
- 命中后仍走正常成功关联流程，`CandidateOrigin = CorpusId:<id>`。

**⚠️ 必须先解决的前提**：
**不能直接搬运 APF 的 `MyData.db` 语料** —— 其来源与版权不明，与 MIT 分发不兼容。
需自行整理（公开常见口令表 + 自采），并纳入 README 的第三方材料收集流程。
**建议先做许可评估，再决定是否纳入。**

---

## 7. 长任务的体验缺口：电源策略与速率平滑

**APF 借鉴点**：`PowerUtilities.SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`；
`SimpleMovingAverage` + `CircularList` 做速率平滑。

**你的现状**：
- `睡眠` 只在 architecture-modern §15.3 出现，且是**事后**处理（"系统睡眠/恢复后重建监视基线"）；
  **没有主动抑制休眠** —— 一场数小时的双层解压被系统休眠打断是真实故障源。
- 进度每 100–200 ms 合并刷新，但没有速率/ETA 平滑。

**升级动作**：

| 项 | 方案 |
|---|---|
| 电源 | 任务运行期间用 `PowerCreateRequest` + `PowerSetRequest(PowerRequestSystemRequired)`（比 `SetThreadExecutionState` 现代）；结束/取消/崩溃必释放；提供"允许休眠（依赖恢复语义）"开关，让用户选 |
| ETA | EWMA 或滑动窗口速率 → 稳定 ETA；固实归档/无输出阶段显示"速率不可估"，不显示乱跳的数字 |
| 休眠期间 | 暂停计时器与速率基线，避免把休眠时长算进速率（与你 §15.3 的基线重建保持一致） |

**验收抓手**：`power_request_is_released_on_cancel_and_crash`、`eta_never_shown_when_stalled`。

---

## 8. 优先级矩阵

| 优先级 | 项 | 范围影响 | 理由 |
|---|---|---|---|
| **S（立即可做）** | §2 PasswordProbe 层 | 无扩张 | 性能关键路径，且可独立验收 |
| **S** | §4 文件名编码 | 无扩张 | 真实空白，中文用户高频痛点，成本低 |
| **S** | §7 电源策略 + ETA | 无扩张 | 长任务可靠性缺口，改动小 |
| **A（改扩展点形状）** | §1 候选流水线 + `CandidateOrigin` | 不改范围 | 现在不留字段，未来 outbox 契约要重做 |
| **A** | §3 BackendRouter + Capabilities | 不改范围 | 让未来加后端变成"注册"而非"改核心" |
| **A** | §5 DeviceTopologyProvider | 不改范围 | 把已写对的策略变成可测、可解释的模块 |
| **B（需策略决策）** | §6 离线语料 | 扩功能 | **先过许可审查**；与云查询不同，但仍是新增行为 |
| **B** | §3 中 UnRAR / codec 后端 | 扩依赖 | 与"官方 7-Zip"约束冲突，须重走许可流程 |

---

## 9. 明确不建议照搬的项

| APF 做法 | 不采用的理由 |
|---|---|
| 以完整解压做密码验证 | 已被你 §7 与本文 §2 取代 |
| 每候选 spawn 外部进程 | 你的常驻 Worker + 命名管道架构更好 |
| DNGuard HVM 全量虚拟化 | 与 MIT 开源 / 可构建 / 可验收根本冲突 |
| 密码明文落库（`PasswordEntity.Text`） | 你已用全库 DPAPI，继续保持 |
| `Timer` 轮询调度 | 用事件通道 + 有界队列（你已有） |
| `PerformanceCounter` 逐进程采样 | 用队列深度背压 + 资源许可（你已有） |
| 云端上报文件指纹与密码 | 违反你的离线基线 |
| 静态 `Priority` + 扩展名选后端 | 改为 `Capabilities` + 实测路由（本文 §3） |
| Telerik 臃肿 UI 组件 | 你的 Tauri + React 更合适 |
| 正则/掩码无上限保护（`MaxPasswordSize` 仅记录不拦截） | 若未来做派生候选，必须**先有上界与预算**，否则可被构造为资源耗尽 |

---

## 10. 与现有文档的衔接建议

| 文档 / 章节 | 建议动作 |
|---|---|
| `architecture.md` §4 | `PasswordResolver` 契约补 `CandidateOrigin`；新增 `PasswordProbe` 职责 |
| `architecture.md` §7 | 明确探针与完整解压的**证据边界**（探针不构成成功） |
| `architecture.md` §8 / §12 | 新增 `DeviceTopologyProvider` 与"拓扑不可测 → 串行"的可观测要求 |
| `architecture.md` §9 | 确认 `CandidateOrigin` 纳入 `PasswordSuccessOutbox` 的幂等键 |
| `architecture-modern.md` §4 | `EngineClient` 补 `BackendCapabilities`；新增 `PasswordProbe` |
| `architecture-modern.md` §16 | 把"派生候选"散点规则收敛为 §1 的流水线抽象 |
| `acceptance-checklist.md` | 追加本文 §1/§2/§4/§5/§7 的验收抓手 |
| `README.md` 第三方材料 | 若纳入 §6 语料或 §3 后备引擎，须补许可与源码收集条目 |

---

## 11. 一句话总结

APF 在你的架构版图上留下的**真实空白只有三块**：归档内文件名编码、主动电源策略、候选来源的扩展点形状。
其余六项（探针层、拓扑调度、后端路由、ETA 平滑、离线语料、许可边界）你是"想到了但没落地"或"落地了但没抽象"。

**优先做 §2 + §4 + §7（零范围扩张），同时把 §1 的 `CandidateOrigin` 字段先留出来。**
