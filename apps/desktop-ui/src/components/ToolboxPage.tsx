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
  ThunderboltOutlined,
  FileAddOutlined,
  FolderAddOutlined,
  DeleteOutlined,
  InboxOutlined,
  InfoCircleOutlined,
  CloseCircleOutlined,
  ArrowRightOutlined,
} from '@ant-design/icons';
import type {
  AppSnapshot,
  ConflictStrategy,
  ToolboxKind,
  ToolboxOptions,
  ToolboxItemPlan,
  ToolboxPreviewResult,
  RenamePatternRule,
  ArchiveExtensionFixItem,
} from '../types';
import { api, isDesktop } from '../api';
import { formatBytes, displayPath } from './shared';
import ToolboxConfirmModal from './ToolboxConfirmModal';

const { Text } = Typography;

interface Props {
  snapshot: AppSnapshot;
  refresh: () => Promise<void>;
  run: (work: () => Promise<unknown>) => Promise<void>;
  initialTab?: ToolboxTab;
}

type ToolboxTab = 'autoFix' | 'normalizeExtensions' | 'batchRename' | 'flattenDirectory';

const getBaseName = (p: string) => {
  const norm = p.replace(/\\/g, '/');
  const idx = norm.lastIndexOf('/');
  return idx >= 0 ? norm.slice(idx + 1) : norm;
};

const getDirName = (p: string) => {
  const norm = p.replace(/\\/g, '/');
  const idx = norm.lastIndexOf('/');
  return idx >= 0 ? norm.slice(0, idx) : '';
};

