const utf8 = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

export function isByteString(value: unknown): value is readonly number[] {
  return Array.isArray(value) && value.every(byte => Number.isInteger(byte) && byte >= 0 && byte <= 255);
}

export function byteText(value: readonly number[]): { kind: 'empty' | 'text' | 'bytes'; value: string } {
  if (value.length === 0) return { kind: 'empty', value: '' };
  try {
    return { kind: 'text', value: utf8.decode(Uint8Array.from(value)) };
  } catch {
    return { kind: 'bytes', value: value.map(byte => '\\x' + byte.toString(16).padStart(2, '0')).join('') };
  }
}
