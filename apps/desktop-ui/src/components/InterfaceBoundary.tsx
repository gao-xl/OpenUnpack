import { Component, type ErrorInfo, type ReactNode } from 'react';
import { Alert, Button, Space, Typography } from 'antd';
import { api, isDesktop } from '../api';

export default class InterfaceBoundary extends Component<{ children: ReactNode }, { failed: boolean; stopping: boolean; message: string }> {
  state = { failed: false, stopping: false, message: '' };
  static getDerivedStateFromError() { return { failed: true }; }
  componentDidCatch(_error: Error, _info: ErrorInfo) { /* Sensitive component state is never added to a diagnostic. */ }
  stop = async () => {
    if (this.state.stopping) return;
    this.setState({ stopping: true, message: '' });
    try { await api.cancel(); this.setState({ message: '停止请求已发送。退出状态以桌面托盘与重新打开后的记录为准。' }); }
    catch { this.setState({ message: '停止请求未确认。请通过桌面托盘停止任务，并保留工作目录供重启核对。' }); }
    finally { this.setState({ stopping: false }); }
  };
  render() {
    if (!this.state.failed) return this.props.children;
    return <main className="interface-fallback">
      <Typography.Title level={2}>界面显示遇到问题</Typography.Title>
      <Alert type="error" showIcon title="任务可能仍在后台运行" description="重新加载只恢复界面，不会自动重试或取消任务。需要停止时可使用下方按钮或桌面托盘；请保留任务工作目录。" />
      <Space wrap style={{ marginTop: 16 }}><Button type="primary" onClick={() => window.location.reload()}>重新加载界面</Button><Button danger disabled={!isDesktop} loading={this.state.stopping} onClick={() => void this.stop()}>请求停止后台任务</Button></Space>
      {this.state.message && <Typography.Paragraph style={{ marginTop: 16 }}>{this.state.message}</Typography.Paragraph>}
    </main>;
  }
}
