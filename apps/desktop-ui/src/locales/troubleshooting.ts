export const troubleshootingMessages: Record<string, [string, string]> = {
  '排障与操作指引：': ['Troubleshooting & Guide:', 'トラブルシューティングとガイド：'],
  '数据库 / 存储异常': ['Database / Storage Error', 'データベース / ストレージ異常'],
  任务数据库繁忙锁死: ['Task database busy / locked', 'タスクデータベースがビジー状態でロック'],
  任务数据库读写失败: ['Task database I/O error', 'タスクデータベースの読み書きに失敗'],
  '数据库被并发锁占用，请稍候。': [
    'Database is locked by concurrent operations, please wait.',
    'データベースが並行ロックで占有されています。しばらくお待ちください。',
  ],
  '任务数据库写入失败，请检查恢复记录。': [
    'Failed to write to task database; please check recovery logs.',
    'タスクデータベースへの書き込みに失败しました。復元ログを確認してください。',
  ],
  '若为繁忙锁（isBusyLock），稍等片刻让其他进程或并发任务释放数据库后重试；': [
    'If busy locked (isBusyLock), wait a moment for other processes or tasks to release the database, then retry;',
    'ビジーロック（isBusyLock）の場合、他のプロセスやタスクが解放するのを待って再試行してください；',
  ],
  '检查磁盘所在分区是否有文件系统故障或只读保护；': [
    'Check if the disk partition has filesystem errors or read-only protection;',
    'ディスクパーティションにファイルシステムエラーや読み取り専用保護がないか確認してください；',
  ],
  '若长时间无法解锁，可重启客户端或检查是否有僵尸 openunpack 进程占用数据库。': [
    'If lock persists, restart the client or check for zombie openunpack processes occupying the database.',
    '長時間ロックが解除されない場合は、クライアントを再起動するかゾンビ openunpack プロセスの有無を確認してください。',
  ],
  磁盘空间耗尽: ['Disk space exhausted', 'ディスク空き容量不足'],
  磁盘可用空间不足以完成解压: [
    'Insufficient free disk space to complete extraction',
    '解凍を完了するのに十分な空き容量がありません',
  ],
  '目标盘剩余空间不足以容纳解压后的全部文件，请清理目标磁盘；': [
    'Target disk has insufficient free space for extracted files; please free up space;',
    '展開後ファイルを収容する空き容量が足りません。出力先ディスクを整理してください；',
  ],
  '前往「设置」切换输出目录到剩余空间更大的磁盘分区；': [
    'Go to Settings to switch the output directory to a disk partition with more free space;',
    '「設定」でより空き容量の大きいパーティションに出力先を変更してください；',
  ],
  '若开启了 SSD 缓存，检查系统缓存目录所在磁盘是否有足够缓冲空间。': [
    'If SSD cache is enabled, check whether the cache disk has adequate buffer space.',
    'SSDキャッシュが有効な場合、キャッシュ先ディスクに十分な空きがあるか確認してください。',
  ],
  调整目录或设置: ['Adjust Directory or Settings', 'ディレクトリまたは設定を変更'],
  文件被占用: ['File in use', 'ファイルが使用中'],
  目标文件或归档被其他进程独占占用: [
    'Target file or archive is exclusively locked by another process',
    '対象ファイルまたはアーカイブが他のプロセスに排他使用されています',
  ],
  '请关闭正在占用该文件的程序：{procInfo}；': [
    'Please close the program occupying this file: {procInfo};',
    'このファイルを占有しているプログラムを終了してください: {procInfo}；',
  ],
  '请检查并关闭可能正在打开该文件的播放器、编辑器或杀毒软件；': [
    'Please check and close players, editors, or antivirus scanning the file;',
    '該当ファイルを開いている可能性のあるプレーヤー、エディター、ウイルス対策ソフトを閉じてください；',
  ],
  '若进程已退出但句柄未释放，请在任务管理器中确认或稍等系统释放；': [
    'If the process has exited but handles linger, check Task Manager or wait for OS release;',
    'プロセス終了後もハンドルが残っている場合、タスクマネージャーで確認するか解放をお待ちください；',
  ],
  '解除占用后点击「重试」继续解压。': [
    'Once unlocked, click "Retry" to continue extraction.',
    '占有が解除されたら「再試行」をクリックして解凍を続行してください。',
  ],
  解除占用后重试: ['Unlock and Retry', '占有解除後に再試行'],
  权限不足: ['Permission denied', '権限が不足しています'],
  无权写入目标路径或访问源文件: [
    'No permission to write to target path or access source file',
    '出力先への書き込みまたは元ファイルへのアクセス権限がありません',
  ],
  '检查输出目录或源文件所在目录的 Windows NTFS 读写权限；': [
    'Check Windows NTFS read/write permissions for the output or source directory;',
    '出力先または元ファイルディレクトリのWindows NTFS読み書き権限を確認してください；',
  ],
  '尝试以管理员身份重新运行 OpenUnpack；': [
    'Try running OpenUnpack as administrator;',
    '管理者として OpenUnpack を再実行してみてください；',
  ],
  '若目标目录位于受保护的系统目录（如 Program Files），建议将输出目录更改为用户文档目录。': [
    'If the target is located in a protected system path (such as Program Files), it is recommended to change the output directory to the user Documents folder.',
    '出力先が保護されたシステムディレクトリ（Program Files等）の場合、ユーザー文書ディレクトリへの変更を推奨します。',
  ],
  更换输出目录: ['Change Output Directory', '出力先ディレクトリを変更'],
  '任务数据库写入失败，已停止后续操作；请检查恢复记录。': [
    'Task database write failed, subsequent operations stopped; please check recovery records.',
    'タスクDB書き込みに失敗し後続操作を停止しました。復旧記録を確認してください。',
  ],
  '检查磁盘所在分区是否已满或处于只读保护状态；': [
    'Check whether the disk partition is full or read-only protected;',
    'ディスクパーティションの空き容量や読み取り専用保護状態を確認してください；',
  ],
  '若任务数据库被杀毒软件或多个客户端实例占用，请稍候再试；': [
    'If the task database is locked by antivirus or multiple client instances, please wait and retry;',
    'ウイルス対策ソフトや多重起動でDBがロックされている場合は、しばらく待ってから再試行してください；',
  ],
  '点击「重试」再次尝试写入数据库。': [
    'Click "Retry" to attempt writing to the database again.',
    '「再試行」をクリックしてデータベースへの書き込みを再試行してください。',
  ],
  '密码缺失 / 需口令': ['Password Missing / Required', 'パスワード未設定 / パスワードが必要'],
  需要解压密码或已保存口令未命中: [
    'Archive password needed or saved passwords missed',
    '解凍パスワードが必要か、保存されたパスワードに一致しません',
  ],
  '该归档包含加密数据，当前密码库中的密码未能成功匹配。': [
    'This archive contains encrypted data; passwords in vault did not match.',
    'このアーカイブは暗号化データを含んでいますが、パスワードマネージャー内のパスワードは一致しませんでした。',
  ],
  '点击「输入密码」录入正确口令，提交后将立即重试解压；': [
    'Click "Enter Password" to provide password; extraction will retry immediately;',
    '「パスワード入力」をクリックして入力すると、直ちに解凍が再試行されます；',
  ],
  '若已知通用解压口令，建议在「密码库」中添加密码或批量导入，后续同类归档将自动免密解压；': [
    'If password is used frequently, add it to Password Vault for automatic future unlocking;',
    '共通パスワードの場合、「パスワードマネージャー」に追加しておくと次回以降自動入力されます；',
  ],
  '若为多层嵌套归档，录入第一层口令解压后，内部加密层仍可逐层提交对应密码。': [
    'For nested archives, entering the first layer allows unlocking inner layers sequentially.',
    '多層アーカイブの場合、第1層の解凍後に内部層のパスワードを順次入力できます。',
  ],
  '密码不匹配 / 归档损坏': [
    'Password Mismatch / Archive Corrupted',
    'パスワード不一致 / アーカイブ破損',
  ],
  密码不匹配或压缩包损坏: [
    'Password mismatch or corrupted archive',
    'パスワード不一致またはアーカイブ破損',
  ],
  '解压失败：密码未能正确解密该文件，或该归档文件已损坏、分卷不完整。': [
    'Extraction failed: password could not decrypt file, or archive is corrupted/incomplete.',
    '解凍失敗：パスワードで復号できないか、アーカイブが破損または分割ファイルが不完全です。',
  ],
  '核对当前密码是否完全正确（注意大小写与前后空格），点击「输入密码」再次尝试；': [
    'Verify password spelling (case-sensitive and spaces), click "Enter Password" to retry;',
    'パスワード（大文字・小文字・空白）を確認し、「パスワード入力」をクリックして再試行してください；',
  ],
  '若该文件并非密码保护，可能是下载不完整或文件损坏，请核对源文件哈希或重新下载；': [
    'If not password protected, file may be incomplete or corrupted; verify hash or redownload;',
    'パスワード保護されていない場合はダウンロード不完全の可能性があるため、ハッシュ確認または再ダウンロードをお試しください；',
  ],
  '若该文件后缀曾被伪装（如 .mp4 实际为 .7z），可在「工具箱」中使用「压缩包后缀自动修复」检查魔数并修正扩展名；':
    [
      'If extension was spoofed (e.g. .mp4 is really .7z), use Toolbox "Archive Extension Autofix" to correct it;',
      '拡張子が偽装されている場合（.mp4が実際は.7zなど）、「ツールボックス」の「拡張子自動修復」で修正してください；',
    ],
  '可在「设置」中开启「允许抢救已损坏归档」，然后重试解压以尽可能提取未损部分。': [
    'Enable "Salvage corrupted archives" in Settings and retry to recover undamaged files.',
    '「設定」で「破損アーカイブの救出を許可」を有効にして再試行すると、無事なファイルを可能な限り救出できます。',
  ],
  后缀自动修复: ['Extension Autofix', '拡張子自動修復'],
  '归档损坏 / 校验失败': [
    'Archive Corrupted / Checksum Failed',
    'アーカイブ破損 / チェックサム不一致',
  ],
  '压缩包损坏、CRC校验不符或分卷数据不完整': [
    'Archive corrupted, CRC mismatch, or incomplete volume',
    'アーカイブ破損、CRC不一致、または分割ボリューム不完全',
  ],
  '压缩包头部数据或文件流校验失败，可能源文件未下载完整或介质受损。': [
    'Header data or stream verification failed; source file may be truncated or disk damaged.',
    'ヘッダーデータまたはストリーム検証に失敗しました。ファイルが不完全かメディアが破損している可能性があります。',
  ],
  '请核对源文件体积是否完整，尝试重新下载该归档；': [
    'Check source file size and try redownloading the archive;',
    '元ファイルのサイズを確認し、アーカイブの再ダウンロードをお試しください；',
  ],
  '若该文件后缀曾被伪装篡改（例如将 .7z/.zip 伪装为 .mp4/.bin），请在「工具箱」中使用「压缩包后缀自动修复」检查并修正后缀；':
    [
      'If file extension was spoofed (e.g. .7z/.zip as .mp4/.bin), use Toolbox extension autofix;',
      '拡張子が偽装されている場合（.7z/.zipが.mp4/.bin等）、「ツールボックス」で拡張子を自動修復してください；',
    ],
  '若属于多分卷压缩包，请确保所有分卷文件位于同一目录且编号完整；': [
    'For multi-volume archives, ensure all parts are in the same folder with continuous numbering;',
    '分割アーカイブの場合、すべてのパートが同じフォルダに連番で揃っていることを確認してください；',
  ],
  '可在「设置」中开启「允许抢救已损坏归档（尽可能保留可读文件）」，然后点击「重试」尝试尽可能恢复部分内容。':
    [
      'Enable "Salvage corrupted archives" in Settings and click "Retry" to recover partial contents.',
      '「設定」で破損アーカイブの救出を有効にし、「再試行」をクリックして一部コンテンツの復旧を試みてください。',
    ],
  分卷压缩包缺少关联卷: [
    'Multi-volume archive missing parts',
    '分割アーカイブの関連パートが見つかりません',
  ],
  '检测到该压缩包为多分卷连续归档，但当前目录缺少后续或前面的分卷文件。': [
    'Detected multi-volume archive, but preceding or succeeding parts are missing.',
    '分割アーカイブですが、前後のパートファイルが見つかりません。',
  ],
  '请确认所有关联分卷（如 .part1.rar/.part2.rar 或 .z01/.z02/.zip）均存放在同一文件夹下；': [
    'Ensure all parts (e.g. .part1.rar/.part2.rar, .z01/.z02/.zip) are in the same directory;',
    'すべての分割ファイル（.part1.rar/.part2.rarや.z01/.z02/.zip等）が同一ディレクトリにあることを確認してください；',
  ],
  '检查分卷文件名是否被修改导致序号断档，确保分卷命名严格连续；': [
    'Check if filenames were altered causing broken sequence; ensure numbering is strictly continuous;',
    'ファイル名の変更による欠番がないか確認し、連番が連続していることを確認してください；',
  ],
  '补齐所有缺失分卷文件后，点击「重试」重新解压。': [
    'After completing missing volumes, click "Retry" to restart extraction.',
    '不足している分割ファイルを揃えた後、「再試行」をクリックしてください。',
  ],
  '空间 / 配额受限': ['Space / Quota Limited', '空き容量 / クォータ制限'],
  磁盘空间不足或展开限制超出配额: [
    'Insufficient disk space or extraction exceeds quota',
    'ディスク容量不足または展開サイズが制限を超過',
  ],
  '解压目标磁盘可用空间或 SSD 缓存配额不足，已安全暂停解压以防止磁盘占满。': [
    'Target disk free space or SSD cache quota is insufficient; extraction paused safely.',
    '出力先ディスクの空き容量またはSSDキャッシュクォータが不足しているため、安全に一時停止しました。',
  ],
  '清理目标磁盘或在「设置」中将「输出目录」更改至可用空间充裕的磁盘盘符；': [
    'Clean target disk or change output directory to a drive with ample space in Settings;',
    '出力先ディスクを整理するか、「設定」で十分な空きがあるドライブに変更してください；',
  ],
  '若启用了 SSD 缓存，可在「设置」中调大「缓存配额」或关闭缓存直接解压至目标目录；': [
    'If SSD cache is enabled, increase cache quota or disable cache in Settings;',
    'SSDキャッシュが有効な場合、「設定」でクォータを増やすかキャッシュを無効にしてください；',
  ],
  '若解压展开膨胀倍数过大，可在「设置」中适当上调「单任务解压后总大小上限」。': [
    'If expansion ratio is large, increase max expanded size per task in Settings.',
    '展開後のサイズが非常に大きい場合、「設定」で1タスクあたりの展開後最大サイズを引き上げてください。',
  ],
  前往设置调整: ['Go to Settings', '設定画面へ移動'],
  同名冲突: ['Naming Conflict', '同名競合'],
  输出目录存在同名文件或目录: [
    'Same-named file or directory exists in output folder',
    '出力先フォルダに同名のファイルまたはディレクトリが存在します',
  ],
  '解压目标路径已存在同名内容，根据当前安全策略未直接覆盖。': [
    'Target path already contains content with same name; not overwritten per safety policy.',
    '出力先パスに同名の項目が存在するため、現在の安全ポリシーに従い上書きしませんでした。',
  ],
  '前往「设置」将同名冲突策略改为「自动更名（创建 (1) 副本）」或「覆盖」；': [
    'Go to Settings and change conflict policy to "Auto-rename" or "Overwrite";',
    '「設定」で同名競合ポリシーを「自動連番リネーム」または「上書き」に変更してください；',
  ],
  '或者手动重命名/移动目标目录中的现有文件后再次重试。': [
    'Or manually rename/move existing content in target directory and retry.',
    'または出力先にある既存ファイルを手動で変更・移動した後に再試行してください。',
  ],
  调整冲突策略: ['Adjust Conflict Policy', '競合ポリシーを変更'],
  '引擎 / 进程异常': ['Engine / Process Error', 'エンジン / プロセス異常'],
  后台解压引擎响应异常或进程被中断: [
    'Extraction engine error or worker process interrupted',
    'バックエンド解凍エンジンの応答異常またはプロセス中断',
  ],
  '解压核心进程异常退出或通信超时，原归档和临时工作区已被安全保护。': [
    'Worker process exited abnormally or timed out; source and temporary workspace protected.',
    '解凍ワーカープロセスが異常終了またはタイムアウトしました。元ファイルと作業領域は保護されています。',
  ],
  '检查系统杀毒软件或安全卫士是否拦截了 OpenUnpack 7-Zip worker 进程；': [
    'Check if antivirus blocked OpenUnpack 7-Zip worker process;',
    'セキュリティ対策ソフトが OpenUnpack のワーカープロセスをブロックしていないか確認してください；',
  ],
  '确认机器可用内存是否充足，关闭其他高负载占用进程；': [
    'Ensure sufficient memory is available and close other high-load processes;',
    '十分な空きメモリがあることを確認し、他の高負荷プロセスを終了してください；',
  ],
  '在「设置」中检查解压引擎可用性，然后点击「重试」。': [
    'Check engine availability in Settings and click "Retry".',
    '「設定」で解凍エンジンの状態を確認し、「再試行」をクリックしてください。',
  ],
  格式不支持: ['Format Unsupported', '非対応フォーマット'],
  当前解压引擎暂不支持该文件格式: [
    'Current extraction engine does not support this file format',
    '現在の解凍エンジンはこのファイル形式に対応していません',
  ],
  '当前归档使用的压缩算法或封装结构不受支持。': [
    'Compression algorithm or wrapper container is not supported.',
    'アーカイブの圧縮アルゴリズムまたはコンテナ形式がサポートされていません。',
  ],
  '可在「工具箱」中使用「压缩包后缀自动修复」检测文件头魔数，验证是否为实际标准格式；': [
    'Use Toolbox "Archive Extension Autofix" to check magic header and verify format;',
    '「ツールボックス」の「拡張子自動修復」でヘッダーのマジックナンバーを確認してください；',
  ],
  '若是私有加密或特殊分包（如专用安装包、专有镜像），建议使用对应专用工具打开。': [
    'If proprietary or custom container (e.g. specialized installer/image), open with dedicated tool.',
    '独自暗号化や専用インストーラー形式の場合は、対応する専用ツールをご利用ください。',
  ],
  工具箱检测: ['Check in Toolbox', 'ツールボックスで確認'],
  任务失败: ['Task Failed', 'タスク失敗'],
  解压任务未能顺利完成: [
    'Extraction task could not be completed',
    '解凍タスクを完了できませんでした',
  ],
  '任务处理遇到错误并已停止。原始压缩文件安全保留。': [
    'Task encountered an error and stopped. Original archive is safely preserved.',
    'エラーが発生したため処理を停止しました。元アーカイブは保護されています。',
  ],
  '点击「详情」查看内外层处理节点与具体阶段日志；': [
    'Click "Details" to inspect inner/outer nodes and stage logs;',
    '「詳細」をクリックして各ノードの処理状況とログを確認してください；',
  ],
  '若是临时读写异常，可直接点击「重试」再次尝试；': [
    'If transient I/O error, click "Retry" directly to attempt again;',
    '一時的なI/Oエラーの場合は、そのまま「再試行」をお試しください；',
  ],
  '若工作目录存在残留临时碎片，可点击「清理临时结果」释放占用。': [
    'If temporary workspace fragments remain, click "Clean Temporary Files" to free disk space.',
    '作業領域に一時ファイルが残っている場合は、「一時ファイルを整理」で空き容量を確保できます。',
  ],
};
