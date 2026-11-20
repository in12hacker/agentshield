// Explicit human input to the existing attribution contract; no inferred pattern or floating-point time.
import type { AttributionRequest, TargetPattern } from './context-wire';
import { decodeAttributionRequest } from './context-wire';

function text(form: FormData, name: string): string {
  const value = form.get(name);
  if (typeof value !== 'string') throw new Error('attribution_input');
  return value;
}
export function secondsToNanoseconds(value: string): string {
  const match = /^([0-9]+)(?:\.([0-9]{1,9}))?(?![\s\S])/.exec(value);
  if (!match) throw new Error('attribution_time');
  const ns = BigInt(match[1]!) * 1_000_000_000n + BigInt((match[2] ?? '').padEnd(9, '0'));
  if (ns > 18446744073709551615n) throw new Error('attribution_time');
  return String(ns);
}
function pointer(form: FormData, name: string): string {
  const mode = text(form, `${name}_mode`);
  if (mode === 'whole') return '';
  const value = text(form, name);
  if (mode !== 'pointer' || !value.startsWith('/') || value.split('~').slice(1).some(t => !/^[01]/.test(t)))
    throw new Error('attribution_pointer');
  return value;
}
export function attributionInput(form: FormData): AttributionRequest {
  const type = text(form, 'target');
  let target: TargetPattern;
  if (type === 'exec_argument') {
    const original = text(form, 'argument_index');
    if (!/^[0-9]+(?![\s\S])/.test(original) || BigInt(original) > 4294967295n) throw new Error('attribution_index');
    target = { type, pointer: pointer(form, 'value_pointer'), index: Number(original) };
  } else if (type === 'file_path' || type === 'executable') {
    target = { type, pointer: pointer(form, 'value_pointer') };
  } else if (type === 'network_peer') {
    target = { type, address_pointer: pointer(form, 'address_pointer'), port_pointer: pointer(form, 'port_pointer') };
  } else throw new Error('attribution_target');
  return decodeAttributionRequest({ pattern: { actor: text(form, 'actor'),
    lookback_ns: secondsToNanoseconds(text(form, 'lookback_seconds')), target },
    after: null, limit: Number(text(form, 'limit')) });
}
