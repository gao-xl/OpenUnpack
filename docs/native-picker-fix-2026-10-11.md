# 原生文件选择器与速度功能验收补充

日期：2026-10-11，Asia/Shanghai。

**上一轮错误路径问题已定位并修复；真实 Windows 成品的文件选择回归 4/4、速度功能流程 3/3 通过。整个项目的稳定版审核仍未放行。**

## 根因与修复

问题在桌面测试驱动的控件定位。旧驱动通过 `FindFirst(AutomationId='1')` 查找“打开”，但 `AutomationId` 在整个对话框内不唯一。实际 UIA 树中，许可证文件项排在真正按钮之前：

| ID | 先匹配的控件 | 目标控件 |
|---|---|---|
| 1 | `ListItem / UIItem`，`7zip-License.txt`，HWND=0 | `Button / Button`，“打开”，有效 HWND |
| 2 | `ListItem / UIItem`，`7zip-provenance.json`，HWND=0 | `Button / Button`，“取消”，有效 HWND |

旧驱动执行了文件项的 Invoke，相当于打开该文件，因此 Host 返回许可证路径。诊断时相同缺陷还让“取消”误打开第二个文件。证据：`artifacts/picker-fix-2026-10-11/native-dialog-state/dialog-state.json`。

修正驱动同时约束控件类型 `Button`、原生类 `Button` 和 ID1/2，并要求唯一匹配、非零 HWND、原生控件 ID、目标 PID、属于本次对话框、原生启用状态全部正确，再调用该原生按钮的 `BM_CLICK`。调用方继续严格核对返回数量、完整规范路径和任务登记路径。

可复用驱动：`scripts/native-file-dialog.ps1`，与最终原生检查实际使用的 `native-dialog-confirm.ps1` 字节摘要相同。没有修改产品 Host/UI、原生授权或归档输出逻辑；已有固定源码、旧候选和失败记录均保留。

## 实际成品验证

运行原速度候选的真实 Release EXE、WebView2、包内自包含 Worker/7z.dll。数据目录和浏览器配置隔离，所有归档为合成文件，没有访问个人密码库。

| 场景 | 结果与证据 |
|---|---|
| 两个目录中的中文、空格路径 | 精确返回各自规范路径，未返回旧文件；`native-picker-regression-r2/returned-paths.jsonl` |
| 取消原生选择器 | 返回空数组，没有登记任务 |
| 界面“添加文件”完整入口 | 原生选择→界面扫描结果→添加队列，目标来源和界面显示一致 |
| 真实速度与阶段 ETA | 队列和详情显示实际输出速度、预计本阶段剩余；暂停新任务时当前任务仍运行 |
| 详情关闭 | 关闭后仍停留批量解压页面，`detail-close-route.json` 的 `queueSelected=true` |
| 写回与严格回读 | 77 个真实采样覆盖外层解压17、复制25、目标回读35；各阶段总量为536870912字节 |
| 输出与原包 | 512 MiB 输出完整 SHA-256 等于按已知字节模式生成的期望；64 MiB排除项不存在；原包SHA与原夹具一致 |
| 完成状态 | 核对目标任务行“已完成”，传输观测清空、界面速度不残留 |
| 退出与源码绑定 | 两轮均无前端异常、宿主退出0；302绑定源码文件0漂移，实际二进制与原候选一致 |

选择器结果：`artifacts/picker-fix-2026-10-11/native-picker-regression-r2/results.json`，4/4通过。

速度结果：`artifacts/picker-fix-2026-10-11/native-button-fix-r5/results.json`，3/3通过。输出独立复算：同目录 `output-verification.json`。截图：`queue-speed.png`、`detail-speed-paused.png`、`final.png`。

独立 QA：`artifacts/picker-fix-2026-10-11/qa-review.md`。没有把分组场景数量当作单元测试总数，也没有把本次局部验收当作项目所有功能的验收。

## 调查中保留的失败

早期键盘导航尝试因无可靠前台窗口停止输入；未使用它授予路径。新按钮类型已正确时，UIA Invoke 在本机非交互前台状态不可用，改用经归属验证且启用的真实按钮消息后准确返回路径。

后续测试脚本的归档显示名称和图标按钮可访问名称与界面不匹配，已按真实名称修正。`button-fix-r4` 的后端输出、速度和详情检查通过，但最后发现页面处于设置页，完成状态定位失败；原因未确定，不能声明该轮通过。App源码没有完成/关闭详情自动换页逻辑。最终r5使用原生视口截图，明确核对关闭详情后页面保留，再在目标任务行检查完成状态；全部通过。此前失败日志和截图保留。

退出阶段 stderr 中的 WebView2 `Chrome_WidgetWin_0` 窗口类注销1412诊断保留；两次成功验收的宿主均正常退出0，前端异常数组为空。

## 复现命令与交付

在项目 PowerShell 根目录执行，`$pickerCandidate` 指向原速度候选：

```powershell
$pickerCandidate=(Get-Content artifacts/feature-speed-eta-2026-10-11/candidate-path.txt).Trim()
node artifacts/picker-fix-2026-10-11/native-picker.cjs $pickerCandidate native-picker-regression-r2
node artifacts/picker-fix-2026-10-11/native-speed.cjs $pickerCandidate button-fix-r5
node artifacts/picker-fix-2026-10-11/verify-output.cjs
```

重跑时为前两条命令使用新的结果目录名称，保留此前结果。脚本需本机可用的 Node、PowerShell、UIAutomation 和所记录的 Playwright 运行时；它们属于开发验证工具，程序本身仍是原 Windows 原生候选。

更新后的内部便携包：`artifacts/picker-fix-2026-10-11/OpenUnpack-speed-eta-2026-10-11-windows-x64-native-verified-portable.zip`。包含同一组程序二进制、许可材料和最新报告、关键原生证据；摘要见同目录 `PACKAGE-SHA256.txt`。

本次闭合单文件选择/取消/界面添加和速度流程。文件夹/多选/拖放、完整密码文件导入与备份恢复界面、干净 Windows 安装升级、物理磁盘与 DPI 等剩余验收仍遵循 [发版审核](release-audit-2026-10-11.md)。
