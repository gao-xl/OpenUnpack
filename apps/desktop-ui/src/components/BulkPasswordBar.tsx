import { t } from '../i18n';
import React, { useState } from 'react';
import { App as AntApp, Button, Select, Space, Typography } from 'antd';

const { Text } = Typography;

export interface BulkPasswordBarProps {
  selectedCount: number;
  disabled: boolean;
  busy: boolean;
  groups: string[];
  onAction: (action: 'enable' | 'disable' | 'delete' | 'setGroups', groups?: string[]) => void;
  onClear: () => void;
}

export default function BulkPasswordBar({
  selectedCount,
  disabled,
  busy,
  groups,
  onAction,
  onClear,
}: BulkPasswordBarProps): React.JSX.Element {
  const [selectedGroups, setSelectedGroups] = useState<string[]>([]);
  const { modal } = AntApp.useApp();

  const isActionDisabled = disabled || busy || selectedCount === 0;
  const isClearDisabled = selectedCount === 0 || busy;

  const validGroups = selectedGroups
    .map((group) => group.trim())
    .filter((group) => group.length > 0);

  const canAddGroups = !isActionDisabled && validGroups.length > 0;

  const handleEnable = () => {
    onAction('enable');
  };

  const handleDisable = () => {
    onAction('disable');
  };

  const handleAddGroups = () => {
    if (validGroups.length === 0) {
      return;
    }
    modal.confirm({ title: t("替换所选密码的分组？"), content: t("将所选条目的分组替换为：{p0}。其他条目不受影响。", { p0: validGroups.join('、') }), okText: t("替换分组"), cancelText: t("取消"), onOk: () => { onAction('setGroups', validGroups); setSelectedGroups([]); } });
  };

  const handleDelete = () => {
    modal.confirm({
      title: t("删除所选密码？"),
      content: t("成功关联也会移除，处理记录不包含密码。"),
      okText: t("删除"),
      okType: 'danger',
      cancelText: t("取消"),
      onOk: () => {
        onAction('delete');
      },
    });
  };

  const groupOptions = groups.map((group) => ({
    label: group,
    value: group,
  }));

  return (
    <Space
      align="center"
      wrap
      size="middle"
      aria-label={t("密码批量操作工具栏")}
      style={{ padding: '8px 12px', background: 'var(--ou-bg-card)', border: '1px solid var(--ou-border-color)', borderRadius: 6 }}
    >
      <Text>{t("已选择 {count} 条", { count: selectedCount })}</Text>

      <Button
        type="link"
        size="small"
        disabled={isClearDisabled}
        onClick={onClear}
        aria-label={t("清空选择")}
      >{t("清空选择")}</Button>

      <Button
        size="small"
        disabled={isActionDisabled}
        onClick={handleEnable}
        aria-label={t("启用所选密码")}
      >{t("启用")}</Button>

      <Button
        size="small"
        disabled={isActionDisabled}
        onClick={handleDisable}
        aria-label={t("禁用所选密码")}
      >{t("禁用")}</Button>

      <Select
        mode="tags"
        size="small"
        style={{ minWidth: 180, maxWidth: 320 }}
        placeholder={t("选择或输入分组")}
        disabled={isActionDisabled}
        value={selectedGroups}
        onChange={(values) => setSelectedGroups(values)}
        options={groupOptions}
        aria-label={t("选择或创建分组标签")}
      />

      <Button
        size="small"
        type="primary"
        disabled={!canAddGroups}
        loading={busy}
        onClick={handleAddGroups}
        aria-label={t("替换所选密码的分组")}
      >{t("替换分组")}</Button>

      <Button size="small" disabled={isActionDisabled} aria-label={t("清空所选密码的分组")} onClick={() => void modal.confirm({ title: t("清空所选密码的分组？"), content: t("这些密码仍保留并可在全部启用密码范围使用。"), okText: t("清空分组"), cancelText: t("取消"), onOk: () => onAction('setGroups', []) })}>{t("清空分组")}</Button>

      <Button
        size="small"
        danger
        disabled={isActionDisabled}
        onClick={handleDelete}
        aria-label={t("删除所选密码")}
      >{t("删除")}</Button>
    </Space>
  );
}
