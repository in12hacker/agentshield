// Shared current SessionField/SessionObservation and NativeIdValue wire values from flow.rs.
import { arrayOf, byteString, exact, invalid, jsonNumberLiteral, oneOf, record, string } from './wire-json';

export type SessionField = { kind: 'header'; name: string } | { kind: 'json_pointer'; pointer: string };
export type SessionValue = { kind: 'header'; value: readonly number[] }
  | { kind: 'json'; value: { kind: 'json_string' | 'json_number'; value: string } };
export interface SessionObservation {
  field: SessionField; values: readonly SessionValue[];
  issue: 'missing' | 'multiple_values' | 'unsupported_value' | 'unparsed_body' | null;
}

export function decodeNativeIdValue(value: unknown): { kind: 'json_string' | 'json_number'; value: string } {
  const item = exact(value, ['kind', 'value']);
  oneOf(item.kind, ['json_string', 'json_number']);
  if (item.kind === 'json_number') jsonNumberLiteral(item.value);
  else string(item.value);
  return item as { kind: 'json_string' | 'json_number'; value: string };
}
export function decodeSessionField(value: unknown): SessionField {
  const field = record(value);
  exact(field, field.kind === 'header' ? ['kind', 'name'] : ['kind', 'pointer']);
  if (field.kind === 'header') string(field.name);
  else { if (field.kind !== 'json_pointer') return invalid(); string(field.pointer); }
  return field as SessionField;
}
export function decodeSessionObservation(value: unknown): SessionObservation {
  const observation = exact(value, ['field', 'values', 'issue']);
  const field = decodeSessionField(observation.field);
  const values = arrayOf(observation.values, value => {
    const item = exact(value, ['kind', 'value']);
    if (item.kind === 'header') byteString(item.value);
    else if (item.kind === 'json') decodeNativeIdValue(item.value);
    else return invalid();
    if ((item.kind === 'header') !== (field.kind === 'header')) return invalid();
    return item;
  });
  if (observation.issue !== null) oneOf(observation.issue, ['missing', 'multiple_values', 'unsupported_value', 'unparsed_body']);
  if (observation.issue === null && values.length !== 1) return invalid();
  return observation as unknown as SessionObservation;
}
