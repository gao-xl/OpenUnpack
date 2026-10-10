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
    modal.confirm({ title: '替换所选密码的分组？', content: `将所选条目的分组替换为：${validGroups.join('、')}。其他条目不受影响。`, okText: '替换分组', cancelText: '取消', onOk: () => { onAction('setGroups', validGroups); setSelectedGroups([]); } });
  };

  const handleDelete = () => {
    modal.confirm({
      title: '删除所选密码？',
      content: '成功关联也会移除，处理记录不包含密码。',
      okText: '删除',
      okType: 'danger',
      cancelText: '取消',
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
      aria-label="密码批量操作工具栏"
      style={{ padding: '8px 12px', background: 'var(--ou-bg-card)', border: '1px solid var(--ou-border-color)', borderRadius: 6 }}
    >
      <Text>
        已选择 <strong>{selectedCount}</strong> 条
      </Text>

      <Button
        type="link"
        size="small"
        disabled={isClearDisabled}
        onClick={onClear}
        aria-label="清空选择"
      >
        清空选择
      </Button>

      <Button
        size="small"
        disabled={isActionDisabled}
        onClick={handleEnable}
        aria-label="启用所选密码"
      >
        启用
      </Button>

      <Button
        size="small"
        disabled={isActionDisabled}
        onClick={handleDisable}
        aria-label="禁用所选密码"
      >
        禁用
      </Button>

      <Select
        mode="tags"
        size="small"
        style={{ minWidth: 180, maxWidth: 320 }}
        placeholder="选择或输入分组"
        disabled={isActionDisabled}
        value={selectedGroups}
        onChange={(values) => setSelectedGroups(values)}
        options={groupOptions}
        aria-label="选择或创建分组标签"
      />

      <Button
        size="small"
        type="primary"
        disabled={!canAddGroups}
        loading={busy}
        onClick={handleAddGroups}
        aria-label="替换所选密码的分组"
      >
        替换分组
      </Button>

      <Button size="small" disabled={isActionDisabled} aria-label="清空所选密码的分组" onClick={() => modal.confirm({ title: '清空所选密码的分组？', content: '这些密码仍保留并可在全部启用密码范围使用。', okText: '清空分组', cancelText: '取消', onOk: () => onAction('setGroups', []) })}>清空分组</Button>

      <Button
        size="small"
        danger
        disabled={isActionDisabled}
        onClick={handleDelete}
        aria-label="删除所选密码"
      >
        删除
      </Button>
    </Space>
  );
}
