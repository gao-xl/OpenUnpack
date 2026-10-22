import { t, useLanguage } from '../i18n';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { App as AntApp, Alert, Button, Empty, Input, Modal, Space, Spin, Tag, Tree, Typography } from 'antd';
import type { DataNode } from 'antd/es/tree';
import { FileOutlined, FolderOutlined, SearchOutlined } from '@ant-design/icons';
import { api, isDesktop } from '../api';
import type { ArchiveEntryView, ArchiveInspectionView, JobView } from '../types';
import { formatBytes } from './shared';

interface DraftNode {
  name: string;
  fullPath: string;
  size: number;
  sizeKnown: boolean;
  isDirectory: boolean;
  isEncrypted: boolean;
  rawName?: string;
  children: Map<string, DraftNode>;
}

const splitPath = (path: string) => path.split(/[\\/]+/).filter(Boolean);

const MAX_TREE_DEPTH = 32;
const MAX_TREE_NODES = 10_000;
const boundedParts = (path: string) => {
  const parts = splitPath(path);
  return parts.length <= MAX_TREE_DEPTH ? parts : [...parts.slice(0, MAX_TREE_DEPTH - 1), parts.slice(MAX_TREE_DEPTH - 1).join('/')];
};

/** Rebuilds the directory hierarchy from flat entry paths; ancestors of matched
 * entries are synthesized so search results keep their folder context. */
function buildTree(entries: ArchiveEntryView[]): DataNode[] {
  const roots = new Map<string, DraftNode>();
  let totalNodes = 0;
  for (const entry of entries) {
    if (totalNodes >= MAX_TREE_NODES) break;
    const rawParts = entry.rawPath && entry.rawPath !== entry.path ? boundedParts(entry.rawPath) : [];
    const parts = boundedParts(entry.path);
    if (!parts.length) continue;
    let level = roots;
    let prefix = '';
    let complete = true;
    for (let index = 0; index < parts.length - 1; index += 1) {
      prefix = prefix ? `${prefix}/${parts[index]}` : parts[index];
      let node = level.get(parts[index]);
      const rawPart = rawParts.length === parts.length && rawParts[index] !== parts[index] ? rawParts[index] : undefined;
      if (!node) {
        if (totalNodes >= MAX_TREE_NODES) { complete = false; break; }
        totalNodes += 1;
        node = { name: parts[index], fullPath: prefix, size: 0, sizeKnown: true, isDirectory: true, isEncrypted: false, rawName: rawPart, children: new Map() };
        level.set(parts[index], node);
      } else if (rawPart && !node.rawName) {
        node.rawName = rawPart;
      }
      level = node.children;
    }
    if (!complete) continue;
    const name = parts[parts.length - 1];
    const rawLeaf = rawParts.length ? rawParts[rawParts.length - 1] : undefined;
    const rawName = rawLeaf && rawLeaf !== name ? rawLeaf : undefined;
    const normalizedPath = prefix ? `${prefix}/${name}` : name;
    const existing = level.get(name);
    if (existing) {
      existing.isEncrypted ||= entry.isEncrypted;
      if (rawName && !existing.rawName) existing.rawName = rawName;
      if (!entry.isDirectory) {
        existing.isDirectory = false;
        existing.size = entry.size;
        existing.sizeKnown = entry.sizeKnown;
      }
    } else if (totalNodes < MAX_TREE_NODES) {
      totalNodes += 1;
      level.set(name, {
        name,
        fullPath: normalizedPath,
        size: entry.size,
        sizeKnown: entry.sizeKnown,
        isDirectory: entry.isDirectory,
        isEncrypted: entry.isEncrypted,
        rawName,
        children: new Map(),
      });
    }
  }
  const convert = (nodes: Map<string, DraftNode>, depth = 0): DataNode[] =>
    depth >= MAX_TREE_DEPTH
      ? []
      : [...nodes.values()]
          .sort((a, b) => (a.isDirectory === b.isDirectory ? a.name.localeCompare(b.name) : a.isDirectory ? -1 : 1))
          .map(node => ({
            key: node.fullPath,
            title: (
              <Space size={6}>
                {node.isDirectory ? <FolderOutlined aria-hidden /> : <FileOutlined aria-hidden />}
                <span>{node.name}</span>
                {node.rawName && (
                  <Typography.Text type="secondary" style={{ fontSize: 12 }} title={t("原始名称: {p0}", { p0: node.rawName })}>{t("(原: {name})", { name: node.rawName })}
                  </Typography.Text>
                )}
                {!node.isDirectory && <span className="muted">{node.sizeKnown ? formatBytes(node.size) : t("大小未知")}</span>}
                {node.isEncrypted && <Tag color="warning">{t("加密")}</Tag>}
              </Space>
            ),
            children: node.children.size ? convert(node.children, depth + 1) : undefined,
          }));
  return convert(roots, 0);
}

