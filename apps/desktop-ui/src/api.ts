import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AppSnapshot, SourceGroup, Settings, PasswordInput, PasswordView, ScanResult, ImportPreview, ImportResult, JobView } from './types';
import { defaultSettings } from './types';
export const isDesktop = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const preview: AppSnapshot = { jobs:[],passwords:[],settings:{...defaultSettings},running:false,paused:false,vaultError:null,dataDirectory:'桌面版启动后显示',engineAvailable:false,recoverableCount:0,cacheUsedBytes:0 };
async function call<T>(name:string,args?:Record<string,unknown>):Promise<T> {
 if(!isDesktop) throw new Error('此页面是界面预览，请打开桌面版使用文件与密码功能。');
 try { return await invoke<T>(name,args); } catch(e) { throw new Error(typeof e === 'string' ? e : '操作未完成，请查看任务状态后重试。'); }
}
export const api = {
 snapshot:()=>isDesktop?call<AppSnapshot>('snapshot'):Promise.resolve(preview),
 scan:(paths:string[],requestId?:string)=>call<ScanResult>('scan_sources',{paths,requestId:requestId??null}),
 cancelScan:(requestId:string)=>call<void>('cancel_scan',{requestId}),
 previewExclusions:(paths:string[],patterns:string[])=>call<{path:string;matched:boolean}[]>('preview_exclusions',{paths,patterns}),
 addJobs:(sources:SourceGroup[],options:Settings)=>call<JobView[]>('add_jobs',{sources,options}),
 start:()=>call<void>('start_jobs'),pause:()=>call<void>('pause_jobs'),
 cancel:(jobId?:string)=>call<void>('cancel_jobs',{jobId:jobId??null}),
 retry:(jobId:string,partial=false)=>call<void>('retry_job',{jobId,partial}),
 retryNode:(jobId:string,nodeId:string)=>call<void>('retry_node',{jobId,nodeId}),
 remove:(jobId:string)=>call<void>('remove_job',{jobId}),
 clearCompleted:()=>call<void>('clear_completed'),
 passwords:()=>call<PasswordView[]>('passwords'),
 savePassword:(input:PasswordInput)=>call<PasswordView>('save_password',{input}),
 deletePassword:(id:string)=>call<void>('delete_password',{id}),
 revealPassword:(id:string)=>call<string>('reveal_password',{id}),
 copyPassword:(id:string)=>call<void>('copy_password',{id}),
 bulkPasswordAction:(ids:string[],action:'enable'|'disable'|'delete'|'group',groups:string[]=[])=>call<void>('bulk_password_action',{ids,action,groups}),
 previewImport:(text:string,group:string,format:string)=>call<ImportPreview>('preview_import',{text,group,format}),
 confirmImport:(text:string,group:string,format:string,token:string)=>call<ImportResult>('confirm_import',{text,group,format,token}),
 exportBackup:(path:string,passphrase:string)=>call<void>('export_backup',{path,passphrase}),
 restoreBackup:(path:string,passphrase:string)=>call<ImportResult>('restore_backup',{path,passphrase}),
 previewRestoreBackup:(path:string,passphrase:string)=>call<ImportPreview>('preview_restore_backup',{path,passphrase}),
 confirmRestoreBackup:(path:string,passphrase:string,token:string)=>call<ImportResult>('confirm_restore_backup',{path,passphrase,token}),
 saveSettings:(settings:Settings)=>call<Settings>('save_settings',{settings}),
 history:()=>call<JobView[]>('history'),
 clearHistory:()=>call<void>('clear_history'),
 recover:(jobId:string,action:string)=>call<void>('recover_job',{jobId,action}),
 openPath:(path:string)=>call<void>('open_path',{path}),
 pickFiles:(folder=false)=>call<string[]>('pick_files',{folder}),
 pickDirectory:()=>call<string|null>('pick_directory'),
 pickSaveBackup:()=>call<string|null>('pick_save_backup'),
 pickOpenBackup:()=>call<string|null>('pick_open_backup'),
 readImportFile:(path:string,encoding='utf-8')=>call<string>('read_import_file',{path,encoding}),
 async subscribe(callback:(snapshot:AppSnapshot)=>void,onError?:(message:string)=>void):Promise<()=>void> {
  if(!isDesktop) return ()=>{};
  let disposed=false, timer:ReturnType<typeof setTimeout>|undefined, busy=false, pending=false;
  const schedule=()=>{if(disposed||timer!==undefined)return;timer=setTimeout(()=>{timer=undefined;void update();},150);};
  const update=async()=>{if(disposed)return;if(busy){pending=true;return;}busy=true;try{const s=await api.snapshot();if(!disposed)callback(s);}catch{if(!disposed)onError?.('当前状态读取未成功，请刷新后核对任务；停止入口仍可使用。');}finally{busy=false;if(pending&&!disposed){pending=false;schedule();}}};
  const unlisten=await listen('state-changed',schedule);
  return ()=>{disposed=true;if(timer)clearTimeout(timer);unlisten();};
 },
 async onDrop(callback:(paths:string[])=>void):Promise<()=>void> {
  if(!isDesktop)return ()=>{};
  const {getCurrentWebview}=await import('@tauri-apps/api/webview');
  return getCurrentWebview().onDragDropEvent(event=>{if(event.payload.type==='drop')callback(event.payload.paths);});
 },
};
