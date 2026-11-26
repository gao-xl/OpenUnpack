import { t, useLanguage } from '../i18n';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  App as AntApp,
  Alert,
  Button,
  Card,
  Checkbox,
  Descriptions,
  Drawer,
  Input,
  Modal,
  Progress,
  Space,
  Tag,
  Tree,
  Typography,
} from 'antd';
import { KeyOutlined, ExclamationCircleOutlined } from '@ant-design/icons';
import type { DataNode } from 'antd/es/tree';
import { api, isDesktop } from '../api';
import { analyzeJobIssue, formatJobMessage } from '../jobTroubleshooting';
import type { JobView, NodeView } from '../types';
import { stageLabels } from '../types';
import {
  percent,
  retryable,
  canDeliverPartial,
  displayPath,
  verificationLabel,
} from '../utils/format';
import { buildNodeForest, StatusTag } from './JobStatusBadges';
import { sourceCleanupLabels, SourceCleanupScope } from './SourceCleanupSelect';
import TransferMetrics from './TransferMetrics';
import ArchiveBrowser from './ArchiveBrowser';

const EMPTY_NODES: NodeView[] = [];

export interface JobDetailsProps {
  job?: JobView;
  close: () => void;
  run: (work: () => Promise<unknown>) => Promise<void>;
  operationsDisabled?: boolean;
  inputDisabled?: boolean;
}
export function JobDetails(props: JobDetailsProps) {
  return <JobDetailsContent key={props.job?.id ?? 'no-task'} {...props} />;
}
function JobDetailsContent({
  job,
  close,
  run,
  operationsDisabled = false,
  inputDisabled,
}: JobDetailsProps) {
  const { language } = useLanguage();
  const daemon = api.isDaemonMode?.() === true;
  const { modal } = AntApp.useApp();
  const [browsing, setBrowsing] = useState(false);
  const [passwordModal, setPasswordModal] = useState(false);
  const [inlinePassword, setInlinePassword] = useState('');
  const [saveToVault, setSaveToVault] = useState(!daemon);
  const [submittingPassword, setSubmittingPassword] = useState(false);
  const passwordDraftEpoch = useRef(0);
  const passwordRequestEpoch = useRef<number | null>(null);
  const closePassword = useCallback(() => {
    passwordDraftEpoch.current += 1;
    passwordRequestEpoch.current = null;
    setPasswordModal(false);
    setInlinePassword('');
    setSubmittingPassword(false);
  }, []);
  const connected = isDesktop || daemon;
  const nativeEngine = isDesktop && !daemon;
  const jobId = job?.id;
  const passwordInputDisabled =
    inputDisabled ??
    (operationsDisabled && !(job?.status === 'running' && job.stage === 'needsPassword'));
  useEffect(
    () => () => {
      passwordDraftEpoch.current += 1;
    },
    [],
  );
  const closeDetails = () => {
    closePassword();
    setBrowsing(false);
    close();
  };

  const submitInlinePassword = useCallback(() => {
    if (!inlinePassword || passwordInputDisabled || submittingPassword || !job) return;
    const epoch = passwordDraftEpoch.current;
    const connectionVersion = api.getConnectionVersion?.() ?? 0;
    if (passwordRequestEpoch.current === epoch) return;
    passwordRequestEpoch.current = epoch;
    setSubmittingPassword(true);
    void run(async () => {
      try {
        const secret = inlinePassword;
        if (saveToVault) {
          await api.savePassword({
            name: t('{value0} 密码', { value0: job.source.displayName }),
            secret,
            groups: job.options.passwordGroup ? [job.options.passwordGroup] : [],
            notes: t('任务详情即时录入'),
            enabled: true,
          });
          if (
            passwordDraftEpoch.current !== epoch ||
            connectionVersion !== (api.getConnectionVersion?.() ?? 0)
          )
            return;
        }
        if (daemon) await api.submitInput(job.id, 'password', secret);
        else await api.retry(job.id, false, secret);
        if (passwordDraftEpoch.current === epoch) closePassword();
      } finally {
        if (passwordDraftEpoch.current === epoch) {
          passwordRequestEpoch.current = null;
          setSubmittingPassword(false);
        }
      }
    });
  }, [
    inlinePassword,
    passwordInputDisabled,
    submittingPassword,
    job,
    saveToVault,
    daemon,
    run,
    closePassword,
  ]);

  const nodes = job?.nodes ?? EMPTY_NODES;
  const canRetry = !!job && retryable(job);
  const treeData = useMemo((): DataNode[] => {
    return buildNodeForest(nodes, (node) => (
      <div className="node-detail">
        <Space wrap>
          <strong>{node.name}</strong>
          <StatusTag status={node.status} />
          <span className="muted">
            {t(stageLabels[node.stage] ?? node.stage)} · {percent(node.progress)}%
            {node.passwordLabel ? ` · ${node.passwordLabel}` : ''}
          </span>
          {node.depth > 0 && ['failed', 'needsPassword'].includes(node.status) && (
            <Button
              size="small"
              disabled={!connected || operationsDisabled || !jobId || !canRetry}
              onClick={(event) => {
                event.stopPropagation();
                if (jobId) void run(() => api.retryNode(jobId, node.id));
              }}
            >
              {t('重试此节点')}
            </Button>
          )}
        </Space>
        <div className="muted">
          {t('已尝试 {count} 次', { count: node.attempts })}
          {node.message ? ` · ${formatJobMessage(node.message)}` : ''}
        </div>
      </div>
    ));
    // Tree labels read the external translation state; language invalidates the cached labels.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nodes, jobId, canRetry, connected, operationsDisabled, run, language]);
  return (
    <Drawer
      title={job?.source.displayName ?? t('任务详情')}
      open={!!job}
      onClose={closeDetails}
      size={620}
    >
      {job && (
        <>
          <div className="action-row">
            <StatusTag status={job.status} />
            <Typography.Text type="secondary">
              {t(stageLabels[job.stage] ?? job.stage)} ·{' '}
              {t('第 {count} 次尝试', { count: job.attempts })}
            </Typography.Text>
          </div>
          <Progress
            percent={percent(job.progress)}
            status={job.status === 'failed' ? 'exception' : undefined}
          />
          <div style={{ marginBottom: 16 }}>
            <TransferMetrics job={job} />
          </div>
          {(() => {
            const analysis = analyzeJobIssue(job);
            if (analysis) {
              return (
                <Card
                  size="small"
                  style={{
                    marginBottom: 20,
                    border:
                      analysis.category === 'needsPassword'
                        ? '1.5px solid var(--ou-warning)'
                        : '1.5px solid var(--ou-danger)',
                    backgroundColor: 'var(--ou-bg-card)',
                    borderRadius: 'var(--ou-radius)',
                  }}
                  title={
                    <Space>
                      {analysis.category === 'needsPassword' ? (
                        <KeyOutlined style={{ color: 'var(--ou-warning)' }} />
                      ) : (
                        <ExclamationCircleOutlined style={{ color: 'var(--ou-danger)' }} />
                      )}
                      <Typography.Text
                        strong
                        style={{
                          color:
                            analysis.category === 'needsPassword'
                              ? 'var(--ou-warning)'
                              : 'var(--ou-danger)',
                        }}
                      >
                        {analysis.title}
                      </Typography.Text>
                      <Tag color={analysis.category === 'needsPassword' ? 'warning' : 'error'}>
                        {analysis.categoryLabel}
                      </Tag>
                    </Space>
                  }
                >
                  <Typography.Paragraph style={{ marginBottom: 8, fontSize: 13 }}>
                    {analysis.summary}
                  </Typography.Paragraph>
                  <div
                    style={{
                      backgroundColor: 'var(--ou-bg-input)',
                      padding: '8px 12px',
                      borderRadius: 4,
                    }}
                  >
                    <Typography.Text
                      strong
                      style={{ display: 'block', fontSize: 12, marginBottom: 4 }}
                    >
                      {t('排障与操作指引：')}
                    </Typography.Text>
                    <ul
                      style={{
                        margin: 0,
                        paddingLeft: 18,
                        fontSize: 12,
                        lineHeight: 1.6,
                        color: 'var(--ou-text-secondary)',
                      }}
                    >
                      {analysis.guideSteps.map((step, idx) => (
                        <li key={idx}>{step}</li>
                      ))}
                    </ul>
                  </div>
                </Card>
              );
            }
            if (job.message) {
              return (
                <Alert
                  type={retryable(job) ? 'warning' : 'info'}
                  title={formatJobMessage(job.message)}
                  showIcon
                  style={{ marginBottom: 20 }}
                />
              );
            }
            return null;
          })()}
          <Space wrap style={{ marginBottom: 16 }}>
            <Button disabled={!connected} onClick={() => setBrowsing(true)}>
              {t('浏览归档内容')}
            </Button>
            {(job.status === 'needsPassword' ||
              (job.status === 'running' && job.stage === 'needsPassword')) && (
              <Button
                type="primary"
                style={{ background: 'var(--ou-warning)', borderColor: 'var(--ou-warning)' }}
                disabled={!connected || passwordInputDisabled}
                onClick={() => {
                  passwordDraftEpoch.current += 1;
                  passwordRequestEpoch.current = null;
                  setInlinePassword('');
                  setSaveToVault(!daemon);
                  setSubmittingPassword(false);
                  setPasswordModal(true);
                }}
              >
                {daemon ? t('提交密码') : t('输入密码并重试')}
              </Button>
            )}
            {retryable(job) && (
              <Button
                disabled={!connected || operationsDisabled}
                onClick={() => void run(() => api.retry(job.id))}
              >
                {t('按当前设置重试任务')}
              </Button>
            )}
            {canDeliverPartial(job) && (
              <Button
                disabled={!connected || operationsDisabled}
                onClick={() =>
                  void modal.confirm({
                    title: t('交付部分结果？'),
                    content: t(
                      '将交付已完成内容并保留未展开的内包；结果会明确标为部分交付，源包保留。你可以先补充密码并重试失败节点。',
                    ),
                    okText: t('交付部分结果'),
                    cancelText: t('返回'),
                    onOk: () => run(() => api.retry(job.id, true)),
                  })
                }
              >
                {t('交付部分结果')}
              </Button>
            )}
            {job.status === 'publishPending' && (
              <>
                <Button
                  disabled={!connected || operationsDisabled}
                  onClick={() => void run(() => api.recover(job.id, 'verify'))}
                >
                  {t('回读并核对发布')}
                </Button>
                <Button
                  disabled={!connected || operationsDisabled}
                  onClick={() => void run(() => api.recover(job.id, 'retry'))}
                >
                  {t('重试封存结果写回')}
                </Button>
              </>
            )}
            {job.status === 'cleanupPending' && (
              <Button
                disabled={!connected || operationsDisabled}
                onClick={() => void run(() => api.recover(job.id, 'cleanup'))}
              >
                {t('重试清理缓存')}
              </Button>
            )}
            {job.workspace &&
              !job.outputPath &&
              ['failed', 'needsPassword', 'interrupted', 'cancelled', 'waitingResource'].includes(
                job.status,
              ) && (
                <Button
                  danger
                  disabled={!connected || operationsDisabled}
                  onClick={() =>
                    void modal.confirm({
                      title: t('清理本次临时结果？'),
                      content: t(
                        '清理本应用创建的工作目录，原始压缩包保留。已展开内容将不再用于节点续接；后续需重新处理源包。',
                      ),
                      okText: t('清理临时结果'),
                      cancelText: t('保留以便重试'),
                      onOk: () => run(() => api.recover(job.id, 'cleanup')),
                    })
                  }
                >
                  {t('清理临时结果')}
                </Button>
              )}
          </Space>
          <Descriptions
            column={1}
            bordered
            size="small"
            items={[
              {
                key: 'source',
                label: t('原始归档'),
                children: (
                  <Typography.Text copyable>{displayPath(job.source.path)}</Typography.Text>
                ),
              },
              {
                key: 'volumes',
                label: t('分卷'),
                children: job.source.volumes.length
                  ? job.source.volumes.map(displayPath).join('\n')
                  : t('单卷'),
              },
              {
                key: 'output',
                label: t('输出目录'),
                children:
                  job.outputPath != null
                    ? displayPath(job.outputPath)
                    : displayPath(job.options.outputRoot) || t('原始归档所在目录'),
              },
              {
                key: 'cache',
                label: t('工作目录'),
                children: job.options.cacheEnabled
                  ? t('SSD 缓存 · {value0}', {
                      value0: displayPath(job.options.cacheRoot) || t('未设置'),
                    })
                  : t('目标卷临时目录'),
              },
              {
                key: 'nested',
                label: t('内包处理'),
                children: job.options.expandNested
                  ? t('最多展开 {value0} 总层（包含外包）', { value0: job.options.maxDepth })
                  : t('保留内包'),
              },
              {
                key: 'password',
                label: t('密码范围'),
                children: job.options.passwordGroup || t('全部启用密码'),
              },
              {
                key: 'verification',
                label: t('已验证范围'),
                children: verificationLabel(job.verificationScope),
              },
              {
                key: 'retention',
                label: t('源文件策略'),
                children: (
                  <>
                    <span>
                      {t(
                        sourceCleanupLabels[job.options.sourceCleanup] || job.options.sourceCleanup,
                      )}
                    </span>
                    <SourceCleanupScope
                      value={job.options.sourceCleanup}
                      expandNested={job.options.expandNested}
                    />
                  </>
                ),
              },
              {
                key: 'rules',
                label: t('输出排除'),
                children: job.options.outputExclusions.length
                  ? job.options.outputExclusions.join('、')
                  : t('未设置排除规则'),
              },
            ]}
          />
          <Typography.Title level={5}>{t('内外层处理节点')}</Typography.Title>
          {job.nodes.length ? (
            <Tree
              key={job.id}
              treeData={treeData}
              defaultExpandAll={job.nodes.length <= 200}
              height={380}
              blockNode
            />
          ) : (
            <Typography.Text type="secondary">{t('任务开始后显示各层处理状态。')}</Typography.Text>
          )}
          {!!Object.keys(job.stageTimes).length && (
            <>
              <Typography.Title level={5}>{t('阶段用时')}</Typography.Title>
              <Descriptions
                size="small"
                column={2}
                items={Object.entries(job.stageTimes).map(([stage, duration]) => ({
                  key: stage,
                  label: t(stageLabels[stage] ?? stage),
                  children: t('{value0} 秒', { value0: (duration / 1000).toFixed(1) }),
                }))}
              />
            </>
          )}
          {nativeEngine && job.outputPath && (
            <Typography.Link onClick={() => void run(() => api.openPath(job.outputPath!))}>
              {t('打开已发布结果')}
            </Typography.Link>
          )}
          <ArchiveBrowser job={browsing ? job : undefined} close={() => setBrowsing(false)} />
          <Modal
            title={t('输入解压密码 — {value0}', { value0: job.source.displayName })}
            open={passwordModal}
            destroyOnHidden
            okText={daemon ? t('提交密码') : t('解密并重试')}
            cancelText={t('取消')}
            okButtonProps={{
              loading: submittingPassword,
              disabled: !inlinePassword || passwordInputDisabled,
            }}
            onCancel={closePassword}
            onOk={submitInlinePassword}
          >
            <Typography.Paragraph>
              {daemon
                ? t('密码将仅提交给当前服务任务；勾选保存后才会写入服务端密码库。')
                : t('请输入该归档文件的解压密码，输入后将立即重新调度此任务。')}
            </Typography.Paragraph>
            <Space orientation="vertical" style={{ width: '100%' }}>
              <Input.Password
                autoFocus
                aria-label={t('即时解压密码')}
                placeholder={t('输入解压密码')}
                value={inlinePassword}
                disabled={passwordInputDisabled}
                onChange={(e) => setInlinePassword(e.target.value)}
                onPressEnter={submitInlinePassword}
              />
              <Checkbox
                checked={saveToVault}
                disabled={inputDisabled}
                onChange={(e) => setSaveToVault(e.target.checked)}
              >
                {daemon
                  ? t('同时保存到服务端加密密码库（需要密码库写入权限）')
                  : t('同时保存到本地加密密码库（推荐，后续同批次或分卷可自动匹配）')}
              </Checkbox>
            </Space>
          </Modal>
        </>
      )}
    </Drawer>
  );
}
