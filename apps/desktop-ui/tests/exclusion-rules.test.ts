import { describe, expect, it } from 'vitest';
import { exclusionPatternByteLength, isExclusionPatternWithinLimit } from '../src/exclusion-rules';

describe('exclusion wire limit', () => {
  it.each([
    { name: 'ASCII 257', pattern: 'a'.repeat(257), bytes: 257, allowed: true },
    { name: 'ASCII 512', pattern: 'a'.repeat(512), bytes: 512, allowed: true },
    { name: 'ASCII 513', pattern: 'a'.repeat(513), bytes: 513, allowed: false },
    { name: 'Chinese 512', pattern: '中'.repeat(170) + 'aa', bytes: 512, allowed: true },
    { name: 'Chinese 513', pattern: '中'.repeat(171), bytes: 513, allowed: false },
    { name: 'Emoji 512', pattern: '😀'.repeat(128), bytes: 512, allowed: true },
    { name: 'Emoji 513', pattern: '😀'.repeat(128) + 'a', bytes: 513, allowed: false },
  ])('counts UTF-8 bytes at the boundary ($name)', ({ pattern, bytes, allowed }) => {
    expect(exclusionPatternByteLength(pattern)).toBe(bytes);
    expect(isExclusionPatternWithinLimit(pattern)).toBe(allowed);
  });
});
