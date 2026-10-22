export const pageMessages: Record<string, [string, string]> = {
  "(原: {name})": ["(original: {name})", "(元：{name})"],
  "扫描跳过与访问提示（{count} 项）": ["Skipped items and access notes ({count} items)", "スキップとアクセスの注意事項（{count} 件）"],
  "确认导入 ({count} 新增)": ["Confirm import ({count} new)", "インポートを確認（{count} 件追加）"],
  "新增: {count}": ["New: {count}", "追加：{count}"],
  "重复: {count}": ["Duplicates: {count}", "重複：{count}"],
  "错误: {count}": ["Errors: {count}", "エラー：{count}"],
  "停止所选任务？": [
    "Stop selected tasks?",
    "選択したタスクを停止しますか？"
  ],
  "将请求停止当前作业。原始压缩包始终保留，现有输出会按恢复规则处理。": [
    "The current jobs will be stopped. Source archives are retained; existing output follows recovery rules.",
    "現在のジョブを停止します。元のアーカイブは保持され、既存の出力は復旧ルールに従って処理されます。"
  ],
  "停止任务": [
    "Stop tasks",
    "タスクを停止"
  ],
  "返回": [
    "Back",
    "戻る"
  ],
  "归档文件": [
    "Archive file",
    "アーカイブファイル"
  ],
  " · {p0} 个分卷": [
    " · {p0} volumes",
    " · {p0} 個の分割ファイル"
  ],
  "状态": [
    "Status",
    "状態"
  ],
  "当前阶段": [
    "Current stage",
    "現在の段階"
  ],
  "密码范围": [
    "Password scope",
    "パスワードの範囲"
  ],
  "全部启用密码": [
    "All enabled passwords",
    "有効なすべてのパスワード"
  ],
  "输出 / 提示": [
    "Output / notes",
    "出力 / 注意事項"
  ],
  "原始归档所在目录": [
    "Source archive directory",
    "元のアーカイブのフォルダー"
  ],
  "操作": [
    "Actions",
    "操作"
  ],
  "详情": [
    "Details",
    "詳細"
  ],
  "输入密码": [
    "Enter password",
    "パスワードを入力"
  ],
  "重试": [
    "Retry",
    "再試行"
  ],
  "停止": [
    "Stop",
    "停止"
  ],
  "退出尚未确认时保留任务和临时缓存；停止完成或重启后处理恢复记录": [
    "Keep tasks and temporary cache until exit is confirmed; handle recovery after stopping or restarting.",
    "終了が確認されるまでタスクと一時キャッシュを保持します。停止完了後または再起動後に復旧記録を処理します。"
  ],
  "尝试删除": [
    "Try deleting",
    "削除を試行"
  ],
  "打开": [
    "Open",
    "開く"
  ],
  "删除任务 {p0}": [
    "Delete task {p0}",
    "タスク {p0} を削除"
  ],
  "正在删除": [
    "Deleting",
    "削除中"
  ],
  "删除任务": [
    "Delete task",
    "タスクを削除"
  ],
  "归档": [
    "Archive",
    "アーカイブ"
  ],
  "大小": [
    "Size",
    "サイズ"
  ],
  "分卷": [
    "Volumes",
    "分割ファイル"
  ],
  "缺少分卷": [
    "Missing volumes",
    "分割ファイルが不足"
  ],
  "{p0} 卷": [
    "{p0} volumes",
    "{p0} 個"
  ],
  "批量解压": [
    "Batch extraction",
    "一括展開"
  ],
  "工具箱": [
    "Toolbox",
    "ツールボックス"
  ],
  "本地目录规范化整理、批量更名与归档平铺提取": [
    "Local directory normalization, batch renaming, and archive flattening.",
    "ローカルディレクトリの正規化、一括リネーム、アーカイブのフラット展開。"
  ],
  "添加压缩文件或文件夹进行批量解压，支持分卷识别与密码匹配。": [
    "Add archives or folders for batch extraction with volume detection and password matching.",
    "アーカイブやフォルダーを追加して一括展開します。分割ファイルの検出とパスワード照合に対応しています。"
  ],
  "添加文件": [
    "Add files",
    "ファイルを追加"
  ],
  "添加文件夹": [
    "Add folder",
    "フォルダーを追加"
  ],
  "当前显示": [
    "Shown",
    "表示中"
  ],
  "处理中": [
    "Processing",
    "処理中"
  ],
  "等待处理": [
    "Pending",
    "待機中"
  ],
  "显示的已完成": [
    "Completed shown",
    "表示中の完了済み"
  ],
  "显示的待处理": [
    "Pending shown",
    "表示中の待機タスク"
  ],
  "显示全部待处理、待核对任务和最近 20 条其他已结束任务": [
    "Shows all pending and unverified tasks plus the latest 20 other finished tasks.",
    "すべての待機中・確認待ちタスクと、その他の終了済みタスクの最新 20 件を表示します。"
  ],
  "保留临时目录的任务始终显示。完整处理记录可在“处理记录”中按页查看；原始任务记录不会因队列显示范围而删除。": [
    "Tasks with retained temporary folders are always shown. View the complete paginated history in History. Queue display limits do not delete task records.",
    "一時フォルダーを保持するタスクは常に表示されます。「処理履歴」で全履歴をページごとに確認できます。キューの表示範囲によって記録が削除されることはありません。"
  ],
  "查看处理记录": [
    "View history",
    "処理履歴を表示"
  ],
  "解压引擎尚未就绪": [
    "Extraction engine is not ready",
    "展開エンジンの準備ができていません"
  ],
  "请确认官方 7-Zip 引擎与作业进程已安装。可管理队列与密码库，开始前需恢复引擎。": [
    "Ensure the official 7-Zip engine and worker are installed. Queue and password management are available; restore the engine before starting.",
    "公式の 7-Zip エンジンとワーカーがインストールされているか確認してください。キューとパスワードは管理できますが、開始前にエンジンの復旧が必要です。"
  ],
  "队列当前处于暂停状态": [
    "The queue is paused",
    "キューは一時停止中です"
  ],
  "新添加的任务已进入等待队列，请点击“继续队列”开始解压。": [
    "New tasks are queued. Click Resume queue to start extraction.",
    "追加したタスクは待機中です。「キューを再開」をクリックして展開を開始してください。"
  ],
  "横幅继续队列": [
    "Resume queue from banner",
    "バナーからキューを再開"
  ],
  "继续队列": [
    "Resume queue",
    "キューを再開"
  ],
  "开始队列": [
    "Start queue",
    "キューを開始"
  ],
  "暂停新任务": [
    "Pause new tasks",
    "新しいタスクを一時停止"
  ],
  "停止所选": [
    "Stop selected",
    "選択項目を停止"
  ],
  "重试所选": [
    "Retry selected",
    "選択項目を再試行"
  ],
  "删除所选任务": [
    "Delete selected tasks",
    "選択したタスクを削除"
  ],
  "删除已完成任务记录并清理临时缓存，原包与已发布结果保留": [
    "Delete finished task records and temporary cache; retain source archives and published output.",
    "終了済みのタスク記録と一時キャッシュを削除します。元のアーカイブと公開済みの結果は保持します。"
  ],
  "删除当前显示的已结束任务": [
    "Delete shown finished tasks",
    "表示中の終了済みタスクを削除"
  ],
  "搜索归档": [
    "Search archives",
    "アーカイブを検索"
  ],
  "搜索文件名或来源路径": [
    "Search file name or source path",
    "ファイル名または元のパスで検索"
  ],
  "任务状态筛选": [
    "Filter task status",
    "タスクの状態で絞り込み"
  ],
  "全部状态": [
    "All statuses",
    "すべての状態"
  ],
  "需要处理": [
    "Needs attention",
    "対応が必要"
  ],
  "已完成": [
    "Completed",
    "完了"
  ],
  "启用": [
    "Enable",
    "有効にする"
  ],
  "关闭": [
    "Close",
    "閉じる"
  ],
  "· 内包": [
    "· nested archives",
    "· 内部アーカイブ"
  ],
  "共 {p0} 层": [
    "{p0} levels",
    "全 {p0} 階層"
  ],
  "保留": [
    "Keep",
    "保持"
  ],
  "目标回读": [
    "Read back destination",
    "出力先を再読み込み"
  ],
  "标准验证": [
    "Standard verification",
    "標準検証"
  ],
  "选择任务 {p0}": [
    "Select task {p0}",
    "タスク {p0} を選択"
  ],
  "暂无解压任务，拖入压缩文件或使用上方按钮添加": [
    "No extraction tasks. Drop archives here or use the buttons above.",
    "展開タスクはありません。アーカイブをドロップするか、上のボタンで追加してください。"
  ],
  "暂无解压任务": [
    "No extraction tasks",
    "展開タスクはありません"
  ],
  "正在扫描来源": [
    "Scanning sources",
    "入力元をスキャン中"
  ],
  "等待扫描停止": [
    "Waiting for scan to stop",
    "スキャンの停止を待機中"
  ],
  "取消扫描": [
    "Cancel scan",
    "スキャンを中止"
  ],
  "正在查找支持的归档并归组分卷。扫描不会验证归档内容是否完整，也不会修改源文件。": [
    "Finding supported archives and grouping volumes. Scanning does not verify archive integrity or modify source files.",
    "対応するアーカイブを検索し、分割ファイルをまとめています。スキャンでは内容の完全性検証や元ファイルの変更は行いません。"
  ],
  "取消请求已发出，等待读取结束。": [
    "Cancellation requested. Waiting for reading to finish.",
    "中止を要求しました。読み込みの完了を待っています。"
  ],
  "扫描规则：{p0}，{p1} 条输入排除规则。": [
    "Scan rules: {p0}; {p1} input exclusion rules.",
    "スキャンルール：{p0}、入力除外ルール {p1} 件。"
  ],
  "检查扫描结果": [
    "Review scan results",
    "スキャン結果を確認"
  ],
  "添加 {p0} 项": [
    "Add {p0} items",
    "{p0} 件を追加"
  ],
  "发现 {p0} 个归档任务": [
    "Found {p0} archive tasks",
    "{p0} 件のアーカイブタスクを検出"
  ],
  "已按分卷自动关联。若提示缺少分卷，请补齐对应文件后再行解压；展开条目可查看具体分卷路径。": [
    "Volumes are grouped automatically. Add missing volumes before extraction. Expand an entry to view volume paths.",
    "分割ファイルは自動で関連付けられます。不足している場合は必要なファイルを揃えてから展開してください。項目を展開すると各パスを確認できます。"
  ],
  "扫描跳过与访问提示（": [
    "Skipped items and access notes (",
    "スキップとアクセスの注意事項（"
  ],
  "项）": [
    "items)",
    "件）"
  ],
  "路径": [
    "Path",
    "パス"
  ],
  "提示": [
    "Notes",
    "注意事項"
  ],
  "输出：": [
    "Output:",
    "出力："
  ],
  "源归档所在目录": [
    "Source archive directory",
    "元アーカイブのフォルダー"
  ],
  "SSD 工作目录": [
    "SSD workspace",
    "SSD 作業フォルダー"
  ],
  "目标卷临时目录": [
    "Destination volume temporary folder",
    "出力先ボリュームの一時フォルダー"
  ],
  "共 {p0} 层展开": [
    "Extract {p0} levels",
    "{p0} 階層まで展開"
  ],
  "保留内包": [
    "Keep nested archives",
    "内部アーカイブを保持"
  ],
  "收起批次选项": [
    "Collapse batch options",
    "バッチオプションを折りたたむ"
  ],
  "本批次选项": [
    "Batch options",
    "このバッチのオプション"
  ],
  "本批次处理选项": [
    "Batch processing options",
    "このバッチの処理オプション"
  ],
  "仅影响本批次；加入队列后设置会冻结。": [
    "Applies to this batch only. Settings are frozen once queued.",
    "このバッチにのみ適用されます。キュー追加後は設定が固定されます。"
  ],
  "输出根目录": [
    "Output root folder",
    "出力ルートフォルダー"
  ],
  "本批次输出目录": [
    "Batch output folder",
    "このバッチの出力フォルダー"
  ],
  "留空在源归档旁创建结果目录": [
    "Leave blank to create a result folder beside the archive",
    "空欄の場合、元のアーカイブと同じ場所に結果フォルダーを作成"
  ],
  "选择": [
    "Select",
    "選択"
  ],
  "清空": [
    "Clear",
    "クリア"
  ],
  "同名目标": [
    "Existing destination",
    "同名の出力先"
  ],
  "新建名称，保留已有内容": [
    "Use a new name and retain existing content",
    "新しい名前を使用し既存の内容を保持"
  ],
  "跳过该任务": [
    "Skip this task",
    "このタスクをスキップ"
  ],
  "密码分组": [
    "Password group",
    "パスワードグループ"
  ],
  "展开内包": [
    "Extract nested archives",
    "内部アーカイブを展開"
  ],
  "总展开层数（含外包）": [
    "Total extraction levels (including outer archive)",
    "展開する階層数（外側を含む）"
  ],
  "先在设置中选择 SSD 工作目录": [
    "Select an SSD workspace in Settings first",
    "先に設定で SSD 作業フォルダーを選択してください"
  ],
  "智能去套娃": [
    "Unwrap single root folder",
    "単一ルートフォルダーを除去"
  ],
  "开启后若压缩包内仅含单个顶层目录，将自动剥离顶层外壳，避免生成同名嵌套文件夹。": [
    "Remove a single top-level folder to avoid redundant nested folders with the same name.",
    "有効にすると、最上位が単一フォルダーの場合にそれを除去し、同名フォルダーの重複を防ぎます。"
  ],
  "验证方式": [
    "Verification method",
    "検証方法"
  ],
  "标准流程验证": [
    "Standard verification",
    "標準検証"
  ],
  "完整目标回读": [
    "Full destination readback",
    "出力先の完全な再読み込み"
  ],
  "完成后源包": [
    "Source archive after completion",
    "完了後の元アーカイブ"
  ],
  "源文件清理范围提示": [
    "Source cleanup scope",
    "元ファイルのクリーンアップ範囲"
  ],
  "解压完全成功并校验通过后，将清理当前所选 {p0} 个任务的全部源文件（共 {p1} 个分卷文件）。若解压失败则绝不处理源文件。": [
    "After successful extraction and verification, all source files for the selected {p0} tasks ({p1} volume files) will be cleaned up. Failed extraction never changes source files.",
    "展開と検証が完全に成功すると、選択した {p0} タスクのすべての元ファイル（分割ファイル計 {p1} 個）をクリーンアップします。展開失敗時に元ファイルを処理することはありません。"
  ],
  "添加后自动开始": [
    "Start automatically after adding",
    "追加後に自動開始"
  ],
  "输入解压密码 — {p0}": [
    "Enter extraction password — {p0}",
    "展開パスワードを入力 — {p0}"
  ],
  "解密并重试": [
    "Decrypt and retry",
    "復号して再試行"
  ],
  "取消": [
    "Cancel",
    "キャンセル"
  ],
  "{p0} 密码": [
    "{p0} password",
    "{p0} のパスワード"
  ],
  "解压即时录入": [
    "Entered during extraction",
    "展開時に入力"
  ],
  "该压缩文件受密码保护。请输入解压密码，输入后将立即重新调度此任务。": [
    "This archive is password protected. Enter its password to reschedule the task immediately.",
    "このアーカイブはパスワードで保護されています。入力するとタスクをすぐに再スケジュールします。"
  ],
  "即时解压密码": [
    "Extraction password",
    "展開パスワード"
  ],
  "输入解压密码": [
    "Enter extraction password",
    "展開パスワードを入力"
  ],
  "同时保存到本地加密密码库（推荐，后续同批次或分卷可自动匹配）": [
    "Also save to the local encrypted vault (recommended for matching later archives and volumes)",
    "暗号化したローカル保管庫にも保存（後続のアーカイブや分割ファイルの自動照合に推奨）"
  ],
  "读取任务详情失败，请重试。": [
    "Could not load task details. Please retry.",
    "タスクの詳細を読み込めませんでした。再試行してください。"
  ],
  "读取历史记录失败": [
    "Could not load history",
    "履歴を読み込めませんでした"
  ],
  "来源名称与路径": [
    "Source name and path",
    "元の名前とパス"
  ],
  "校验方式": [
    "Verification",
    "検証方法"
  ],
  "完成时间": [
    "Finished at",
    "完了日時"
  ],
  "打开结果": [
    "Open output",
    "結果を開く"
  ],
  "回读并核对": [
    "Read back and verify",
    "再読み込みして検証"
  ],
  "重试写回": [
    "Retry publishing",
    "書き戻しを再試行"
  ],
  "清理保留的临时结果？": [
    "Clean up retained temporary output?",
    "保持された一時結果を削除しますか？"
  ],
  "仅清理本应用标记的工作目录与临时目录，已发布结果与原始压缩包不会因此删除。": [
    "Only workspaces and temporary folders marked by this app are cleaned. Published output and source archives are retained.",
    "このアプリがマークした作業・一時フォルダーのみを削除します。公開済みの結果と元のアーカイブは保持します。"
  ],
  "清理临时目录": [
    "Clean up temporary folder",
    "一時フォルダーを削除"
  ],
  "删除任务 ": [
    "Delete task ",
    "タスクを削除 "
  ],
  "处理记录": [
    "History",
    "処理履歴"
  ],
  "按页查看最近": [
    "Browse the latest",
    "最新の"
  ],
  "条处理记录，显示上限可在设置中调整。详情在打开时读取。": [
    "records by page. Adjust the limit in Settings. Details load when opened.",
    "件をページごとに表示します。表示上限は設定で変更でき、詳細は開く際に読み込まれます。"
  ],
  "最近记录总数": [
    "Recent records",
    "最近の記録数"
  ],
  "处理失败": [
    "Failed",
    "処理失敗"
  ],
  "可恢复/待核对": [
    "Recoverable / unverified",
    "復旧可能 / 確認待ち"
  ],
  "（已保留显示此前读取的记录）": [
    "(Previously loaded records are retained)",
    "(以前読み込んだ記録を表示しています)"
  ],
  "重试读取": [
    "Reload",
    "再読み込み"
  ],
  "读取任务详情失败": [
    "Could not load task details",
    "タスクの詳細を読み込めませんでした"
  ],
  "重试详情": [
    "Retry details",
    "詳細を再読み込み"
  ],
  "搜索文件路径或名称": [
    "Search file path or name",
    "ファイルのパスまたは名前で検索"
  ],
  "刷新记录": [
    "Refresh history",
    "履歴を更新"
  ],
  "清理历史记录": [
    "Clear history",
    "履歴を削除"
  ],
  "仅清除历史任务记录，已提取/生成的文件保留，确定清空吗？": [
    "Clear task history? Extracted and generated files will be retained.",
    "タスク履歴を削除しますか？展開・生成されたファイルは保持されます。"
  ],
  "历史记录已清空": [
    "History cleared",
    "履歴を削除しました"
  ],
  "清空历史记录": [
    "Clear history",
    "履歴をすべて削除"
  ],
  "全选命中项": [
    "Select all matches",
    "一致する項目をすべて選択"
  ],
  "删除筛选结果 ({count})": [
    "Delete filtered results ({count})",
    "絞り込み結果を削除 ({count})"
  ],
  "删除当前筛选的 {count} 条记录": [
    "Delete the {count} filtered records",
    "絞り込んだ {count} 件の記録を削除"
  ],
  "只删除符合当前筛选条件的记录，被筛选隐藏的任务会保留；已提取的文件不会删除。": [
    "Only records matching the current filter are removed; tasks hidden by the filter stay, and extracted files are never deleted.",
    "現在の絞り込み条件に一致する記録だけを削除します。絞り込みで隠れているタスクは残り、抽出済みのファイルは削除されません。"
  ],
  "选择任务 ": [
    "Select task ",
    "タスクを選択 "
  ],
  "暂无历史记录": [
    "No history",
    "履歴はありません"
  ],
  "密码已复制，剪贴板将在 30 秒后自动清除（已排除系统历史与云同步；第三方工具可能不受约束）。": [
    "Password copied. The clipboard will clear in 30 seconds. System history and cloud sync are excluded; third-party tools may still capture it.",
    "コピーしました。30 秒後にクリップボードを消去します。システム履歴とクラウド同期は対象外にしていますが、他社ツールには適用されない場合があります。"
  ],
  "密码读取失败，请关闭后重试。": [
    "Could not read the password. Close and retry.",
    "パスワードを読み込めませんでした。閉じて再試行してください。"
  ],
  "切换 {p0} 启用状态": [
    "Toggle {p0}",
    "{p0} の有効状態を切り替え"
  ],
  "名称/标识": [
    "Name / identifier",
    "名前 / 識別子"
  ],
  "分组": [
    "Groups",
    "グループ"
  ],
  "备注": [
    "Notes",
    "メモ"
  ],
  "成功次数": [
    "Successes",
    "成功回数"
  ],
  "上次使用": [
    "Last used",
    "最終使用"
  ],
  "复制 {p0} 密码": [
    "Copy password for {p0}",
    "{p0} のパスワードをコピー"
  ],
  "查看密码": [
    "View password",
    "パスワードを表示"
  ],
  "编辑 {p0}": [
    "Edit {p0}",
    "{p0} を編集"
  ],
  "确认删除该密码条目吗？": [
    "Delete this password entry?",
    "このパスワードを削除しますか？"
  ],
  "删除 {p0}": [
    "Delete {p0}",
    "{p0} を削除"
  ],
  "密码库": [
    "Password vault",
    "パスワード保管庫"
  ],
  "按分组管理候选密码。列表与处理记录只显示标识，密码由当前 Windows 账户保护。": [
    "Manage candidate passwords by group. Lists and history show identifiers only. Passwords are protected by your Windows account.",
    "候補のパスワードをグループで管理します。一覧と履歴には識別子のみが表示され、パスワードは現在の Windows アカウントで保護されます。"
  ],
  "按名称、分组或备注搜索...": [
    "Search name, group or notes...",
    "名前、グループ、メモで検索..."
  ],
  "搜索密码条目": [
    "Search passwords",
    "パスワードを検索"
  ],
  "密码分组筛选": [
    "Filter password group",
    "パスワードグループで絞り込み"
  ],
  "全部分组": [
    "All groups",
    "すべてのグループ"
  ],
  "密码启用状态筛选": [
    "Filter enabled status",
    "有効状態で絞り込み"
  ],
  "已启用": [
    "Enabled",
    "有効"
  ],
  "已停用": [
    "Disabled",
    "無効"
  ],
  "批量导入": [
    "Bulk import",
    "一括インポート"
  ],
  "添加密码": [
    "Add password",
    "パスワードを追加"
  ],
  "临时剪贴板": [
    "Temporary clipboard",
    "一時クリップボード"
  ],
  "共 {p0} 条": [
    "{p0} entries",
    "全 {p0} 件"
  ],
  "编辑密码条目": [
    "Edit password entry",
    "パスワードを編集"
  ],
  "新建密码条目": [
    "New password entry",
    "パスワードを新規作成"
  ],
  "保存": [
    "Save",
    "保存"
  ],
  "名称 / 标识": [
    "Name / identifier",
    "名前 / 識別子"
  ],
  "请输入密码名称或标识": [
    "Enter a password name or identifier",
    "パスワードの名前または識別子を入力してください"
  ],
  "例如：日常解压密码": [
    "For example: everyday archive password",
    "例：普段使う展開パスワード"
  ],
  "密码明文": [
    "Password",
    "パスワード（平文）"
  ],
  "请输入密码": [
    "Enter a password",
    "パスワードを入力してください"
  ],
  "密码不支持 NUL、换行或回车。": [
    "Passwords cannot contain NUL or line breaks.",
    "パスワードに NUL や改行を使用できません。"
  ],
  "{p0}首尾空格与大小写会完整保留；仅支持单行密码。": [
    "{p0}Leading/trailing spaces and letter case are preserved. Single-line passwords only.",
    "{p0}前後の空白と大文字・小文字は保持されます。1 行のパスワードのみ対応します。"
  ],
  "留空保持原密码。": [
    "Leave blank to keep the current password. ",
    "空欄の場合、現在のパスワードを保持します。"
  ],
  "密码明文，保存后加密存储": [
    "Enter password; encrypted when saved",
    "パスワードを入力（保存時に暗号化）"
  ],
  "分组标签": [
    "Group tags",
    "グループタグ"
  ],
  "选择或输入标签后回车": [
    "Select or enter a tag and press Enter",
    "タグを選択または入力して Enter"
  ],
  "备注信息": [
    "Notes",
    "メモ"
  ],
  "来源说明或其他备注": [
    "Source details or other notes",
    "出典などのメモ"
  ],
  "启用状态": [
    "Enabled",
    "有効状態"
  ],
  "查看密码 - {p0}": [
    "View password - {p0}",
    "パスワードを表示 - {p0}"
  ],
  "安全性提示": [
    "Security notice",
    "セキュリティに関する注意"
  ],
  "密码将在 {p0} 秒后隐藏。窗口失焦或隐藏时立即清空显示。": [
    "Password hides in {p0} seconds. It clears immediately when the window loses focus or is hidden.",
    "パスワードは {p0} 秒後に非表示になります。ウィンドウがフォーカスを失うか非表示になると、直ちに表示を消去します。"
  ],
  "正在读取安全密码...": [
    "Reading password securely...",
    "パスワードを安全に読み込み中..."
  ],
  "批量导入密码": [
    "Bulk import passwords",
    "パスワードを一括インポート"
  ],
  "确认导入": [
    "Confirm import",
    "インポートを確認"
  ],
  "({p0} 新增)": [
    "({p0} new)",
    "({p0} 件追加)"
  ],
  "格式：": [
    "Format:",
    "形式："
  ],
  "导入文本格式": [
    "Import text format",
    "インポートするテキスト形式"
  ],
  "TXT：每行一个密码": [
    "TXT: one password per line",
    "TXT：1 行に 1 パスワード"
  ],
  "TSV：名称、密码、分组、备注": [
    "TSV: name, password, groups, notes",
    "TSV：名前、パスワード、グループ、メモ"
  ],
  "CSV：逗号分隔 / 自定义列映射": [
    "CSV: comma-separated / custom columns",
    "CSV：カンマ区切り / 列のカスタマイズ"
  ],
  "默认分组：": [
    "Default group:",
    "既定のグループ："
  ],
  "导入默认分组": [
    "Default import group",
    "インポート時の既定グループ"
  ],
  "默认分组（可选）": [
    "Default group (optional)",
    "既定のグループ（任意）"
  ],
  "读取前选择编码：": [
    "Choose encoding before reading:",
    "読み込み前に文字コードを選択："
  ],
  "导入文件编码": [
    "Import file encoding",
    "インポートファイルの文字コード"
  ],
  "选择文件": [
    "Choose file",
    "ファイルを選択"
  ],
  "待导入密码文本": [
    "Password text to import",
    "インポートするパスワードテキスト"
  ],
  "粘贴待导入密码内容...": [
    "Paste passwords to import...",
    "インポートするパスワードを貼り付け..."
  ],
  "密码按原样保存": [
    "Passwords are preserved exactly",
    "パスワードはそのまま保存されます"
  ],
  "TXT 格式每行一个密码，空行自动忽略；首尾空格与大小写完整保留。": [
    "TXT: one password per line. Blank lines are ignored; spaces and letter case are preserved.",
    "TXT は 1 行に 1 パスワードです。空行は無視され、前後の空白と大文字・小文字は保持されます。"
  ],
  "TSV 格式每行四列，使用 Tab 分隔：名称、密码、分组、备注；分组可用逗号分隔。无需包含表头。": [
    "TSV: four tab-separated columns: name, password, groups, notes. Separate groups with commas. No header required.",
    "TSV はタブ区切りの 4 列（名前、パスワード、グループ、メモ）です。グループはカンマ区切りにできます。ヘッダーは不要です。"
  ],
  "CSV 格式支持逗号分隔、带双引号转义；支持首行表头自动识别密码列，或按 [名称, 密码, 分组, 备注] 顺序排列。": [
    "CSV supports comma-separated values and double-quote escaping. Detect password columns from headers, or use [name, password, groups, notes].",
    "CSV はカンマ区切りと二重引用符によるエスケープに対応します。ヘッダーからパスワード列を自動検出するか、[名前、パスワード、グループ、メモ] の順に並べてください。"
  ],
  "生成导入预览": [
    "Preview import",
    "インポートをプレビュー"
  ],
  "预览结果": [
    "Preview results",
    "プレビュー結果"
  ],
  "新增:": [
    "New:",
    "追加："
  ],
  "重复:": [
    "Duplicates:",
    "重複："
  ],
  "错误:": [
    "Errors:",
    "エラー："
  ],
  "存在解析错误条目，必须先纠正或清理错误行方可执行导入。": [
    "Fix or remove rows with parsing errors before importing.",
    "インポートする前に、解析エラーのある行を修正または削除してください。"
  ],
  "行号": [
    "Line",
    "行番号"
  ],
  "标识/名称": [
    "Identifier / name",
    "識別子 / 名前"
  ],
  "说明": [
    "Description",
    "説明"
  ],
  "替换所选密码的分组？": [
    "Replace groups for selected passwords?",
    "選択したパスワードのグループを置き換えますか？"
  ],
  "将所选条目的分组替换为：{p0}。其他条目不受影响。": [
    "Replace the selected entries' groups with: {p0}. Other entries are unaffected.",
    "選択した項目のグループを {p0} に置き換えます。他の項目には影響しません。"
  ],
  "替换分组": [
    "Replace groups",
    "グループを置き換え"
  ],
  "删除所选密码？": [
    "Delete selected passwords?",
    "選択したパスワードを削除しますか？"
  ],
  "成功关联也会移除，处理记录不包含密码。": [
    "Successful associations are also removed. History does not contain passwords.",
    "成功時の関連付けも削除されます。処理履歴にパスワードは含まれません。"
  ],
  "删除": [
    "Delete",
    "削除"
  ],
  "密码批量操作工具栏": [
    "Bulk password actions",
    "パスワード一括操作ツールバー"
  ],
  "已选择": [
    "Selected",
    "選択済み"
  ],
  "条": [
    "entries",
    "件"
  ],
  "清空选择": [
    "Clear selection",
    "選択を解除"
  ],
  "启用所选密码": [
    "Enable selected passwords",
    "選択したパスワードを有効化"
  ],
  "禁用所选密码": [
    "Disable selected passwords",
    "選択したパスワードを無効化"
  ],
  "禁用": [
    "Disable",
    "無効にする"
  ],
  "选择或输入分组": [
    "Select or enter groups",
    "グループを選択または入力"
  ],
  "选择或创建分组标签": [
    "Select or create group tags",
    "グループタグを選択または作成"
  ],
  "替换所选密码的分组": [
    "Replace selected passwords' groups",
    "選択したパスワードのグループを置き換え"
  ],
  "清空所选密码的分组": [
    "Clear selected passwords' groups",
    "選択したパスワードのグループをクリア"
  ],
  "清空所选密码的分组？": [
    "Clear groups for selected passwords?",
    "選択したパスワードのグループをクリアしますか？"
  ],
  "这些密码仍保留并可在全部启用密码范围使用。": [
    "Passwords are retained and remain available under all enabled passwords.",
    "パスワードは保持され、有効なすべてのパスワードの範囲で利用できます。"
  ],
  "清空分组": [
    "Clear groups",
    "グループをクリア"
  ],
  "删除所选密码": [
    "Delete selected passwords",
    "選択したパスワードを削除"
  ],
  "原始名称: {p0}": [
    "Original name: {p0}",
    "元の名前：{p0}"
  ],
  "(原:": [
    "(original:",
    "(元："
  ],
  "大小未知": [
    "Unknown size",
    "サイズ不明"
  ],
  "加密": [
    "Encrypted",
    "暗号化"
  ],
  "停止归档检查未确认，请稍后核对后台状态。": [
    "Archive inspection stop is not confirmed. Check the worker status later.",
    "アーカイブ検査の停止が確認できません。後でバックグラウンドの状態を確認してください。"
  ],
  "项": [
    "items",
    "件"
  ],
  "共 {p0}": [
    "Total {p0}",
    "合計 {p0}"
  ],
  "总展开大小未知": [
    "Total extracted size unknown",
    "展開後の合計サイズは不明"
  ],
  "编码:": [
    "Encoding:",
    "文字コード："
  ],
  "(自动修正)": [
    "(automatically corrected)",
    "(自動修正)"
  ],
  "命中": [
    "Matches",
    "一致"
  ],
  "归档预览确认密码": [
    "Password confirmed by archive preview",
    "アーカイブのプレビューで確認したパスワード"
  ],
  "浏览归档内容 — {p0}": [
    "Browse archive — {p0}",
    "アーカイブを参照 — {p0}"
  ],
  "应用此密码并重试解压": [
    "Apply password and retry extraction",
    "このパスワードで展開を再試行"
  ],
  "浏览器预览模式": [
    "Browser preview mode",
    "ブラウザープレビューモード"
  ],
  "归档浏览需要在桌面版中运行。": [
    "Archive browsing requires the desktop app.",
    "アーカイブの参照にはデスクトップ版が必要です。"
  ],
  "此归档需要密码才能列出内容": [
    "A password is required to list this archive",
    "このアーカイブの一覧表示にはパスワードが必要です"
  ],
  "归档密码": [
    "Archive password",
    "アーカイブのパスワード"
  ],
  "输入归档密码后重试": [
    "Enter archive password to retry",
    "アーカイブのパスワードを入力して再試行"
  ],
  "使用该密码浏览": [
    "Browse with this password",
    "このパスワードで参照"
  ],
  "无法读取归档内容": [
    "Could not read archive contents",
    "アーカイブの内容を読み込めません"
  ],
  "等待可用资源或读取归档目录中…": [
    "Waiting for resources or reading archive directory…",
    "リソースを待機中、またはアーカイブ一覧を読み込み中…"
  ],
  "搜索归档条目": [
    "Search archive entries",
    "アーカイブ項目を検索"
  ],
  "按名称或路径搜索，例如 folder/ 或 *.txt": [
    "Search name or path, e.g. folder/ or *.txt",
    "名前やパスで検索（例：folder/、*.txt）"
  ],
  "归档条目树": [
    "Archive entry tree",
    "アーカイブ項目ツリー"
  ],
  "没有匹配的条目": [
    "No matching entries",
    "一致する項目はありません"
  ],
  "归档内没有条目": [
    "This archive is empty",
    "アーカイブに項目はありません"
  ],
  "发布结果尚未核对，请先回读并核对或重试写回，再删除任务。": [
    "Verify the published output by reading it back or retry publishing before deleting the task.",
    "結果が未確認です。再読み込みで検証するか、書き戻しを再試行してからタスクを削除してください。"
  ],
  "正在等待后台进程停止，请停止完成后再删除任务。": [
    "Waiting for the worker to stop. Delete the task after it has stopped.",
    "バックグラウンドプロセスの停止を待っています。停止完了後にタスクを削除してください。"
  ],
  "未能删除 {p0} 个任务。{p1}": [
    "Could not delete {p0} tasks. {p1}",
    "{p0} 件のタスクを削除できませんでした。{p1}"
  ],
  "删除 {p0} 个任务？": [
    "Delete {p0} tasks?",
    "{p0} 件のタスクを削除しますか？"
  ],
  "删除任务？": [
    "Delete task?",
    "タスクを削除しますか？"
  ],
  "重试删除": [
    "Retry deletion",
    "削除を再試行"
  ],
  "运行中的任务将先停止，确认后台进程停止后再删除任务记录，并清理本应用的临时缓存。": [
    "Running tasks will stop first. Once the worker has stopped, task records and app temporary cache are deleted.",
    "実行中のタスクを先に停止します。バックグラウンドプロセスの停止確認後、タスク記録とアプリの一時キャッシュを削除します。"
  ],
  "任务记录将同时从队列和处理记录中移除。": [
    "Task records are removed from both the queue and history.",
    "タスク記録はキューと処理履歴の両方から削除されます。"
  ],
  "原始压缩包和已经发布到目标目录的结果会保留。": [
    "Source archives and output already published to the destination are retained.",
    "元のアーカイブと、出力先に公開済みの結果は保持されます。"
  ],
  "删除任务不会删除这些文件，也不能恢复此前已执行的源包清理。": [
    "Deleting tasks does not delete these files or undo previous source archive cleanup.",
    "タスクの削除ではこれらのファイルは削除されません。実行済みの元アーカイブのクリーンアップも元に戻せません。"
  ],
  "正在等待任务停止并清理缓存，请稍候。": [
    "Waiting for tasks to stop and clearing cache. Please wait.",
    "タスクの停止とキャッシュの削除を待っています。しばらくお待ちください。"
  ],
  "这些任务未删除，可重试或取消": [
    "These tasks were not deleted. Retry or cancel.",
    "これらのタスクは削除されていません。再試行またはキャンセルしてください。"
  ],
  "请先选择要整理的目标文件夹": [
    "Select a folder to organize first",
    "先に整理するフォルダーを選択してください"
  ],
  "扫描完成，共扫描 {p0} 个文件，未发现需要调整的项目。": [
    "Scanned {p0} files. No changes are needed.",
    "{p0} ファイルをスキャンしました。変更が必要な項目はありません。"
  ],
  "分析完成：发现 {p0} 项可调整文件，其中 {p1} 项检测到同名冲突。": [
    "Analysis complete: {p0} files can be changed; {p1} have name conflicts.",
    "解析完了：変更可能なファイル {p0} 件、うち名前の競合 {p1} 件。"
  ],
  "没有可执行的变更项": [
    "No changes to apply",
    "実行できる変更はありません"
  ],
  "成功调整: {p0} 个文件": [
    "Changed: {p0} files",
    "変更成功：{p0} ファイル"
  ],
  "跳过: {p0}": [
    "Skipped: {p0}",
    "スキップ：{p0}"
  ],
  "清理空目录: {p0} 个": [
    "Empty folders removed: {p0}",
    "空フォルダー削除：{p0} 件"
  ],
  "失败: {p0} 项": [
    "Failed: {p0}",
    "失敗：{p0} 件"
  ],
  "操作全部完成！{p0}": [
    "All operations completed. {p0}",
    "すべての操作が完了しました。{p0}"
  ],
  "操作部分完成。{p0}。错误: {p1}": [
    "Partially completed. {p0}. Errors: {p1}",
    "一部の操作が完了しました。{p0}。エラー：{p1}"
  ],
  "原文件 / 相对路径": [
    "Original file / relative path",
    "元のファイル / 相対パス"
  ],
  "拟议新名称 / 操作": [
    "Proposed name / action",
    "変更後の名前 / 操作"
  ],
  "重命名": [
    "Rename",
    "名前を変更"
  ],
  "平铺移动": [
    "Flatten and move",
    "平坦化して移動"
  ],
  "跳过": [
    "Skip",
    "スキップ"
  ],
  "冲突已处理": [
    "Conflict resolved",
    "競合を解決済み"
  ],
  "安全": [
    "Safe",
    "安全"
  ],
  "扩展名规范化": [
    "Normalize extensions",
    "拡張子の正規化"
  ],
  "智能批量更名": [
    "Smart batch rename",
    "一括名前変更"
  ],
  "平铺提取与整理": [
    "Flatten and organize",
    "平坦化して整理"
  ],
  "源目录路径 (严格 NativeAccess 授权)": [
    "Source folder (strict NativeAccess authorization)",
    "元フォルダー（厳格な NativeAccess 認可）"
  ],
  "请选择本地目录，系统将通过权限守卫隔离执行...": [
    "Select a local folder; execution is isolated by access guards...",
    "ローカルフォルダーを選択してください。アクセス保護により実行が分離されます..."
  ],
  "选择文件夹": [
    "Choose folder",
    "フォルダーを選択"
  ],
  "自动规范化归档扩展名": [
    "Automatically normalize archive extensions",
    "アーカイブの拡張子を自動正規化"
  ],
  "智能识别修复被错误切割或双重后缀的复合归档（如 .tar.gz.tar 修正为 .tar.gz、.tgz 规范为 .tar.gz），并通过头部魔数嗅探校验真实归档格式（7z/ZIP/RAR/TAR/ZSTD/LZ4），规范缺少后缀或命名错乱的文件。": [
    "Repair malformed or doubled archive extensions (e.g. .tar.gz.tar and .tgz to .tar.gz). Detect actual formats using file signatures (7z/ZIP/RAR/TAR/ZSTD/LZ4) to correct missing or invalid extensions.",
    "不正な拡張子や重複した拡張子を修正します（例：.tar.gz.tar や .tgz を .tar.gz に統一）。ファイル署名で実際の形式（7z/ZIP/RAR/TAR/ZSTD/LZ4）を検出し、欠落・誤記した拡張子を補正します。"
  ],
  "更名规则配置": [
    "Rename rules",
    "名前変更ルール"
  ],
  "查找文本 / 正则": [
    "Find text / regular expression",
    "検索テキスト / 正規表現"
  ],
  "留空则不执行替换": [
    "Leave blank to skip replacement",
    "空欄の場合は置換しません"
  ],
  "替换为": [
    "Replace with",
    "置換後"
  ],
  "替换目标文本": [
    "Replacement text",
    "置換するテキスト"
  ],
  "匹配设置": [
    "Matching options",
    "照合設定"
  ],
  "区分大小写": [
    "Case sensitive",
    "大文字・小文字を区別"
  ],
  "忽略大小写": [
    "Ignore case",
    "大文字・小文字を無視"
  ],
  "正则匹配": [
    "Regular expression",
    "正規表現"
  ],
  "普通文本": [
    "Plain text",
    "通常のテキスト"
  ],
  "前缀": [
    "Prefix",
    "接頭辞"
  ],
  "在文件名开头添加": [
    "Add to start of file name",
    "ファイル名の先頭に追加"
  ],
  "后缀": [
    "Suffix",
    "接尾辞"
  ],
  "在扩展名前添加": [
    "Add before extension",
    "拡張子の前に追加"
  ],
  "启用自动编号": [
    "Auto numbering on",
    "自動連番オン"
  ],
  "禁用自动编号": [
    "Auto numbering off",
    "自動連番オフ"
  ],
  "起始序号:": [
    "Start number:",
    "開始番号："
  ],
  "位数 (补零):": [
    "Digits (zero padding):",
    "桁数（ゼロ埋め）："
  ],
  "平铺提取选项": [
    "Flatten options",
    "平坦化オプション"
  ],
  "平铺目标目录 (留空则平铺至源目录根路径)": [
    "Destination folder (blank uses source root)",
    "移動先フォルダー（空欄の場合は元のルート）"
  ],
  "默认提取至源目录根路径...": [
    "Defaults to source root folder...",
    "既定では元のルートフォルダーへ移動..."
  ],
  "自定义目标": [
    "Custom destination",
    "移動先を指定"
  ],
  "清除": [
    "Clear",
    "クリア"
  ],
  "提取后自动清理空目录": [
    "Remove empty folders after extraction",
    "移動後に空フォルダーを自動削除"
  ],
  "自动清理": [
    "Auto cleanup",
    "自動削除"
  ],
  "保留空目录": [
    "Keep empty folders",
    "空フォルダーを保持"
  ],
  "包含子文件夹 (递归扫描)": [
    "Include subfolders (recursive scan)",
    "サブフォルダーを含む（再帰スキャン）"
  ],
  "包含": [
    "Include",
    "含める"
  ],
  "仅根目录": [
    "Root only",
    "ルートのみ"
  ],
  "同名冲突策略": [
    "Name conflict policy",
    "同名競合の処理"
  ],
  "自动追加序号 (_1, _2)": [
    "Append numbers (_1, _2)",
    "連番を追加（_1、_2）"
  ],
  "跳过冲突文件": [
    "Skip conflicting files",
    "競合ファイルをスキップ"
  ],
  "报错并中止": [
    "Report error and stop",
    "エラーを表示して中止"
  ],
  "分析并预览改动 (Dry-Run)": [
    "Analyze and preview changes (dry run)",
    "解析して変更をプレビュー（試行）"
  ],
  "改动预览与核对": [
    "Review changes",
    "変更を確認"
  ],
  "共发现": [
    "Found",
    "検出数"
  ],
  "项需要变更": [
    "items to change",
    "件の変更対象"
  ],
  "项命名冲突": [
    "name conflicts",
    "件の名前競合"
  ],
  "确认并执行改动 (": [
    "Confirm and apply changes (",
    "確認して変更を実行（"
  ],
  "二次确认：执行本地目录整理": [
    "Confirm local folder organization",
    "最終確認：ローカルフォルダーの整理"
  ],
  "确认立即执行": [
    "Apply now",
    "今すぐ実行"
  ],
  "此操作将直接重命名或移动本地磁盘上的文件": [
    "This operation directly renames or moves files on disk",
    "この操作はディスク上のファイルの名前変更や移動を直接行います"
  ],
  "改动执行后无法自动撤销。已自动通过 NativeAccess 权限模型校验目录边界，禁止跨越软链接及系统目录。": [
    "Changes cannot be undone automatically. NativeAccess validates folder boundaries and blocks symbolic links and system folders.",
    "変更は自動で元に戻せません。NativeAccess でフォルダー境界を検証し、シンボリックリンクやシステムフォルダーへの移動を禁止します。"
  ],
  "操作类型：": [
    "Operation:",
    "操作の種類："
  ],
  "待处理文件数：": [
    "Files to process:",
    "処理対象ファイル数："
  ],
  "冲突处理机制：": [
    "Conflict handling:",
    "競合の処理："
  ],
  "自动追加数字后缀 (_1, _2) 规避冲突": [
    "Append numeric suffixes (_1, _2) to avoid conflicts",
    "数字の接尾辞（_1、_2）を追加して競合を回避"
  ],
  "跳过目标已存在的冲突文件": [
    "Skip files whose destination already exists",
    "移動先が存在するファイルをスキップ"
  ],
  "发生冲突时报错中止": [
    "Stop with an error on conflict",
    "競合時にエラーで中止"
  ],
  "后续动作：": [
    "Afterwards:",
    "後続の操作："
  ],
  "将自动清理平铺提取后留下的空子目录": [
    "Automatically remove empty subfolders left after flattening",
    "平坦化後に残る空のサブフォルダーを自動削除"
  ],
  "按页查看最近 {count} 条处理记录，显示上限可在设置中调整。详情在打开时读取。": [
    "Browse the latest {count} records by page. Adjust the limit in Settings. Details load when opened.",
    "最新 {count} 件の処理履歴をページごとに表示します。表示上限は設定で変更でき、詳細は開く際に読み込まれます。"
  ],
  "已选择 {count} 条": [
    "Selected {count} entries",
    "{count} 件を選択中"
  ],
  "共发现 {count} 项需要变更": [
    "{count} items to change",
    "変更対象 {count} 件"
  ],
  "包含 {count} 项命名冲突": [
    "{count} name conflicts",
    "名前の競合 {count} 件"
  ],
  "确认并执行改动 ({count})": [
    "Confirm and apply changes ({count})",
    "確認して変更を実行（{count} 件）"
  ],
  "{count} 项": [
    "{count} items",
    "{count} 件"
  ],
  "加密 {count} 项": [
    "{count} encrypted items",
    "暗号化 {count} 件"
  ],
  "编码: {encoding} (自动修正)": [
    "Encoding: {encoding} (automatically corrected)",
    "文字コード：{encoding}（自動修正）"
  ],
  "命中 {matched} / {total} 项": [
    "Matches: {matched} / {total}",
    "一致：{matched} / {total} 件"
  ],
  "任务完成，即将自动关机": [
    "Tasks completed, shutting down soon",
    "タスク完了、間もなく自動シャットダウンします"
  ],
  "任务完成，即将进入休眠": [
    "Tasks completed, going to sleep soon",
    "タスク完了、間もなくスリープ状態に入ります"
  ],
  "取消操作": [
    "Cancel",
    "キャンセル"
  ],
  "立即执行": [
    "Execute now",
    "今すぐ実行"
  ],
  "所有解压任务已完成。计算机将在倒计时结束后自动关机，按“取消操作”可终止。": [
    "All tasks completed. The computer will shut down after the countdown. Click Cancel to abort.",
    "すべての解凍タスクが完了しました。カウントダウン終了後に自動シャットダウンします。「キャンセル」で中止できます。"
  ],
  "所有解压任务已完成。计算机将在倒计时结束后进入休眠，按“取消操作”可终止。": [
    "All tasks completed. The computer will sleep after the countdown. Click Cancel to abort.",
    "すべての解凍タスクが完了しました。カウントダウン終了後にスリープ状态に入ります。「キャンセル」で中止できます。"
  ],
  "指定密码重试": [
    "Retry with password",
    "パスワードを指定して再試行"
  ],
  "已将选择性提取任务添加至队列": [
    "Selective extraction task added to queue",
    "選択的展開タスクをキューに追加しました"
  ],
  "提取所选条目 ({count})": [
    "Extract selected ({count})",
    "選択した項目を展開 ({count})"
  ],
  "提取所选至...": [
    "Extract selected to...",
    "選択した項目の展開先を指定..."
  ],
  "已成功提取 {count} 个文件至 {path}": [
    "Extracted {count} files to {path}",
    "{count} 個のファイルを {path} に展開しました"
  ],
  "提取已完成，但无法打开输出目录：{error}": [
    "Extraction completed, but the output folder could not be opened: {error}",
    "展開は完了しましたが、出力先フォルダーを開けませんでした：{error}"
  ],
  "提取所选条目需要密码": [
    "A password is required to extract the selected entries",
    "選択した項目の展開にはパスワードが必要です"
  ],
  "密码不匹配或归档已损坏": [
    "The password does not match, or the archive is damaged",
    "パスワードが一致しないか、アーカイブが破損しています"
  ],
  "输入密码后重试提取；若仍失败，请检查密码或归档完整性。密码仅用于本次提取，不会保存到密码库。": [
    "Enter a password and retry extraction. If it still fails, check the password and archive integrity. The password is used only for this extraction and is not saved to the vault.",
    "パスワードを入力して展開を再試行してください。それでも失敗する場合は、パスワードとアーカイブの整合性を確認してください。パスワードは今回の展開にのみ使用し、保管庫には保存しません。"
  ],
  "所选条目的解压密码": [
    "Password for the selected entries",
    "選択した項目の展開パスワード"
  ],
  "使用该密码提取所选": [
    "Extract selected with this password",
    "このパスワードで選択した項目を展開"
  ],
  "全选": [
    "Select all",
    "すべて選択"
  ],
  "已选 {count} 项": [
    "{count} selected",
    "{count} 件選択中"
  ]
};
