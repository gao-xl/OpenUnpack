import {
  Alert,
  Button,
  Empty,
  Form,
  InputNumber,
  List,
  Select,
  Space,
  Switch,
  Tag,
  Typography,
} from 'antd';
import {
  AppstoreOutlined,
  BellOutlined,
  DeleteOutlined,
  ExportOutlined,
  EyeOutlined,
  FolderAddOutlined,
  FolderOutlined,
  ImportOutlined,
} from '@ant-design/icons';
import { t } from '../i18n';
import { api } from '../api';
import { displayPath } from './shared';
import SourceCleanupSelect, { SourceCleanupScope } from './SourceCleanupSelect';
import ApiSettingsPanel from './ApiSettingsPanel';
import type { SettingsTabContext } from './SettingsTabsBasic';

const { Text } = Typography;

export const SETTING_TAB_KEYWORDS: Record<string, string[]> = {
  output: [
    '输出',
    '目录',
    '路径',
    '同名',
    '冲突',
    '去套娃',
    '空目录',
    'output',
    'path',
    'conflict',
    'unwrap',
  ],
  processing: [
    '策略',
    '嵌套',
    '内包',
    '层数',
    '密码',
    '校验',
    '自动',
    '损坏',
    '抢救',
    '乱码',
    '编码',
    'gbk',
    'utf8',
    'crc',
  ],
  performance: [
    '性能',
    '缓存',
    'ssd',
    '并发',
    '线程',
    '优先级',
    '配额',
    '空间',
    'cpu',
    'cache',
    'thread',
    'concurrency',
  ],
  exclusion: ['筛选', '排除', '规则', '过滤', '子目录', '通配符', 'filter', 'exclude'],
  watch: ['监听', '下载', '自动入队', '文件夹', 'watch', 'folder'],
  completion: [
    '完成',
    '源包',
    '删除',
    '回收站',
    '关机',
    '休眠',
    '声音',
    '提示音',
    'cleanup',
    'delete',
    'sound',
    'shutdown',
  ],
  ui: [
    '界面',
    '外观',
    '皮肤',
    '主题',
    '暗色',
    '深色',
    '语言',
    '置顶',
    '托盘',
    '备份',
    '导出',
    '恢复',
    '右键',
    '菜单',
    'theme',
    'skin',
    'backup',
  ],
  api: ['api', 'token', '密钥', '端口', '客户端', '自动化', 'axum', 'http'],
};

