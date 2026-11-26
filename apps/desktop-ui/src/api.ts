import { listen } from '@tauri-apps/api/event';
import type {
  LocalApiSettings,
  LocalApiStatus,
  ApiClientInput,
  CreatedApiClient,
} from './local-api-types';
import type {
  AppSnapshot,
  SourceGroup,
  Settings,
  PasswordInput,
  PasswordView,
  ScanResult,
  ImportPreview,
  ImportResult,
  JobView,
  HistoryPageQuery,
  JobHistoryPage,
  ArchiveInspectionView,
  ToolboxOptions,
  ToolboxPreviewResult,
  ToolboxExecuteRequest,
  ToolboxExecuteResult,
  ArchiveExtensionFixItem,
  SnapshotUpdate,
  SelectiveExtractRequest,
  SelectiveExtractResult,
} from './types';
import {
  call,
  callNative,
  connectDaemon,
  getActiveDaemon,
  getConnectionVersion,
  getDaemonConnection,
  isDaemonMode,
  isDesktop,
  isWebConsole,
  preview,
  setDaemonConnection,
  setLanguage,
  verifyDaemonConnection,
} from './api/connection';
import {
  onNeedPassword,
  onProgress,
  subscribe,
  type JobProgressUpdatePayload,
  type NeedPasswordEventPayload,
} from './api/events';

export { isDesktop, type JobProgressUpdatePayload, type NeedPasswordEventPayload };

/**
 * 类型化的 API 门面：方法名与宿主命令 / 守护进程路由一一对应。
 * 桌面模式走 Tauri invoke，守护进程模式走同源 HTTP。
 */
