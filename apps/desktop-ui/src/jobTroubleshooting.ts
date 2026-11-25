import { t } from './i18n';

export type JobIssueCategory =
  | 'needsPassword'
  | 'corrupted'
  | 'missingVolume'
  | 'resourceLimit'
  | 'targetExists'
  | 'workerError'
  | 'storageError'
  | 'fileLocked'
  | 'permissionDenied'
  | 'unsupported'
  | 'generalFailure';

export interface JobIssueAnalysis {
  category: JobIssueCategory;
  categoryLabel: string;
  title: string;
  summary: string;
  guideSteps: string[];
  suggestedAction?: 'inputPassword' | 'toolboxAutofix' | 'openSettings' | 'retry' | 'cleanTemp';
  suggestedActionLabel?: string;
  isRetryable: boolean;
}

export function stripUnpackErrorEnvelope(message?: string): string {
  if (!message) return '';
  const tagStart = message.indexOf('[UnpackError:');
  if (tagStart !== -1 && message.endsWith(']')) {
    return message.slice(0, tagStart).trim();
  }
  return message;
}

export function formatJobMessage(message?: string): string {
  const cleaned = stripUnpackErrorEnvelope(message);
  if (!cleaned) return '';
  const direct = t(cleaned);
  if (direct !== cleaned) return direct;
  const colonIdx = cleaned.indexOf('：');
  if (colonIdx > 0) {
    const prefix = cleaned.slice(0, colonIdx);
    const rest = cleaned.slice(colonIdx + 1);
    const transPrefix = t(prefix);
    if (transPrefix !== prefix) {
      return `${transPrefix}: ${rest}`;
    }
  }
  return cleaned;
}

