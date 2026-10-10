export interface PasswordInput { id?: string; name: string; secret?: string; groups: string[]; notes: string; enabled: boolean }
export interface PasswordView { id: string; name: string; groups: string[]; notes: string; enabled: boolean; successCount: number; lastSuccess: string | null; revision: number }
export interface ImportRow { line: number; name: string; group: string; status: string; message: string }
export interface ImportPreview { token: string; newCount: number; duplicateCount: number; errorCount: number; rows: ImportRow[] }
export interface ImportResult { added: number; duplicates: number }
export interface SourceGroup { path: string; displayName: string; volumes: string[]; size: number; missingVolume: boolean }
export interface ScanNotice { path: string; message: string }
export interface ScanResult { sources: SourceGroup[]; notices: ScanNotice[] }
export interface Settings {
 outputRoot: string; conflictPolicy: string; passwordGroup: string; cacheEnabled: boolean; cacheInput: boolean; cacheRoot: string;
 cacheQuotaBytes: number; minFreeBytes: number; expandNested: boolean; maxDepth: number; verification: string;
 maxEntries: number; maxExpandedBytes: number; recursiveScan: boolean; inputExclusions: string[]; outputExclusions: string[];
 smallFirst: boolean; theme: string; historyLimit: number; alwaysOnTop: boolean; completionSound: boolean;
 unwrapSingleRoot: boolean; sourceCleanup: string; minimizeToTray: boolean;
}
export interface NodeView { id: string; parentId: string | null; name: string; depth: number; status: string; stage: string; progress: number; attempts: number; message: string; passwordLabel: string | null }
export interface JobView {
 id: string; source: SourceGroup; options: Settings; status: string; stage: string; progress: number; message: string; attempts: number;
 outputPath: string | null; verificationScope: string; createdAt: string; finishedAt: string | null; workspace: string | null; nodes: NodeView[]; stageTimes: Record<string,number>;
}
export interface AppSnapshot { jobs: JobView[]; passwords: PasswordView[]; settings: Settings; running: boolean; paused: boolean; vaultError: string | null; dataDirectory: string; engineAvailable: boolean; recoverableCount: number; cacheUsedBytes: number }
export const defaultSettings: Settings = { outputRoot:'',conflictPolicy:'rename',passwordGroup:'',cacheEnabled:false,cacheInput:true,cacheRoot:'',cacheQuotaBytes:20*1024**3,minFreeBytes:1024**3,expandNested:true,maxDepth:2,verification:'standard',maxEntries:100000,maxExpandedBytes:100*1024**3,recursiveScan:true,inputExclusions:[],outputExclusions:[],smallFirst:false,theme:'system',historyLimit:500,alwaysOnTop:false,completionSound:false,unwrapSingleRoot:false,sourceCleanup:'keep',minimizeToTray:false };
export const statusLabels: Record<string,string> = {queued:'等待处理',running:'处理中',paused:'已暂停',stopping:'正在停止',completed:'已完成',partial:'部分结果',needsPassword:'需要密码',waitingResource:'等待资源',failed:'失败',cancelled:'已取消',skipped:'已跳过',interrupted:'中断待处理',publishPending:'发布待核对',cleanupPending:'完成，缓存待清理'};
export const stageLabels: Record<string,string> = {queued:'等待',checking:'来源检查',caching:'缓存输入',extracting:'外层解压',nested:'内层处理',planning:'结果规划',copying:'写回目标',verifying:'目标回读',publishing:'发布结果',cleanup:'清理缓存',finished:'完成',waiting:'等待处理'};
