import { Button, Card, Divider, Form, Input, InputNumber, Space, Switch } from 'antd';
import { FolderOpenOutlined } from '@ant-design/icons';
import { t } from '../i18n';
import type { ToolboxKind } from '../types';

export interface ToolboxRuleConfigProps {
  kind: ToolboxKind;
  daemon: boolean;
  nativeEngine: boolean;
  searchPattern: string;
  replacePattern: string;
  caseSensitive: boolean;
  useRegex: boolean;
  prefix: string;
  suffix: string;
  enableNumbering: boolean;
  numberingStart: number;
  numberingDigits: number;
  targetDirectory: string;
  removeEmptyFolders: boolean;
  onSearchPatternChange: (v: string) => void;
  onReplacePatternChange: (v: string) => void;
  onCaseSensitiveChange: (v: boolean) => void;
  onUseRegexChange: (v: boolean) => void;
  onPrefixChange: (v: string) => void;
  onSuffixChange: (v: string) => void;
  onEnableNumberingChange: (v: boolean) => void;
  onNumberingStartChange: (v: number) => void;
  onNumberingDigitsChange: (v: number) => void;
  onTargetDirectoryChange: (v: string) => void;
  onRemoveEmptyFoldersChange: (v: boolean) => void;
  onPickTarget: () => Promise<void>;
}

export default function ToolboxRuleConfig(props: ToolboxRuleConfigProps) {
  const {
    kind,
    daemon,
    nativeEngine,
    searchPattern,
    replacePattern,
    caseSensitive,
    useRegex,
    prefix,
    suffix,
    enableNumbering,
    numberingStart,
    numberingDigits,
    targetDirectory,
    removeEmptyFolders,
    onSearchPatternChange: setSearchPattern,
    onReplacePatternChange: setReplacePattern,
    onCaseSensitiveChange: setCaseSensitive,
    onUseRegexChange: setUseRegex,
    onPrefixChange: setPrefix,
    onSuffixChange: setSuffix,
    onEnableNumberingChange: setEnableNumbering,
    onNumberingStartChange: setNumberingStart,
    onNumberingDigitsChange: setNumberingDigits,
    onTargetDirectoryChange: setTargetDirectory,
    onRemoveEmptyFoldersChange: setRemoveEmptyFolders,
    onPickTarget: handlePickTarget,
  } = props;
  return (
    <>
      {kind === 'batchRename' && (
        <Card type="inner" title={t('更名规则配置')} style={{ marginBottom: 16 }}>
          <Space orientation="vertical" style={{ width: '100%' }} size={12}>
            <Space wrap style={{ width: '100%' }}>
              <Form.Item label={t('查找文本 / 正则')} style={{ marginBottom: 0, minWidth: 240 }}>
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
    </>
  );
}
