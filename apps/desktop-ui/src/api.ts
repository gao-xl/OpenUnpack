import { invoke } from '@tauri-apps/api/core';
import { t } from './i18n';
import type { LocalApiSettings, LocalApiStatus, ApiClientInput, CreatedApiClient } from './local-api-types';
import { listen } from '@tauri-apps/api/event';
import type { AppSnapshot, SourceGroup, Settings, PasswordInput, PasswordView, ScanResult, ImportPreview, ImportResult, JobView, HistoryPageQuery, JobHistoryPage, ArchiveInspectionView, ToolboxOptions, ToolboxPreviewResult, ToolboxExecuteRequest, ToolboxExecuteResult, SnapshotUpdate, SelectiveExtractRequest, SelectiveExtractResult } from './types';
import { defaultSettings } from './types';
import { SnapshotCursor, validVersion } from './snapshot-cursor';
import type { StateInvalidated } from './types';
export const isDesktop = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const preview: AppSnapshot = { jobs:[],passwords:[],settings:{...defaultSettings},running:false,paused:false,vaultError:null,get dataDirectory(){return t('桌面版启动后显示');},engineAvailable:false,recoverableCount:0,cacheUsedBytes:0,sessionId:'preview',revision:'0' };

interface DaemonConnection { url: string; token: string; }
const tokenKey = 'openunpack_daemon_token';
const tokenUrlKey = 'openunpack_daemon_token_url';
const urlKey = 'openunpack_daemon_url';
const webConsole = typeof document !== 'undefined' && document.querySelector('meta[name="openunpack-web-console"]')?.getAttribute('content') === '1';
function isLoopbackHost(hostname: string): boolean {
 return hostname === '127.0.0.1' || hostname === 'localhost';
}
function daemonUrl(raw: string): string {
 let parsed: URL;
 try { parsed = new URL(raw.trim()); } catch { throw new Error(t('守护进程地址格式无效，请填写完整地址。')); }
 if (parsed.username || parsed.password || parsed.search || parsed.hash || parsed.pathname !== '/') {
  throw new Error(t('请填写服务根地址，不要包含账户、路径、查询参数或片段。'));
 }
 if (isDesktop) {
  if (parsed.protocol !== 'http:' || !isLoopbackHost(parsed.hostname)) throw new Error(t('桌面连接仅支持本机 HTTP 服务；远端服务请在浏览器打开其同源 HTTPS 控制台。'));
 } else if (!['http:', 'https:'].includes(parsed.protocol) || parsed.origin !== window.location?.origin) {
  throw new Error(t('浏览器仅能连接当前页面的同源服务，请直接打开目标服务的控制台。'));
 }
 return parsed.origin;
}
let activeDaemon: DaemonConnection | null = (() => {
 try {
  const savedUrl = localStorage.getItem(urlKey);
  const target = savedUrl ? daemonUrl(savedUrl) : webConsole ? daemonUrl(window.location.origin) : null;
  if (!target) return null;
  // A session credential is bound to the endpoint that the user approved.
  const savedTokenUrl = sessionStorage.getItem(tokenUrlKey);
  let token = savedTokenUrl === target ? sessionStorage.getItem(tokenKey) || '' : '';
  const legacy = localStorage.getItem(tokenKey);
  if (!token && legacy && savedUrl && daemonUrl(savedUrl) === target) {
   token = legacy;
   sessionStorage.setItem(tokenKey, token);
   sessionStorage.setItem(tokenUrlKey, target);
  }
  localStorage.removeItem(tokenKey);
  return { url: target, token };
 } catch {
  try { localStorage.removeItem(tokenKey); sessionStorage.removeItem(tokenKey); sessionStorage.removeItem(tokenUrlKey); } catch { /* No persistent credential fallback. */ }
  return null;
 }
})();
let connectionEpoch = 0;
const nativeCommands = new Set(['api_status', 'api_configure', 'api_create_client', 'api_revoke_client', 'api_open_docs', 'set_ui_language', 'pick_files', 'pick_directory', 'pick_save_backup', 'pick_open_backup', 'copy_password', 'open_path', 'execute_post_action', 'get_shell_context_menu_status', 'set_shell_context_menu_status']);
const engineLocalCommands = new Set(['copy_password', 'open_path', 'execute_post_action']);
function isRecord(value: unknown): value is Record<string, unknown> { return value !== null && typeof value === 'object' && !Array.isArray(value); }
function settingsShape(value: unknown): value is Settings {
 return isRecord(value) && Object.entries(defaultSettings).every(([key, sample]) => {
  const field = value[key];
  if (Array.isArray(sample)) return Array.isArray(field) && field.every(item => typeof item === 'string');
  if (sample === null) return field === null || typeof field === 'string';
  return typeof field === typeof sample && (typeof field !== 'number' || Number.isFinite(field));
 });
}
function sourceShape(value: unknown): boolean {
 return isRecord(value) && typeof value.path === 'string' && typeof value.displayName === 'string'
  && Array.isArray(value.volumes) && value.volumes.every(path => typeof path === 'string')
  && typeof value.size === 'number' && Number.isFinite(value.size) && typeof value.missingVolume === 'boolean';
}
function passwordShape(value: unknown): boolean {
 return isRecord(value) && typeof value.id === 'string' && typeof value.name === 'string' && typeof value.notes === 'string'
  && Array.isArray(value.groups) && value.groups.every(group => typeof group === 'string') && typeof value.enabled === 'boolean'
  && typeof value.successCount === 'number' && typeof value.revision === 'number' && (value.lastSuccess === null || typeof value.lastSuccess === 'string');
}
function jobShape(value: unknown): value is JobView {
 return isRecord(value) && typeof value.id === 'string' && sourceShape(value.source)
  && typeof value.status === 'string' && typeof value.stage === 'string' && typeof value.progress === 'number'
  && typeof value.message === 'string' && typeof value.attempts === 'number' && typeof value.verificationScope === 'string'
  && (value.outputPath === null || typeof value.outputPath === 'string') && (value.workspace === null || typeof value.workspace === 'string')
  && (value.finishedAt === null || typeof value.finishedAt === 'string') && typeof value.createdAt === 'string'
  && settingsShape(value.options) && Array.isArray(value.nodes) && value.nodes.every(node => isRecord(node) && typeof node.id === 'string' && typeof node.name === 'string' && typeof node.status === 'string' && typeof node.stage === 'string' && typeof node.depth === 'number' && typeof node.progress === 'number' && typeof node.attempts === 'number' && typeof node.message === 'string')
  && isRecord(value.stageTimes) && Object.values(value.stageTimes).every(time => typeof time === 'number' && Number.isFinite(time));
}
function snapshotShape(value: unknown): value is AppSnapshot {
 return isRecord(value) && validVersion(value) && Array.isArray(value.jobs) && value.jobs.every(jobShape)
  && Array.isArray(value.passwords) && value.passwords.every(passwordShape) && settingsShape(value.settings)
  && typeof value.running === 'boolean' && typeof value.paused === 'boolean' && typeof value.engineAvailable === 'boolean'
  && typeof value.dataDirectory === 'string' && typeof value.cacheUsedBytes === 'number' && Number.isFinite(value.cacheUsedBytes)
  && typeof value.recoverableCount === 'number' && (value.vaultError === null || typeof value.vaultError === 'string');
}
function checkResult(command: string, value: unknown) {
 const valid = command === 'snapshot' || command === 'queue_snapshot' ? snapshotShape(value)
  : command === 'history' || command === 'add_jobs' || command === 'add_archive' ? Array.isArray(value) && value.every(jobShape)
  : command === 'save_settings' || command === 'read_settings' ? settingsShape(value)
  : command === 'job_details' ? value === null || jobShape(value)
  : command === 'history_page' ? isRecord(value) && Array.isArray(value.items) && typeof value.total === 'number' && typeof value.filteredTotal === 'number' && validVersion(value)
  : true;
 if (!valid) throw new Error(t('守护进程返回的数据格式无效，请核对服务版本。'));
}
async function daemonRequest(connection: DaemonConnection, endpoint: string, method: 'GET' | 'POST', body?: unknown, signal?: AbortSignal): Promise<unknown> {
 const controller = new AbortController();
 const cancel = () => controller.abort();
 if (signal?.aborted) controller.abort(); else signal?.addEventListener('abort', cancel, { once: true });
 const timer = setTimeout(cancel, 10000);
 try {
  const response = await fetch(connection.url + endpoint, {
   method, redirect: 'error', credentials: 'omit', cache: 'no-store', signal: controller.signal,
   headers: { ...(body === undefined ? {} : { 'Content-Type': 'application/json' }), ...(connection.token ? { Authorization: 'Bearer ' + connection.token } : {}) },
   body: body === undefined ? undefined : JSON.stringify(body),
  });
  const payload: unknown = await response.json().catch(() => null);
  if (!response.ok) {
   const detail = isRecord(payload) && isRecord(payload.error) && typeof payload.error.message === 'string' ? payload.error.message : 'HTTP ' + response.status;
   throw new Error(connection.token ? detail.replaceAll(connection.token, '[REDACTED]') : detail);
  }
  if (!isRecord(payload) || !Object.prototype.hasOwnProperty.call(payload, 'data')) throw new Error(t('守护进程返回的数据格式无效，请核对服务版本。'));
  return payload.data;
 } catch (error) {
  if (controller.signal.aborted) throw new Error(t('守护进程请求已取消或超时，请核对任务状态后重试。'));
  if (error instanceof TypeError) throw new Error(t('无法连接守护进程，请核对地址、网络与服务状态。'));
  throw error;
 } finally { clearTimeout(timer); signal?.removeEventListener('abort', cancel); }
}
async function callNative<T>(name: string, args?: Record<string, unknown>): Promise<T> {
 if (!isDesktop) throw new Error(activeDaemon ? t('此操作需要本机桌面客户端。') : t('此页面是界面预览，请打开桌面版使用文件与密码功能。'));
 if (activeDaemon && engineLocalCommands.has(name)) throw new Error(t('连接守护进程时不能对本机执行此操作。'));
 try { return await invoke<T>(name, args); } catch (error) { throw new Error(typeof error === 'string' ? error : t('操作未完成，请查看任务状态后重试。')); }
}
async function callDaemon<T>(command: string, args?: Record<string, unknown>, signal?: AbortSignal): Promise<T> {
 if (!activeDaemon) throw new Error(t('未配置守护进程连接。'));
 const value = await daemonRequest(activeDaemon, '/api/v1/commands', 'POST', { command, ...(args ? { args } : {}) }, signal);
 checkResult(command, value);
 return value as T;
}
async function call<T>(name:string,args?:Record<string,unknown>):Promise<T> {
 if (nativeCommands.has(name)) return callNative<T>(name, args);
 if (activeDaemon) return callDaemon<T>(name, args);
 return callNative<T>(name, args);
}
function setDaemonConnection(url: string | null, token: string | null) {
 const next = url ? { url: daemonUrl(url), token: (token || '').trim() } : null;
 connectionEpoch += 1;
 activeDaemon = next;
 try {
  localStorage.removeItem(tokenKey);
  if (next) {
   localStorage.setItem(urlKey, next.url);
   sessionStorage.setItem(tokenKey, next.token);
   sessionStorage.setItem(tokenUrlKey, next.url);
  } else {
   localStorage.removeItem(urlKey); sessionStorage.removeItem(tokenKey); sessionStorage.removeItem(tokenUrlKey);
  }
 } catch { /* Continue only in memory when storage is unavailable. */ }
}
async function verifyDaemonConnection(url: string, token: string) {
 const candidate = { url: daemonUrl(url), token: token.trim() };
 if (!candidate.token) throw new Error(t('请填写 API 应用密钥。'));
 const health = await daemonRequest({ ...candidate, token: '' }, '/api/v1/health', 'GET');
 if (!isRecord(health) || health.service !== 'OpenUnpack' || health.apiVersion !== 1) throw new Error(t('目标服务不是兼容的 OpenUnpack API。'));
 const capabilities = await daemonRequest(candidate, '/api/v1/capabilities', 'GET');
 if (!isRecord(capabilities) || capabilities.apiVersion !== 1 || !Array.isArray(capabilities.commands) || !capabilities.commands.includes('queue_snapshot')) throw new Error(t('目标服务不是兼容的 OpenUnpack API。'));
 return candidate;
}
async function connectDaemon(url: string, token: string) {
 const candidate = await verifyDaemonConnection(url, token);
 setDaemonConnection(candidate.url, candidate.token);
}
// Native UI language is local configuration even while viewing a daemon.
let languageUpdate: Promise<void> = Promise.resolve();
function setLanguage(language:'zh-CN'|'en-US'|'ja-JP'):Promise<void> {
 if (!isDesktop) return Promise.resolve();
 const result = languageUpdate.then(() => callNative<void>('set_ui_language', { language }));
 languageUpdate = result.catch(() => {});
 return result;
}
export interface JobProgressUpdatePayload { jobId: string; completedBytes?: number; totalBytes?: number; currentFile?: string; bytesPerSecond?: number; remainingSeconds?: number; [key:string]:unknown; }
export interface NeedPasswordEventPayload { jobId: string; archivePath?: string; [key:string]:unknown; }
const progressListeners = new Set<(update: JobProgressUpdatePayload) => void>();
const needPasswordListeners = new Set<(event: NeedPasswordEventPayload) => void>();
export const api = {
 setLanguage,
 apiStatus:()=>callNative<LocalApiStatus>('api_status'),
 apiConfigure:(settings:LocalApiSettings)=>callNative<LocalApiStatus>('api_configure',{settings}),
 apiCreateClient:(input:ApiClientInput)=>callNative<CreatedApiClient>('api_create_client',{input}),
 apiRevokeClient:(id:string)=>callNative<void>('api_revoke_client',{id}),
 apiOpenDocs:()=>callNative<void>('api_open_docs'),
 snapshot:()=> (isDesktop || activeDaemon) ? call<AppSnapshot>('queue_snapshot') : Promise.resolve(preview),
 fullSnapshot:()=> (isDesktop || activeDaemon) ? call<AppSnapshot>('snapshot') : Promise.resolve(preview),
 snapshotUpdate:(sinceRevision?:string|null):Promise<SnapshotUpdate> => activeDaemon ? api.snapshot().then(full => ({sessionId:full.sessionId,revision:full.revision,full,delta:null})) : isDesktop ? call<SnapshotUpdate>('snapshot_update',{sinceRevision:sinceRevision??null}) : Promise.resolve({sessionId:'preview',revision:'0',full:preview,delta:null}),
 scan:(paths:string[],requestId?:string)=>call<ScanResult>('scan_sources',{paths,requestId:requestId??null}),
 cancelScan:(requestId:string)=>call<void>('cancel_scan',{requestId}),
 previewExclusions:(paths:string[],patterns:string[],scope:'input'|'output'='input',requestId?:string)=>call<{path:string;matched:boolean}[]>('preview_exclusions',{paths,patterns,scope,requestId:requestId??null}),
 cancelPreviewExclusions:(requestId:string)=>call<void>('cancel_preview_exclusions',{requestId}),
 addJobs:(sources:SourceGroup[],options:Settings)=>call<JobView[]>('add_jobs',{sources,options}),
 start:()=>call<void>('start_jobs'),startSelected:(jobIds:string[])=>call<void>('start_selected_jobs',{jobIds}),pause:()=>call<void>('pause_jobs'),
 cancel:(jobId?:string)=>call<void>('cancel_jobs',{jobId:jobId??null}),
 retry:(jobId:string,partial=false,password?:string)=>call<void>('retry_job',password !== undefined ? {jobId,partial,password} : {jobId,partial}),
 retryNode:(jobId:string,nodeId:string)=>call<void>('retry_node',{jobId,nodeId}),
 remove:(jobId:string)=>call<void>('remove_job',{jobId}),
 clearCompleted:()=>call<void>('clear_completed'),
 savePassword:(input:PasswordInput)=>call<PasswordView>('save_password',{input}),
 deletePassword:(id:string)=>call<void>('delete_password',{id}),
 revealPassword:(id:string)=>call<string>('reveal_password',{id}),
 copyPassword:(id:string)=>call<void>('copy_password',{id}),
 bulkPasswordAction:(ids:string[],action:'enable'|'disable'|'delete'|'group',groups:string[]=[])=>call<void>('bulk_password_action',{ids,action,groups}),
 previewImport:(text:string,group:string,format:string)=>call<ImportPreview>('preview_import',{text,group,format}),
 confirmImport:(text:string,group:string,format:string,token:string)=>call<ImportResult>('confirm_import',{text,group,format,token}),
 exportBackup:(path:string,passphrase:string)=>call<void>('export_backup',{path,passphrase}),
 previewRestoreBackup:(path:string,passphrase:string)=>call<ImportPreview>('preview_restore_backup',{path,passphrase}),
 confirmRestoreBackup:(path:string,passphrase:string,token:string)=>call<ImportResult>('confirm_restore_backup',{path,passphrase,token}),
 saveSettings:(settings:Settings)=>call<Settings>('save_settings',{settings}),
 history:()=>call<JobView[]>('history'),
 historyPage:(query:HistoryPageQuery)=>call<JobHistoryPage>('history_page',{query}),
 jobDetails:(jobId:string)=>call<JobView|null>('job_details',{jobId}),
 inspectArchive:(path:string,password:string|null,inspectId:string)=>call<ArchiveInspectionView>('inspect_archive',{path,password,inspectId}),
 cancelInspectArchive:(inspectId:string)=>call<void>('cancel_inspect_archive',{inspectId}),
 extractSelective:(request:SelectiveExtractRequest)=>call<SelectiveExtractResult>('extract_selective',{request}),
 clearHistory:()=>call<void>('clear_history'),
 recover:(jobId:string,action:string)=>call<void>('recover_job',{jobId,action}),
 openPath:(path:string)=>call<void>('open_path',{path}),
 pickFiles:(folder=false)=>call<string[]>('pick_files',{folder}),
 pickDirectory:()=>call<string|null>('pick_directory'),
 pickSaveBackup:()=>call<string|null>('pick_save_backup'),
 pickOpenBackup:()=>call<string|null>('pick_open_backup'),
 readImportFile:(path:string,encoding='utf-8')=>call<string>('read_import_file',{path,encoding}),
  previewToolbox:(options:ToolboxOptions)=>call<ToolboxPreviewResult>('preview_toolbox_operation',{options}),
  executeToolbox:(request:ToolboxExecuteRequest)=>call<ToolboxExecuteResult>('execute_toolbox_operation',{request}),
  executePostAction:(action:string,targetPath?:string)=>call<void>('execute_post_action',{action,targetPath:targetPath??null}),
  getShellContextMenuStatus:()=>call<boolean>('get_shell_context_menu_status'),
  setShellContextMenuStatus:(enabled:boolean)=>call<boolean>('set_shell_context_menu_status',{enabled}),
 setDaemonConnection,
 connectDaemon,
 verifyDaemonConnection,
 getConnectionVersion:()=>connectionEpoch,
 getDaemonConnection:()=>({url:activeDaemon?.url ?? null,token:activeDaemon?.token ?? null}),
 isDaemonMode:()=>activeDaemon !== null,
 isWebConsole:()=>webConsole,
  addArchive:(path:string,outputDir?:string,password?:string,deleteSource?:boolean,autoStart=true)=>
    call<JobView[]>('add_archive',{path,outputDir:outputDir??null,password:password??null,deleteSource:deleteSource??null,autoStart}),
  submitInput:(jobId:string,inputType:string,value:string)=>
    call<{success:boolean;jobId:string;inputType:string;resumedInFlight?:boolean}>('submit_input',{jobId,inputType,value}),
  onProgress:(callback:(update:JobProgressUpdatePayload)=>void)=>{
    progressListeners.add(callback);
    return ()=>progressListeners.delete(callback);
  },
  onNeedPassword:(callback:(event:NeedPasswordEventPayload)=>void)=>{
    needPasswordListeners.add(callback);
    return ()=>needPasswordListeners.delete(callback);
  },
 async subscribe(callback:(snapshot:AppSnapshot)=>void,onError?:(message:string)=>void):Promise<()=>void> {
  if (activeDaemon) {
   const connection = { ...activeDaemon };
   const epoch = connectionEpoch;
   const abort = new AbortController();
   let disposed = false, busy = false, pending = false;
   let ws: WebSocket | undefined;
   let timer: ReturnType<typeof setTimeout> | undefined;
   let reconnect: ReturnType<typeof setTimeout> | undefined;
   const current = () => !disposed && epoch === connectionEpoch;
   const schedule = () => { if (!current() || timer !== undefined) return; timer = setTimeout(() => { timer = undefined; void update(); }, 150); };
   const update = async () => {
    if (!current()) return;
    if (busy) { pending = true; return; }
    busy = true;
    try {
     const result = await daemonRequest(connection, '/api/v1/commands', 'POST', {command:'queue_snapshot'}, abort.signal);
     checkResult('queue_snapshot', result);
     if (current()) callback(result as AppSnapshot);
    } catch { if (current()) onError?.(t('当前状态读取未成功，请刷新后核对任务；停止入口仍可使用。')); }
    finally { busy = false; if (pending && current()) { pending = false; schedule(); } }
   };
   const connect = async () => {
    if (!current()) return;
    try {
     const value = await daemonRequest(connection, '/api/v1/ws-ticket', 'POST', undefined, abort.signal);
     if (!current()) return;
     if (!isRecord(value) || typeof value.ticket !== 'string' || !value.ticket || value.ticket.length > 256) throw new Error('Invalid ticket');
     const socketUrl = new URL('/api/v1/ws', connection.url);
     socketUrl.protocol = socketUrl.protocol === 'https:' ? 'wss:' : 'ws:';
     socketUrl.searchParams.set('ticket', value.ticket);
     ws = new WebSocket(socketUrl.toString());
     ws.onopen = schedule;
     ws.onmessage = event => {
      if (!current()) return;
      try {
       const parsed: unknown = JSON.parse(typeof event.data === 'string' ? event.data : '');
       if (!isRecord(parsed)) return;
       if (parsed.event === 'state_changed' || parsed.event === 'ready') schedule();
       else if (isRecord(parsed.data) && typeof parsed.data.jobId === 'string') {
        if (parsed.event === 'job_progress') progressListeners.forEach(listener => listener(parsed.data as JobProgressUpdatePayload));
        if (parsed.event === 'need_password') needPasswordListeners.forEach(listener => listener(parsed.data as NeedPasswordEventPayload));
       }
      } catch { /* Ignore malformed optional events; snapshots remain authoritative. */ }
     };
     ws.onclose = () => { if (current()) reconnect = setTimeout(() => { void connect(); }, 2000); };
     ws.onerror = () => ws?.close();
    } catch {
     if (current()) { onError?.(t('状态订阅未建立，请使用刷新检查任务状态。')); reconnect = setTimeout(() => { void connect(); }, 5000); }
    }
   };
   void connect(); schedule();
   return () => { disposed = true; abort.abort(); if (timer) clearTimeout(timer); if (reconnect) clearTimeout(reconnect); ws?.close(); };
  }
  if(!isDesktop) return ()=>{};
  const nativeEpoch = connectionEpoch;
  let disposed=false, timer:ReturnType<typeof setTimeout>|undefined, busy=false, pending=false;
  const currentNative = () => !disposed && nativeEpoch === connectionEpoch;
  const cursor = new SnapshotCursor();
  let wanted: StateInvalidated | undefined;
  const retiredSessions = new Set<string>();
  let staleReads = 0;
  const schedule=()=>{if(!currentNative()||timer!==undefined)return;timer=setTimeout(()=>{timer=undefined;void update();},150);};
  const update=async()=>{if(!currentNative())return;if(busy){pending=true;return;}busy=true;try{
    const updateResult = await api.snapshotUpdate(cursor.currentRevision);
    if (!currentNative()) return;
    let s: AppSnapshot | null = null;
    if (updateResult && 'jobs' in updateResult) {
      if (cursor.accept(updateResult as unknown as AppSnapshot)) s = updateResult as unknown as AppSnapshot;
    } else {
      s = cursor.applyUpdate(updateResult);
      if (!s) {
        const full = await api.snapshot();
        if (cursor.accept(full)) s = full;
      }
    }
    if (currentNative() && s) {
      if (!validVersion(s)) throw new Error('Invalid snapshot version');
      if (wanted && (s.sessionId !== wanted.sessionId || BigInt(s.revision) < BigInt(wanted.revision))) {
        if (++staleReads <= 2) pending = true;
        else onError?.(t('当前状态读取未成功，请刷新后核对任务；停止入口仍可使用。'));
      } else { staleReads = 0; callback(s); }
    }
  }catch{if(currentNative())onError?.(t('当前状态读取未成功，请刷新后核对任务；停止入口仍可使用。'));}finally{busy=false;if(pending&&currentNative()){pending=false;schedule();}}};
  const unlisten=await listen<StateInvalidated>('state-changed',event=>{
    if (!currentNative()) return;
    if (!validVersion(event?.payload)) { schedule(); return; }
    const next = event.payload;
    if (retiredSessions.has(next.sessionId)) return;
    if (wanted?.sessionId === next.sessionId && BigInt(next.revision) <= BigInt(wanted.revision)) return;
    if (wanted && wanted.sessionId !== next.sessionId) retiredSessions.add(wanted.sessionId);
    wanted = next; staleReads = 0; schedule();
  });
  schedule(); // Close the initial snapshot/listener-registration gap.
  return ()=>{disposed=true;if(timer)clearTimeout(timer);unlisten();};
 },
 async onDrop(callback:(paths:string[])=>void):Promise<()=>void> {
  if(!isDesktop || activeDaemon)return ()=>{};
  const {getCurrentWebview}=await import('@tauri-apps/api/webview');
  return getCurrentWebview().onDragDropEvent(event=>{if(event.payload.type==='drop')callback(event.payload.paths);});
 },
};
