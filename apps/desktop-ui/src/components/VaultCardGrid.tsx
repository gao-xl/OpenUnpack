import { Button, Checkbox, Popconfirm, Space, Switch, Tag } from 'antd';
import {
  CopyOutlined,
  DeleteOutlined,
  EditOutlined,
  EyeOutlined,
  KeyOutlined,
} from '@ant-design/icons';
import { t } from '../i18n';
import { api } from '../api';
import type { PasswordView } from '../types';
import VirtualGrid from './VirtualGrid';

export interface VaultCardGridProps {
  items: PasswordView[];
  selected: string[];
  onSelectChange: React.Dispatch<React.SetStateAction<string[]>>;
  connected: boolean;
  nativeEngine: boolean;
  saving: boolean;
  vaultError: string | null;
  onToggle: (item: PasswordView, enabled: boolean) => void;
  onCopy: (item: PasswordView) => void;
  onReveal: (item: PasswordView) => void;
  onEdit: (item: PasswordView) => void;
  run: (work: () => Promise<unknown>) => Promise<void>;
}

export default function VaultCardGrid({
  items: filteredPasswords,
  selected,
  onSelectChange: setSelected,
  connected,
  nativeEngine,
  saving,
  vaultError,
  onToggle: handleToggle,
  onCopy: copyPassword,
  onReveal: handleReveal,
  onEdit: openEdit,
  run,
}: VaultCardGridProps) {
  return (
    <VirtualGrid
      items={filteredPasswords}
      itemKey={(item) => item.id}
      minItemWidth={310}
      rowHeight={165}
      containerHeight={520}
      renderItem={(item) => {
        const isSelected = selected.includes(item.id);
        return (
          <div
            className="telemetry-card"
            style={{
              padding: '12px 14px',
              border: isSelected ? '1.5px solid var(--ou-primary)' : undefined,
              display: 'flex',
              flexDirection: 'column',
              gap: 8,
              height: '100%',
              boxSizing: 'border-box',
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0, flex: 1 }}>
                <Checkbox
                  checked={isSelected}
                  disabled={!connected || saving || !!vaultError}
                  onChange={(e) => {
                    const checked = e.target.checked;
                    setSelected((prev) =>
                      checked ? [...prev, item.id] : prev.filter((id) => id !== item.id),
                    );
                  }}
                />
                <KeyOutlined
                  style={{ color: item.enabled ? 'var(--ou-primary)' : 'var(--ou-text-muted)' }}
                />
                <span
                  style={{
                    fontWeight: 600,
                    fontSize: 13,
                    color: 'var(--ou-text-title)',
                    wordBreak: 'break-all',
                  }}
                >
                  {item.name}
                </span>
              </div>
              <Switch
                size="small"
                checked={item.enabled}
                disabled={!connected || saving || !!vaultError}
                onChange={(checked) => handleToggle(item, checked)}
                aria-label={t('启用状态')}
              />
            </div>

            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, minHeight: 22 }}>
              {item.groups?.length ? (
                item.groups.map((g) => (
                  <Tag key={g} color="blue" style={{ margin: 0, fontSize: 10 }}>
                    {g}
                  </Tag>
                ))
              ) : (
                <span style={{ fontSize: 11, color: 'var(--ou-text-muted)' }}>{t('未分组')}</span>
              )}
            </div>

            {item.notes && (
              <div
                style={{
                  fontSize: 11,
                  color: 'var(--ou-text-muted)',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                }}
              >
                {item.notes}
              </div>
            )}

            <div
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                borderTop: '1px solid var(--ou-border-color)',
                paddingTop: 6,
                marginTop: 'auto',
              }}
            >
              <span
                style={{
                  fontFamily: 'var(--ou-font-mono)',
                  fontSize: 12,
                  color: 'var(--ou-text-muted)',
                  letterSpacing: 2,
                }}
              >
                ••••••••
              </span>
              <Space size={4}>
                <Button
                  size="small"
                  icon={<CopyOutlined />}
                  disabled={!nativeEngine || !!vaultError}
                  onClick={() => copyPassword(item)}
                  aria-label={t('复制 {p0} 密码', { p0: item.name })}
                />
                <Button
                  size="small"
                  icon={<EyeOutlined />}
                  disabled={!connected || !!vaultError}
                  onClick={() => handleReveal(item)}
                  aria-label={t('查看 {p0} 密码', { p0: item.name })}
                />
                <Button
                  size="small"
                  disabled={!connected || saving || !!vaultError}
                  icon={<EditOutlined />}
                  onClick={() => openEdit(item)}
                  aria-label={t('编辑 {p0}', { p0: item.name })}
                />
                <Popconfirm
                  disabled={!connected || saving || !!vaultError}
                  title={t('确认删除该密码条目吗？')}
                  onConfirm={() => void run(() => api.deletePassword(item.id))}
                >
                  <Button
                    size="small"
                    disabled={!connected || saving || !!vaultError}
                    danger
                    icon={<DeleteOutlined />}
                    aria-label={t('删除 {p0}', { p0: item.name })}
                  />
                </Popconfirm>
              </Space>
            </div>
          </div>
        );
      }}
    />
  );
}