export const api = {
  setLanguage,
  apiStatus: () => callNative<LocalApiStatus>('api_status'),
  apiConfigure: (settings: LocalApiSettings) =>
    callNative<LocalApiStatus>('api_configure', { settings }),
  apiCreateClient: (input: ApiClientInput) =>
    callNative<CreatedApiClient>('api_create_client', { input }),
  apiRevokeClient: (id: string) => callNative<void>('api_revoke_client', { id }),
  apiOpenDocs: () => callNative<void>('api_open_docs'),
  snapshot: () =>
    isDesktop || getActiveDaemon() ? call<AppSnapshot>('queue_snapshot') : Promise.resolve(preview),
  fullSnapshot: () =>
    isDesktop || getActiveDaemon() ? call<AppSnapshot>('snapshot') : Promise.resolve(preview),
  snapshotUpdate: (sinceRevision?: string | null): Promise<SnapshotUpdate> =>
    getActiveDaemon()
      ? api.snapshot().then((full) => ({
          sessionId: full.sessionId,
          revision: full.revision,
          full,
          delta: null,
        }))
      : isDesktop
        ? call<SnapshotUpdate>('snapshot_update', { sinceRevision: sinceRevision ?? null })
        : Promise.resolve({ sessionId: 'preview', revision: '0', full: preview, delta: null }),
  scan: (paths: string[], requestId?: string) =>
    call<ScanResult>('scan_sources', { paths, requestId: requestId ?? null }),
  cancelScan: (requestId: string) => call<void>('cancel_scan', { requestId }),
  previewExclusions: (
    paths: string[],
    patterns: string[],
    scope: 'input' | 'output' = 'input',
    requestId?: string,
  ) =>
    call<{ path: string; matched: boolean }[]>('preview_exclusions', {
      paths,
      patterns,
      scope,
      requestId: requestId ?? null,
    }),
  cancelPreviewExclusions: (requestId: string) =>
    call<void>('cancel_preview_exclusions', { requestId }),
  addJobs: (sources: SourceGroup[], options: Settings) =>
    call<JobView[]>('add_jobs', { sources, options }),
  start: () => call<void>('start_jobs'),
  startSelected: (jobIds: string[]) => call<void>('start_selected_jobs', { jobIds }),
  pause: () => call<void>('pause_jobs'),
  cancel: (jobId?: string) => call<void>('cancel_jobs', { jobId: jobId ?? null }),
  retry: (jobId: string, partial = false, password?: string) =>
    call<void>(
      'retry_job',
      password !== undefined ? { jobId, partial, password } : { jobId, partial },
    ),
  retryNode: (jobId: string, nodeId: string) => call<void>('retry_node', { jobId, nodeId }),
  remove: (jobId: string) => call<void>('remove_job', { jobId }),
  clearCompleted: () => call<void>('clear_completed'),
  savePassword: (input: PasswordInput) => call<PasswordView>('save_password', { input }),
  deletePassword: (id: string) => call<void>('delete_password', { id }),
  revealPassword: (id: string) => call<string>('reveal_password', { id }),
  copyPassword: (id: string) => call<void>('copy_password', { id }),
  bulkPasswordAction: (
    ids: string[],
    action: 'enable' | 'disable' | 'delete' | 'group',
    groups: string[] = [],
  ) => call<void>('bulk_password_action', { ids, action, groups }),
  previewImport: (text: string, group: string, format: string) =>
    call<ImportPreview>('preview_import', { text, group, format }),
  confirmImport: (text: string, group: string, format: string, token: string) =>
    call<ImportResult>('confirm_import', { text, group, format, token }),
  exportBackup: (path: string, passphrase: string) =>
    call<void>('export_backup', { path, passphrase }),
  previewRestoreBackup: (path: string, passphrase: string) =>
    call<ImportPreview>('preview_restore_backup', { path, passphrase }),
  confirmRestoreBackup: (path: string, passphrase: string, token: string) =>
    call<ImportResult>('confirm_restore_backup', { path, passphrase, token }),
  saveSettings: (settings: Settings) => call<Settings>('save_settings', { settings }),
  history: () => call<JobView[]>('history'),
  historyPage: (query: HistoryPageQuery) => call<JobHistoryPage>('history_page', { query }),
  jobDetails: (jobId: string) => call<JobView | null>('job_details', { jobId }),
  inspectArchive: (path: string, password: string | null, inspectId: string) =>
    call<ArchiveInspectionView>('inspect_archive', { path, password, inspectId }),
  cancelInspectArchive: (inspectId: string) => call<void>('cancel_inspect_archive', { inspectId }),
  extractSelective: (request: SelectiveExtractRequest) =>
    call<SelectiveExtractResult>('extract_selective', { request }),
  clearHistory: () => call<void>('clear_history'),
  recover: (jobId: string, action: string) => call<void>('recover_job', { jobId, action }),
  openPath: (path: string) => call<void>('open_path', { path }),
  pickFiles: (folder = false) => call<string[]>('pick_files', { folder }),
  pickDirectory: () => call<string | null>('pick_directory'),
  pickSaveBackup: () => call<string | null>('pick_save_backup'),
  pickOpenBackup: () => call<string | null>('pick_open_backup'),
  readImportFile: (path: string, encoding = 'utf-8') =>
    call<string>('read_import_file', { path, encoding }),
  previewToolbox: (options: ToolboxOptions) =>
    call<ToolboxPreviewResult>('preview_toolbox_operation', { options }),
  executeToolbox: (request: ToolboxExecuteRequest) =>
    call<ToolboxExecuteResult>('execute_toolbox_operation', { request }),
  fixArchiveExtensions: (paths: string[]) =>
    call<ArchiveExtensionFixItem[]>('fix_archive_extensions', { paths }),
  executePostAction: (action: string, targetPath?: string) =>
    call<void>('execute_post_action', { action, targetPath: targetPath ?? null }),
  getShellContextMenuStatus: () => call<boolean>('get_shell_context_menu_status'),
  setShellContextMenuStatus: (enabled: boolean) =>
    call<boolean>('set_shell_context_menu_status', { enabled }),
  setDaemonConnection,
  connectDaemon,
  verifyDaemonConnection,
  getConnectionVersion,
  getDaemonConnection,
  isDaemonMode,
  isWebConsole,
  addArchive: (
    path: string,
    outputDir?: string,
    password?: string,
    deleteSource?: boolean,
    autoStart = true,
  ) =>
    call<JobView[]>('add_archive', {
      path,
      outputDir: outputDir ?? null,
      password: password ?? null,
      deleteSource: deleteSource ?? null,
      autoStart,
    }),
  submitInput: (jobId: string, inputType: string, value: string) =>
    call<{ success: boolean; jobId: string; inputType: string; resumedInFlight?: boolean }>(
      'submit_input',
      { jobId, inputType, value },
    ),
  submitNativeInput: (jobId: string, value: string) =>
    callNative<boolean>('submit_native_input', { jobId, inputType: 'password', value }),
  onProgress,
  onNeedPassword,
  subscribe,
  async onDrop(callback: (paths: string[]) => void): Promise<() => void> {
    if (!isDesktop || getActiveDaemon()) return () => {};
    const { getCurrentWebview } = await import('@tauri-apps/api/webview');
    return getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'drop') callback(event.payload.paths);
    });
  },
  async onWindowDragDrop(
    onHover: (dragging: boolean) => void,
    onDrop?: (paths: string[]) => void,
  ): Promise<() => void> {
    if (!isDesktop || getActiveDaemon()) return () => {};
    const { getCurrentWebview } = await import('@tauri-apps/api/webview');
    return getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'enter' || event.payload.type === 'over') {
        onHover(true);
      } else if (event.payload.type === 'leave') {
        onHover(false);
      } else if (event.payload.type === 'drop') {
        onHover(false);
        onDrop?.(event.payload.paths);
      }
    });
  },
  takePendingOpenPaths: (): Promise<string[]> =>
    isDesktop ? callNative<string[]>('take_pending_open_paths') : Promise.resolve([]),
  async onExternalOpenPaths(callback: (paths: string[]) => void): Promise<() => void> {
    if (!isDesktop) return () => {};
    return listen<string[]>('external-open-paths', (event) => {
      if (Array.isArray(event?.payload) && event.payload.length > 0) {
        callback(event.payload);
      }
    });
  },
};
