import { loadConfig } from './config.mjs';
import { Gateway } from './gateway.mjs';
import { runTask } from './agent.mjs';
import { options } from './options.mjs';

let json = false;
try {
  const args = options();
  json = args.json;
  if (args.help) {
    console.log('Gemini 子 agent\n--task <任务> --workspace <目录> [--context <背景>] [--json] [--allow-write]\n--models 查看模型列表。Windows 使用 run.ps1 自动读取加密凭据。');
  } else {
    const config = await loadConfig();
    if (args.models) console.log(JSON.stringify(await new Gateway(config).models(), null, 2));
    else {
      const controller = new AbortController();
      process.once('SIGINT', () => controller.abort());
      const result = await runTask({ task: args.task, context: args.context, workspace: args.workspace,
        allowWrite: args['allow-write'], config, signal: controller.signal });
      console.log(json ? JSON.stringify(result, null, 2) : result.answer);
      if (result.status !== 'completed') process.exitCode = 2;
    }
  }
} catch (error) {
  console.error(json ? JSON.stringify({ status: 'error', error: error.message }) : error.message);
  process.exitCode = 1;
}
