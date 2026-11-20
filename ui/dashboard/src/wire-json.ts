// Shared current-wire primitives and the sole lossless JSON boundary.
export type Facts = Readonly<Record<string, unknown>>;
export type FailureCode = 'not_found' | 'cursor_expired' | 'unsupported' | 'unauthorized'
  | 'unavailable' | 'busy' | 'conflict' | 'invalid' | 'failed';
export type JsonU64 = number | bigint;

// 只在 JSON 边界构造；opaque 数值没有隐式 JS 浮点语义。
class JsonNumber {
  readonly #lexeme: string;
  private constructor(lexeme: string) { this.#lexeme = lexeme; Object.freeze(this); }
  static parse(lexeme: string): JsonNumber {
    jsonNumberLiteral(lexeme);
    return new JsonNumber(lexeme);
  }
  get lexeme(): string { return this.#lexeme; }
}

// Native JSON-number fields retain their exact validated lexical representation.
export function jsonNumberLiteral(value: unknown): string {
  if (typeof value !== 'string' || !/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?(?![\s\S])/.test(value)) return invalid();
  return value;
}

// 比较／序列化共享同一个数值对象；不导出可绕过验证的构造器。
export const jsonNumberLexeme = (value: unknown): string | null => value instanceof JsonNumber ? value.lexeme : null;

export function invalid(): never { throw new Error('invalid_r68_frame'); }
export function record(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value) || value instanceof JsonNumber) return invalid();
  return value as Record<string, unknown>;
}
export function exact(value: unknown, required: readonly string[], optional: readonly string[] = []) {
  const object = record(value);
  if (required.some(key => !Object.hasOwn(object, key))
    || Object.keys(object).some(key => !required.includes(key) && !optional.includes(key))) invalid();
  return object;
}
export function oneOf<T extends string>(value: unknown, options: readonly T[]): T {
  if (typeof value !== 'string' || !options.includes(value as T)) return invalid();
  return value as T;
}
export function string(value: unknown): string { return typeof value === 'string' ? value : invalid(); }
// Shared text/JSON representation; retain the original tree and avoid a
// dependency from raw runtime decoding back into the business decoder.
export function decodeChatContent(value: unknown): Facts {
  const object = exact(value, ['kind', 'value']);
  oneOf(object.kind, ['text', 'json']);
  if (object.kind === 'text') string(object.value);
  return object;
}
export function boolean(value: unknown): boolean { return typeof value === 'boolean' ? value : invalid(); }
export function integer(value: unknown, min: number, max: number): number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= min && value <= max
    ? value : invalid();
}
export function arrayOf<T>(value: unknown, decode: (item: unknown) => T): T[] {
  return Array.isArray(value) ? value.map(decode) : invalid();
}
export const byteString = (value: unknown): readonly number[] => arrayOf(value, byte => integer(byte, 0, 255));
export function bytes(value: unknown, length: number): readonly number[] {
  if (!Array.isArray(value) || value.length !== length) return invalid();
  return byteString(value);
}
export const failureCode = (value: unknown): FailureCode => oneOf(value, ['not_found', 'cursor_expired', 'unsupported',
  'unauthorized', 'unavailable', 'busy', 'conflict', 'invalid', 'failed'] as const);
export function decimalU64(value: unknown): string {
  if (typeof value !== 'string' || !/^(0|[1-9][0-9]{0,19})(?![\s\S])/.test(value)
    || BigInt(value) > 18446744073709551615n) return invalid();
  return value;
}
export function numericU64(value: unknown): JsonU64 {
  if (typeof value === 'bigint') return value >= 0n && value <= 18446744073709551615n ? value : invalid();
  return integer(value, 0, Number.MAX_SAFE_INTEGER);
}

// Validate the JSON structure as it is read so the browser never builds a discarded
// rounded copy. The native scalar string parser still checks escapes and control bytes.
export function parseUnique(text: string, options?: { integerOnly: true }): unknown {
  let offset = 0;
  const number = /-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/y;
  const whitespace = /[ \t\r\n]*/y;

  function skip() {
    whitespace.lastIndex = offset;
    whitespace.test(text);
    offset = whitespace.lastIndex;
  }
  function consume(character: string): boolean {
    if (text[offset] !== character) return false;
    offset++;
    return true;
  }
  function expect(character: string) {
    if (!consume(character)) invalid();
  }
  function readString(): string {
    const start = offset;
    expect('"');
    while (offset < text.length) {
      if (text[offset] === '\\') offset += 2;
      else if (text[offset++] === '"') return JSON.parse(text.slice(start, offset)) as string;
    }
    return invalid();
  }
  function readObject(): Record<string, unknown> {
    const object: Record<string, unknown> = {};
    skip();
    if (consume('}')) return object;
    do {
      skip();
      const key = readString();
      if (Object.hasOwn(object, key)) invalid();
      skip();
      expect(':');
      Object.defineProperty(object, key, { value: readValue(), enumerable: true, writable: true, configurable: true });
      skip();
    } while (consume(','));
    expect('}');
    return object;
  }
  function readArray(): unknown[] {
    const entries: unknown[] = [];
    skip();
    if (consume(']')) return entries;
    do {
      entries.push(readValue());
      skip();
    } while (consume(','));
    expect(']');
    return entries;
  }
  function readValue(): unknown {
    skip();
    if (text[offset] === '"') return readString();
    if (consume('{')) return readObject();
    if (consume('[')) return readArray();
    if (text.startsWith('true', offset)) { offset += 4; return true; }
    if (text.startsWith('false', offset)) { offset += 5; return false; }
    if (text.startsWith('null', offset)) { offset += 4; return null; }
    number.lastIndex = offset;
    const token = number.exec(text)?.[0];
    if (!token) return invalid();
    const pureInteger = /^-?\d+$/.test(token);
    if (options?.integerOnly && !pureInteger) return invalid();
    offset = number.lastIndex;
    if (!pureInteger || token === '-0') return JsonNumber.parse(token);
    const numeric = Number(token);
    return Number.isSafeInteger(numeric) ? numeric : BigInt(token);
  }
  const parsed = readValue();
  skip();
  if (offset !== text.length) return invalid();
  return parsed;
}

// One lossless serializer for both compact requests and readable source evidence.
function stringifyFacts(value: unknown, indent: number | null): string {
  if (value instanceof JsonNumber) return value.lexeme;
  if (typeof value === 'bigint') return value.toString();
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) throw new Error('invalid_r68_fact');
    if (Object.is(value, -0)) return '-0';
  }
  if (value === null || typeof value !== 'object') {
    const literal = JSON.stringify(value);
    if (literal === undefined) throw new Error('invalid_r68_fact');
    return literal;
  }
  const newline = indent === null ? '' : '\n';
  const pad = indent === null ? '' : '  '.repeat(indent + 1);
  const end = indent === null ? '' : '  '.repeat(indent);
  const next = indent === null ? null : indent + 1;
  const separator = ',' + newline;
  if (Array.isArray(value)) {
    return value.length ? '[' + newline + value.map(item => pad + stringifyFacts(item, next)).join(separator) + newline + end + ']' : '[]';
  }
  const entries = Object.entries(value);
  if (entries.length === 0) return '{}';
  const colon = indent === null ? ':' : ': ';
  const fields = entries.map(([key, item]) => pad + JSON.stringify(key) + colon + stringifyFacts(item, next));
  return '{' + newline + fields.join(separator) + newline + end + '}';
}

export const factsJson = (value: unknown) => stringifyFacts(value, 0);
export const wireJson = (value: unknown) => stringifyFacts(value, null);