export function useSystemSettingsTabs(ctx: SettingsTabContext) {
  const {
    snapshot,
    draft,
    connected,
    nativeEngine,
    draftSkin,
    presets,
    language,
    shellMenuEnabled,
    shellMenuBusy,
    update,
    handleSkinChange,
    setLanguage,
    setShellMenuEnabled,
    setShellMenuBusy,
    startExport,
    startRestore,
    run,
    message,
  } = ctx;
  return [
    {
      key: 'watch',
      label: (
        <span>
          <EyeOutlined /> {t('自动监听')}
        </span>
      ),
      children: (
        <Form layout="vertical">
          <Form.Item
            label={t('自动监听目录（下载与自动入队）')}
            help={t(
              '监听指定文件夹（如下载目录），当压缩包下载落盘且写入完成时，自动添加至待解压队列并匹配密码',
            )}
          >
            <div style={{ marginBottom: 12 }}>
              <Button
                type="dashed"
                icon={<FolderAddOutlined />}
                disabled={!nativeEngine}
                onClick={() => {
                  void (async () => {
                    await run(async () => {
                      const dir = await api.pickDirectory();
                      if (dir) {
                        const current = draft.watchedFolders || [];
                        if (!current.includes(dir)) {
                          update('watchedFolders', [...current, dir]);
                        }
                      }
                    });
                  })();
                }}
              >
                {t('添加监听文件夹')}
              </Button>
            </div>
            {(draft.watchedFolders || []).length === 0 ? (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={t('暂未添加监听文件夹。添加后即可在后台自动监听新下载的压缩包。')}
              />
            ) : (
              <List
                size="small"
                bordered
                dataSource={draft.watchedFolders || []}
                renderItem={(folder) => (
                  <List.Item
                    actions={[
                      <Button
                        key="delete"
                        disabled={!nativeEngine}
                        type="text"
                        danger
                        icon={<DeleteOutlined />}
                        onClick={() => {
                          update(
                            'watchedFolders',
                            (draft.watchedFolders || []).filter((f) => f !== folder),
                          );
                        }}
                      >
                        {t('移除')}
                      </Button>,
                    ]}
                  >
                    <Space>
                      <FolderOutlined style={{ color: 'var(--ou-info)' }} />
                      <Text code style={{ wordBreak: 'break-all' }}>
                        {displayPath(folder)}
                      </Text>
                    </Space>
                  </List.Item>
                )}
              />
            )}
          </Form.Item>
          <Alert
            title={t('防抢读稳定机制说明')}
            description={t(
              '针对浏览器（如 Chrome/Edge）、下载工具（IDM 等）分块下载特性，系统采用排他读锁探测与尺寸稳定窗口判定，仅在文件完全落盘并释放写入锁后才会自动入队，确保解压完整可靠。',
            )}
            type="info"
            showIcon
            style={{ marginTop: 16 }}
          />
        </Form>
      ),
    },
    {
      key: 'completion',
      label: (
        <span>
          <BellOutlined /> {t('完成后动作')}
        </span>
      ),
      children: (
        <Form layout="vertical">
          <Form.Item
            label={t('源压缩包处理')}
            help={
              <SourceCleanupScope value={draft.sourceCleanup} expandNested={draft.expandNested} />
            }
          >
            <SourceCleanupSelect
              value={draft.sourceCleanup}
              onChange={(v) => update('sourceCleanup', v)}
              disabled={!connected}
            />
          </Form.Item>
          <Alert
            title={t('安全保护规则')}
            description={t(
              '默认保留原始压缩包。选择清理时，仅在解压完全成功且校验通过后才会处理源文件；解压未完成或失败时始终保留。',
            )}
            type="info"
            showIcon
            style={{ marginBottom: 16 }}
          />
          <Form.Item label={t('任务完成音效')}>
            <Space>
              <Switch
                aria-label={t('处理结束提示音')}
                checked={draft.completionSound}
                onChange={(v) => update('completionSound', v)}
              />
              <span>{t('任务交付后播放系统提示音')}</span>
            </Space>
          </Form.Item>
          <Form.Item
            label={t('全部队列完成后动作')}
            help={t(
              '当整个任务队列全部处理完成且无人值守时执行的后续动作；休眠和关机将提前展示 30 秒带音效的倒计时窗口，可随时取消。',
            )}
          >
            <Select
              aria-label={t('全部队列完成后动作')}
              disabled={!nativeEngine}
              value={draft.postExtractionAction || 'none'}
              onChange={(v) => update('postExtractionAction', v)}
              style={{ width: 300 }}
              options={[
                { value: 'none', label: t('不执行任何操作（默认）') },
                { value: 'openOutput', label: t('打开最终解压输出目录') },
                { value: 'sleep', label: t('进入系统睡眠/休眠（带30秒倒计时）') },
                { value: 'shutdown', label: t('自动关闭计算机（带30秒倒计时）') },
              ]}
            />
          </Form.Item>
        </Form>
      ),
    },
    {
      key: 'localapi',
      label: (
        <span>
          <AppstoreOutlined /> {t('本机 API')}
        </span>
      ),
      destroyOnHidden: true,
      children: <ApiSettingsPanel run={run} />,
    },
    {
      key: 'interfacebackup',
      label: (
        <span>
          <AppstoreOutlined /> {t('界面与备份')}
        </span>
      ),
      children: (
        <Form layout="vertical">
          <Space wrap size="large">
            <Form.Item label={t('界面语言')}>
              <Select
                aria-label={t('界面语言')}
                value={language}
                onChange={setLanguage}
                style={{ width: 170 }}
                options={[
                  { value: 'zh-CN', label: '简体中文' },
                  { value: 'en-US', label: 'English' },
                  { value: 'ja-JP', label: '日本語' },
                ]}
              />
            </Form.Item>
            <Form.Item label={t('界面主题')}>
              <Select
                aria-label={t('界面主题')}
                value={draft.theme}
                onChange={(v) => {
                  update('theme', v);
                  window.dispatchEvent(
                    new CustomEvent('openunpack:theme-preview', { detail: { theme: v } }),
                  );
                }}
                style={{ width: 130 }}
                options={[
                  { value: 'light', label: t('浅色') },
                  { value: 'dark', label: t('深色') },
                  { value: 'system', label: t('跟随系统') },
                ]}
              />
            </Form.Item>
            <Form.Item label={t('界面皮肤')}>
              <Select
                aria-label={t('界面皮肤')}
                value={draftSkin}
                onChange={handleSkinChange}
                style={{ width: 170 }}
                options={presets.map((p) => ({ value: p.id, label: p.name }))}
              />
            </Form.Item>
            <Form.Item label={t('历史记录上限')}>
              <InputNumber
                aria-label={t('历史记录上限')}
                min={50}
                max={5000}
                value={draft.historyLimit}
                onChange={(v) => update('historyLimit', v ?? 500)}
              />
            </Form.Item>
            <Form.Item label={t('窗口置顶')}>
              <Switch
                aria-label={t('窗口置顶')}
                checked={draft.alwaysOnTop}
                onChange={(v) => update('alwaysOnTop', v)}
              />
            </Form.Item>
            <Form.Item label={t('最小化到托盘')}>
              <Switch
                aria-label={t('最小化到托盘')}
                checked={draft.minimizeToTray}
                onChange={(v) => update('minimizeToTray', v)}
              />
            </Form.Item>
          </Space>
          <div style={{ marginBottom: 18 }}>
            <div
              style={{
                fontSize: 13,
                fontWeight: 600,
                color: 'var(--ou-text-title)',
                marginBottom: 8,
              }}
            >
              {t('皮肤预设与实时预览')}
            </div>
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(210px, 1fr))',
                gap: 10,
              }}
            >
              {presets.map((p) => {
                const isSelected = draftSkin === p.id;
                return (
                  <div
                    key={p.id}
                    role="button"
                    tabIndex={0}
                    onClick={() => handleSkinChange(p.id)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        handleSkinChange(p.id);
                      }
                    }}
                    aria-label={`${p.name} ${isSelected ? t('(已选)') : ''}`}
                    style={{
                      padding: '12px 14px',
                      borderRadius: p.preview.borderRadius,
                      border: isSelected
                        ? '2px solid var(--ou-primary)'
                        : '1px solid var(--ou-border-color)',
                      background: isSelected ? 'var(--ou-primary-bg)' : 'var(--ou-bg-card)',
                      cursor: 'pointer',
                      position: 'relative',
                      display: 'flex',
                      flexDirection: 'column',
                      gap: 6,
                      boxShadow: isSelected ? 'var(--ou-shadow-card)' : 'none',
                      transition: 'all 0.15s ease',
                    }}
                  >
                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'space-between',
                      }}
                    >
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <span
                          style={{
                            width: 12,
                            height: 12,
                            borderRadius: p.preview.borderRadius === 0 ? 0 : '50%',
                            background: p.preview.accentColor,
                            display: 'inline-block',
                            flexShrink: 0,
                          }}
                        />
                        <span
                          style={{ fontWeight: 600, fontSize: 13, color: 'var(--ou-text-title)' }}
                        >
                          {p.name}
                        </span>
                      </div>
                      {isSelected && (
                        <Tag color="processing" style={{ margin: 0, fontSize: 10 }}>
                          {t('生效中')}
                        </Tag>
                      )}
                    </div>
                    <div style={{ fontSize: 11, color: 'var(--ou-text-muted)', lineHeight: 1.4 }}>
                      {p.description}
                    </div>
                    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 4 }}>
                      {p.tags.map((tag) => (
                        <Tag
                          key={tag}
                          style={{
                            fontSize: 10,
                            margin: 0,
                            padding: '0 4px',
                            borderRadius: p.preview.borderRadius,
                          }}
                        >
                          {tag}
                        </Tag>
                      ))}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
          <Alert
            style={{ marginBottom: 16 }}
            title={t('安全存储')}
            description={t('本地密码库由当前 Windows 账户凭据自动加密保护。')}
            type="success"
          />
          <Form.Item label={t('本机数据目录')}>
            <Space wrap>
              <Text code style={{ wordBreak: 'break-all' }}>
                {snapshot.dataDirectory || t('桌面版启动后显示')}
              </Text>
              <Button
                disabled={!nativeEngine || !snapshot.dataDirectory}
                onClick={() => void run(() => api.openPath(snapshot.dataDirectory))}
              >
                {t('打开数据目录')}
              </Button>
            </Space>
          </Form.Item>
          <Form.Item label={t('解压引擎')}>
            <Text type={snapshot.engineAvailable ? 'success' : 'warning'}>
              {snapshot.engineAvailable
                ? t('本地引擎和作业进程已就绪')
                : t('尚未就绪，请检查安装包中的 worker 与 7-Zip 文件')}
            </Text>
          </Form.Item>
          <Form.Item
            label={t('资源管理器右键菜单集成')}
            help={t('在 Windows 资源管理器右键点击压缩包时显示“使用 OpenUnpack 解压”，一键入队')}
          >
            <Space>
              <Switch
                aria-label={t('资源管理器右键菜单集成')}
                checked={shellMenuEnabled}
                loading={shellMenuBusy}
                disabled={!nativeEngine}
                onChange={(checked) => {
                  void (async () => {
                    setShellMenuBusy(true);
                    try {
                      const res = await api.setShellContextMenuStatus(checked);
                      setShellMenuEnabled(res);
                      message.success(
                        checked ? t('已添加资源管理器右键菜单') : t('已移除资源管理器右键菜单'),
                      );
                    } catch (err: unknown) {
                      message.error(String(err));
                    } finally {
                      setShellMenuBusy(false);
                    }
                  })();
                }}
              />
              <span>{shellMenuEnabled ? t('已启用右键快捷解压') : t('未启用')}</span>
            </Space>
          </Form.Item>
          <Form.Item label={t('密码库备份与恢复')}>
            <Space>
              <Button
                aria-label={t('导出加密备份')}
                icon={<ExportOutlined aria-hidden />}
                disabled={!nativeEngine}
                onClick={() => void startExport()}
              >
                {t('导出加密备份')}
              </Button>
              <Button
                aria-label={t('恢复备份并合并')}
                icon={<ImportOutlined aria-hidden />}
                disabled={!nativeEngine}
                onClick={() => void startRestore()}
              >
                {t('恢复备份并合并')}
              </Button>
            </Space>
          </Form.Item>
        </Form>
      ),
    },
  ];
}
