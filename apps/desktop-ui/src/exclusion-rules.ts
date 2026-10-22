export const MAX_EXCLUSION_PATTERN_BYTES = 512;

// IPC and both engines apply the limit to the original UTF-8 representation.
export function exclusionPatternByteLength(pattern: string): number {
  return new TextEncoder().encode(pattern).length;
}

export function isExclusionPatternWithinLimit(pattern: string): boolean {
  return exclusionPatternByteLength(pattern) <= MAX_EXCLUSION_PATTERN_BYTES;
}
