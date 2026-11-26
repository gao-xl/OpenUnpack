import { Tag, Tooltip } from 'antd';
import { ThunderboltOutlined } from '@ant-design/icons';
import type { DataNode } from 'antd/es/tree';
import { t } from '../i18n';
import type { JobView, NodeView } from '../types';
import { statusLabels } from '../types';

export function getExtensionBadge(displayName: string) {
  const parts = displayName.split('.');
  let ext = parts.length > 1 ? parts.pop()?.toUpperCase() || 'ZIP' : 'ZIP';
  if (ext.length > 4) ext = ext.slice(0, 4);
  if (ext === '7Z' || /^\d{3}$/.test(ext)) {
    return {
      label: ext,
      bg: 'var(--ou-primary-bg)',
      text: 'var(--ou-primary)',
      border: 'var(--ou-primary-border)',
    };
  }
  if (ext === 'RAR') {
    return {
      label: 'RAR',
      bg: 'var(--ou-success-bg)',
      text: 'var(--ou-success)',
      border: 'var(--ou-success-border)',
    };
  }
  if (['TAR', 'GZ', 'BZ2', 'XZ'].includes(ext)) {
    return {
      label: ext,
      bg: 'var(--ou-accent-bg)',
      text: 'var(--ou-accent)',
      border: 'var(--ou-accent-border)',
    };
  }
  return {
    label: ext,
    bg: 'var(--ou-warning-bg)',
    text: 'var(--ou-warning)',
    border: 'var(--ou-warning-border)',
  };
}

export function ArchiveFormatBadge({ name }: { name: string }) {
  const badge = getExtensionBadge(name);
  return (
    <span
      style={{
        fontSize: 10,
        fontFamily: 'var(--ou-font-mono)',
        fontWeight: 600,
        padding: '1px 5px',
        borderRadius: 4,
        backgroundColor: badge.bg,
        color: badge.text,
        border: `1px solid ${badge.border}`,
        lineHeight: '14px',
        marginTop: 2,
        flexShrink: 0,
        display: 'inline-block',
      }}
    >
      {badge.label}
    </span>
  );
}

export function StatusTag({ status }: { status: string }) {
  const color = ['completed', 'cleanupPending'].includes(status)
    ? 'success'
    : ['failed', 'needsPassword'].includes(status)
      ? 'error'
      : ['running', 'stopping'].includes(status)
        ? 'processing'
        : ['interrupted', 'waitingResource', 'publishPending', 'partial', 'salvaged'].includes(
              status,
            )
          ? 'warning'
          : 'default';
  return (
    <Tag
      color={color}
      style={{
        borderRadius: 4,
        fontWeight: 500,
        fontSize: 12,
        padding: '1px 7px',
        margin: 0,
      }}
    >
      {t(statusLabels[status] ?? status)}
    </Tag>
  );
}

export function JobAccelerationBadge({ job }: { job: JobView }) {
  const isSsd =
    Boolean(job.workspace?.includes('OpenUnpack-cache')) ||
    Boolean(job.message?.includes('使用中转缓存')) ||
    Boolean(job.stageTimes?.['copying']);
  if (!isSsd) return null;

  const tip = job.message?.includes('使用中转缓存') ? job.message : t('已启用固态加速中转');

  return (
    <Tooltip title={tip}>
      <Tag
        color="cyan"
        style={{
          borderRadius: 4,
          fontWeight: 600,
          fontSize: 11,
          padding: '1px 6px',
          margin: 0,
          cursor: 'help',
          display: 'inline-flex',
          alignItems: 'center',
          gap: 2,
        }}
      >
        <ThunderboltOutlined />
        <span>{t('SSD加速')}</span>
      </Tag>
    </Tooltip>
  );
}

const MAX_NODE_TREE_DEPTH = 32;
// Keep the rendering hierarchy bounded even for corrupt parent references. Deep
// descendants become new roots; cycles and duplicate IDs are displayed once.
export function buildNodeForest(
  nodes: NodeView[],
  title: (node: NodeView) => React.ReactNode,
): DataNode[] {
  const byId = new Map<string, NodeView>();
  for (const node of nodes) if (!byId.has(node.id)) byId.set(node.id, node);
  const children = new Map<string, NodeView[]>();
  const initialRoots: NodeView[] = [];
  for (const node of byId.values()) {
    if (!node.parentId || node.parentId === node.id || !byId.has(node.parentId))
      initialRoots.push(node);
    else {
      const siblings = children.get(node.parentId) ?? [];
      siblings.push(node);
      children.set(node.parentId, siblings);
    }
  }
  const roots: DataNode[] = [];
  const seen = new Set<string>();
  const append = (root: NodeView) => {
    const stack = [{ node: root, target: roots, depth: 0 }];
    while (stack.length) {
      const current = stack.pop()!;
      if (seen.has(current.node.id)) continue;
      seen.add(current.node.id);
      const branch: DataNode = { key: current.node.id, title: title(current.node) };
      current.target.push(branch);
      const descendants = children.get(current.node.id) ?? [];
      if (!descendants.length) continue;
      const resetDepth = current.depth + 1 >= MAX_NODE_TREE_DEPTH;
      const target: DataNode[] = resetDepth ? roots : [];
      if (!resetDepth) branch.children = target;
      for (let index = descendants.length - 1; index >= 0; index -= 1) {
        stack.push({ node: descendants[index], target, depth: resetDepth ? 0 : current.depth + 1 });
      }
    }
  };
  for (const node of initialRoots) append(node);
  for (const node of byId.values()) if (!seen.has(node.id)) append(node);
  return roots;
}
