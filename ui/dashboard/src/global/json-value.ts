import { jsonNumberLexeme } from '../wire-json';

// Compare parsed facts without serializing or cloning them.
export function sameJsonValue(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  const aNumber = jsonNumberLexeme(left), bNumber = jsonNumberLexeme(right);
  if (aNumber !== null || bNumber !== null) return aNumber === bNumber;
  if (left === null || right === null || typeof left !== 'object' || typeof right !== 'object') return false;
  if (Array.isArray(left) || Array.isArray(right)) return Array.isArray(left) && Array.isArray(right)
    && left.length === right.length && left.every((item, index) => sameJsonValue(item, right[index]));
  const a = left as Readonly<Record<string, unknown>>;
  const b = right as Readonly<Record<string, unknown>>;
  return Object.keys(a).length === Object.keys(b).length
    && Object.keys(a).every(key => Object.hasOwn(b, key) && sameJsonValue(a[key], b[key]));
}