export function analyzeJobIssue(job: {
  status: string;
  stage?: string;
  message?: string;
}): JobIssueAnalysis | null {
  const isTerminalFailure = ['failed', 'needsPassword', 'interrupted', 'cancelled'].includes(
    job.status,
  );
  const rawMsg = (job.message || '').trim();
  const msg = stripUnpackErrorEnvelope(rawMsg);
  const lowerMsg = rawMsg.toLowerCase();

  // If not failed/needsPassword and no message, no issue to report
  if (!isTerminalFailure && !msg) {
    return null;
  }
  if (['completed', 'cleanupPending'].includes(job.status) && !msg) {
    return null;
  }

  // 0. Structured UnpackError parsing (JSON payload inside message or typed tags)
  const tagStart = rawMsg.indexOf('[UnpackError:');
  if (tagStart !== -1 && rawMsg.endsWith(']')) {
    const jsonStr = rawMsg.slice(tagStart + '[UnpackError:'.length, rawMsg.length - 1);
    try {
      const errObj = JSON.parse(jsonStr) as {
        kind?: string;
        isBusyLock?: boolean;
        processName?: string;
        occupyingPid?: number;
      };
      const cleanSummary = msg;
      if (errObj.kind === 'storage') {
        return {
          category: 'storageError',
          categoryLabel: t('数据库 / 存储异常'),
          title: errObj.isBusyLock ? t('任务数据库繁忙锁死') : t('任务数据库读写失败'),
          summary:
            cleanSummary ||
            (errObj.isBusyLock
              ? t('数据库被并发锁占用，请稍候。')
              : t('任务数据库写入失败，请检查恢复记录。')),
          guideSteps: [
            t('若为繁忙锁（isBusyLock），稍等片刻让其他进程或并发任务释放数据库后重试；'),
            t('检查磁盘所在分区是否有文件系统故障或只读保护；'),
            t('若长时间无法解锁，可重启客户端或检查是否有僵尸 openunpack 进程占用数据库。'),
          ],
          suggestedAction: 'retry',
          suggestedActionLabel: t('重试'),
          isRetryable: true,
        };
      } else if (errObj.kind === 'diskFull') {
        return {
          category: 'resourceLimit',
          categoryLabel: t('磁盘空间耗尽'),
          title: t('磁盘可用空间不足以完成解压'),
          summary: cleanSummary,
          guideSteps: [
            t('目标盘剩余空间不足以容纳解压后的全部文件，请清理目标磁盘；'),
            t('前往「设置」切换输出目录到剩余空间更大的磁盘分区；'),
            t('若开启了 SSD 缓存，检查系统缓存目录所在磁盘是否有足够缓冲空间。'),
          ],
          suggestedAction: 'openSettings',
          suggestedActionLabel: t('调整目录或设置'),
          isRetryable: true,
        };
      } else if (errObj.kind === 'fileLocked') {
        const procInfo = errObj.processName
          ? `${errObj.processName} (PID: ${errObj.occupyingPid})`
          : errObj.occupyingPid
            ? `PID ${errObj.occupyingPid}`
            : '';
        return {
          category: 'fileLocked',
          categoryLabel: t('文件被占用'),
          title: t('目标文件或归档被其他进程独占占用'),
          summary: cleanSummary,
          guideSteps: [
            procInfo
              ? t('请关闭正在占用该文件的程序：{procInfo}；', { procInfo })
              : t('请检查并关闭可能正在打开该文件的播放器、编辑器或杀毒软件；'),
            t('若进程已退出但句柄未释放，请在任务管理器中确认或稍等系统释放；'),
            t('解除占用后点击「重试」继续解压。'),
          ],
          suggestedAction: 'retry',
          suggestedActionLabel: t('解除占用后重试'),
          isRetryable: true,
        };
      } else if (errObj.kind === 'permissionDenied') {
        return {
          category: 'permissionDenied',
          categoryLabel: t('权限不足'),
          title: t('无权写入目标路径或访问源文件'),
          summary: cleanSummary,
          guideSteps: [
            t('检查输出目录或源文件所在目录的 Windows NTFS 读写权限；'),
            t('尝试以管理员身份重新运行 OpenUnpack；'),
            t(
              '若目标目录位于受保护的系统目录（如 Program Files），建议将输出目录更改为用户文档目录。',
            ),
          ],
          suggestedAction: 'openSettings',
          suggestedActionLabel: t('更换输出目录'),
          isRetryable: true,
        };
      }
    } catch {
      // fallback to text matching below
    }
  }

  // 0.1 Plain StorageError detection
  if (
    msg.includes('StorageError') ||
    msg.includes('任务数据库写入失败') ||
    msg.includes('任务数据库读写失败')
  ) {
    return {
      category: 'storageError',
      categoryLabel: t('数据库 / 存储异常'),
      title: t('任务数据库读写失败'),
      summary: msg || t('任务数据库写入失败，已停止后续操作；请检查恢复记录。'),
      guideSteps: [
        t('检查磁盘所在分区是否已满或处于只读保护状态；'),
        t('若任务数据库被杀毒软件或多个客户端实例占用，请稍候再试；'),
        t('点击「重试」再次尝试写入数据库。'),
      ],
      suggestedAction: 'retry',
      suggestedActionLabel: t('重试'),
      isRetryable: true,
    };
  }

  // 1. Explicit Needs Password status or stage
  if (job.status === 'needsPassword' || job.stage === 'needsPassword') {
    return {
      category: 'needsPassword',
      categoryLabel: t('密码缺失 / 需口令'),
      title: t('需要解压密码或已保存口令未命中'),
      summary: msg || t('该归档包含加密数据，当前密码库中的密码未能成功匹配。'),
      guideSteps: [
        t('点击「输入密码」录入正确口令，提交后将立即重试解压；'),
        t('若已知通用解压口令，建议在「密码库」中添加密码或批量导入，后续同类归档将自动免密解压；'),
        t('若为多层嵌套归档，录入第一层口令解压后，内部加密层仍可逐层提交对应密码。'),
      ],
      suggestedAction: 'inputPassword',
      suggestedActionLabel: t('输入密码'),
      isRetryable: true,
    };
  }

  // 2. Ambiguous combination: Password Mismatch OR Archive Corrupted (e.g. 7-Zip Exit Code 2 with status === 'failed')
  if (msg.includes('PasswordMismatchOrCorrupt') || (msg.includes('密码') && msg.includes('损坏'))) {
    return {
      category: 'corrupted',
      categoryLabel: t('密码不匹配 / 归档损坏'),
      title: t('密码不匹配或压缩包损坏'),
      summary: msg || t('解压失败：密码未能正确解密该文件，或该归档文件已损坏、分卷不完整。'),
      guideSteps: [
        t('核对当前密码是否完全正确（注意大小写与前后空格），点击「输入密码」再次尝试；'),
        t('若该文件并非密码保护，可能是下载不完整或文件损坏，请核对源文件哈希或重新下载；'),
        t(
          '若该文件后缀曾被伪装（如 .mp4 实际为 .7z），可在「工具箱」中使用「压缩包后缀自动修复」检查魔数并修正扩展名；',
        ),
        t('可在「设置」中开启「允许抢救已损坏归档」，然后重试解压以尽可能提取未损部分。'),
      ],
      suggestedAction: 'toolboxAutofix',
      suggestedActionLabel: t('后缀自动修复'),
      isRetryable: true,
    };
  }

  // 3. Password Mentioned in message without corruption mention
  if (
    msg.includes('NeedsPassword') ||
    msg.includes('密码') ||
    lowerMsg.includes('password') ||
    lowerMsg.includes('wrong password') ||
    lowerMsg.includes('cannot open encrypted') ||
    lowerMsg.includes('encrypted')
  ) {
    return {
      category: 'needsPassword',
      categoryLabel: t('密码缺失 / 需口令'),
      title: t('需要解压密码或已保存口令未命中'),
      summary: msg || t('该归档包含加密数据，当前密码库中的密码未能成功匹配。'),
      guideSteps: [
        t('点击「输入密码」录入正确口令，提交后将立即重试解压；'),
        t('若已知通用解压口令，建议在「密码库」中添加密码或批量导入，后续同类归档将自动免密解压；'),
        t('若为多层嵌套归档，录入第一层口令解压后，内部加密层仍可逐层提交对应密码。'),
      ],
      suggestedAction: 'inputPassword',
      suggestedActionLabel: t('输入密码'),
      isRetryable: true,
    };
  }

  // 4. Corrupted Archive / CRC Check Failure / Magic Byte Mismatch
  if (
    msg.includes('Corrupt') ||
    msg.includes('损坏') ||
    lowerMsg.includes('crc') ||
    lowerMsg.includes('data error') ||
    lowerMsg.includes('header error') ||
    lowerMsg.includes('unexpected end') ||
    lowerMsg.includes('broken') ||
    lowerMsg.includes('checksum')
  ) {
    return {
      category: 'corrupted',
      categoryLabel: t('归档损坏 / 校验失败'),
      title: t('压缩包损坏、CRC校验不符或分卷数据不完整'),
      summary: msg || t('压缩包头部数据或文件流校验失败，可能源文件未下载完整或介质受损。'),
      guideSteps: [
        t('请核对源文件体积是否完整，尝试重新下载该归档；'),
        t(
          '若该文件后缀曾被伪装篡改（例如将 .7z/.zip 伪装为 .mp4/.bin），请在「工具箱」中使用「压缩包后缀自动修复」检查并修正后缀；',
        ),
        t('若属于多分卷压缩包，请确保所有分卷文件位于同一目录且编号完整；'),
        t(
          '可在「设置」中开启「允许抢救已损坏归档（尽可能保留可读文件）」，然后点击「重试」尝试尽可能恢复部分内容。',
        ),
      ],
      suggestedAction: 'toolboxAutofix',
      suggestedActionLabel: t('后缀自动修复'),
      isRetryable: true,
    };
  }

  // 5. Missing Volume
  if (
    msg.includes('MissingVolume') ||
    msg.includes('缺少分卷') ||
    lowerMsg.includes('cannot find archive') ||
    lowerMsg.includes('missing volume')
  ) {
    return {
      category: 'missingVolume',
      categoryLabel: t('缺少分卷'),
      title: t('分卷压缩包缺少关联卷'),
      summary: msg || t('检测到该压缩包为多分卷连续归档，但当前目录缺少后续或前面的分卷文件。'),
      guideSteps: [
        t('请确认所有关联分卷（如 .part1.rar/.part2.rar 或 .z01/.z02/.zip）均存放在同一文件夹下；'),
        t('检查分卷文件名是否被修改导致序号断档，确保分卷命名严格连续；'),
        t('补齐所有缺失分卷文件后，点击「重试」重新解压。'),
      ],
      suggestedAction: 'retry',
      suggestedActionLabel: t('重试'),
      isRetryable: true,
    };
  }

  // 6. Resource Limit / Disk Full
  if (
    msg.includes('NoSpace') ||
    msg.includes('ResourceLimit') ||
    msg.includes('CacheQuotaContention') ||
    msg.includes('SharedResourceUnavailable') ||
    msg.includes('空间不足') ||
    msg.includes('配额') ||
    lowerMsg.includes('disk full') ||
    lowerMsg.includes('no space')
  ) {
    return {
      category: 'resourceLimit',
      categoryLabel: t('空间 / 配额受限'),
      title: t('磁盘空间不足或展开限制超出配额'),
      summary: msg || t('解压目标磁盘可用空间或 SSD 缓存配额不足，已安全暂停解压以防止磁盘占满。'),
      guideSteps: [
        t('清理目标磁盘或在「设置」中将「输出目录」更改至可用空间充裕的磁盘盘符；'),
        t('若启用了 SSD 缓存，可在「设置」中调大「缓存配额」或关闭缓存直接解压至目标目录；'),
        t('若解压展开膨胀倍数过大，可在「设置」中适当上调「单任务解压后总大小上限」。'),
      ],
      suggestedAction: 'openSettings',
      suggestedActionLabel: t('前往设置调整'),
      isRetryable: true,
    };
  }

  // 7. Target File Exists
  if (msg.includes('TargetExists') || msg.includes('同名文件') || msg.includes('冲突')) {
    return {
      category: 'targetExists',
      categoryLabel: t('同名冲突'),
      title: t('输出目录存在同名文件或目录'),
      summary: msg || t('解压目标路径已存在同名内容，根据当前安全策略未直接覆盖。'),
      guideSteps: [
        t('前往「设置」将同名冲突策略改为「自动更名（创建 (1) 副本）」或「覆盖」；'),
        t('或者手动重命名/移动目标目录中的现有文件后再次重试。'),
      ],
      suggestedAction: 'openSettings',
      suggestedActionLabel: t('调整冲突策略'),
      isRetryable: true,
    };
  }

  // 8. Worker Engine Failure / IPC Disconnect
  if (
    msg.includes('Worker') ||
    msg.includes('ProtocolError') ||
    msg.includes('通信异常') ||
    msg.includes('进程') ||
    lowerMsg.includes('worker') ||
    lowerMsg.includes('pipe')
  ) {
    return {
      category: 'workerError',
      categoryLabel: t('引擎 / 进程异常'),
      title: t('后台解压引擎响应异常或进程被中断'),
      summary: msg || t('解压核心进程异常退出或通信超时，原归档和临时工作区已被安全保护。'),
      guideSteps: [
        t('检查系统杀毒软件或安全卫士是否拦截了 OpenUnpack 7-Zip worker 进程；'),
        t('确认机器可用内存是否充足，关闭其他高负载占用进程；'),
        t('在「设置」中检查解压引擎可用性，然后点击「重试」。'),
      ],
      suggestedAction: 'retry',
      suggestedActionLabel: t('重试'),
      isRetryable: true,
    };
  }

  // 9. Unsupported Format
  if (msg.includes('Unsupported') || msg.includes('暂不支持') || lowerMsg.includes('unsupported')) {
    return {
      category: 'unsupported',
      categoryLabel: t('格式不支持'),
      title: t('当前解压引擎暂不支持该文件格式'),
      summary: msg || t('当前归档使用的压缩算法或封装结构不受支持。'),
      guideSteps: [
        t('可在「工具箱」中使用「压缩包后缀自动修复」检测文件头魔数，验证是否为实际标准格式；'),
        t('若是私有加密或特殊分包（如专用安装包、专有镜像），建议使用对应专用工具打开。'),
      ],
      suggestedAction: 'toolboxAutofix',
      suggestedActionLabel: t('工具箱检测'),
      isRetryable: false,
    };
  }

  // 10. General Failure
  if (isTerminalFailure) {
    return {
      category: 'generalFailure',
      categoryLabel: t('任务失败'),
      title: t('解压任务未能顺利完成'),
      summary: msg || t('任务处理遇到错误并已停止。原始压缩文件安全保留。'),
      guideSteps: [
        t('点击「详情」查看内外层处理节点与具体阶段日志；'),
        t('若是临时读写异常，可直接点击「重试」再次尝试；'),
        t('若工作目录存在残留临时碎片，可点击「清理临时结果」释放占用。'),
      ],
      suggestedAction: 'retry',
      suggestedActionLabel: t('重试'),
      isRetryable: true,
    };
  }

  return null;
}
