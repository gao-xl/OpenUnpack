import { t } from '../i18n';
import { Alert, Modal, Space, Typography } from 'antd';
import { ExclamationCircleOutlined } from '@ant-design/icons';
import type { ConflictStrategy, ToolboxKind } from '../types';

const { Text } = Typography;

interface ToolboxConfirmModalProps {
  open: boolean;
  executing: boolean;
  kind: ToolboxKind;
  selectedCount: number;
  conflictStrategy: ConflictStrategy;
  removeEmptyFolders: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export default function ToolboxConfirmModal({
  open,
  executing,
  kind,
  selectedCount,
  conflictStrategy,
  removeEmptyFolders,
  onConfirm,
  onCancel,
}: ToolboxConfirmModalProps) {
  return (
    <Modal
      title={
        <Space>
          <ExclamationCircleOutlined style={{ color: 'var(--ou-danger)' }} />
          <span>{t('二次确认：执行本地目录整理')}</span>
        </Space>
      }
      open={open}
      onOk={onConfirm}
      confirmLoading={executing}
      onCancel={onCancel}
      okText={t('确认立即执行')}
      okButtonProps={{ danger: true }}
      cancelText={t('取消')}
    >
      <Space direction="vertical" size={12} style={{ width: '100%', marginTop: 8 }}>
        <Alert
          type="warning"
          showIcon
          title={t('此操作将直接重命名或移动本地磁盘上的文件')}
          description={t(
            '改动执行后无法自动撤销。已自动通过 NativeAccess 权限模型校验目录边界，禁止跨越软链接及系统目录。',
          )}
        />
        <div>
          <Text strong>{t('操作类型：')}</Text>{' '}
          {kind === 'normalizeExtensions' && t('扩展名规范化')}
          {kind === 'batchRename' && t('智能批量更名')}
          {kind === 'flattenDirectory' && t('平铺提取与整理')}
        </div>
        <div>
          <Text strong>{t('待处理文件数：')}</Text> {t('{count} 项', { count: selectedCount })}
        </div>
        <div>
          <Text strong>{t('冲突处理机制：')}</Text>{' '}
          {conflictStrategy === 'rename'
            ? t('自动追加数字后缀 (_1, _2) 规避冲突')
            : conflictStrategy === 'skip'
              ? t('跳过目标已存在的冲突文件')
              : t('发生冲突时报错中止')}
        </div>
        {kind === 'flattenDirectory' && removeEmptyFolders && (
          <div>
            <Text strong>{t('后续动作：')}</Text>
            {t('将自动清理平铺提取后留下的空子目录')}
          </div>
        )}
      </Space>
    </Modal>
  );
}
