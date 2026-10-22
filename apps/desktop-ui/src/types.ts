export interface PasswordInput { id?: string | null; name: string; secret?: string | null; groups: string[]; notes: string; enabled: boolean }
export interface PasswordView { id: string; name: string; groups: string[]; notes: string; enabled: boolean; successCount: number; lastSuccess: string | null; revision: number }
export interface ImportRow { line: number; name: string; group: string; status: string; message: string }
export interface ImportPreview { token: string; newCount: number; duplicateCount: number; errorCount: number; rows: ImportRow[] }
export interface ImportResult { added: number; duplicates: number }
export interface SourceGroup { path: string; displayName: string; volumes: string[]; size: number; missingVolume: boolean }
export interface ScanNotice { path: string; message: string }
export interface ScanResult { sources: SourceGroup[]; notices: ScanNotice[] }
export interface Settings {
 outputRoot: string; conflictPolicy: string; passwordGroup: string; cacheEnabled: boolean; cachePolicy: string; cacheInput: boolean; cacheRoot: string;
 cacheQuotaBytes: number; minFreeBytes: number; expandNested: boolean; maxDepth: number; verification: string;
 maxEntries: number; maxExpandedBytes: number; recursiveScan: boolean; inputExclusions: string[]; outputExclusions: string[]; outputInclusions: string[];
 smallFirst: boolean; theme: string; historyLimit: number; alwaysOnTop: boolean; completionSound: boolean;
 unwrapSingleRoot: boolean; sourceCleanup: string; minimizeToTray: boolean; concurrency: number;
 engineThreads: number; workerPriority: string; salvageCorrupted: boolean; autoFixEncoding: boolean; cleanEmptyDirs: boolean; deduplicateHardlinks: boolean;
  autoStartOnAdd: boolean; postExtractionAction: string; onComplete: string | null; watchedFolders: string[];
}
export interface NodeView { id: string; parentId: string | null; name: string; depth: number; status: string; stage: string; progress: number; attempts: number; message: string; passwordLabel: string | null }
export interface TransferProgress { completedBytes: number; totalBytes: number; bytesPerSecond: number | null; remainingSeconds: number | null; updatedAt: string }
export interface JobView {
 id: string; source: SourceGroup; options: Settings; status: string; stage: string; progress: number; message: string; attempts: number;
 outputPath: string | null; verificationScope: string; createdAt: string; finishedAt: string | null; workspace: string | null; nodes: NodeView[]; stageTimes: Record<string,number>; transferProgress: TransferProgress | null;
}
export interface AppSnapshot { jobs: JobView[]; passwords: PasswordView[]; settings: Settings; running: boolean; paused: boolean; vaultError: string | null; dataDirectory: string; engineAvailable: boolean; recoverableCount: number; cacheUsedBytes: number; sessionId: string; revision: string }
export interface SnapshotState {
  running: boolean;
  paused: boolean;
  vaultError: string | null;
  dataDirectory: string;
  engineAvailable: boolean;
  recoverableCount: number;
  cacheUsedBytes: number;
}
export interface SnapshotDelta {
  jobsChanged: JobView[];
  jobsRemoved: string[];
  passwordsChanged: PasswordView[];
  passwordsRemoved: string[];
  settings: Settings | null;
  state: SnapshotState | null;
}
export interface SnapshotUpdate {
  sessionId: string;
  revision: string;
  full: AppSnapshot | null;
  delta: SnapshotDelta | null;
}
export interface JobSummary { id: string; source: SourceGroup; status: string; stage: string; progress: number; message: string; attempts: number; outputPath: string | null; verificationScope: string; createdAt: string; finishedAt: string | null; workspace: string | null }
export interface HistoryPageQuery { offset: number; limit: number; status: string; search: string }
export interface JobHistoryPage { items: JobSummary[]; offset: number; limit: number; filteredTotal: number; total: number; completed: number; failed: number; recoverable: number; clearAllowed: boolean; sessionId: string; revision: string }
export interface StateInvalidated { sessionId: string; revision: string }
export interface JobProgressUpdate {
  jobId: string;
  stage: string;
  progress: number;
  completedBytes: number;
  totalBytes: number;
  bytesPerSecond: number;
  remainingSeconds: number | null;
  currentFile?: string | null;
}
export interface NeedPasswordEvent {
  jobId: string;
  archivePath: string;
  timeoutSeconds: number;
  inputType: string;
}
export interface ArchiveEntryView { path: string; size: number; sizeKnown: boolean; isDirectory: boolean; isEncrypted: boolean; rawPath: string }
export interface ArchiveInspectionView { archivePath: string; totalEntries: number; totalSize: number; totalSizeKnown: boolean; encryptedEntries: number; entries: ArchiveEntryView[]; detectedEncoding: string }
export interface SelectiveExtractRequest {
  archivePath: string;
  entryPaths: string[];
  destinationDir?: string | null;
  password?: string | null;
  flatten: boolean;
}
export interface SelectiveExtractResult {
  outputDirectory: string;
  extractedFiles: string[];
  totalBytes: number;
}
export const DESKTOP_API_VERSION = 1;
export const MAX_CONCURRENT_JOBS = 5;
export const defaultSettings: Settings = { outputRoot:'',conflictPolicy:'rename',passwordGroup:'',cacheEnabled:false,cachePolicy:'auto',cacheInput:true,cacheRoot:'',cacheQuotaBytes:20*1024**3,minFreeBytes:1024**3,expandNested:true,maxDepth:2,verification:'standard',maxEntries:100000,maxExpandedBytes:100*1024**3,recursiveScan:true,inputExclusions:[],outputExclusions:[],outputInclusions:[],smallFirst:false,theme:'system',historyLimit:500,alwaysOnTop:false,completionSound:false,unwrapSingleRoot:false,sourceCleanup:'keep',minimizeToTray:false,concurrency:1,engineThreads:0,workerPriority:'belowNormal',salvageCorrupted:true,autoFixEncoding:true,cleanEmptyDirs:true,deduplicateHardlinks:true,autoStartOnAdd:true,postExtractionAction:'none',onComplete:null,watchedFolders:[] };
export const statusLabels: Record<string,string> = {queued:'等待处理',running:'处理中',paused:'已暂停',stopping:'正在停止',completed:'已完成',partial:'部分结果',salvaged:'抢救完成',needsPassword:'需要密码',waitingResource:'等待资源',failed:'失败',cancelled:'已取消',skipped:'已跳过',interrupted:'中断待处理',publishPending:'发布待核对',cleanupPending:'完成，缓存待清理'};
export const stageLabels: Record<string,string> = {queued:'等待',checking:'来源检查',caching:'缓存输入',extracting:'外层解压',nested:'内层处理',planning:'结果规划',copying:'写回目标',verifying:'目标回读',publishing:'发布结果',cleanup:'清理缓存',finished:'完成',waiting:'等待处理'};

