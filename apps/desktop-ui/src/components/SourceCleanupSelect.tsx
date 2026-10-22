import { t } from '../i18n';
import { App as AntApp, Select, Typography } from 'antd';

export const sourceCleanupLabels: Record<string, string> = {
  keep: '保留原始压缩包',
  recycle: '完整交付后移入回收站',
  delete: '完整交付后直接删除',
};

export function SourceCleanupScope({ value, expandNested }: { value: string; expandNested: boolean }) {
  return <Typography.Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 0 }}>
    {value === 'keep'
      ? t("原始外层压缩包及全部分卷会保留。")
      : value === 'recycle'
        ? t("完整解压、发布并校验通过后，将原始外层压缩包及全部分卷移入回收站。")
        : t("完整解压、发布并校验通过后，将原始外层压缩包及全部分卷直接删除。")} {' '}
    {expandNested
      ? t("多层解压后，结果中不保留已成功展开的内包及分卷；未展开的内包保留。任务外的压缩包不受影响。")
      : t("未启用多层解压，压缩包中的内包作为普通文件保留。")} {' '}
    {value !== 'keep' && t("任一层失败、取消、达到层数限制、仅交付部分内容或设置输出筛选时，外层源压缩包保留。")}
  </Typography.Paragraph>;
}

export default function SourceCleanupSelect({ value, onChange, disabled = false }: { value: string; onChange: (value: string) => void; disabled?: boolean }) {
  const { modal } = AntApp.useApp();
  return <Select aria-label={t("完整交付后源压缩包处理")} value={value} disabled={disabled} style={{ minWidth: 220 }} options={Object.entries(sourceCleanupLabels).map(([option, label]) => ({ value: option, label: t(label) }))} onChange={next => {
    if (next !== 'delete') { onChange(next); return; }
    modal.confirm({
      title: t("启用源包直接删除？"),
      content: t("完整交付并核对后，原始压缩包及分卷将直接删除，不进入回收站。部分交付、输出排除或核对失败时保留源包。此设置只影响使用该策略的新任务。"),
      okText: t("启用直接删除"), okButtonProps: { danger: true }, cancelText: t("继续保留"),
      onOk: () => onChange(next),
    });
  }} />;
}
