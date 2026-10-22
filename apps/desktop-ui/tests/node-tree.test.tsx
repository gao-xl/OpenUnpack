import { expect, it } from 'vitest';
import type { DataNode } from 'antd/es/tree';
import { buildNodeForest } from '../src/components/shared';
import type { NodeView } from '../src/types';

const node = (id: string, parentId: string | null): NodeView => ({
  id, parentId, name: id, depth: 0, status: 'completed', stage: 'finished', progress: 1, attempts: 0, message: '', passwordLabel: null,
});
function inspectTree(roots: DataNode[]) {
  const keys: string[] = [];
  let maxDepth = 0;
  const stack = roots.map(branch => ({ branch, depth: 1 }));
  while (stack.length) {
    const current = stack.pop()!;
    keys.push(String(current.branch.key));
    maxDepth = Math.max(maxDepth, current.depth);
    for (const child of current.branch.children ?? []) stack.push({ branch: child, depth: current.depth + 1 });
  }
  return { keys, maxDepth };
}
it('keeps very deep hierarchies bounded without recursive stack overflow or losing nodes', () => {
  const input = Array.from({ length: 30_000 }, (_, index) => node(String(index), index ? String(index - 1) : null));
  const result = inspectTree(buildNodeForest(input, current => current.name));
  expect(result.keys).toHaveLength(input.length);
  expect(new Set(result.keys).size).toBe(input.length);
  expect(result.maxDepth).toBeLessThanOrEqual(32);
});
it('shows cyclic, self-parented, missing-parent and duplicate nodes once', () => {
  const input = [node('a', 'b'), node('b', 'a'), node('self', 'self'), node('orphan', 'missing'), node('a', null)];
  const result = inspectTree(buildNodeForest(input, current => current.name));
  expect(result.keys.sort()).toEqual(['a', 'b', 'orphan', 'self']);
});