export type ToolboxKind = 'normalizeExtensions' | 'batchRename' | 'flattenDirectory';
export type ConflictStrategy = 'rename' | 'skip' | 'error';
export type ToolboxAction = 'rename' | 'move' | 'skip';

export interface RenamePatternRule {
  search: string;
  replace: string;
  caseSensitive: boolean;
  useRegex: boolean;
  prefix: string | null;
  suffix: string | null;
  numberingStart: number | null;
  numberingDigits: number | null;
}

export interface ToolboxOptions {
  kind: ToolboxKind;
  sourceDirectory: string;
  targetDirectory: string | null;
  recursive: boolean;
  removeEmptyFolders: boolean;
  conflictStrategy: ConflictStrategy;
  renameRule: RenamePatternRule | null;
}

export interface ToolboxItemPlan {
  sourcePath: string;
  relativePath: string;
  proposedName: string;
  proposedPath: string;
  size: number;
  action: ToolboxAction;
  hasConflict: boolean;
  conflictReason: string | null;
}

export interface ToolboxPreviewResult {
  totalScanned: number;
  totalPlanned: number;
  conflictsCount: number;
  plans: ToolboxItemPlan[];
  token: string;
}

export interface ToolboxExecuteRequest {
  options: ToolboxOptions;
  approvedPaths: string[] | null;
  token: string | null;
}

export interface ToolboxExecuteResult {
  successCount: number;
  skippedCount: number;
  failedCount: number;
  cleanedFoldersCount: number;
  errors: string[];
}