export default function ToolboxPage({ snapshot: _snapshot, run: _run, initialTab }: Props) {
  const { message } = AntApp.useApp();
  const daemon = api.isDaemonMode?.() === true;
  const connected = isDesktop || daemon;
  const nativeEngine = isDesktop && !daemon;

  const [activeTab, setActiveTab] = useState<ToolboxTab>(initialTab ?? 'normalizeExtensions');
  const activeTabRef = useRef(activeTab);
  useEffect(() => {
    activeTabRef.current = activeTab;
  }, [activeTab]);

  // Direct auto-fix extension state
  const [fixPaths, setFixPaths] = useState<string[]>([]);
  const [fixResults, setFixResults] = useState<ArchiveExtensionFixItem[] | null>(null);
  const [fixing, setFixing] = useState(false);

  const handlePickFixFiles = async (folder: boolean) => {
    try {
      const picked = await api.pickFiles(folder);
      if (picked && picked.length > 0) {
        setFixPaths((prev) => {
          const next = [...prev];
          for (const p of picked) {
            if (!next.includes(p)) next.push(p);
          }
          return next;
        });
        setFixResults(null);
      }
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e));
    }
  };

  const handleRemoveFixPath = (target: string) => {
    setFixPaths((prev) => prev.filter((p) => p !== target));
  };

  const handleClearFixPaths = () => {
    setFixPaths([]);
    setFixResults(null);
  };

  const handleExecuteDirectFix = async () => {
    if (fixPaths.length === 0) {
      message.warning(t('请先添加需要检测或修改后缀的文件'));
      return;
    }
    setFixing(true);
    try {
      const results = await api.fixArchiveExtensions(fixPaths);
      setFixResults(results);
      const renamed = results.filter((r) => r.status === 'renamed').length;
      const alreadyCorrect = results.filter((r) => r.status === 'already_correct').length;
      const errors = results.filter((r) => r.status === 'error').length;
      if (errors === 0) {
        if (renamed > 0) {
          message.success(
            t('后缀修改完成！共直接修复 {p0} 个文件的扩展名，{p1} 个已为正确格式。', {
              p0: renamed,
              p1: alreadyCorrect,
            }),
          );
        } else {
          message.info(t('扫描完成，所有检测到的压缩包后缀均已正确，无需修改。'));
        }
      } else {
        message.warning(
          t('处理完成：{p0} 项成功修改，{p1} 项遇到错误。', { p0: renamed, p1: errors }),
        );
      }
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setFixing(false);
    }
  };

  const handleOpenDirectory = async (filePath: string) => {
    try {
      await api.openPath(filePath);
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e));
    }
  };

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
  const [previewState, setPreviewState] = useState<{
    owner: ToolboxOptions;
    result: ToolboxPreviewResult;
  } | null>(null);
  const [selection, setSelection] = useState<{ owner: ToolboxOptions; paths: string[] } | null>(
    null,
  );
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
      targetDirectory:
        kind === 'flattenDirectory' && targetDirectory.trim() ? targetDirectory.trim() : null,
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
  const setConfirmModalVisible = (visible: boolean) =>
    setConfirmationOwner(visible ? currentOptions : null);
  const setPreviewResult = (result: ToolboxPreviewResult | null) =>
    setPreviewState(result ? { owner: currentOptions, result } : null);
  const setSelectedPaths = (paths: string[]) => setSelection({ owner: currentOptions, paths });
  useEffect(() => {
    previewEpochRef.current += 1;
  }, [currentOptions]);
  useEffect(
    () => () => {
      previewEpochRef.current += 1;
    },
    [],
  );

  // Drag-and-drop listener for ToolboxPage: unified registration, leak-safe unmount
  useEffect(() => {
    if (!isDesktop || typeof api.onDrop !== 'function') return;
    let disposed = false;
    let stop: (() => void) | undefined;
    void api
      .onDrop((droppedPaths) => {
        if (disposed || !droppedPaths.length) return;
        if (activeTabRef.current === 'autoFix') {
          setFixPaths((prev) => {
            const next = [...prev];
            for (const p of droppedPaths) {
              if (!next.includes(p)) next.push(p);
            }
            return next;
          });
          setFixResults(null);
        } else {
          setSourceDirectory(droppedPaths[0]);
          setPreviewState(null);
        }
      })
      .then((cleanup) => {
        // If the component already unmounted before registration completed,
        // invoke cleanup immediately to prevent Tauri listener leak.
        if (disposed) {
          if (typeof cleanup === 'function') {
            try {
              cleanup();
            } catch {
              // Ignore teardown errors
            }
          }
        } else {
          stop = cleanup;
        }
      })
      .catch(() => {});
    return () => {
      disposed = true;
      if (typeof stop === 'function') {
        try {
          stop();
        } catch {
          // Ignore teardown errors
        }
      }
    };
  }, []);

  const handlePreview = async () => {
    if (!sourceDirectory.trim()) {
      message.warning(t('请先选择要整理的目标文件夹'));
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
          message.info(
            t('扫描完成，共扫描 {p0} 个文件，未发现需要调整的项目。', { p0: res.totalScanned }),
          );
        } else {
          message.success(
            t('分析完成：发现 {p0} 项可调整文件，其中 {p1} 项检测到同名冲突。', {
              p0: res.totalPlanned,
              p1: res.conflictsCount,
            }),
          );
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
      message.warning(t('没有可执行的变更项'));
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
        t('成功调整: {p0} 个文件', { p0: result.successCount }),
        result.skippedCount > 0 ? t('跳过: {p0}', { p0: result.skippedCount }) : '',
        result.cleanedFoldersCount > 0
          ? t('清理空目录: {p0} 个', { p0: result.cleanedFoldersCount })
          : '',
        result.failedCount > 0 ? t('失败: {p0} 项', { p0: result.failedCount }) : '',
      ]
        .filter(Boolean)
        .join('，');

      if (result.failedCount === 0) {
        message.success(t('操作全部完成！{p0}', { p0: details }));
      } else {
        message.warning(
          t('操作部分完成。{p0}。错误: {p1}', { p0: details, p1: result.errors.join('; ') }),
        );
      }
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setExecuting(false);
    }
  };

  const tableColumns = [
    {
      title: t('原文件 / 相对路径'),
      dataIndex: 'relativePath',
      key: 'relativePath',
      render: (rel: string) => (
        <Space orientation="vertical" size={2}>
          <Text code ellipsis style={{ maxWidth: 300 }}>
            {displayPath(rel)}
          </Text>
        </Space>
      ),
    },
    {
      title: t('拟议新名称 / 操作'),
      dataIndex: 'proposedName',
      key: 'proposedName',
      render: (name: string, record: ToolboxItemPlan) => (
        <Space orientation="vertical" size={2}>
          <Space>
            <Text strong style={{ color: record.hasConflict ? 'var(--ou-warning)' : undefined }}>
              {name}
            </Text>
            {record.action === 'rename' && <Tag color="blue">{t('重命名')}</Tag>}
            {record.action === 'move' && <Tag color="cyan">{t('平铺移动')}</Tag>}
            {record.action === 'skip' && <Tag color="default">{t('跳过')}</Tag>}
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
      title: t('大小'),
      dataIndex: 'size',
      key: 'size',
      width: 100,
      render: (size: number) => formatBytes(size),
    },
    {
      title: t('状态'),
      key: 'status',
      width: 120,
      render: (_: unknown, record: ToolboxItemPlan) => {
        if (record.action === 'skip') {
          return <Tag color="default">{t('跳过')}</Tag>;
        }
        if (record.hasConflict) {
          return <Tag color="warning">{t('冲突已处理')}</Tag>;
        }
        return <Tag color="success">{t('安全')}</Tag>;
      },
    },
  ];

  const fixResultColumns = [
    {
      title: t('原文件与路径'),
      dataIndex: 'originalPath',
      key: 'originalPath',
      render: (path: string) => {
        const fileName = getBaseName(path);
        const dir = getDirName(path);
        return (
          <Space orientation="vertical" size={2}>
            <Text strong>{fileName}</Text>
            <Text code ellipsis style={{ maxWidth: 360, fontSize: 12 }}>
              {displayPath(dir || path)}
            </Text>
          </Space>
        );
      },
    },
    {
      title: t('修复后名称 / 真实格式'),
      key: 'proposedName',
      render: (_: unknown, record: ArchiveExtensionFixItem) => {
        const origName = getBaseName(record.originalPath);
        if (record.status === 'renamed' && record.proposedName) {
          return (
            <Space>
              <Text delete type="secondary">
                {origName}
              </Text>
              <ArrowRightOutlined style={{ color: 'var(--ou-success)' }} />
              <Text strong style={{ color: 'var(--ou-success)' }}>
                {record.proposedName}
              </Text>
            </Space>
          );
        }
        if (record.status === 'already_correct') {
          return (
            <Space>
              <Text strong>{record.proposedName || origName}</Text>
              <Text type="secondary" style={{ fontSize: 12 }}>
                ({t('格式已合规')})
              </Text>
            </Space>
          );
        }
        return <Text type="secondary">{origName}</Text>;
      },
    },
    {
      title: t('状态'),
      dataIndex: 'status',
      key: 'status',
      width: 130,
      render: (status: string) => {
        if (status === 'renamed') {
          return (
            <Tag color="success" icon={<CheckCircleOutlined />}>
              {t('已修改后缀')}
            </Tag>
          );
        }
        if (status === 'already_correct') {
          return (
            <Tag color="blue" icon={<InfoCircleOutlined />}>
              {t('无需修改')}
            </Tag>
          );
        }
        if (status === 'unrecognized') {
          return <Tag color="default">{t('非压缩包')}</Tag>;
        }
        return (
          <Tag color="error" icon={<CloseCircleOutlined />}>
            {t('失败')}
          </Tag>
        );
      },
    },
    {
      title: t('详细说明'),
      dataIndex: 'message',
      key: 'message',
      render: (msg: string) => <Text style={{ fontSize: 12 }}>{msg}</Text>,
    },
    {
      title: t('操作'),
      key: 'actions',
      width: 120,
      render: (_: unknown, record: ArchiveExtensionFixItem) => {
        const target = record.newPath || record.originalPath;
        return (
          <Button
            size="small"
            type="link"
            icon={<FolderOpenOutlined />}
            disabled={!nativeEngine || !target}
            onClick={() => void handleOpenDirectory(target)}
          >
            {t('打开目录')}
          </Button>
        );
      },
    },
  ];

  return (
    <div style={{ maxWidth: 1100, margin: '0 auto', paddingBottom: 24 }}>
      <div className="page-header" style={{ marginBottom: 14 }}>
        <div>
          <h2 className="page-title">{t('工具箱')}</h2>
          <p className="page-description">
            {t('压缩包后缀物理自动修复、本地目录规范化整理、批量更名与归档平铺提取')}
          </p>
        </div>
      </div>
      <Card
        style={{
          marginBottom: 16,
          borderRadius: 'var(--ou-radius)',
          border: '1px solid var(--ou-border-color)',
        }}
      >
        <Tabs
          activeKey={activeTab}
          onChange={(k) => {
            const nextTab = k as ToolboxTab;
            setActiveTab(nextTab);
            if (nextTab !== 'autoFix') {
              setKind(nextTab);
            }
            setPreviewResult(null);
          }}
          items={[
            {
              key: 'normalizeExtensions',
              label: (
                <span>
                  <FileDoneOutlined />
                  {t('目录扩展名规范化 (预览分析)')}
                </span>
              ),
            },
            {
              key: 'autoFix',
              label: (
                <span>
                  <ThunderboltOutlined style={{ color: 'var(--ou-primary)' }} />
                  {t('压缩包后缀自动修复 (直接修改)')}
                </span>
              ),
            },
            {
              key: 'batchRename',
              label: (
                <span>
                  <SwapOutlined />
                  {t('智能批量更名')}
                </span>
              ),
            },
            {
              key: 'flattenDirectory',
              label: (
                <span>
                  <ClearOutlined />
                  {t('平铺提取与整理')}
                </span>
              ),
            },
          ]}
        />

        {activeTab === 'autoFix' ? (
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
                <Button
                  danger
                  icon={<DeleteOutlined />}
                  onClick={handleClearFixPaths}
                  disabled={fixing}
                >
                  {t('清空待处理列表 ({p0})', { p0: fixPaths.length })}
                </Button>
              )}
            </Space>

            {fixPaths.length === 0 && !fixResults && (
              <div
                className="dropzone-container"
                role="region"
                aria-label={
                  isDesktop ? t('添加文件，或将伪装可疑压缩包拖拽到此处') : t('自动修复区域')
                }
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
                <InboxOutlined
                  style={{ fontSize: 40, color: 'var(--ou-primary)', marginBottom: 8 }}
                />
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
        ) : (
          <Form layout="vertical">
            <Form.Item label={t('源目录路径 (严格 NativeAccess 授权)')}>
              <Space.Compact style={{ width: '100%' }}>
                <Input
                  value={sourceDirectory}
                  aria-label={t('工具箱源目录')}
                  onChange={(event) => setSourceDirectory(event.target.value)}
                  placeholder={t('请选择本地目录，系统将通过权限守卫隔离执行...')}
                  readOnly={!daemon}
                />
                <Button
                  icon={<FolderOpenOutlined />}
                  disabled={!nativeEngine}
                  onClick={() => void handlePickSource()}
                >
                  {t('选择文件夹')}
                </Button>
              </Space.Compact>
            </Form.Item>

            {kind === 'normalizeExtensions' && (
              <Alert
                type="info"
                showIcon
                title={t('自动规范化归档扩展名')}
                description={t(
                  '智能识别修复被错误切割或双重后缀的复合归档（如 .tar.gz.tar 修正为 .tar.gz、.tgz 规范为 .tar.gz），并通过头部魔数嗅探校验真实归档格式（7z/ZIP/RAR/TAR/ZSTD/LZ4），规范缺少后缀或命名错乱的文件。',
                )}
                style={{ marginBottom: 16 }}
              />
            )}

            {kind === 'batchRename' && (
              <Card type="inner" title={t('更名规则配置')} style={{ marginBottom: 16 }}>
                <Space orientation="vertical" style={{ width: '100%' }} size={12}>
                  <Space wrap style={{ width: '100%' }}>
                    <Form.Item
                      label={t('查找文本 / 正则')}
                      style={{ marginBottom: 0, minWidth: 240 }}
                    >
                      <Input
                        value={searchPattern}
                        onChange={(e) => setSearchPattern(e.target.value)}
                        placeholder={t('留空则不执行替换')}
                      />
                    </Form.Item>
                    <Form.Item label={t('替换为')} style={{ marginBottom: 0, minWidth: 240 }}>
                      <Input
                        value={replacePattern}
                        onChange={(e) => setReplacePattern(e.target.value)}
                        placeholder={t('替换目标文本')}
                      />
                    </Form.Item>
                    <Form.Item label={t('匹配设置')} style={{ marginBottom: 0 }}>
                      <Space style={{ paddingTop: 4 }}>
                        <Switch
                          checked={caseSensitive}
                          onChange={setCaseSensitive}
                          checkedChildren={t('区分大小写')}
                          unCheckedChildren={t('忽略大小写')}
                        />
                        <Switch
                          checked={useRegex}
                          onChange={setUseRegex}
                          checkedChildren={t('正则匹配')}
                          unCheckedChildren={t('普通文本')}
                        />
                      </Space>
                    </Form.Item>
                  </Space>

                  <Space wrap style={{ width: '100%' }}>
                    <Form.Item label={t('前缀')} style={{ marginBottom: 0, minWidth: 200 }}>
                      <Input
                        value={prefix}
                        onChange={(e) => setPrefix(e.target.value)}
                        placeholder={t('在文件名开头添加')}
                      />
                    </Form.Item>
                    <Form.Item label={t('后缀')} style={{ marginBottom: 0, minWidth: 200 }}>
                      <Input
                        value={suffix}
                        onChange={(e) => setSuffix(e.target.value)}
                        placeholder={t('在扩展名前添加')}
                      />
                    </Form.Item>
                  </Space>

                  <Divider style={{ margin: '8px 0' }} />

                  <Space wrap align="center">
                    <Switch
                      checked={enableNumbering}
                      onChange={setEnableNumbering}
                      checkedChildren={t('启用自动编号')}
                      unCheckedChildren={t('禁用自动编号')}
                    />
                    {enableNumbering && (
                      <Space>
                        <span>{t('起始序号:')}</span>
                        <InputNumber
                          min={1}
                          max={999999}
                          value={numberingStart}
                          onChange={(v) => setNumberingStart(v ?? 1)}
                        />
                        <span>{t('位数 (补零):')}</span>
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
              <Card type="inner" title={t('平铺提取选项')} style={{ marginBottom: 16 }}>
                <Form.Item label={t('平铺目标目录 (留空则平铺至源目录根路径)')}>
                  <Space.Compact style={{ width: '100%' }}>
                    <Input
                      value={targetDirectory}
                      aria-label={t('工具箱目标目录')}
                      onChange={(event) => setTargetDirectory(event.target.value)}
                      placeholder={t('默认提取至源目录根路径...')}
                      readOnly={!daemon}
                    />
                    <Button
                      icon={<FolderOpenOutlined />}
                      disabled={!nativeEngine}
                      onClick={() => void handlePickTarget()}
                    >
                      {t('自定义目标')}
                    </Button>
                    {targetDirectory && (
                      <Button onClick={() => setTargetDirectory('')}>{t('清除')}</Button>
                    )}
                  </Space.Compact>
                </Form.Item>

                <Space size={24}>
                  <Form.Item label={t('提取后自动清理空目录')} style={{ marginBottom: 0 }}>
                    <Switch
                      checked={removeEmptyFolders}
                      onChange={setRemoveEmptyFolders}
                      checkedChildren={t('自动清理')}
                      unCheckedChildren={t('保留空目录')}
                    />
                  </Form.Item>
                </Space>
              </Card>
            )}

            <Space wrap size={24} style={{ marginBottom: 16 }}>
              {kind !== 'flattenDirectory' && (
                <Form.Item label={t('包含子文件夹 (递归扫描)')} style={{ marginBottom: 0 }}>
                  <Switch
                    checked={recursive}
                    onChange={setRecursive}
                    checkedChildren={t('包含')}
                    unCheckedChildren={t('仅根目录')}
                  />
                </Form.Item>
              )}

              <Form.Item label={t('同名冲突策略')} style={{ marginBottom: 0 }}>
                <Radio.Group
                  value={conflictStrategy}
                  onChange={(e) => setConflictStrategy(e.target.value as ConflictStrategy)}
                >
                  <Radio.Button value="rename">{t('自动追加序号 (_1, _2)')}</Radio.Button>
                  <Radio.Button value="skip">{t('跳过冲突文件')}</Radio.Button>
                  <Radio.Button value="error">{t('报错并中止')}</Radio.Button>
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
              >
                {t('分析并预览改动 (Dry-Run)')}
              </Button>
            </div>
          </Form>
        )}
      </Card>

      {previewResult && (
        <Card
          title={
            <Space>
              <span>{t('改动预览与核对')}</span>
              <Tag color="blue">
                {t('共发现 {count} 项需要变更', { count: previewResult.totalPlanned })}
              </Tag>
              {previewResult.conflictsCount > 0 && (
                <Tag color="warning">
                  {t('包含 {count} 项命名冲突', { count: previewResult.conflictsCount })}
                </Tag>
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
            >
              {t('确认并执行改动 ({count})', { count: selectedPaths.length })}
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

      <ToolboxConfirmModal
        open={confirmModalVisible && !!previewResult}
        executing={executing}
        kind={kind}
        selectedCount={selectedPaths.length}
        conflictStrategy={conflictStrategy}
        removeEmptyFolders={removeEmptyFolders}
        onConfirm={() => void handleExecute()}
        onCancel={() => setConfirmModalVisible(false)}
      />
    </div>
  );
}
