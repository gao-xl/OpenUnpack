import { t } from '../i18n';
import { Checkbox, Input, Modal, Space, Tag, Typography } from 'antd';
import { api } from '../api';
import type { JobView, PasswordView } from '../types';

interface QueuePasswordModalProps {
  passwordJob: JobView | null;
  passwordOwnerPresent: boolean;
  daemon: boolean;
  submittingPassword: boolean;
  inputPassword: string;
  saveToVault: boolean;
  passwords: PasswordView[];
  onChangePassword: (value: string) => void;
  onChangeSaveToVault: (checked: boolean) => void;
  onSubmit: () => void;
  onCancel: () => void;
}

export default function QueuePasswordModal({
  passwordJob,
  passwordOwnerPresent,
  daemon,
  submittingPassword,
  inputPassword,
  saveToVault,
  passwords,
  onChangePassword,
  onChangeSaveToVault,
  onSubmit,
  onCancel,
}: QueuePasswordModalProps) {
  return (
    <Modal
      title={t('输入解压密码 — {p0}', { p0: passwordJob?.source.displayName ?? '' })}
      open={!!passwordJob && passwordOwnerPresent}
      onCancel={onCancel}
      destroyOnHidden
      okText={daemon ? t('提交密码') : t('解密并重试')}
      cancelText={t('取消')}
      okButtonProps={{ loading: submittingPassword, disabled: !inputPassword }}
      onOk={onSubmit}
    >
      <Typography.Paragraph>
        {daemon
          ? t('密码将仅提交给当前服务任务；勾选保存后才会写入服务端密码库。')
          : t('该压缩文件受密码保护。请输入解压密码，输入后将立即重新调度此任务。')}
      </Typography.Paragraph>
      <Space direction="vertical" style={{ width: '100%' }}>
        <Input.Password
          autoFocus
          aria-label={t('即时解压密码')}
          placeholder={t('输入解压密码')}
          value={inputPassword}
          onChange={(e) => onChangePassword(e.target.value)}
          onPressEnter={onSubmit}
        />
        {passwords.length > 0 && (
          <div style={{ marginTop: 2, marginBottom: 4 }}>
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {t('快捷填入最近密码：')}
            </Typography.Text>
            <Space wrap size={[4, 4]} style={{ marginTop: 4 }}>
              {passwords
                .filter((p) => p.enabled)
                .slice(0, 5)
                .map((p) => (
                  <Tag
                    key={p.id}
                    color="blue"
                    style={{ cursor: 'pointer', margin: 0, fontSize: 11 }}
                    onClick={() => {
                      void (async () => {
                        try {
                          const secret = await api.revealPassword(p.id);
                          if (secret) onChangePassword(secret);
                        } catch {}
                      })();
                    }}
                  >
                    {p.name}
                  </Tag>
                ))}
            </Space>
          </div>
        )}
        <Checkbox checked={saveToVault} onChange={(e) => onChangeSaveToVault(e.target.checked)}>
          {daemon
            ? t('同时保存到服务端加密密码库（需要密码库写入权限）')
            : t('同时保存到本地加密密码库（推荐，后续同批次或分卷可自动匹配）')}
        </Checkbox>
      </Space>
    </Modal>
  );
}
