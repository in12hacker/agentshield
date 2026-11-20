// Shared immutable event facts. No envelope/session imports (including at runtime).
import { boolean, bytes, decimalU64, exact, failureCode, integer, invalid, oneOf,
  record, string, type Facts, type FailureCode } from './wire-json';

export type DecisionId = readonly number[];
export type Termination = 'none' | 'actor' | 'actor_and_parent';
export type Decision = { type: 'allow' } | { type: 'deny'; terminate: Termination };
export interface EventKey { source_instance: readonly number[]; sequence_domain: string; sequence: string }
export interface OriginalEvent extends Facts { header: Facts & EventKey }
export interface RuleHit {
  group_id: string; group_number: number; rule_id: string; rule_number: number;
  source: { kind: 'configuration' | 'rules_document'; version: readonly number[] };
  log: boolean; deny_message: string | null;
}
export interface Ask {
  type: 'ask'; decision_id: DecisionId; hit: RuleHit; event: OriginalEvent;
  choices: readonly Decision[]; timeout: Decision; remaining_ms: number;
}
export type Outcome = 'allowed' | 'denied' | 'cancelled' | 'unknown' | { failed: { native_error: number } };
export type Signal = { status: 'not_requested' | 'signal_sent' | 'not_running' }
  | { status: 'failed'; native_error: number };
export interface TerminationReport { requested: Termination; actor: Signal; parent: Signal }
export interface Proposal {
  decision: Decision; reason: 'direct' | 'user' | 'timeout' | 'no_interaction' | 'no_match';
}
export type Report = { type: 'receipt'; proposal: Proposal | null; outcome: Outcome; termination: TerminationReport }
  | { type: 'failed'; proposal: Proposal | null; code: FailureCode; message: string };
