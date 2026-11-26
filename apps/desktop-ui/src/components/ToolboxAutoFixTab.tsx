import { Alert, Button, Card, Space, Table, Tag, Typography } from 'antd';
import {
  CheckCircleOutlined,
  DeleteOutlined,
  FileAddOutlined,
  FolderAddOutlined,
  InboxOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons';
import type { ColumnsType } from 'antd/es/table';
import { t } from '../i18n';
import { isDesktop } from '../api';
import type { ArchiveExtensionFixItem } from '../types';
import { displayPath } from './shared';

const { Text } = Typography;

const getBaseName = (p: string) => {
  const norm = p.replace(/\\/g, '/');
  const idx = norm.lastIndexOf('/');
  return idx >= 0 ? norm.slice(idx + 1) : norm;
};

export interface ToolboxAutoFixTabProps {
  nativeEngine: boolean;
  fixing: boolean;
  fixPaths: string[];
  fixResults: ArchiveExtensionFixItem[] | null;
  fixResultColumns: ColumnsType<ArchiveExtensionFixItem>;
  onPickFixFiles: (folder: boolean) => Promise<void>;
  onClearFixPaths: () => void;
  onRemoveFixPath: (path: string) => void;
  onExecuteDirectFix: () => Promise<void>;
}

export default function ToolboxAutoFixTab({
  nativeEngine,
  fixing,
  fixPaths,
  fixResults,
  fixResultColumns,
  onPickFixFiles: handlePickFixFiles,
  onClearFixPaths: handleClearFixPaths,
  onRemoveFixPath: handleRemoveFixPath,
  onExecuteDirectFix: handleExecuteDirectFix,
}: ToolboxAutoFixTabProps) {
  return (
    <div>
      <Alert
        type="info"
        showIcon
        title={t('自动识别真实格式并直接就地修改文件后缀')}
        description={t(
          '深度嗅探文件头部二进制魔数（Magic Bytes），精准识别 ZIP、7z、RAR、Tar、Gzip、Bzip2、XZ、Zstd、LZ4、WIM、CAB 等常见归档格式。直接物理重命名修正被伪装为 .mp4、.dat、.bin、.bak、.tar.gz.tar 或丢失后缀的文件；智能避让同名冲突，保障数据安全。',
        )}
        style={{ marginBottom: 16 }}
      />

      <Space wrap size={12} style={{ marginBottom: 16 }}>
        <Button
          type="primary"
          icon={<FileAddOutlined />}
          onClick={() => void handlePickFixFiles(false)}
          disabled={!nativeEngine || fixing}
        >
          {t('选择文件 (支持多选)')}
        </Button>
        <Button
          icon={<FolderAddOutlined />}
          onClick={() => void handlePickFixFiles(true)}
          disabled={!nativeEngine || fixing}
        >
          {t('选择文件夹 (递归检测)')}
        </Button>
        {fixPaths.length > 0 && (
          <Button danger icon={<DeleteOutlined />} onClick={handleClearFixPaths} disabled={fixing}>
            {t('清空待处理列表 ({p0})', { p0: fixPaths.length })}
          </Button>
        )}
      </Space>

      {fixPaths.length === 0 && !fixResults && (
        <div
          className="dropzone-container"
          role="region"
          aria-label={isDesktop ? t('添加文件，或将伪装可疑压缩包拖拽到此处') : t('自动修复区域')}
          tabIndex={nativeEngine ? 0 : undefined}
          onClick={() => {
            if (nativeEngine && !fixing) void handlePickFixFiles(false);
          }}
          onKeyDown={(e) => {
            if (nativeEngine && !fixing && (e.key === 'Enter' || e.key === ' ')) {
              e.preventDefault();
              void handlePickFixFiles(false);
            }
          }}
          style={{
            margin: '16px 0',
            padding: '36px 20px',
            border: '1.5px dashed var(--ou-border-color)',
            borderRadius: 'var(--ou-radius)',
            textAlign: 'center',
            cursor: nativeEngine ? 'pointer' : 'default',
            background: 'var(--ou-bg-card)',
          }}
        >
          <InboxOutlined style={{ fontSize: 40, color: 'var(--ou-primary)', marginBottom: 8 }} />
          <div style={{ fontSize: 15, fontWeight: 500, color: 'var(--ou-text-color)' }}>
            {isDesktop
              ? t('点击添加文件，或将伪装/可疑压缩包直接拖拽到此处')
              : t('请在桌面客户端中使用自动修复功能')}
          </div>
          <div style={{ fontSize: 13, color: 'var(--ou-text-secondary)', marginTop: 4 }}>
            {t(
              '支持 .mp4、.dat、.bin、.bak、.tar.gz.tar 或无扩展名等任意伪装归档，直接物理重命名修改后缀',
            )}
          </div>
        </div>
      )}

      {fixPaths.length > 0 && (
        <Card
          type="inner"
          title={
            <Space>
              <span>{t('待处理文件清单')}</span>
              <Tag color="blue">{t('{p0} 项', { p0: fixPaths.length })}</Tag>
            </Space>
          }
          extra={
            <Button
              type="primary"
              size="middle"
              icon={<ThunderboltOutlined />}
              loading={fixing}
              onClick={() => void handleExecuteDirectFix()}
              style={{ fontWeight: 600 }}
            >
              {t('开始自动修复后缀 (物理重命名)')}
            </Button>
          }
          style={{ marginBottom: 16 }}
        >
          <div style={{ maxHeight: 240, overflowY: 'auto' }}>
            <Space orientation="vertical" style={{ width: '100%' }} size={6}>
              {fixPaths.slice(0, 100).map((p) => (
                <div
                  key={p}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    padding: '4px 8px',
                    background: 'var(--ou-bg-input)',
                    borderRadius: 4,
                  }}
                >
                  <Space style={{ minWidth: 0, overflow: 'hidden' }}>
                    <Text strong ellipsis style={{ maxWidth: 280 }}>
                      {getBaseName(p)}
                    </Text>
                    <Text code ellipsis style={{ maxWidth: 400, fontSize: 12 }}>
                      {displayPath(p)}
                    </Text>
                  </Space>
                  <Button
                    type="text"
                    danger
                    size="small"
                    icon={<DeleteOutlined />}
                    disabled={fixing}
                    onClick={() => handleRemoveFixPath(p)}
                  />
                </div>
              ))}
              {fixPaths.length > 100 && (
                <Text
                  type="secondary"
                  style={{ textAlign: 'center', display: 'block', marginTop: 4 }}
                >
                  {t('... 等共 {p0} 个待处理项目', { p0: fixPaths.length })}
                </Text>
              )}
            </Space>
          </div>
        </Card>
      )}

      {fixResults && (
        <Card
          title={
            <Space wrap>
              <CheckCircleOutlined style={{ color: 'var(--ou-success)' }} />
              <span>{t('后缀修复结果核对')}</span>
              <Tag color="blue">{t('共处理 {p0} 项', { p0: fixResults.length })}</Tag>
              <Tag color="success">
                {t('成功修改: {p0}', {
                  p0: fixResults.filter((r) => r.status === 'renamed').length,
                })}
              </Tag>
              <Tag color="processing">
                {t('无需修改: {p0}', {
                  p0: fixResults.filter((r) => r.status === 'already_correct').length,
                })}
              </Tag>
              {fixResults.some((r) => r.status === 'unrecognized') && (
                <Tag color="default">
                  {t('非压缩包: {p0}', {
                    p0: fixResults.filter((r) => r.status === 'unrecognized').length,
                  })}
                </Tag>
              )}
              {fixResults.some((r) => r.status === 'error') && (
                <Tag color="error">
                  {t('失败: {p0}', {
                    p0: fixResults.filter((r) => r.status === 'error').length,
                  })}
                </Tag>
              )}
            </Space>
          }
          extra={<Button onClick={handleClearFixPaths}>{t('完成并处理下一批')}</Button>}
        >
          <Table
            dataSource={fixResults}
            columns={fixResultColumns}
            rowKey="originalPath"
            pagination={{ pageSize: 10, showSizeChanger: true }}
          />
        </Card>
      )}
    </div>
  );
}
