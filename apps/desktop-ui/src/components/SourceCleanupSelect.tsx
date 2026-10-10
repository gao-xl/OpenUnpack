import { App as AntApp, Select } from 'antd';

export const sourceCleanupLabels: Record<string, string> = {
  keep: '保留原始压缩包',
  recycle: '完整交付后移入回收站',
  delete: '完整交付后直接删除',
};

export default function SourceCleanupSelect({ value, onChange, disabled = false }: { value: string; onChange: (value: string) => void; disabled?: boolean }) {
  const { modal } = AntApp.useApp();
  return <Select aria-label="完整交付后源压缩包处理" value={value} disabled={disabled} style={{ minWidth: 220 }} options={Object.entries(sourceCleanupLabels).map(([option, label]) => ({ value: option, label }))} onChange={next => {
    if (next !== 'delete') { onChange(next); return; }
    modal.confirm({
      title: '启用源包直接删除？',
      content: '完整交付并核对后，原始压缩包及分卷将直接删除，不进入回收站。部分交付、输出排除或核对失败时保留源包。此设置只影响使用该策略的新任务。',
      okText: '启用直接删除', okButtonProps: { danger: true }, cancelText: '继续保留',
      onOk: () => onChange(next),
    });
  }} />;
}
