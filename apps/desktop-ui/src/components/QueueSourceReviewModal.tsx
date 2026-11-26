import {
  Alert,
  Button,
  Input,
  InputNumber,
  Modal,
  Select,
  Space,
  Switch,
  Table,
  Typography,
} from 'antd';
import { SettingOutlined } from '@ant-design/icons';
import type { ColumnsType } from 'antd/es/table';
import { t } from '../i18n';
import type { ScanResult, Settings, SourceGroup } from '../types';
import { displayPath } from './shared';
import SourceCleanupSelect, { SourceCleanupScope } from './SourceCleanupSelect';

export interface QueueSourceReviewModalProps {
  scan?: ScanResult;
  sources: string[];
  onSourcesChange: (keys: string[]) => void;
  options: Settings;
  onOptionsChange: React.Dispatch<React.SetStateAction<Settings>>;
  showOptions: boolean;
  onToggleOptions: (show: boolean) => void;
  addingJobs: boolean;
  connected: boolean;
  nativeEngine: boolean;
  daemon: boolean;
  groups: string[];
  totalSelectedVolumes: number;
  sourceColumns: ColumnsType<SourceGroup>;
  onPickOutputDir: () => void;
  onCancel: () => void;
  onConfirmAdd: () => void;
}

export default function QueueSourceReviewModal({
  scan,
  sources,
  onSourcesChange: setSources,
  options,
  onOptionsChange: setOptions,
  showOptions,
  onToggleOptions: setShowOptions,
  addingJobs,
  connected,
  nativeEngine,
  daemon,
  groups,
  totalSelectedVolumes,
  sourceColumns,
  onPickOutputDir,
  onCancel,
  onConfirmAdd,
}: QueueSourceReviewModalProps) {
  return (
    <Modal
      title={t('检查扫描结果')}
      open={!!scan}
      confirmLoading={addingJobs}
      cancelButtonProps={{ disabled: addingJobs }}
      onCancel={onCancel}
      width={840}
      okText={t('添加 {p0} 项', { p0: sources.length })}
      cancelText={t('返回')}
      okButtonProps={{ disabled: !connected || !sources.length || addingJobs }}
      onOk={onConfirmAdd}
    >
      <Alert
        type="info"
        showIcon
        title={t('发现 {p0} 个归档任务', { p0: scan?.sources.length ?? 0 })}
        description={t(
          '已按分卷自动关联。若提示缺少分卷，请补齐对应文件后再行解压；展开条目可查看具体分卷路径。',
        )}
        style={{ marginBottom: 12 }}
      />
      {!!scan?.notices.length && (
        <>
          <Typography.Title level={5}>
            {t('扫描跳过与访问提示（{count} 项）', { count: scan.notices.length })}
          </Typography.Title>
          <Table
            size="small"
            rowKey={(notice, index) => `${index}-${notice.path}`}
            dataSource={scan.notices}
            pagination={{ pageSize: 4 }}
            columns={[
              { title: t('路径'), dataIndex: 'path', ellipsis: true },
              { title: t('提示'), dataIndex: 'message' },
            ]}
          />
        </>
      )}
      <Table<SourceGroup>
        rowKey="path"
        size="small"
        columns={sourceColumns}
        dataSource={scan?.sources}
        expandable={{
          expandedRowRender: (source) => (
            <ul>
              {source.volumes.map((path) => (
                <li key={path} style={{ wordBreak: 'break-all' }}>
                  {displayPath(path)}
                </li>
              ))}
            </ul>
          ),
        }}
        rowSelection={{
          selectedRowKeys: sources,
          onChange: (keys) => setSources(keys.map(String)),
          getCheckboxProps: (source) => ({ disabled: source.missingVolume }),
        }}
        pagination={{ pageSize: 8 }}
      />
      <div className="toolbar" style={{ marginTop: 12 }}>
        <Typography.Text type="secondary">
          {t('输出：')}
          {displayPath(options.outputRoot) || t('源归档所在目录')} ·{' '}
          {options.cacheEnabled ? t('SSD 工作目录') : t('目标卷临时目录')} ·{' '}
          {options.expandNested ? t('共 {p0} 层展开', { p0: options.maxDepth }) : t('保留内包')}
        </Typography.Text>
        <Button
          icon={<SettingOutlined aria-hidden />}
          aria-expanded={showOptions}
          aria-controls="batch-options-panel"
          onClick={() => setShowOptions(!showOptions)}
        >
          {showOptions ? t('收起批次选项') : t('本批次选项')}
        </Button>
      </div>
      {showOptions && (
        <div
          id="batch-options-panel"
          style={{
            marginTop: 12,
            padding: 16,
            background: 'var(--ou-bg-card)',
            border: '1px solid var(--ou-border-color)',
            borderRadius: 8,
          }}
        >
          <Typography.Title level={5} style={{ marginTop: 0 }}>
            {t('本批次处理选项')}
          </Typography.Title>
          <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>
            {t('仅影响本批次；加入队列后设置会冻结。')}
          </Typography.Paragraph>
          <div className="setting-row">
            <span>{t('输出根目录')}</span>
            <Space.Compact style={{ width: '100%' }}>
              <Input
                readOnly={!daemon}
                onChange={(event) =>
                  setOptions((prev) => ({ ...prev, outputRoot: event.target.value }))
                }
                aria-label={t('本批次输出目录')}
                value={displayPath(options.outputRoot)}
                placeholder={t('留空在源归档旁创建结果目录')}
              />
              <Button disabled={!nativeEngine} onClick={onPickOutputDir}>
                {t('选择')}
              </Button>
              <Button onClick={() => setOptions((prev) => ({ ...prev, outputRoot: '' }))}>
                {t('清空')}
              </Button>
            </Space.Compact>
          </div>
          <div className="setting-row">
            <span>{t('同名目标')}</span>
            <Select
              style={{ width: 220 }}
              value={options.conflictPolicy}
              onChange={(conflictPolicy) => setOptions((prev) => ({ ...prev, conflictPolicy }))}
              options={[
                { value: 'rename', label: t('新建名称，保留已有内容') },
                { value: 'skip', label: t('跳过该任务') },
                ...(options.extractionEngine === 'inPlace'
                  ? [{ value: 'overwrite', label: t('覆盖同名文件（先移开，完成后再删除）') }]
                  : []),
              ]}
            />
          </div>
          <div className="setting-row">
            <span>{t('解压引擎')}</span>
            <Select
              style={{ width: 260 }}
              value={options.extractionEngine || 'standard'}
              onChange={(extractionEngine) =>
                setOptions((prev) => ({
                  ...prev,
                  extractionEngine,
                  expandNested: extractionEngine === 'inPlace' ? false : prev.expandNested,
                }))
              }
              options={[
                { value: 'standard', label: t('标准（7-Zip 引擎）') },
                { value: 'inPlace', label: t('原地解压（边解压边释放源包空间）') },
              ]}
            />
          </div>
          {options.extractionEngine === 'inPlace' && (
            <Alert
              type="warning"
              showIcon
              style={{ marginBottom: 8 }}
              title={t('原地解压会消耗源压缩包')}
              description={t(
                '解压前会通读压缩包并校验全部条目；通过后边解压边截断源包以释放空间，完成后源包不再保留。中断后重新开始可从断点继续，期间请勿删除压缩包旁的 .hatchzip 文件。',
              )}
            />
          )}
          <div className="setting-row">
            <span>{t('密码分组')}</span>
            <Select
              value={options.passwordGroup}
              style={{ width: 220 }}
              options={[
                { value: '', label: t('全部启用密码') },
                ...groups.map((group) => ({ value: group, label: group })),
              ]}
              onChange={(passwordGroup) => setOptions((prev) => ({ ...prev, passwordGroup }))}
            />
          </div>
          <div className="setting-row">
            <span>{t('展开内包')}</span>
            <Space>
              <Switch
                checked={options.expandNested}
                onChange={(expandNested) => setOptions((prev) => ({ ...prev, expandNested }))}
              />
              <InputNumber
                aria-label={t('总展开层数（含外包）')}
                min={1}
                max={8}
                disabled={!options.expandNested}
                value={options.maxDepth}
                onChange={(value) => setOptions((prev) => ({ ...prev, maxDepth: value ?? 2 }))}
              />
            </Space>
          </div>
          <div className="setting-row">
            <span>{t('SSD 工作目录')}</span>
            <Switch
              checked={options.cacheEnabled}
              onChange={(cacheEnabled) => setOptions((prev) => ({ ...prev, cacheEnabled }))}
            />
          </div>
          {options.cacheEnabled && (
            <Alert
              type={options.cacheRoot ? 'info' : 'warning'}
              title={displayPath(options.cacheRoot) || t('先在设置中选择 SSD 工作目录')}
              showIcon
              style={{ marginBottom: 8 }}
            />
          )}
          <div className="setting-row">
            <span>{t('智能去套娃')}</span>
            <Space orientation="vertical" align="end">
              <Switch
                checked={options.unwrapSingleRoot}
                onChange={(unwrapSingleRoot) =>
                  setOptions((prev) => ({ ...prev, unwrapSingleRoot }))
                }
              />
            </Space>
          </div>
          <Typography.Text
            type="secondary"
            style={{ fontSize: 12, display: 'block', marginTop: -4, marginBottom: 8 }}
          >
            {t('开启后若压缩包内仅含单个顶层目录，将自动剥离顶层外壳，避免生成同名嵌套文件夹。')}
          </Typography.Text>
          <div className="setting-row">
            <span>{t('验证方式')}</span>
            <Select
              value={options.verification}
              options={[
                { value: 'standard', label: t('标准流程验证') },
                { value: 'strict', label: t('完整目标回读') },
              ]}
              onChange={(verification) => setOptions((prev) => ({ ...prev, verification }))}
              style={{ width: 220 }}
            />
          </div>
          <div className="setting-row">
            <span>{t('完成后源包')}</span>
            <SourceCleanupSelect
              value={options.sourceCleanup}
              onChange={(sourceCleanup) => setOptions((prev) => ({ ...prev, sourceCleanup }))}
              disabled={!connected}
            />
          </div>
          <SourceCleanupScope value={options.sourceCleanup} expandNested={options.expandNested} />
          {options.sourceCleanup !== 'keep' && (
            <Alert
              type="warning"
              showIcon
              style={{ marginTop: 8 }}
              title={t('源文件清理范围提示')}
              description={t(
                '解压完全成功并校验通过后，将清理当前所选 {p0} 个任务的全部源文件（共 {p1} 个分卷文件）。若解压失败则绝不处理源文件。',
                { p0: sources.length, p1: totalSelectedVolumes },
              )}
            />
          )}
          <div className="setting-row" style={{ marginTop: 8 }}>
            <span>{t('添加后自动开始')}</span>
            <Switch
              checked={options.autoStartOnAdd}
              onChange={(autoStartOnAdd) => setOptions((prev) => ({ ...prev, autoStartOnAdd }))}
            />
          </div>
        </div>
      )}
    </Modal>
  );
}