export interface OperationResultFacts {
  decision_id: DecisionId | null; event: OriginalEvent; hit: RuleHit | null; result: Report;
}
export interface OperationResult extends OperationResultFacts { type: 'operation_result' }
const termination = (value: unknown) => oneOf(value, ['none', 'actor', 'actor_and_parent'] as const);
export function decision(value: unknown): Decision {
  const o = record(value);
  if (o.type === 'allow') { exact(o, ['type']); return { type: 'allow' }; }
  exact(o, ['type', 'terminate']);
  if (o.type !== 'deny') return invalid();
  return { type: 'deny', terminate: termination(o.terminate) };
}
export function hit(value: unknown): RuleHit {
  const o = exact(value, ['group_id', 'group_number', 'rule_id', 'rule_number', 'source', 'log', 'deny_message']);
  const source = exact(o.source, ['kind', 'version']);
  return { group_id: string(o.group_id), group_number: integer(o.group_number, 1, 0xffffffff),
    rule_id: string(o.rule_id), rule_number: integer(o.rule_number, 1, 0xffffffff),
    source: { kind: oneOf(source.kind, ['configuration', 'rules_document'] as const), version: bytes(source.version, 32) },
    log: boolean(o.log), deny_message: o.deny_message === null ? null : string(o.deny_message) };
}
function nonzeroU64(value: unknown): string {
  const decimal = decimalU64(value);
  return decimal === '0' ? invalid() : decimal;
}
export function eventKey(value: unknown): EventKey {
  const o = exact(value, ['source_instance', 'sequence_domain', 'sequence']);
  return { source_instance: bytes(o.source_instance, 16), sequence_domain: nonzeroU64(o.sequence_domain), sequence: nonzeroU64(o.sequence) };
}
export function event(value: unknown): OriginalEvent {
  const o = exact(value, ['header', 'host_boot_id', 'process', 'payload', 'syscall', 'kernel_stack', 'user_stack']);
  const header = record(o.header);
  oneOf(header.kind, ['process', 'file', 'network']);
  oneOf(header.stage, ['observation', 'controlled_request', 'execution_result']);
  integer(header.operation, 0, 0xffffffff);
  const key = eventKey({ source_instance: header.source_instance, sequence_domain: header.sequence_domain, sequence: header.sequence });
  record(o.process); record(o.payload);
  if (o.host_boot_id !== null) bytes(o.host_boot_id, 16);
  if (o.syscall !== null) record(o.syscall);
  if (o.kernel_stack !== null) record(o.kernel_stack);
  if (o.user_stack !== null) record(o.user_stack);
  return { ...o, header: { ...header, ...key } };
}
export function proposal(value: unknown): Proposal | null {
  if (value === null) return null;
  const o = exact(value, ['decision', 'reason']);
  return { decision: decision(o.decision), reason: oneOf(o.reason, ['direct', 'user', 'timeout', 'no_interaction', 'no_match'] as const) };
}
export const nativeError = (value: unknown) => integer(value, -0x80000000, 0x7fffffff);
function signal(value: unknown): Signal {
  const o = record(value);
  if (o.status === 'failed') {
    exact(o, ['status', 'native_error']); return { status: 'failed', native_error: nativeError(o.native_error) };
  }
  exact(o, ['status']);
  return { status: oneOf(o.status, ['not_requested', 'signal_sent', 'not_running'] as const) };
}
export function outcome(value: unknown): Outcome {
  if (typeof value === 'string') return oneOf(value, ['allowed', 'denied', 'cancelled', 'unknown'] as const);
  const failure = exact(exact(value, ['failed']).failed, ['native_error']);
  return { failed: { native_error: nativeError(failure.native_error) } };
}
export function terminationReport(value: unknown): TerminationReport {
  const o = exact(value, ['requested', 'actor', 'parent']);
  return { requested: termination(o.requested), actor: signal(o.actor), parent: signal(o.parent) };
}
function report(value: unknown): Report {
  const o = record(value);
  if (o.type === 'failed') {
    exact(o, ['type', 'proposal', 'code', 'message']);
    return { type: 'failed', proposal: proposal(o.proposal), message: string(o.message), code: failureCode(o.code) };
  }
  exact(o, ['type', 'proposal', 'outcome', 'termination']);
  if (o.type !== 'receipt') return invalid();
  return { type: 'receipt', proposal: proposal(o.proposal), outcome: outcome(o.outcome), termination: terminationReport(o.termination) };
}
export function decodeAsk(value: unknown): Ask {
  const o = exact(value, ['type', 'decision_id', 'hit', 'event', 'choices', 'timeout', 'remaining_ms']);
  if (o.type !== 'ask' || !Array.isArray(o.choices)) return invalid();
  return { type: 'ask', decision_id: bytes(o.decision_id, 16), hit: hit(o.hit), event: event(o.event),
    choices: o.choices.map(decision), timeout: decision(o.timeout), remaining_ms: integer(o.remaining_ms, 0, Number.MAX_SAFE_INTEGER) };
}
export function decodeOperationResult(value: unknown): OperationResult {
  const o = exact(value, ['type', 'decision_id', 'event', 'hit', 'result']);
  if (o.type !== 'operation_result') return invalid();
  return { type: 'operation_result', ...operationResultFacts(o) };
}
export function decodeOperationResultFacts(value: unknown): OperationResultFacts {
  return operationResultFacts(exact(value, ['decision_id', 'event', 'hit', 'result']));
}
function operationResultFacts(o: Record<string, unknown>): OperationResultFacts {
  return { decision_id: o.decision_id === null ? null : bytes(o.decision_id, 16),
    event: event(o.event), hit: o.hit === null ? null : hit(o.hit), result: report(o.result) };
}
export const sameId = (a: DecisionId, b: DecisionId) => a.length === b.length && a.every((byte, i) => byte === b[i]);
export const sameEventKey = (a: EventKey, b: EventKey) => sameId(a.source_instance, b.source_instance)
  && a.sequence_domain === b.sequence_domain && a.sequence === b.sequence;
