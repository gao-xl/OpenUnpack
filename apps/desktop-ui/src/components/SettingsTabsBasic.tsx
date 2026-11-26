import {
  Alert,
  Button,
  Form,
  Input,
  InputNumber,
  Radio,
  Select,
  Slider,
  Space,
  Switch,
} from 'antd';
import {
  ClearOutlined,
  FilterOutlined,
  FolderOpenOutlined,
  SafetyCertificateOutlined,
  SettingOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons';
import { t } from '../i18n';
import type { AppSnapshot, Settings } from '../types';
import { MAX_CONCURRENT_JOBS } from '../types';
import type { SkinId, SkinPreset } from '../theme';
import { displayPath } from './shared';
import RulePreview from './RulePreview';

const { TextArea } = Input;
const GB = 1024 ** 3;

export interface SettingsTabContext {
  snapshot: AppSnapshot;
  draft: Settings;
  daemon: boolean;
  connected: boolean;
  nativeEngine: boolean;
  passwordGroups: string[];
  inputExclusionsText: string;
  outputExclusionsText: string;
  draftSkin: SkinId;
  presets: readonly SkinPreset[];
  language: 'zh-CN' | 'en-US' | 'ja-JP';
  shellMenuEnabled: boolean;
  shellMenuBusy: boolean;
  update: <K extends keyof Settings>(key: K, val: Settings[K]) => void;
  setInputExclusionsText: (val: string) => void;
  setOutputExclusionsText: (val: string) => void;
  handlePickOutput: () => Promise<void>;
  handlePickCache: () => Promise<void>;
  handleSkinChange: (nextSkin: SkinId) => void;
  setLanguage: (lang: 'zh-CN' | 'en-US' | 'ja-JP') => void;
  setShellMenuEnabled: (val: boolean) => void;
  setShellMenuBusy: (val: boolean) => void;
  startExport: () => Promise<void>;
  startRestore: () => Promise<void>;
  run: (work: () => Promise<unknown>) => Promise<void>;
  message: { success: (msg: string) => void; error: (msg: string) => void };
}

export function useBasicSettingsTabs(ctx: SettingsTabContext) {
  const {
    draft,
    daemon,
    nativeEngine,
    passwordGroups,
    inputExclusionsText,
    outputExclusionsText,
    update,
    setInputExclusionsText,
    setOutputExclusionsText,
    handlePickOutput,
    handlePickCache,
    run,
  } = ctx;
  return [
    {
      key: 'output',
      label: (
        <span>
          <SettingOutlined /> {t('输出管理')}
        </span>
      ),
      children: (
        <Form layout="vertical">
          <Form.Item
            label={t('输出根目录')}
            help={t('留空时在原归档旁创建独立结果目录，避免文件散落；现有文件不会覆盖。')}
          >
            <Space.Compact style={{ width: '100%' }}>
              <Input
                aria-label={t('默认输出根目录')}
                value={displayPath(draft.outputRoot)}
                readOnly={!daemon}
                onChange={(event) => update('outputRoot', event.target.value)}
                placeholder={t('原文件所在目录（默认）')}
              />
              <Button
                icon={<FolderOpenOutlined />}
                disabled={!nativeEngine}
                onClick={() => void handlePickOutput()}
              >
                {t('选择目录')}
              </Button>
              <Button icon={<ClearOutlined />} onClick={() => update('outputRoot', '')}>
                {t('清空')}
              </Button>
            </Space.Compact>
          </Form.Item>
          <Form.Item label={t('同名冲突策略')}>
            <Select
              aria-label={t('同名冲突策略')}
              value={draft.conflictPolicy}
              onChange={(v) => update('conflictPolicy', v)}
              options={[
                { value: 'rename', label: t('自动重命名') },
                { value: 'skip', label: t('跳过') },
              ]}
            />
          </Form.Item>
          <Form.Item label={t('智能去套娃（目录精简）')}>
            <Space>
              <Switch
                aria-label={t('整理唯一顶层目录')}
                checked={draft.unwrapSingleRoot}
                onChange={(v) => update('unwrapSingleRoot', v)}
              />
              <span>{t('当解压结果顶层仅含唯一一个文件夹时，自动剥离多余的顶层外壳目录')}</span>
            </Space>
          </Form.Item>
          <Form.Item label={t('清理空目录')}>
            <Space>
              <Switch
                aria-label={t('清理空目录')}
                checked={draft.cleanEmptyDirs}
                onChange={(v) => update('cleanEmptyDirs', v)}
              />
              <span>{t('解压完成后自动清理结果中的各级空目录')}</span>
            </Space>
          </Form.Item>
          <Alert
            title={t('保护说明')}
            description={t(
              '避免解压出双层同名嵌套目录，同时防止无根目录的散落文件污染上级文件夹。',
            )}
            type="info"
            showIcon
          />
        </Form>
      ),
    },
    {
      key: 'processing',
      label: (
        <span>
          <SafetyCertificateOutlined /> {t('处理策略')}
        </span>
      ),
      children: (
        <Form layout="vertical">
          <Form.Item label={t('嵌套解压')}>
            <Space>
              <Switch
                aria-label={t('自动展开内包')}
                checked={draft.expandNested}
                onChange={(v) => update('expandNested', v)}
              />
              <span>{t('展开内层嵌套压缩包')}</span>
              {draft.expandNested && (
                <Space style={{ marginLeft: 16 }}>
                  <span>{t('总展开层数（含外包）：')}</span>
                  <InputNumber
                    aria-label={t('总展开层数')}
                    min={1}
                    max={8}
                    value={draft.maxDepth}
                    onChange={(v) => update('maxDepth', v ?? 2)}
                  />
                </Space>
              )}
            </Space>
          </Form.Item>
          <Form.Item label={t('优先密码分组')}>
            <Select
              aria-label={t('默认密码分组')}
              allowClear
              placeholder={t('全部可用密码')}
              value={draft.passwordGroup || ''}
              onChange={(v) => update('passwordGroup', v || '')}
              options={[
                { value: '', label: t('全部启用密码') },
                ...passwordGroups.map((g) => ({ value: g, label: g })),
              ]}
            />
          </Form.Item>
          <Form.Item label={t('校验级别')}>
            <Radio.Group
              aria-label={t('写回验证模式')}
              value={draft.verification}
              onChange={(e) => update('verification', e.target.value as string)}
            >
              <Radio value="standard">{t('标准（流式摘要+引擎完整性）')}</Radio>
              <Radio value="strict">{t('完整校验（回读目标目录，更耗时）')}</Radio>
            </Radio.Group>
          </Form.Item>
          <Form.Item label={t('添加后自动解压')}>
            <Space>
              <Switch
                aria-label={t('添加后自动解压')}
                checked={draft.autoStartOnAdd}
                onChange={(v) => update('autoStartOnAdd', v)}
              />
              <span>{t('添加文件或确认批次后自动开始队列，无需手动点击“开始队列”')}</span>
            </Space>
          </Form.Item>
          <Form.Item label={t('损坏包抢救模式')}>
            <Space>
              <Switch
                aria-label={t('损坏包抢救提取')}
                checked={draft.salvageCorrupted}
                onChange={(v) => update('salvageCorrupted', v)}
              />
              <span>{t('遇到 CRC 错误或数据损坏时尽可能提取完好文件，严禁删除源压缩包')}</span>
            </Space>
          </Form.Item>
          <Form.Item label={t('文件名乱码修复')}>
            <Space>
              <Switch
                aria-label={t('自动识别并修复文件名乱码')}
                checked={draft.autoFixEncoding}
                onChange={(v) => update('autoFixEncoding', v)}
              />
              <span>
                {t('针对传统非 UTF-8 编码的 ZIP 归档（GBK/Shift-JIS/Big5）自动推断并修复乱码')}
              </span>
            </Space>
          </Form.Item>
        </Form>
      ),
    },
    {
      key: 'performance',
      label: (
        <span>
          <ThunderboltOutlined /> {t('性能与缓存')}
        </span>
      ),
      children: (
        <Form layout="vertical">
          <Form.Item label={t('缓存开关')}>
            <Space size="large">
              <Space>
                <Switch
                  aria-label={t('启用缓存工作目录')}
                  checked={draft.cacheEnabled}
                  onChange={(v) => update('cacheEnabled', v)}
                />
                <span>{t('启用中转缓存')}</span>
              </Space>
              <Space>
                <Switch
                  aria-label={t('缓存输入源包')}
                  disabled={!draft.cacheEnabled}
                  checked={draft.cacheInput}
                  onChange={(v) => update('cacheInput', v)}
                />
                <span>{t('使用中转缓存时复制输入')}</span>
              </Space>
              <Space>
                <Switch
                  aria-label={t('小文件优先处理')}
                  checked={draft.smallFirst}
                  onChange={(v) => update('smallFirst', v)}
                />
                <span>{t('小文件优先处理')}</span>
              </Space>
            </Space>
          </Form.Item>
          <Form.Item
            label={t('缓存选择策略')}
            help={t('关闭缓存即直接解压；自动模式在缺少收益证据时优先直写。中转缓存不保证更快。')}
          >
            <Select
              aria-label={t('缓存选择策略')}
              disabled={!draft.cacheEnabled}
              value={draft.cachePolicy}
              onChange={(value) => update('cachePolicy', value)}
              options={[
                { value: 'auto', label: t('自动选择（推荐）') },
                { value: 'always', label: t('总是使用中转缓存') },
              ]}
              style={{ width: 280 }}
            />
          </Form.Item>
          <Form.Item
            label={t('并发解压任务数')}
            help={t('同时最多 {value0} 个任务，默认 1；机械硬盘建议设为 1，SSD 可按实际负载增加', {
              value0: MAX_CONCURRENT_JOBS,
            })}
          >
            <Space size="large" align="center" style={{ width: '100%', maxWidth: 380 }}>
              <Slider
                aria-label={t('并发解压任务数')}
                min={1}
                max={MAX_CONCURRENT_JOBS}
                marks={{ 1: '1', 2: '2', 3: '3', 4: '4', 5: '5' }}
                step={1}
                value={draft.concurrency ?? 1}
                onChange={(v) => update('concurrency', v)}
                style={{ flex: 1, minWidth: 200 }}
              />
              <InputNumber
                aria-label={t('并发解压任务数数值')}
                min={1}
                max={MAX_CONCURRENT_JOBS}
                value={draft.concurrency ?? 1}
                onChange={(v) =>
                  update('concurrency', Math.min(MAX_CONCURRENT_JOBS, Math.max(1, v ?? 1)))
                }
              />
            </Space>
          </Form.Item>
          <Form.Item
            label={t('引擎工作线程数')}
            help={t(
              '7-Zip 解压线程数，0 表示按并发任务数分配核心（每任务至少 1 个）；手动设置的线程数保持不变',
            )}
          >
            <Space size="large" align="center" style={{ width: '100%', maxWidth: 380 }}>
              <Slider
                aria-label={t('引擎工作线程数')}
                min={0}
                max={32}
                marks={{ 0: t('自动'), 4: '4', 8: '8', 16: '16', 32: '32' }}
                step={1}
                value={draft.engineThreads ?? 0}
                onChange={(v) => update('engineThreads', v)}
                style={{ flex: 1, minWidth: 200 }}
              />
              <InputNumber
                aria-label={t('引擎工作线程数数值')}
                min={0}
                max={64}
                value={draft.engineThreads ?? 0}
                onChange={(v) => update('engineThreads', Math.min(64, Math.max(0, v ?? 0)))}
              />
            </Space>
          </Form.Item>
          <Form.Item
            label={t('工作进程优先级')}
            help={t('调整后台解压进程的 CPU 抢占优先级，避免系统卡顿')}
          >
            <Select
              aria-label={t('工作进程优先级')}
              value={draft.workerPriority || 'belowNormal'}
              onChange={(v) => update('workerPriority', v)}
              style={{ width: 260 }}
              options={[
                { value: 'belowNormal', label: t('低于正常（推荐，解压不卡前台）') },
                { value: 'idle', label: t('空闲（后台静默解压，资源最低消耗）') },
                { value: 'normal', label: t('正常（与其他应用同级竞争 CPU）') },
              ]}
            />
          </Form.Item>
          <Form.Item
            label={t('SSD 工作目录')}
            help={t('启用中转缓存时必须设置工作目录，建议选择 SSD 所在分区。')}
          >
            <Space.Compact style={{ width: '100%' }}>
              <Input
                aria-label={t('SSD 工作目录')}
                value={displayPath(draft.cacheRoot)}
                readOnly={!daemon}
                onChange={(event) => update('cacheRoot', event.target.value)}
                placeholder={t('请选择固态硬盘上的工作目录')}
              />
              <Button
                icon={<FolderOpenOutlined />}
                disabled={!nativeEngine}
                onClick={() => void handlePickCache()}
              >
                {t('选择目录')}
              </Button>
              <Button icon={<ClearOutlined />} onClick={() => update('cacheRoot', '')}>
                {t('清空')}
              </Button>
            </Space.Compact>
          </Form.Item>
          <Space wrap size="large">
            <Form.Item label={t('缓存配额 (GiB)')}>
              <InputNumber
                aria-label={t('缓存配额 GiB')}
                min={1}
                max={4096}
                value={Math.round(draft.cacheQuotaBytes / GB)}
                onChange={(v) => update('cacheQuotaBytes', Math.max(1, v ?? 20) * GB)}
              />
            </Form.Item>
            <Form.Item label={t('最小剩余磁盘 (GiB)')}>
              <InputNumber
                aria-label={t('最小剩余空间 GiB')}
                min={0.5}
                max={1024}
                step={0.5}
                value={Number((draft.minFreeBytes / GB).toFixed(1))}
                onChange={(v) => update('minFreeBytes', Math.max(0.5, v ?? 1) * GB)}
              />
            </Form.Item>
            <Form.Item label={t('根任务累计解压上限 (GiB)')}>
              <InputNumber
                aria-label={t('根任务累计解压上限 GiB')}
                min={1}
                max={8192}
                value={Math.round(draft.maxExpandedBytes / GB)}
                onChange={(v) => update('maxExpandedBytes', Math.max(1, v ?? 100) * GB)}
              />
            </Form.Item>
            <Form.Item label={t('最大文件条数')}>
              <InputNumber
                aria-label={t('最大文件条数')}
                min={100}
                max={100000}
                step={1000}
                value={draft.maxEntries}
                onChange={(v) => update('maxEntries', Math.max(100, v ?? 100000))}
              />
            </Form.Item>
          </Space>
        </Form>
      ),
    },
    {
      key: 'exclusion',
      label: (
        <span>
          <FilterOutlined /> {t('筛选排除')}
        </span>
      ),
      children: (
        <Form layout="vertical">
          <Form.Item label={t('扫描范围')}>
            <Space>
              <Switch
                aria-label={t('递归扫描子目录')}
                checked={draft.recursiveScan}
                onChange={(v) => update('recursiveScan', v)}
              />
              <span>{t('递归扫描子目录')}</span>
            </Space>
          </Form.Item>
          <Form.Item
            label={t('输入排除规则（每行一条）')}
            help={t('最多 100 条规则，单条不超过 512 UTF-8 字节')}
          >
            <TextArea
              aria-label={t('扫描输入排除规则')}
              rows={3}
              value={inputExclusionsText}
              onChange={(e) => {
                const val = e.target.value;
                setInputExclusionsText(val);
                update(
                  'inputExclusions',
                  val
                    .split('\n')
                    .map((s) => s.trim())
                    .filter(Boolean),
                );
              }}
              placeholder={t('例如: *.tmp')}
            />
          </Form.Item>
          <Form.Item
            help={t('最多 100 条规则，单条不超过 512 UTF-8 字节')}
            label={
              <Space>
                <span>{t('输出排除规则（每行一条）')}</span>
                <Button
                  size="small"
                  type="link"
                  aria-label={t('添加 URL、快捷方式及系统文件规则')}
                  onClick={() => {
                    const presets = [
                      '*.url',
                      '*.lnk',
                      '__MACOSX/*',
                      '.DS_Store',
                      'Thumbs.db',
                      'desktop.ini',
                    ];
                    const current = outputExclusionsText
                      .split('\n')
                      .map((s) => s.trim())
                      .filter(Boolean);
                    const merged = Array.from(new Set([...current, ...presets]));
                    setOutputExclusionsText(merged.join('\n'));
                    update('outputExclusions', merged);
                  }}
                >
                  {t('添加 URL、快捷方式及系统文件规则')}
                </Button>
              </Space>
            }
          >
            <TextArea
              aria-label={t('归档输出排除规则')}
              rows={3}
              value={outputExclusionsText}
              onChange={(e) => {
                const val = e.target.value;
                setOutputExclusionsText(val);
                update(
                  'outputExclusions',
                  val
                    .split('\n')
                    .map((s) => s.trim())
                    .filter(Boolean),
                );
              }}
              placeholder={t('例如: *.bak')}
            />
          </Form.Item>
          <Alert
            title={t('规则范围')}
            description={t(
              '支持通配符（* 匹配任意字符，? 匹配单字符），不区分大小写。输入规则过滤要添加的压缩包，输出规则过滤解压生成的文件（排除的内层压缩包不会继续展开）。',
            )}
            type="warning"
            showIcon
          />
          <RulePreview
            inputPatterns={draft.inputExclusions}
            outputPatterns={draft.outputExclusions}
            run={run}
          />
        </Form>
      ),
    },
  ];
}
