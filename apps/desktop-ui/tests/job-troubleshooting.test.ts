import { describe, expect, it } from 'vitest';
import { analyzeJobIssue } from '../src/jobTroubleshooting';

describe('任务失败与异常排障分析', () => {
  it('识别密码缺失与口令错误，输出对应指引', () => {
    const analysis = analyzeJobIssue({
      status: 'needsPassword',
      message: '已保存密码未能打开压缩包，请补充密码后重试；损坏也可能导致此状态。',
    });
    expect(analysis).not.toBeNull();
    expect(analysis?.category).toBe('needsPassword');
    expect(analysis?.title).toContain('需要解压密码');
    expect(analysis?.suggestedAction).toBe('inputPassword');
    expect(analysis?.guideSteps.length).toBeGreaterThan(0);
  });

  it('识别归档损坏与 CRC 校验失败，输出对应指引与工具箱后缀修复建议', () => {
    const analysis = analyzeJobIssue({
      status: 'failed',
      message: '密码不匹配或压缩包损坏，解压失败。',
    });
    expect(analysis).not.toBeNull();
    expect(analysis?.category).toBe('corrupted');
    expect(analysis?.title).toContain('压缩包损坏');
    expect(analysis?.suggestedAction).toBe('toolboxAutofix');
    expect(analysis?.guideSteps.some((step) => step.includes('工具箱'))).toBe(true);
    expect(analysis?.guideSteps.some((step) => step.includes('抢救'))).toBe(true);
  });

  it('识别缺少分卷错误，输出分卷排查指引', () => {
    const analysis = analyzeJobIssue({
      status: 'failed',
      message: '压缩包缺少分卷，请补齐后重试。',
    });
    expect(analysis).not.toBeNull();
    expect(analysis?.category).toBe('missingVolume');
    expect(analysis?.guideSteps.some((step) => step.includes('分卷'))).toBe(true);
  });

  it('识别磁盘空间不足错误，输出设置调整指引', () => {
    const analysis = analyzeJobIssue({
      status: 'failed',
      message: '磁盘空间、缓存配额或展开限制不足，请调整后重试。',
    });
    expect(analysis).not.toBeNull();
    expect(analysis?.category).toBe('resourceLimit');
    expect(analysis?.suggestedAction).toBe('openSettings');
  });

  it('识别后台进程通信异常，输出安全卫士排障指引', () => {
    const analysis = analyzeJobIssue({
      status: 'failed',
      message: '后台解压进程通信异常，解压失败。',
    });
    expect(analysis).not.toBeNull();
    expect(analysis?.category).toBe('workerError');
    expect(
      analysis?.guideSteps.some((step) => step.includes('杀毒软件') || step.includes('worker')),
    ).toBe(true);
  });

  it('精准解析结构化 UnpackError: StorageError 繁忙锁死与数据库异常', () => {
    const analysis = analyzeJobIssue({
      status: 'failed',
      message:
        '任务数据库正被其他进程或事务占用（openunpack.db），已安全挂起并等待释放：database is locked [UnpackError:{"kind":"storage","path":"C:/db/openunpack.db","detail":"database is locked","isBusyLock":true}]',
    });
    expect(analysis).not.toBeNull();
    expect(analysis?.category).toBe('storageError');
    expect(analysis?.title).toContain('任务数据库繁忙锁死');
    expect(analysis?.isRetryable).toBe(true);
    expect(analysis?.guideSteps.some((s) => s.includes('isBusyLock') || s.includes('并发锁'))).toBe(
      true,
    );
  });

  it('精准解析结构化 UnpackError: DiskFull 磁盘空间不足与可用字节', () => {
    const analysis = analyzeJobIssue({
      status: 'failed',
      message:
        '磁盘空间不足：D:/output 剩余 50.0 MB，本次解压需要至少 5000.0 MB；请清理磁盘后重试。 [UnpackError:{"kind":"diskFull","path":"D:/output","neededBytes":524288000,"availableBytes":52428800}]',
    });
    expect(analysis).not.toBeNull();
    expect(analysis?.category).toBe('resourceLimit');
    expect(analysis?.categoryLabel).toBe('磁盘空间耗尽');
    expect(analysis?.summary).toContain('剩余 50.0 MB');
    expect(analysis?.suggestedAction).toBe('openSettings');
  });

  it('精准解析结构化 UnpackError: FileLocked 文件占用进程 PID 与程序名称', () => {
    const analysis = analyzeJobIssue({
      status: 'failed',
      message:
        '目标文件已被其他程序打开占用（占用进程: potplayer.exe [PID: 4321]），无法写入或移动：video.mp4；请关闭占用程序后重试。 [UnpackError:{"kind":"fileLocked","path":"video.mp4","occupyingPid":4321,"processName":"potplayer.exe"}]',
    });
    expect(analysis).not.toBeNull();
    expect(analysis?.category).toBe('fileLocked');
    expect(analysis?.categoryLabel).toBe('文件被占用');
    expect(analysis?.title).toContain('被其他进程独占占用');
    expect(
      analysis?.guideSteps.some((s) => s.includes('potplayer.exe') && s.includes('4321')),
    ).toBe(true);
    expect(analysis?.isRetryable).toBe(true);
  });

  it('精准解析结构化 UnpackError: PermissionDenied 权限不足与所需操作', () => {
    const analysis = analyzeJobIssue({
      status: 'failed',
      message:
        '没有足够的系统权限访问路径（缺少 Write 权限）：C:/Program Files/test；请以管理员权限运行或调整目录权限。 [UnpackError:{"kind":"permissionDenied","path":"C:/Program Files/test","requiredAccess":"Write"}]',
    });
    expect(analysis).not.toBeNull();
    expect(analysis?.category).toBe('permissionDenied');
    expect(analysis?.categoryLabel).toBe('权限不足');
    expect(analysis?.guideSteps.some((s) => s.includes('NTFS') || s.includes('管理员'))).toBe(true);
  });

  it('普通 StorageError 文本兜底识别', () => {
    const analysis = analyzeJobIssue({
      status: 'failed',
      message: '任务数据库写入失败，已停止后续操作；请检查恢复记录。',
    });
    expect(analysis).not.toBeNull();
    expect(analysis?.category).toBe('storageError');
    expect(analysis?.title).toContain('任务数据库读写失败');
  });

  it('成功状态且无错误信息返回 null', () => {
    const analysis = analyzeJobIssue({
      status: 'completed',
      message: '',
    });
    expect(analysis).toBeNull();
  });
});
