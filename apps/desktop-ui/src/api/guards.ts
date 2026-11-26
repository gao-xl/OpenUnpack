import { t } from '../i18n';
import type { AppSnapshot, JobView, Settings } from '../types';
import { defaultSettings } from '../types';
import { validVersion } from '../snapshot-cursor';

export const nativeCommands = new Set([
  'api_status',
  'api_configure',
  'api_create_client',
  'api_revoke_client',
  'api_open_docs',
  'set_ui_language',
  'pick_files',
  'pick_directory',
  'pick_save_backup',
  'pick_open_backup',
  'copy_password',
  'open_path',
  'execute_post_action',
  'get_shell_context_menu_status',
  'set_shell_context_menu_status',
  'take_pending_open_paths',
  'submit_native_input',
]);
export const engineLocalCommands = new Set(['copy_password', 'open_path', 'execute_post_action']);
export const daemonOnlyCommands = new Set(['start_selected_jobs', 'add_archive', 'submit_input']);
export function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}
export function settingsShape(value: unknown): value is Settings {
  return (
    isRecord(value) &&
    Object.entries(defaultSettings).every(([key, sample]) => {
      const field = value[key];
      if (Array.isArray(sample))
        return Array.isArray(field) && field.every((item) => typeof item === 'string');
      if (sample === null) return field === null || typeof field === 'string';
      return (
        typeof field === typeof sample && (typeof field !== 'number' || Number.isFinite(field))
      );
    })
  );
}
export function sourceShape(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.path === 'string' &&
    typeof value.displayName === 'string' &&
    Array.isArray(value.volumes) &&
    value.volumes.every((path) => typeof path === 'string') &&
    typeof value.size === 'number' &&
    Number.isFinite(value.size) &&
    typeof value.missingVolume === 'boolean'
  );
}
export function passwordShape(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.name === 'string' &&
    typeof value.notes === 'string' &&
    Array.isArray(value.groups) &&
    value.groups.every((group) => typeof group === 'string') &&
    typeof value.enabled === 'boolean' &&
    typeof value.successCount === 'number' &&
    typeof value.revision === 'number' &&
    (value.lastSuccess === null || typeof value.lastSuccess === 'string')
  );
}
export function jobShape(value: unknown): value is JobView {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    sourceShape(value.source) &&
    typeof value.status === 'string' &&
    typeof value.stage === 'string' &&
    typeof value.progress === 'number' &&
    typeof value.message === 'string' &&
    typeof value.attempts === 'number' &&
    typeof value.verificationScope === 'string' &&
    (value.outputPath === null || typeof value.outputPath === 'string') &&
    (value.workspace === null || typeof value.workspace === 'string') &&
    (value.finishedAt === null || typeof value.finishedAt === 'string') &&
    typeof value.createdAt === 'string' &&
    settingsShape(value.options) &&
    Array.isArray(value.nodes) &&
    value.nodes.every(
      (node) =>
        isRecord(node) &&
        typeof node.id === 'string' &&
        typeof node.name === 'string' &&
        typeof node.status === 'string' &&
        typeof node.stage === 'string' &&
        typeof node.depth === 'number' &&
        typeof node.progress === 'number' &&
        typeof node.attempts === 'number' &&
        typeof node.message === 'string',
    ) &&
    isRecord(value.stageTimes) &&
    Object.values(value.stageTimes).every(
      (time) => typeof time === 'number' && Number.isFinite(time),
    )
  );
}
export function snapshotShape(value: unknown): value is AppSnapshot {
  return (
    isRecord(value) &&
    validVersion(value) &&
    Array.isArray(value.jobs) &&
    value.jobs.every(jobShape) &&
    Array.isArray(value.passwords) &&
    value.passwords.every(passwordShape) &&
    settingsShape(value.settings) &&
    typeof value.running === 'boolean' &&
    typeof value.paused === 'boolean' &&
    typeof value.engineAvailable === 'boolean' &&
    typeof value.dataDirectory === 'string' &&
    typeof value.cacheUsedBytes === 'number' &&
    Number.isFinite(value.cacheUsedBytes) &&
    typeof value.recoverableCount === 'number' &&
    (value.vaultError === null || typeof value.vaultError === 'string')
  );
}
export function checkResult(command: string, value: unknown) {
  const valid =
    command === 'snapshot' || command === 'queue_snapshot'
      ? snapshotShape(value)
      : command === 'history' || command === 'add_jobs' || command === 'add_archive'
        ? Array.isArray(value) && value.every(jobShape)
        : command === 'save_settings' || command === 'read_settings'
          ? settingsShape(value)
          : command === 'job_details'
            ? value === null || jobShape(value)
            : command === 'history_page'
              ? isRecord(value) &&
                Array.isArray(value.items) &&
                typeof value.total === 'number' &&
                typeof value.filteredTotal === 'number' &&
                validVersion(value)
              : true;
  if (!valid) throw new Error(t('守护进程返回的数据格式无效，请核对服务版本。'));
}
export const LONG_RUNNING_DAEMON_COMMANDS = new Set([
  'scan_sources',
  'preview_exclusions',
  'inspect_archive',
  'export_backup',
  'preview_restore_backup',
  'confirm_restore_backup',
  'extract_selective',
  'preview_toolbox_operation',
  'execute_toolbox_operation',
  'fix_archive_extensions',
]);
