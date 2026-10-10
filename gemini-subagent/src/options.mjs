import { parseArgs } from 'node:util';
import path from 'node:path';

export function options(args = process.argv.slice(2)) {
  const { values } = parseArgs({ args, options: {
    task: { type: 'string' }, context: { type: 'string' }, workspace: { type: 'string' },
    'allow-write': { type: 'boolean', default: false }, json: { type: 'boolean', default: false },
    'dynamic-workspace': { type: 'boolean', default: false },
    models: { type: 'boolean', default: false }, help: { type: 'boolean', default: false },
  } });
  return { ...values, workspace: path.resolve(values.workspace || process.cwd()) };
}
