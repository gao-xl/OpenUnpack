import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import { loadConfig } from './config.mjs';
import { runTask } from './agent.mjs';
import { Workspace } from './workspace.mjs';
import { options } from './options.mjs';
import path from 'node:path';
import { readFile } from 'node:fs/promises';

try {
  const args = options();
  const config = await loadConfig();
  const dynamic = args['dynamic-workspace'];
  const root = dynamic ? undefined : await Workspace.open(args.workspace);
  const pkg = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'));
  const server = new McpServer({ name: 'gemini-subagent', version: pkg.version }, {
    instructions: `把文件查找、代码阅读、摘要等基础任务委派给 gemini_delegate。模型 ${config.model}。
一次委派应有清晰目标、必要上下文和验收要求。返回结果后由主 agent 复核。
${dynamic ? '主 agent 必须在每次调用的 workspace 参数中指定当前项目的绝对路径。Gemini 只能访问该任务目录。' : `工作区固定为 ${root.root}。`}
默认只读。请勿传入密码或密钥。工具不支持 shell、网络浏览、部署或发消息。`,
  });
  server.registerTool('gemini_delegate', {
    title: '委派基础任务给 Gemini',
    description: `使用 ${config.model} 完成文件查找、代码阅读、摘要等基础任务，返回结果及执行记录。每次任务独立上下文。`,
    inputSchema: {
      task: z.string().min(1).max(24000).describe('明确的任务、范围与验收要求'),
      context: z.string().max(48000).optional().describe('主 agent 提供的必要背景，禁止传凭据'),
      workspace: (dynamic ? z.string().min(1) : z.string().optional()).describe('插件模式必须传当前项目的绝对目录，由主 agent 选择；固定工作区模式不可传此参数'),
      subdirectory: z.string().optional().describe('配置工作区内的相对目录，默认根目录'),
      allow_write: z.boolean().default(false).describe('仅当服务启动时带 --allow-write 才可开启此任务写入'),
    },
    annotations: { readOnlyHint: !args['allow-write'], destructiveHint: !!args['allow-write'],
      idempotentHint: false, openWorldHint: true },
  }, async (input, extra) => {
    try {
      if (input.allow_write && !args['allow-write']) throw new Error('服务未开启写入权限。需用 --allow-write 重新启动。');
      let taskRoot = root;
      if (dynamic) {
        if (!input.workspace || !path.isAbsolute(input.workspace)) throw new Error('插件委派必须提供 workspace 绝对路径。');
        taskRoot = await Workspace.open(input.workspace);
      } else if (input.workspace !== undefined) throw new Error('固定工作区服务不接受 workspace 参数。');
      const workspace = await taskRoot.resolve(input.subdirectory || '.');
      const token = extra._meta?.progressToken;
      const result = await runTask({ task: input.task, context: input.context, workspace,
        allowWrite: input.allow_write, config, signal: extra.signal,
        onProgress: async event => {
          if (token !== undefined) await extra.sendNotification({ method: 'notifications/progress',
            params: { progressToken: token, progress: event.turn + event.toolCalls / (config.maxToolCalls + 1), message: event.message } });
        },
      });
      return { content: [{ type: 'text', text: JSON.stringify(result) }], structuredContent: result,
        isError: result.status !== 'completed' };
    } catch (error) {
      return { content: [{ type: 'text', text: error.code ? `文件操作失败：${error.code}` : error.message }], isError: true };
    }
  });
  await server.connect(new StdioServerTransport());
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
