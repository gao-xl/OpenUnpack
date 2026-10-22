import { t } from '../i18n';
import { useState, useMemo, useEffect, useRef } from 'react';
import {
  App as AntApp,
  Card,
  Tabs,
  Form,
  Input,
  InputNumber,
  Button,
  Switch,
  Radio,
  Modal,
  Alert,
  Space,
  Typography,
  Table,
  Tag,
  Divider,
} from 'antd';
import {
  FolderOpenOutlined,
  EyeOutlined,
  CheckCircleOutlined,
  ExclamationCircleOutlined,
  SwapOutlined,
  ClearOutlined,
  FileDoneOutlined,
} from '@ant-design/icons';
import type {
  AppSnapshot,
  ConflictStrategy,
  ToolboxKind,
  ToolboxOptions,
  ToolboxItemPlan,
  ToolboxPreviewResult,
  RenamePatternRule,
} from '../types';
import { api, isDesktop } from '../api';
import { formatBytes, displayPath } from './shared';

const { Text } = Typography;

interface Props {
  snapshot: AppSnapshot;
  refresh: () => Promise<void>;
  run: (work: () => Promise<unknown>) => Promise<void>;
}

export default function ToolboxPage({ snapshot: _snapshot, run: _run }: Props) {
  const { message } = AntApp.useApp();
  const daemon = api.isDaemonMode?.() === true;
  const connected = isDesktop || daemon;
  const nativeEngine = isDesktop && !daemon;

  const [kind, setKind] = useState<ToolboxKind>('normalizeExtensions');
  const [sourceDirectory, setSourceDirectory] = useState('');
  const [targetDirectory, setTargetDirectory] = useState('');
  const [recursive, setRecursive] = useState(true);
  const [removeEmptyFolders, setRemoveEmptyFolders] = useState(true);
  const [conflictStrategy, setConflictStrategy] = useState<ConflictStrategy>('rename');

  // Batch rename rule state
  const [searchPattern, setSearchPattern] = useState('');
  const [replacePattern, setReplacePattern] = useState('');
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [useRegex, setUseRegex] = useState(false);
  const [prefix, setPrefix] = useState('');
  const [suffix, setSuffix] = useState('');
  const [enableNumbering, setEnableNumbering] = useState(false);
  const [numberingStart, setNumberingStart] = useState<number>(1);
  const [numberingDigits, setNumberingDigits] = useState<number>(3);

  // Preview & execution state
  const [analysisOwner, setAnalysisOwner] = useState<ToolboxOptions | null>(null);
  const [previewState, setPreviewState] = useState<{ owner: ToolboxOptions; result: ToolboxPreviewResult } | null>(null);
  const [selection, setSelection] = useState<{ owner: ToolboxOptions; paths: string[] } | null>(null);
  const [confirmationOwner, setConfirmationOwner] = useState<ToolboxOptions | null>(null);
  const [executing, setExecuting] = useState(false);
  const previewEpochRef = useRef(0);

  const handlePickSource = async () => {
    try {
      const picked = await api.pickDirectory();
      if (picked) {
        setSourceDirectory(picked);
        setPreviewResult(null);
      }
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e));
    }
  };

  const handlePickTarget = async () => {
    try {
      const picked = await api.pickDirectory();
      if (picked) {
        setTargetDirectory(picked);
        setPreviewResult(null);
      }
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e));
    }
  };

  const currentOptions = useMemo<ToolboxOptions>(() => {
    const renameRule: RenamePatternRule | null =
      kind === 'batchRename'
        ? {
            search: searchPattern,
            replace: replacePattern,
            caseSensitive,
            useRegex,
            prefix: prefix ? prefix : null,
            suffix: suffix ? suffix : null,
            numberingStart: enableNumbering ? numberingStart : null,
            numberingDigits: enableNumbering ? numberingDigits : null,
          }
        : null;

    return {
      kind,
      sourceDirectory,
      targetDirectory: kind === 'flattenDirectory' && targetDirectory.trim() ? targetDirectory.trim() : null,
      recursive: kind === 'flattenDirectory' ? true : recursive,
      removeEmptyFolders: kind === 'flattenDirectory' ? removeEmptyFolders : false,
      conflictStrategy,
      renameRule,
    };
  }, [
    kind,
    sourceDirectory,
    targetDirectory,
    recursive,
    removeEmptyFolders,
    conflictStrategy,
    searchPattern,
    replacePattern,
    caseSensitive,
    useRegex,
    prefix,
    suffix,
    enableNumbering,
    numberingStart,
    numberingDigits,
  ]);

  // Preview results and loading belong to the options that produced them.
  const analyzing = analysisOwner === currentOptions;
  const previewResult = previewState?.owner === currentOptions ? previewState.result : null;
  const selectedPaths = selection?.owner === currentOptions ? selection.paths : [];
  const confirmModalVisible = confirmationOwner === currentOptions;
  const setConfirmModalVisible = (visible: boolean) => setConfirmationOwner(visible ? currentOptions : null);
  const setPreviewResult = (result: ToolboxPreviewResult | null) => setPreviewState(result ? { owner: currentOptions, result } : null);
  const setSelectedPaths = (paths: string[]) => setSelection({ owner: currentOptions, paths });
  useEffect(() => {
    previewEpochRef.current += 1;
  }, [currentOptions]);
  useEffect(() => () => { previewEpochRef.current += 1; }, []);

  const handlePreview = async () => {
    if (!sourceDirectory.trim()) {
      message.warning(t("请先选择要整理的目标文件夹"));
      return;
    }
    const epoch = ++previewEpochRef.current;
    setAnalysisOwner(currentOptions);
    try {
      const res = await api.previewToolbox(currentOptions);
      if (epoch === previewEpochRef.current) {
        setPreviewResult(res);
        setSelectedPaths(res.plans.filter((p) => p.action !== 'skip').map((p) => p.sourcePath));
        if (res.totalPlanned === 0) {
          message.info(t("扫描完成，共扫描 {p0} 个文件，未发现需要调整的项目。", { p0: res.totalScanned }));
        } else {
          message.success(t("分析完成：发现 {p0} 项可调整文件，其中 {p1} 项检测到同名冲突。", { p0: res.totalPlanned, p1: res.conflictsCount }));
        }
      }
    } catch (e) {
      if (epoch === previewEpochRef.current) {
        message.error(e instanceof Error ? e.message : String(e));
      }
    } finally {
      if (epoch === previewEpochRef.current) {
        setAnalysisOwner(null);
      }
    }
  };

  const handleExecute = async () => {
    if (!previewResult || selectedPaths.length === 0) {
      message.warning(t("没有可执行的变更项"));
      return;
    }
    setExecuting(true);
    try {
      const result = await api.executeToolbox({
        options: currentOptions,
        approvedPaths: selectedPaths,
        token: previewResult.token,
      });
      setConfirmModalVisible(false);
      setPreviewResult(null);
      setSelectedPaths([]);

      const details = [
        t("成功调整: {p0} 个文件", { p0: result.successCount }),
        result.skippedCount > 0 ? t("跳过: {p0}", { p0: result.skippedCount }) : '',
        result.cleanedFoldersCount > 0 ? t("清理空目录: {p0} 个", { p0: result.cleanedFoldersCount }) : '',
        result.failedCount > 0 ? t("失败: {p0} 项", { p0: result.failedCount }) : '',
      ]
        .filter(Boolean)
        .join('，');

      if (result.failedCount === 0) {
        message.success(t("操作全部完成！{p0}", { p0: details }));
      } else {
        message.warning(t("操作部分完成。{p0}。错误: {p1}", { p0: details, p1: result.errors.join('; ') }));
      }
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setExecuting(false);
    }
  };

  const tableColumns = [
    {
      title: t("原文件 / 相对路径"),
      dataIndex: 'relativePath',
      key: 'relativePath',
      render: (rel: string) => (
        <Space direction="vertical" size={2}>
          <Text code ellipsis style={{ maxWidth: 300 }}>
            {displayPath(rel)}
          </Text>
        </Space>
      ),
    },
    {
      title: t("拟议新名称 / 操作"),
      dataIndex: 'proposedName',
      key: 'proposedName',
      render: (name: string, record: ToolboxItemPlan) => (
        <Space direction="vertical" size={2}>
          <Space>
            <Text strong style={{ color: record.hasConflict ? 'var(--ou-warning)' : undefined }}>
              {name}
            </Text>
            {record.action === 'rename' && <Tag color="blue">{t("重命名")}</Tag>}
            {record.action === 'move' && <Tag color="cyan">{t("平铺移动")}</Tag>}
            {record.action === 'skip' && <Tag color="default">{t("跳过")}</Tag>}
          </Space>
          {record.conflictReason && (
            <Text type="warning" style={{ fontSize: 12 }}>
              <ExclamationCircleOutlined /> {record.conflictReason}
            </Text>
          )}
        </Space>
      ),
    },
    {
      title: t("大小"),
      dataIndex: 'size',
      key: 'size',
      width: 100,
      render: (size: number) => formatBytes(size),
    },
    {
      title: t("状态"),
      key: 'status',
      width: 120,
      render: (_: unknown, record: ToolboxItemPlan) => {
        if (record.action === 'skip') {
          return <Tag color="default">{t("跳过")}</Tag>;
        }
        if (record.hasConflict) {
          return <Tag color="warning">{t("冲突已处理")}</Tag>;
        }
        return <Tag color="success">{t("安全")}</Tag>;
      },
    },
  ];

  return (
    <div style={{ maxWidth: 1100, margin: '0 auto', paddingBottom: 40, padding: '16px 20px' }}>
      <div className="page-header" style={{ marginBottom: 14 }}>
        <div>
          <h2 className="page-title">{t("工具箱")}</h2>
          <p className="page-description">
            {t("本地目录规范化整理、批量更名与归档平铺提取")}
          </p>
        </div>
      </div>
      <Card style={{ marginBottom: 16, borderRadius: 'var(--ou-radius)', border: '1px solid var(--ou-border-color)' }}>
        <Tabs
          activeKey={kind}
          onChange={(k) => {
            setKind(k as ToolboxKind);
            setPreviewResult(null);
          }}
          items={[
            {
              key: 'normalizeExtensions',
              label: (
                <span>
                  <FileDoneOutlined />{t("扩展名规范化")}</span>
              ),
            },
            {
              key: 'batchRename',
              label: (
                <span>
                  <SwapOutlined />{t("智能批量更名")}</span>
              ),
            },
            {
              key: 'flattenDirectory',
              label: (
                <span>
                  <ClearOutlined />{t("平铺提取与整理")}</span>
              ),
            },
          ]}
        />

        <Form layout="vertical">
          <Form.Item label={t("源目录路径 (严格 NativeAccess 授权)")}>
            <Space.Compact style={{ width: '100%' }}>
              <Input
                value={sourceDirectory}
                aria-label={t("工具箱源目录")}
                onChange={event => setSourceDirectory(event.target.value)}
                placeholder={t("请选择本地目录，系统将通过权限守卫隔离执行...")}
                readOnly={!daemon}
              />
              <Button icon={<FolderOpenOutlined />} disabled={!nativeEngine} onClick={() => void handlePickSource()}>{t("选择文件夹")}</Button>
            </Space.Compact>
          </Form.Item>

          {kind === 'normalizeExtensions' && (
            <Alert
              type="info"
              showIcon
              title={t("自动规范化归档扩展名")}
              description={t("智能识别修复被错误切割或双重后缀的复合归档（如 .tar.gz.tar 修正为 .tar.gz、.tgz 规范为 .tar.gz），并通过头部魔数嗅探校验真实归档格式（7z/ZIP/RAR/TAR/ZSTD/LZ4），规范缺少后缀或命名错乱的文件。")}
              style={{ marginBottom: 16 }}
            />
          )}

          {kind === 'batchRename' && (
            <Card type="inner" title={t("更名规则配置")} style={{ marginBottom: 16 }}>
              <Space direction="vertical" style={{ width: '100%' }} size={12}>
                <Space wrap style={{ width: '100%' }}>
                  <Form.Item label={t("查找文本 / 正则")} style={{ marginBottom: 0, minWidth: 240 }}>
                    <Input
                      value={searchPattern}
                      onChange={(e) => setSearchPattern(e.target.value)}
                      placeholder={t("留空则不执行替换")}
                    />
                  </Form.Item>
                  <Form.Item label={t("替换为")} style={{ marginBottom: 0, minWidth: 240 }}>
                    <Input
                      value={replacePattern}
                      onChange={(e) => setReplacePattern(e.target.value)}
                      placeholder={t("替换目标文本")}
                    />
                  </Form.Item>
                  <Form.Item label={t("匹配设置")} style={{ marginBottom: 0 }}>
                    <Space style={{ paddingTop: 4 }}>
                      <Switch
                        checked={caseSensitive}
                        onChange={setCaseSensitive}
                        checkedChildren={t("区分大小写")}
                        unCheckedChildren={t("忽略大小写")}
                      />
                      <Switch
                        checked={useRegex}
                        onChange={setUseRegex}
                        checkedChildren={t("正则匹配")}
                        unCheckedChildren={t("普通文本")}
                      />
                    </Space>
                  </Form.Item>
                </Space>

                <Space wrap style={{ width: '100%' }}>
                  <Form.Item label={t("前缀")} style={{ marginBottom: 0, minWidth: 200 }}>
                    <Input
                      value={prefix}
                      onChange={(e) => setPrefix(e.target.value)}
                      placeholder={t("在文件名开头添加")}
                    />
                  </Form.Item>
                  <Form.Item label={t("后缀")} style={{ marginBottom: 0, minWidth: 200 }}>
                    <Input
                      value={suffix}
                      onChange={(e) => setSuffix(e.target.value)}
                      placeholder={t("在扩展名前添加")}
                    />
                  </Form.Item>
                </Space>

                <Divider style={{ margin: '8px 0' }} />

                <Space wrap align="center">
                  <Switch
                    checked={enableNumbering}
                    onChange={setEnableNumbering}
                    checkedChildren={t("启用自动编号")}
                    unCheckedChildren={t("禁用自动编号")}
                  />
                  {enableNumbering && (
                    <Space>
                      <span>{t("起始序号:")}</span>
                      <InputNumber
                        min={1}
                        max={999999}
                        value={numberingStart}
                        onChange={(v) => setNumberingStart(v ?? 1)}
                      />
                      <span>{t("位数 (补零):")}</span>
                      <InputNumber
                        min={1}
                        max={6}
                        value={numberingDigits}
                        onChange={(v) => setNumberingDigits(v ?? 3)}
                      />
                    </Space>
                  )}
                </Space>
              </Space>
            </Card>
          )}

          {kind === 'flattenDirectory' && (
            <Card type="inner" title={t("平铺提取选项")} style={{ marginBottom: 16 }}>
              <Form.Item label={t("平铺目标目录 (留空则平铺至源目录根路径)")}>
                <Space.Compact style={{ width: '100%' }}>
                  <Input
                    value={targetDirectory}
                    aria-label={t("工具箱目标目录")}
                    onChange={event => setTargetDirectory(event.target.value)}
                    placeholder={t("默认提取至源目录根路径...")}
                    readOnly={!daemon}
                  />
                  <Button icon={<FolderOpenOutlined />} disabled={!nativeEngine} onClick={() => void handlePickTarget()}>{t("自定义目标")}</Button>
                  {targetDirectory && (
                    <Button onClick={() => setTargetDirectory('')}>{t("清除")}</Button>
                  )}
                </Space.Compact>
              </Form.Item>

              <Space size={24}>
                <Form.Item label={t("提取后自动清理空目录")} style={{ marginBottom: 0 }}>
                  <Switch
                    checked={removeEmptyFolders}
                    onChange={setRemoveEmptyFolders}
                    checkedChildren={t("自动清理")}
                    unCheckedChildren={t("保留空目录")}
                  />
                </Form.Item>
              </Space>
            </Card>
          )}

          <Space wrap size={24} style={{ marginBottom: 16 }}>
            {kind !== 'flattenDirectory' && (
              <Form.Item label={t("包含子文件夹 (递归扫描)")} style={{ marginBottom: 0 }}>
                <Switch
                  checked={recursive}
                  onChange={setRecursive}
                  checkedChildren={t("包含")}
                  unCheckedChildren={t("仅根目录")}
                />
              </Form.Item>
            )}

            <Form.Item label={t("同名冲突策略")} style={{ marginBottom: 0 }}>
              <Radio.Group
                value={conflictStrategy}
                onChange={(e) => setConflictStrategy(e.target.value as ConflictStrategy)}
              >
                <Radio.Button value="rename">{t("自动追加序号 (_1, _2)")}</Radio.Button>
                <Radio.Button value="skip">{t("跳过冲突文件")}</Radio.Button>
                <Radio.Button value="error">{t("报错并中止")}</Radio.Button>
              </Radio.Group>
            </Form.Item>
          </Space>

          <div>
            <Button
              type="primary"
              icon={<EyeOutlined />}
              onClick={() => void handlePreview()}
              loading={analyzing}
              disabled={!connected || !sourceDirectory}
            >{t("分析并预览改动 (Dry-Run)")}</Button>
          </div>
        </Form>
      </Card>

      {previewResult && (
        <Card
          title={
            <Space>
              <span>{t("改动预览与核对")}</span>
              <Tag color="blue">{t("共发现 {count} 项需要变更", { count: previewResult.totalPlanned })}</Tag>
              {previewResult.conflictsCount > 0 && (
                <Tag color="warning">{t("包含 {count} 项命名冲突", { count: previewResult.conflictsCount })}</Tag>
              )}
            </Space>
          }
          extra={
            <Button
              type="primary"
              danger
              icon={<CheckCircleOutlined />}
              disabled={!connected || selectedPaths.length === 0}
              onClick={() => setConfirmModalVisible(true)}
            >{t("确认并执行改动 ({count})", { count: selectedPaths.length })}
            </Button>
          }
        >
          <Table
            dataSource={previewResult.plans}
            columns={tableColumns}
            rowKey="sourcePath"
            pagination={{ pageSize: 10, showSizeChanger: true }}
            rowSelection={{
              selectedRowKeys: selectedPaths,
              onChange: (keys) => setSelectedPaths(keys as string[]),
              getCheckboxProps: (record) => ({
                disabled: record.action === 'skip',
              }),
            }}
          />
        </Card>
      )}

      <Modal
        title={
          <Space>
            <ExclamationCircleOutlined style={{ color: 'var(--ou-danger)' }} />
            <span>{t("二次确认：执行本地目录整理")}</span>
          </Space>
        }
        open={confirmModalVisible && !!previewResult}
        onOk={() => void handleExecute()}
        confirmLoading={executing}
        onCancel={() => setConfirmModalVisible(false)}
        okText={t("确认立即执行")}
        okButtonProps={{ danger: true }}
        cancelText={t("取消")}
      >
        <Space direction="vertical" size={12} style={{ width: '100%', marginTop: 8 }}>
          <Alert
            type="warning"
            showIcon
            title={t("此操作将直接重命名或移动本地磁盘上的文件")}
            description={t("改动执行后无法自动撤销。已自动通过 NativeAccess 权限模型校验目录边界，禁止跨越软链接及系统目录。")}
          />
          <div>
            <Text strong>{t("操作类型：")}</Text>{' '}
            {kind === 'normalizeExtensions' && t("扩展名规范化")}
            {kind === 'batchRename' && t("智能批量更名")}
            {kind === 'flattenDirectory' && t("平铺提取与整理")}
          </div>
          <div>
            <Text strong>{t("待处理文件数：")}</Text> {t("{count} 项", { count: selectedPaths.length })}</div>
          <div>
            <Text strong>{t("冲突处理机制：")}</Text>{' '}
            {conflictStrategy === 'rename'
              ? t("自动追加数字后缀 (_1, _2) 规避冲突")
              : conflictStrategy === 'skip'
              ? t("跳过目标已存在的冲突文件")
              : t("发生冲突时报错中止")}
          </div>
          {kind === 'flattenDirectory' && removeEmptyFolders && (
            <div>
              <Text strong>{t("后续动作：")}</Text>{t("将自动清理平铺提取后留下的空子目录")}</div>
          )}
        </Space>
      </Modal>
    </div>
  );
}
