import {
  Alert,
  Button,
  Card,
  Form,
  Input,
  Modal,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Typography,
} from 'antd';
import { FolderOpenOutlined } from '@ant-design/icons';
import type { FormInstance } from 'antd/es/form';
import { t } from '../i18n';
import type { ImportPreview, PasswordInput, PasswordView } from '../types';

const { Text, Paragraph } = Typography;

export interface VaultModalsProps {
  connected: boolean;
  nativeEngine: boolean;
  saving: boolean;
  groups: string[];
  isEditOpen: boolean;
  editingItem: PasswordView | null;
  form: FormInstance<PasswordInput>;
  secretVisible: boolean;
  onSecretVisibleChange: (v: boolean) => void;
  onEditValuesChange: () => void;
  onSaveEdit: () => Promise<void>;
  onCancelEdit: () => void;
  revealTarget: PasswordView | null;
  revealedSecret: string | null;
  revealError: string | null;
  revealSeconds: number;
  onCloseReveal: () => void;
  isImportOpen: boolean;
  importFormat: 'txt' | 'tsv' | 'csv';
  importGroup: string;
  importEncoding: string;
  importText: string;
  preview: ImportPreview | null;
  onImportFormatChange: (fmt: 'txt' | 'tsv' | 'csv') => void;
  onImportGroupChange: (grp: string) => void;
  onImportEncodingChange: (enc: string) => void;
  onImportTextChange: (txt: string) => void;
  onPickImportFile: () => Promise<void>;
  onPreviewImport: () => void;
  onConfirmImport: () => Promise<void>;
  onCancelImport: () => void;
}

