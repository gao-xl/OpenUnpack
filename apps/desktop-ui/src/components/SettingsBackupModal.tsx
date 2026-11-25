import { t } from '../i18n';
import { Alert, Input, Modal, Space, Table, Typography } from 'antd';
import type { ImportPreview } from '../types';

const { Text } = Typography;

interface SettingsBackupModalProps {
  modalMode: 'export' | 'restore' | null;
  modalPath: string;
  passphrase: string;
  confirmPass: string;
  modalBusy: boolean;
  restorePreview: ImportPreview | null;
  onChangePassphrase: (value: string) => void;
  onChangeConfirmPass: (value: string) => void;
  onSubmit: () => void;
  onCancel: () => void;
}

export default function SettingsBackupModal({
  modalMode,
  modalPath,
  passphrase,
  confirmPass,
  modalBusy,
  restorePreview,
  onChangePassphrase,
  onChangeConfirmPass,
  onSubmit,
  onCancel,
}: SettingsBackupModalProps) {
  return (
    <Modal
      title={modalMode === 'export' ? t('导出密码库备份') : t('恢复密码库备份')}
      open={modalMode !== null}
      onCancel={onCancel}
      destroyOnHidden
      okText={
        modalMode === 'export'
          ? t('导出加密备份')
          : restorePreview
            ? t('确认合并')
            : t('解密并预览')
      }
      cancelText={t('取消')}
      okButtonProps={{
        loading: modalBusy,
        disabled:
          Array.from(passphrase).length < 8 ||
          (modalMode === 'export' && passphrase !== confirmPass) ||
          !!restorePreview?.errorCount,
      }}
      onOk={onSubmit}
    >
      <Space direction="vertical" style={{ width: '100%' }}>
        <div>
          <Text type="secondary">{t('文件路径：')}</Text>
          <Text code style={{ wordBreak: 'break-all' }}>
            {modalPath}
          </Text>
        </div>
        <Alert
          type="info"
          title={modalMode === 'restore' ? t('恢复备份说明') : t('导出备份说明')}
          description={
            modalMode === 'restore'
              ? t(
                  '解密并校验条目后可预览内容，确认后合并到当前库。重复条目将保留名称和备注并合并分组。密码错误或文件损坏不会影响当前密码库。',
                )
              : t('备份文件将使用你设置的独立密码加密，可用于在其他电脑或系统中恢复。请妥善保存该密码。')
          }
        />
        <div>
          <Text>{t('备份密码（至少 8 位）：')}</Text>
          <Input.Password
            aria-label={t('备份密码')}
            autoComplete="new-password"
            disabled={modalBusy}
            value={passphrase}
            onChange={(e) => onChangePassphrase(e.target.value)}
            placeholder={t('至少 8 位密码')}
          />
        </div>
        {modalMode === 'export' && (
          <div>
            <Text>{t('确认密码：')}</Text>
            <Input.Password
              value={confirmPass}
              onChange={(e) => onChangeConfirmPass(e.target.value)}
              placeholder={t('请再次输入密码')}
            />
          </div>
        )}
        {restorePreview && (
          <>
            <Space wrap>
              <Text strong>{t('备份预览')}</Text>
              <Text>{t('新增 {count} 条', { count: restorePreview.newCount })}</Text>
              <Text>{t('重复 {count} 条', { count: restorePreview.duplicateCount })}</Text>
              <Text type={restorePreview.errorCount ? 'danger' : undefined}>
                {t('错误 {count} 条', { count: restorePreview.errorCount })}
              </Text>
            </Space>
            <Table
              size="small"
              rowKey={(row) => `${row.line}-${row.name}`}
              dataSource={restorePreview.rows}
              pagination={{ pageSize: 5 }}
              columns={[
                { title: t('条目'), dataIndex: 'line' },
                { title: t('名称'), dataIndex: 'name' },
                { title: t('分组'), dataIndex: 'group' },
                { title: t('状态'), dataIndex: 'status' },
                { title: t('说明'), dataIndex: 'message' },
              ]}
            />
          </>
        )}
      </Space>
    </Modal>
  );
}
