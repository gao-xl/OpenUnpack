import { t } from '../i18n';
import { useCallback, useEffect, useRef, useState } from 'react';
import { Alert, Button, Card, Checkbox, Form, Input, InputNumber, Modal, Popconfirm, Space, Switch, Table, Tag, Typography } from 'antd';
import { api, isDesktop } from '../api';
import type { ApiClientInput, ApiClientView, ApiScope, LocalApiStatus } from '../local-api-types';

const { Text } = Typography;
const emptyClient = (): ApiClientInput => ({ name: '', scopes: ['jobs.read'], readRoots: [], writeRoots: [] });
const automation: ApiScope[] = ['jobs.read', 'jobs.write', 'files.read', 'files.write', 'settings.read'];

export default function ApiSettingsPanel({ run }: { run: (work: () => Promise<unknown>) => Promise<void> }) {
  const [status, setStatus] = useState<LocalApiStatus | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [port, setPort] = useState<number | null>(47980);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [draft, setDraft] = useState<ApiClientInput>(emptyClient);
  const [token, setToken] = useState('');
  const [daemonUrl, setDaemonUrl] = useState(api.getDaemonConnection().url || '');
  const [daemonToken, setDaemonToken] = useState(api.getDaemonConnection().token || '');
  const [daemonActive, setDaemonActive] = useState(false);
  const mounted = useRef(false);
  const busyRef = useRef(false);
  const loadEpoch = useRef(0);
  const connectionDraftEpoch = useRef(0);

  const load = useCallback(async (updateDraft = false) => {
    const epoch = ++loadEpoch.current;
    try {
      const next = await api.apiStatus();
      if (!mounted.current || epoch !== loadEpoch.current) return;
      setStatus(next);
      if (updateDraft) { setEnabled(next.settings.enabled); setPort(next.settings.port); }
      setError(null);
    } catch (failure) {
      if (mounted.current && epoch === loadEpoch.current) setError(failure instanceof Error ? failure.message : t("读取 API 状态失败，请重试。"));
    }
  }, []);
  useEffect(() => {
    mounted.current = true;
    queueMicrotask(() => {
      if (isDesktop && mounted.current) void load(true);
      const saved = api.getDaemonConnection();
      const connectionEpoch = connectionDraftEpoch.current;
      if (saved.url && saved.token) void api.verifyDaemonConnection(saved.url, saved.token)
        .then(() => { if (mounted.current && connectionEpoch === connectionDraftEpoch.current) setDaemonActive(true); })
        .catch(() => { if (mounted.current && connectionEpoch === connectionDraftEpoch.current) { setDaemonActive(false); setError(t('守护进程连接未验证，请核对地址和密钥后重新连接。')); } });
    });
    return () => { mounted.current = false; loadEpoch.current += 1; connectionDraftEpoch.current += 1; };
  }, [load]);

  const work = async (action: () => Promise<void>) => {
    if (busyRef.current) return;
    loadEpoch.current += 1;
    busyRef.current = true; setBusy(true); setError(null);
    try {
      await run(async () => {
        try { await action(); }
        catch (failure) { if (mounted.current) setError(failure instanceof Error ? failure.message : t("操作未完成，请重试。")); throw failure; }
      });
    } catch (failure) {
      if (mounted.current) setError(failure instanceof Error ? failure.message : t("操作未完成，请重试。"));
    } finally { busyRef.current = false; if (mounted.current) setBusy(false); }
  };
  const save = () => work(async () => {
    if (port === null || !Number.isInteger(port) || port < 1 || port > 65535) return;
    try {
      const next = await api.apiConfigure({ enabled, port });
      if (mounted.current) { setStatus(next); setEnabled(next.settings.enabled); setPort(next.settings.port); }
    } finally { if (mounted.current) await load(); }
  });
  const pick = (write: boolean) => work(async () => {
    const selected = await api.pickDirectory();
    if (selected && mounted.current) setDraft(previous => {
      const field = write ? 'writeRoots' : 'readRoots';
      return { ...previous, [field]: Array.from(new Set([...previous[field], selected])) };
    });
  });
  const create = () => work(async () => {
    const created = await api.apiCreateClient({ ...draft, name: draft.name.trim() });
    if (mounted.current) { setCreating(false); setToken(created.token); setDraft(emptyClient()); }
    if (mounted.current) await load();
  });
  const revoke = (id: string) => work(async () => { await api.apiRevokeClient(id); if (mounted.current) await load(); });
  const permissions = status?.permissions ?? [];
  const labels = new Map(permissions.map(permission => [permission.scope, t(permission.label)]));
  const sensitive = permissions.filter(permission => permission.sensitive && draft.scopes.includes(permission.scope));
  const nameValid = !!draft.name.trim() && new TextEncoder().encode(draft.name.trim()).length <= 128
    && !Array.from(draft.name).some(character => character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127);
  const portValid = port !== null && Number.isInteger(port) && port >= 1 && port <= 65535;

  return <Space orientation="vertical" size="middle" style={{ width: '100%' }}>
    <Alert type="info" showIcon title={t("让本机其他软件调用 OpenUnpack")} description={t("使用 HTTP/JSON 调用现有解压、密码库、历史和工具功能。程序运行期间提供服务，仅接受本机连接。")} />
    {error && !creating && <Alert type="error" showIcon title={t("API 操作未完成")} description={error} />}
    {status?.error && <Alert type="warning" showIcon title={t("API 服务未就绪")} description={status.error} />}
    <Card size="small" title={<Space>{t("远程 / 本地守护进程连接")}<Tag color={daemonActive ? 'processing' : 'default'}>{daemonActive ? t("已连接") : t("未连接")}</Tag></Space>}>
      <Text type="secondary">{t("桌面版连接本机 HTTP 服务；远端服务请直接打开其同源 HTTPS 控制台。")}</Text>
      <Form layout="vertical" style={{ marginTop: 12 }}>
        <Space size="large" wrap align="start">
          <Form.Item label={t("守护进程地址")} htmlFor="daemon-url">
            <Input
              style={{ width: 260 }}
              id="daemon-url"
              placeholder="http://127.0.0.1:47980"
              value={daemonUrl}
              onChange={e => { connectionDraftEpoch.current += 1; setDaemonUrl(e.target.value); setDaemonToken(''); setDaemonActive(false); }}
              disabled={busy}
            />
          </Form.Item>
          <Form.Item label={t("API 密钥 (Token)")} htmlFor="daemon-token">
            <Input.Password
              style={{ width: 240 }}
              id="daemon-token"
              autoComplete="off"
              placeholder={t("请填写 API 应用密钥") }
              value={daemonToken}
              onChange={e => { connectionDraftEpoch.current += 1; setDaemonToken(e.target.value); setDaemonActive(false); }}
              disabled={busy}
            />
          </Form.Item>
          <Form.Item label=" ">
            <Space>
              <Button
                type="primary"
                disabled={!daemonUrl.trim() || busy}
                loading={busy}
                onClick={() => void work(async () => {
                  const epoch = connectionDraftEpoch.current;
                  const candidate = await api.verifyDaemonConnection(daemonUrl, daemonToken);
                  if (!mounted.current || epoch !== connectionDraftEpoch.current) return;
                  api.setDaemonConnection(candidate.url, candidate.token);
                  setDaemonActive(true);
                  window.location.reload();
                })}
              >
                {t("连接守护进程")}
              </Button>
              {(daemonActive || api.getDaemonConnection().url) && (
                <Button
                  onClick={() => {
                    connectionDraftEpoch.current += 1;
                    api.setDaemonConnection(null, null);
                    setDaemonUrl('');
                    setDaemonToken('');
                    setDaemonActive(false);
                    window.location.reload();
                  }}
                >
                  {isDesktop ? t("切回内置引擎") : t("断开守护进程")}
                </Button>
              )}
            </Space>
          </Form.Item>
        </Space>
      </Form>
    </Card>
    <Card size="small" title={<Space>{t("本机连接")}<Tag color={status?.running ? 'success' : 'default'}>{status?.running ? t("运行中") : t("未运行")}</Tag></Space>}>
      <Form layout="vertical">
        <Space size="large" wrap align="start">
          <Form.Item label={t("启用本机 API")}><Switch aria-label={t("启用本机 API")} checked={enabled} disabled={!isDesktop || !status || busy} onChange={setEnabled} /></Form.Item>
          <Form.Item label={t("端口")} validateStatus={!portValid ? 'error' : undefined} help={!portValid ? t("请输入 1 至 65535 的整数。") : undefined}><InputNumber aria-label={t("本机 API 端口")} min={1} max={65535} precision={0} value={port} disabled={!isDesktop || !status || busy} onChange={setPort} /></Form.Item>
          <Form.Item label={t("连接设置")}><Button type="primary" disabled={!isDesktop || !status || busy || !portValid} loading={busy} onClick={() => void save()}>{t("保存连接设置")}</Button></Form.Item>
        </Space>
        {status?.baseUrl && <Form.Item label={t("调用地址")}><Text code>{status.baseUrl}</Text></Form.Item>}
        <Space wrap><Button disabled={!isDesktop || !status?.running || busy} onClick={() => void work(() => api.apiOpenDocs())}>{t("打开接口文档")}</Button><Button disabled={!isDesktop || busy} onClick={() => void load()}>{t("刷新连接状态")}</Button></Space>
      </Form>
    </Card>
    <Card size="small" title={t("已授权的第三方应用")} extra={<Button disabled={!isDesktop || !status || busy || !!token} type="primary" onClick={() => { setError(null); setDraft(emptyClient()); setCreating(true); }}>{t("添加应用")}</Button>}>
      <Text type="secondary">{t("每个应用使用独立密钥。撤销授权会拒绝后续请求，现有解压任务仍按任务状态继续处理。")}</Text>
      <Table<ApiClientView> style={{ marginTop: 12 }} rowKey="id" size="small" dataSource={status?.clients ?? []} pagination={false} locale={{ emptyText: t("尚未授权任何第三方应用") }} columns={[
        { title: t("应用名称"), dataIndex: 'name' },
        { title: t("允许的操作"), key: 'scopes', render: (_, client) => <Space wrap size={4}>{client.scopes.map(scope => <Tag key={scope}>{labels.get(scope) ?? scope}</Tag>)}</Space> },
        { title: t("批准的位置"), key: 'roots', render: (_, client) => <Space orientation="vertical" size={0}>{client.readRoots.map(path => <Text key={`r-${path}`} style={{ wordBreak: 'break-all' }}>{t("可读：")}{path}</Text>)}{client.writeRoots.map(path => <Text key={`w-${path}`} style={{ wordBreak: 'break-all' }}>{t("可写：")}{path}</Text>)}{!client.readRoots.length && !client.writeRoots.length && <Text type="secondary">{t("未批准文件位置")}</Text>}</Space> },
        { title: t("操作"), key: 'actions', render: (_, client) => <Popconfirm title={t("撤销“{value0}”的访问权限？", { value0: client.name })} description={t("此密钥将无法继续调用；任务和密码不会因此删除。")} onConfirm={() => { void revoke(client.id); }}><Button danger size="small" disabled={busy}>{t("撤销授权")}</Button></Popconfirm> },
      ]} />
    </Card>
    <Modal title={t("添加第三方应用")} open={creating} onCancel={() => { if (!busyRef.current) { setCreating(false); setDraft(emptyClient()); setError(null); } }} mask={{ closable: !busy }} closable={!busy} destroyOnHidden confirmLoading={busy} okText={t("生成密钥")} cancelText={t("取消")} okButtonProps={{ disabled: !nameValid || !draft.scopes.length || !isDesktop || busy }} onOk={() => void create()}>
      <Form layout="vertical">
        {error && <Alert type="error" showIcon title={t("应用授权未完成")} description={error} style={{ marginBottom: 16 }} />}
        <Form.Item label={t("应用名称")} validateStatus={draft.name && !nameValid ? 'error' : undefined} help={draft.name && !nameValid ? t("名称过长或含有不可用字符，请修改。") : undefined}><Input aria-label={t("API 应用名称")} maxLength={128} value={draft.name} disabled={busy} onChange={event => setDraft(previous => ({ ...previous, name: event.target.value }))} placeholder={t("例如：下载管理器、自动整理脚本")} /></Form.Item>
        <Form.Item label={t("允许的操作")}>
          <Space wrap style={{ marginBottom: 8 }}><Button size="small" disabled={busy} onClick={() => setDraft(previous => ({ ...previous, scopes: ['jobs.read'] }))}>{t("只读任务")}</Button><Button size="small" disabled={busy} onClick={() => setDraft(previous => ({ ...previous, scopes: [...automation] }))}>{t("解压自动化")}</Button><Button size="small" disabled={busy} onClick={() => setDraft(previous => ({ ...previous, scopes: permissions.map(permission => permission.scope) }))}>{t("全部功能")}</Button></Space>
          <Checkbox.Group aria-label={t("API 应用权限")} disabled={busy} value={draft.scopes} onChange={values => setDraft(previous => ({ ...previous, scopes: values }))} options={permissions.map(permission => ({ value: permission.scope, label: t(permission.label) }))} style={{ display: 'flex', flexDirection: 'column', gap: 6 }} />
        </Form.Item>
        {!!sensitive.length && <Alert type="warning" showIcon title={t("包含敏感操作")} description={sensitive.map(permission => t(permission.label)).join('、')} style={{ marginBottom: 16 }} />}
        <Form.Item label={t("批准使用的文件位置")} extra={t("应用只能在批准的位置读取或修改文件。未批准位置时仍可使用其获准的任务和密码库功能。")}>
          <Space wrap><Button disabled={busy} onClick={() => void pick(false)}>{t("批准可读目录")}</Button><Button disabled={busy} onClick={() => void pick(true)}>{t("批准可写目录")}</Button></Space>
          {(['readRoots', 'writeRoots'] as const).map(field => draft[field].map(path => <div key={`${field}-${path}`} style={{ marginTop: 6, wordBreak: 'break-all' }}><Text>{field === 'readRoots' ? t("可读") : t("可写")}：{path}</Text><Button type="link" size="small" disabled={busy} onClick={() => setDraft(previous => ({ ...previous, [field]: previous[field].filter(value => value !== path) }))}>{t("移除")}</Button></div>))}
        </Form.Item>
      </Form>
    </Modal>
    <Modal title={t("保存应用密钥")} open={!!token} onCancel={() => setToken('')} onOk={() => setToken('')} mask={{ closable: false }} okText={t("已保存，关闭")} cancelButtonProps={{ style: { display: 'none' } }} destroyOnHidden>
      <Space orientation="vertical" style={{ width: '100%' }}><Alert type="warning" showIcon title={t("密钥只显示这一次")} description={t("请复制保存，并交给对应应用使用。关闭后无法再次查看；丢失时可以撤销授权并重新添加应用。")} /><Input.Password aria-label={t("新 API 应用密钥")} readOnly autoComplete="off" value={token} /><Text type="secondary">{t("调用时使用 Authorization: Bearer 密钥。接口文档包含完整参数及调试入口。")}</Text></Space>
    </Modal>
  </Space>;
}