export default function VaultModals(props: VaultModalsProps) {
  const {
    connected,
    nativeEngine,
    saving,
    groups,
    isEditOpen,
    editingItem,
    form,
    secretVisible,
    onSecretVisibleChange: setSecretVisible,
    onEditValuesChange,
    onSaveEdit: handleSave,
    onCancelEdit: clearEdit,
    revealTarget,
    revealedSecret,
    revealError,
    revealSeconds,
    onCloseReveal: closeReveal,
    isImportOpen,
    importFormat,
    importGroup,
    importEncoding,
    importText,
    preview,
    onImportFormatChange,
    onImportGroupChange,
    onImportEncodingChange,
    onImportTextChange,
    onPickImportFile: handlePickFile,
    onPreviewImport: handlePreviewImport,
    onConfirmImport: handleConfirmImport,
    onCancelImport: clearImport,
  } = props;
  return (
    <>
      <Modal
        title={editingItem ? t('编辑密码条目') : t('新建密码条目')}
        open={isEditOpen}
        onOk={() => void handleSave()}
        confirmLoading={saving}
        okText={t('保存')}
        cancelText={t('取消')}
        onCancel={clearEdit}
        destroyOnHidden
      >
        <Form
          form={form}
          layout="vertical"
          preserve={false}
          onValuesChange={() => {
            onEditValuesChange();
          }}
        >
          <Form.Item
            name="name"
            label={t('名称 / 标识')}
            rules={[{ required: true, message: t('请输入密码名称或标识') }]}
          >
            <Input placeholder={t('例如：日常解压密码')} maxLength={100} />
          </Form.Item>
          <Form.Item
            name="secret"
            label={t('密码明文')}
            rules={[
              ...(editingItem ? [] : [{ required: true, message: t('请输入密码') }]),
              {
                validator: (_rule, value: string | undefined) =>
                  value && /[\0\r\n]/.test(value)
                    ? Promise.reject(new Error(t('密码不支持 NUL、换行或回车。')))
                    : Promise.resolve(),
              },
            ]}
            extra={t('{p0}首尾空格与大小写会完整保留；仅支持单行密码。', {
              p0: editingItem ? t('留空保持原密码。') : '',
            })}
          >
            <Input.Password
              placeholder={t('密码明文，保存后加密存储')}
              visibilityToggle={{ visible: secretVisible, onVisibleChange: setSecretVisible }}
              autoComplete="new-password"
            />
          </Form.Item>
          <Form.Item name="groups" label={t('分组标签')}>
            <Select
              mode="tags"
              placeholder={t('选择或输入标签后回车')}
              tokenSeparators={[',']}
              notFoundContent={t('暂无已有分组，直接输入新标签后按回车添加')}
              options={groups.map((group) => ({ value: group, label: group }))}
            />
          </Form.Item>
          <Form.Item name="notes" label={t('备注信息')}>
            <Input.TextArea rows={2} placeholder={t('来源说明或其他备注')} maxLength={500} />
          </Form.Item>
          <Form.Item name="enabled" label={t('启用状态')} valuePropName="checked">
            <Switch checkedChildren={t('已启用')} unCheckedChildren={t('已停用')} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={t('查看密码 - {p0}', { p0: revealTarget?.name ?? '' })}
        open={Boolean(revealTarget)}
        onCancel={closeReveal}
        footer={[
          <Button key="close" onClick={closeReveal}>
            {t('关闭')}
          </Button>,
        ]}
        destroyOnHidden
      >
        <Alert
          type="warning"
          showIcon
          title={t('安全性提示')}
          description={t('密码将在 {p0} 秒后隐藏。窗口失焦或隐藏时立即清空显示。', {
            p0: revealSeconds,
          })}
          style={{ marginBottom: 16 }}
        />
        <div style={{ textAlign: 'center', padding: '12px 0' }}>
          {revealedSecret !== null ? (
            <Paragraph style={{ fontSize: 18, fontFamily: 'monospace', margin: 0 }}>
              <Text code strong style={{ fontSize: 16 }}>
                {revealedSecret}
              </Text>
            </Paragraph>
          ) : revealError ? (
            <Text type="danger">{revealError}</Text>
          ) : (
            <Text type="secondary">{t('正在读取安全密码...')}</Text>
          )}
        </div>
      </Modal>

      <Modal
        title={t('批量导入密码')}
        open={isImportOpen}
        width={720}
        destroyOnHidden
        onCancel={clearImport}
        footer={[
          <Button key="cancel" onClick={clearImport}>
            {t('取消')}
          </Button>,
          <Button
            key="confirm"
            type="primary"
            disabled={!connected || !preview?.token || preview.errorCount > 0 || saving}
            loading={saving}
            onClick={() => void handleConfirmImport()}
          >
            {preview ? t('确认导入 ({count} 新增)', { count: preview.newCount }) : t('确认导入')}
          </Button>,
        ]}
      >
        <Space orientation="vertical" style={{ width: '100%' }} size="middle">
          <Space wrap>
            <span>{t('格式：')}</span>
            <Select
              aria-label={t('导入文本格式')}
              value={importFormat}
              onChange={onImportFormatChange}
              options={[
                { label: t('TXT：每行一个密码'), value: 'txt' },
                { label: t('TSV：名称、密码、分组、备注'), value: 'tsv' },
                { label: t('CSV：逗号分隔 / 自定义列映射'), value: 'csv' },
              ]}
              style={{ width: 240 }}
            />
            <span>{t('默认分组：')}</span>
            <Input
              aria-label={t('导入默认分组')}
              value={importGroup}
              placeholder={t('默认分组（可选）')}
              onChange={(e) => onImportGroupChange(e.target.value)}
              style={{ width: 140 }}
            />
            <span>{t('读取前选择编码：')}</span>
            <Select
              aria-label={t('导入文件编码')}
              value={importEncoding}
              onChange={onImportEncodingChange}
              options={[
                { label: 'UTF-8', value: 'utf-8' },
                { label: 'GB18030', value: 'gb18030' },
                { label: 'UTF-16LE', value: 'utf-16le' },
              ]}
              style={{ width: 110 }}
            />
            <Button
              icon={<FolderOpenOutlined />}
              disabled={!nativeEngine}
              onClick={() => void handlePickFile()}
            >
              {t('选择文件')}
            </Button>
          </Space>

          <Input.TextArea
            aria-label={t('待导入密码文本')}
            rows={5}
            placeholder={t('粘贴待导入密码内容...')}
            value={importText}
            onChange={(e) => onImportTextChange(e.target.value)}
          />
          <Alert
            type="info"
            showIcon
            title={t('密码按原样保存')}
            description={
              importFormat === 'txt'
                ? t('TXT 格式每行一个密码，空行自动忽略；首尾空格与大小写完整保留。')
                : importFormat === 'tsv'
                  ? t(
                      'TSV 格式每行四列，使用 Tab 分隔：名称、密码、分组、备注；分组可用逗号分隔。无需包含表头。',
                    )
                  : t(
                      'CSV 格式支持逗号分隔、带双引号转义；支持首行表头自动识别密码列，或按 [名称, 密码, 分组, 备注] 顺序排列。',
                    )
            }
          />

          <Button
            type="dashed"
            block
            onClick={handlePreviewImport}
            disabled={!connected || !importText.length}
          >
            {t('生成导入预览')}
          </Button>

          {preview && (
            <Card size="small" title={t('预览结果')}>
              <Space style={{ marginBottom: 8 }}>
                <Tag color="green">{t('新增: {count}', { count: preview.newCount })}</Tag>
                <Tag color="orange">{t('重复: {count}', { count: preview.duplicateCount })}</Tag>
                <Tag color={preview.errorCount > 0 ? 'red' : 'default'}>
                  {t('错误: {count}', { count: preview.errorCount })}
                </Tag>
              </Space>
              {preview.errorCount > 0 && (
                <Alert
                  type="error"
                  title={t('存在解析错误条目，必须先纠正或清理错误行方可执行导入。')}
                  style={{ marginBottom: 8 }}
                />
              )}
              <Table
                rowKey={(r) => `${r.line}-${r.name}`}
                dataSource={preview.rows}
                size="small"
                pagination={{ pageSize: 5 }}
                columns={[
                  { title: t('行号'), dataIndex: 'line', key: 'line', width: 60 },
                  { title: t('标识/名称'), dataIndex: 'name', key: 'name' },
                  { title: t('分组'), dataIndex: 'group', key: 'group' },
                  { title: t('状态'), dataIndex: 'status', key: 'status', width: 80 },
                  { title: t('说明'), dataIndex: 'message', key: 'message' },
                ]}
              />
            </Card>
          )}
        </Space>
      </Modal>
    </>
  );
}