const collectKeys = (nodes: DataNode[]): string[] => {
  const keys: string[] = [];
  const stack: DataNode[] = [...nodes];
  while (stack.length > 0) {
    const node = stack.pop()!;
    keys.push(String(node.key));
    if (node.children) {
      for (let i = node.children.length - 1; i >= 0; i -= 1) {
        stack.push(node.children[i]);
      }
    }
  }
  return keys;
};

interface ArchiveBrowserProps { job?: JobView; close: () => void }
export default function ArchiveBrowser(props: ArchiveBrowserProps) {
  const connectionVersion = api.getConnectionVersion?.() ?? 0;
  const owner = props.job ? `${connectionVersion}:${props.job.id}:${props.job.source.path}` : 'no-archive';
  return <ArchiveBrowserContent key={owner} {...props} />;
}
function ArchiveBrowserContent({ job, close }: ArchiveBrowserProps) {
  const daemon = api.isDaemonMode?.() === true;
  const connected = isDesktop || daemon;
  const nativeEngine = isDesktop && !daemon;
  const { language } = useLanguage();
  const { message } = AntApp.useApp();
  const [loading, setLoading] = useState(false);
  const [inspection, setInspection] = useState<ArchiveInspectionView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [needPassword, setNeedPassword] = useState(false);
  const [password, setPassword] = useState('');
  const [search, setSearch] = useState('');
  const [expandedKeys, setExpandedKeys] = useState<string[] | null>(null);
  const [checkedKeys, setCheckedKeys] = useState<string[]>([]);
  const [extractingSelected, setExtractingSelected] = useState(false);
  const [selectedOutputRoot, setSelectedOutputRoot] = useState<string | null>(null);
  const [extractionPasswordError, setExtractionPasswordError] = useState<'required' | 'mismatchOrCorrupt' | null>(null);
  const [applyingPassword, setApplyingPassword] = useState(false);
  const loadSeqRef = useRef(0);
  const activeRequestRef = useRef<{ id: string; owner: string } | null>(null);
  const applySeqRef = useRef<number | null>(null);
  const extractSeqRef = useRef<number | null>(null);
  const mountedRef = useRef(false);
  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; loadSeqRef.current += 1; };
  }, []);
  const cancelRequest = useCallback((id: string) => {
    void api.cancelInspectArchive(id).catch(() => {
      if (mountedRef.current) void message.warning(t("停止归档检查未确认，请稍后核对后台状态。"));
    });
  }, [message]);
  const cancelActive = useCallback((owner?: string) => {
    const current = activeRequestRef.current;
    if (!current || (owner !== undefined && current.owner !== owner)) return;
    activeRequestRef.current = null;
    cancelRequest(current.id);
  }, [cancelRequest]);

  const load = useCallback(async (path: string, secret: string | null, owner: string) => {
    cancelActive();
    const seq = ++loadSeqRef.current;
    const request = { id: crypto.randomUUID(), owner };
    activeRequestRef.current = request;
    applySeqRef.current = null;
    setApplyingPassword(false);
    setLoading(true);
    setInspection(null);
    setError(null);
    setNeedPassword(false);
    if (!secret) {
      setPassword('');
    }
    setSearch('');
    setExpandedKeys(null);
    setCheckedKeys([]);
    setSelectedOutputRoot(null);
    setExtractionPasswordError(null);
    try {
      const result = await api.inspectArchive(path, secret, request.id);
      if (seq !== loadSeqRef.current) return;
      setInspection(result);
      setError(null);
      setNeedPassword(false);
    } catch (cause) {
      if (seq !== loadSeqRef.current) return;
      const message = cause instanceof Error ? cause.message : String(cause);
      if (message.includes('NeedsPassword')) {
        setNeedPassword(true);
        setError(null);
      } else {
        setError(message);
      }
    } finally {
      if (seq === loadSeqRef.current) {
        if (activeRequestRef.current === request) activeRequestRef.current = null;
        setLoading(false);
      }
    }
  }, [cancelActive]);

  const handleClose = useCallback(() => {
    loadSeqRef.current += 1;
    setPassword('');
    setInspection(null);
    setError(null);
    setNeedPassword(false);
    setCheckedKeys([]);
    setSelectedOutputRoot(null);
    setExtractionPasswordError(null);
    applySeqRef.current = null;
    setApplyingPassword(false);
    cancelActive();
    close();
  }, [close, cancelActive]);

  const currentJobId = job?.id;
  const currentPath = job?.source.path;

  useEffect(() => {
    let disposed = false;
    if (!currentJobId || !currentPath) {
      const seq = ++loadSeqRef.current;
      queueMicrotask(() => {
        if (disposed || loadSeqRef.current !== seq) return;
        setPassword('');
        setInspection(null);
        setError(null);
        setNeedPassword(false);
        setCheckedKeys([]);
        setLoading(false);
        setApplyingPassword(false);
      });
      return () => { disposed = true; };
    }
    queueMicrotask(() => {
      if (!disposed) void load(currentPath, null, currentJobId);
    });
    return () => {
      disposed = true;
      loadSeqRef.current += 1;
      cancelActive(currentJobId);
    };
  }, [currentJobId, currentPath, load, cancelActive]);

  const query = search.trim().toLowerCase();
  const matched = useMemo(() => {
    if (!inspection) return [];
    if (!query) return inspection.entries;
    return inspection.entries.filter(
      entry => entry.path.toLowerCase().includes(query) || (entry.rawPath && entry.rawPath.toLowerCase().includes(query)),
    );
  }, [inspection, query]);
  // buildTree reads the external translation state; language invalidates its cached labels.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const tree = useMemo(() => buildTree(matched), [matched, language]);
  const allKeys = useMemo(() => collectKeys(tree), [tree]);
  const expanded = useMemo(() => {
    if (expandedKeys !== null) return expandedKeys;
    if (allKeys.length <= 200) return allKeys;
    return tree.filter(node => node.children?.length).slice(0, 20).map(node => String(node.key));
  }, [allKeys, expandedKeys, tree]);

  const isNonStandardEncoding = inspection?.detectedEncoding && !['UTF-8', 'ASCII'].includes(inspection.detectedEncoding.toUpperCase());

  const stats = inspection && (
    <Space wrap size={12}>
      <Typography.Text type="secondary">{t("{count} 项", { count: inspection.totalEntries })}</Typography.Text>
      <Typography.Text type="secondary">{inspection.totalSizeKnown ? t("共 {p0}", { p0: formatBytes(inspection.totalSize) }) : t("总展开大小未知")}</Typography.Text>
      {inspection.encryptedEntries > 0 && <Typography.Text type="secondary">{t("加密 {count} 项", { count: inspection.encryptedEntries })}</Typography.Text>}
      {isNonStandardEncoding && (
        <Tag color="cyan">{t("编码: {encoding} (自动修正)", { encoding: inspection.detectedEncoding ?? '' })}</Tag>
      )}
      {query && <Typography.Text type="secondary">{t("命中 {matched} / {total} 项", { matched: matched.length, total: inspection.totalEntries })}</Typography.Text>}
    </Space>
  );

  const applyPassword = async () => {
    if (!job || !password) return;
    const seq = loadSeqRef.current;
    if (applySeqRef.current === seq) return;
    applySeqRef.current = seq;
    setApplyingPassword(true);
    try {
      await api.savePassword({
        name: t("{p0} 密码", { p0: job.source.displayName }),
        secret: password,
        groups: job.options.passwordGroup ? [job.options.passwordGroup] : [],
        notes: t("归档预览确认密码"),
        enabled: true,
      });
      if (loadSeqRef.current !== seq) return;
      await api.retry(job.id);
      if (loadSeqRef.current === seq) handleClose();
    } catch (cause) {
      if (loadSeqRef.current === seq) setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      if (loadSeqRef.current === seq) { applySeqRef.current = null; setApplyingPassword(false); }
    }
  };

  const handleExtractSelected = async (chooseDir = false) => {
    if (!job || checkedKeys.length === 0) return;
    const seq = loadSeqRef.current;
    if (extractSeqRef.current === seq) return;
    const connectionVersion = api.getConnectionVersion?.() ?? 0;
    const current = () => mountedRef.current && loadSeqRef.current === seq && connectionVersion === (api.getConnectionVersion?.() ?? 0);
    extractSeqRef.current = seq;
    setExtractingSelected(true);
    let outputRoot = selectedOutputRoot ?? job.options.outputRoot;
    try {
      if (chooseDir) {
        if (!nativeEngine) return;
        const picked = await api.pickDirectory();
        if (!current() || !picked) return;
        outputRoot = picked;
        setSelectedOutputRoot(picked);
      }
      const res = await api.extractSelective({
        archivePath: job.source.path,
        entryPaths: checkedKeys,
        destinationDir: outputRoot || undefined,
        password: password || undefined,
        flatten: false,
      });
      if (!current()) return;
      void message.success(t("已成功提取 {count} 个文件至 {path}", { count: res.extractedFiles.length, path: res.outputDirectory }));
      if (nativeEngine) {
        void api.openPath(res.outputDirectory).catch(cause => {
          void message.warning(t("提取已完成，但无法打开输出目录：{error}", { error: cause instanceof Error ? cause.message : String(cause) }));
        });
      }
      handleClose();
    } catch (cause) {
      if (!current()) return;
      const detail = cause instanceof Error ? cause.message : String(cause);
      if (detail.startsWith('NeedsPassword')) setExtractionPasswordError('required');
      else if (detail.startsWith('PasswordMismatchOrCorrupt')) setExtractionPasswordError('mismatchOrCorrupt');
      else void message.error(detail);
    } finally {
      if (extractSeqRef.current === seq) extractSeqRef.current = null;
      if (mountedRef.current && loadSeqRef.current === seq) setExtractingSelected(false);
    }
  };

  return (
    <Modal
      title={t("浏览归档内容 — {p0}", { p0: job?.source.displayName ?? '' })}
      open={!!job}
      onCancel={handleClose}
      destroyOnHidden
      width={720}
      footer={
        <Space>
          <Button onClick={handleClose}>{t("关闭")}</Button>
          {inspection && !!password && !extractionPasswordError && (
            <Button type="primary" loading={applyingPassword} onClick={() => void applyPassword()}>{t("应用此密码并重试解压")}</Button>
          )}
          {inspection && nativeEngine && (
            <Button
              disabled={checkedKeys.length === 0}
              loading={extractingSelected}
              onClick={() => void handleExtractSelected(true)}
            >
              {t("提取所选至...")}
            </Button>
          )}
          {inspection && (
            <Button
              type="primary"
              disabled={checkedKeys.length === 0}
              loading={extractingSelected}
              onClick={() => void handleExtractSelected(false)}
            >
              {t("提取所选条目 ({count})", { count: checkedKeys.length })}
            </Button>
          )}
        </Space>
      }
    >
      {!connected && <Alert type="info" showIcon title={t("浏览器预览模式")} description={t("归档浏览需要在桌面版中运行。")} />}
      {connected && (
        <Space orientation="vertical" style={{ width: '100%' }}>
          {extractionPasswordError && (
            <Alert
              type="warning"
              showIcon
              title={extractionPasswordError === 'required' ? t("提取所选条目需要密码") : t("密码不匹配或归档已损坏")}
              description={
                <Space orientation="vertical" style={{ width: '100%' }}>
                  <Typography.Text>{t("输入密码后重试提取；若仍失败，请检查密码或归档完整性。密码仅用于本次提取，不会保存到密码库。")}</Typography.Text>
                  <Space.Compact style={{ width: '100%' }}>
                    <Input.Password
                      aria-label={t("所选条目的解压密码")}
                      value={password}
                      onChange={event => setPassword(event.target.value)}
                      onPressEnter={() => { if (password.length && checkedKeys.length) void handleExtractSelected(); }}
                    />
                    <Button type="primary" disabled={!password.length || checkedKeys.length === 0} loading={extractingSelected} onClick={() => void handleExtractSelected()}>{t("使用该密码提取所选")}</Button>
                  </Space.Compact>
                </Space>
              }
            />
          )}
          {needPassword && (
            <Alert
              type="warning"
              showIcon
              title={t("此归档需要密码才能列出内容")}
              description={
                <Space.Compact style={{ width: '100%', marginTop: 8 }}>
                  <Input.Password
                    aria-label={t("归档密码")}
                    value={password}
                    placeholder={t("输入归档密码后重试")}
                    onChange={event => setPassword(event.target.value)}
                    onPressEnter={() => { if (password.length && job) void load(job.source.path, password, job.id); }}
                  />
                  <Button disabled={!password.length} onClick={() => { if (job) void load(job.source.path, password, job.id); }}>{t("使用该密码浏览")}</Button>
                </Space.Compact>
              }
            />
          )}
          {error && (
            <Alert
              type="error"
              showIcon
              title={t("无法读取归档内容")}
              description={error}
              action={<Button size="small" onClick={() => { if (job) void load(job.source.path, password || null, job.id); }}>{t("重试")}</Button>}
            />
          )}
          {loading && (
            <div style={{ textAlign: 'center', padding: '32px 0' }}>
              <Spin />
              <div className="muted" style={{ marginTop: 8 }}>{t("等待可用资源或读取归档目录中…")}</div>
            </div>
          )}
          {!loading && inspection && (
            <>
              {stats}
              <Input
                aria-label={t("搜索归档条目")}
                allowClear
                value={search}
                prefix={<SearchOutlined aria-hidden />}
                placeholder={t("按名称或路径搜索，例如 folder/ 或 *.txt")}
                onChange={event => { setSearch(event.target.value); setExpandedKeys(null); }}
              />
              <Space wrap size={8} style={{ width: '100%', justifyContent: 'space-between' }}>
                <Space size={8}>
                  <Button size="small" onClick={() => setCheckedKeys(allKeys)}>{t("全选")}</Button>
                  <Button size="small" disabled={checkedKeys.length === 0} onClick={() => setCheckedKeys([])}>{t("清空选择")}</Button>
                  {checkedKeys.length > 0 && (
                    <Tag color="blue">{t("已选 {count} 项", { count: checkedKeys.length })}</Tag>
                  )}
                </Space>
              </Space>
              {matched.length ? (
                <Tree
                  aria-label={t("归档条目树")}
                  showLine
                  checkable
                  selectable={false}
                  checkedKeys={checkedKeys}
                  onCheck={keys => {
                    const next = Array.isArray(keys) ? keys : keys.checked;
                    setCheckedKeys(next.map(String));
                  }}
                  treeData={tree}
                  expandedKeys={expanded}
                  onExpand={nextKeys => setExpandedKeys(nextKeys.map(String))}
                  height={380}
                  style={{ maxHeight: 380, overflow: 'auto' }}
                />
              ) : (
                <Empty description={query ? t("没有匹配的条目") : t("归档内没有条目")} />
              )}
            </>
          )}
        </Space>
      )}
    </Modal>
  );
}
