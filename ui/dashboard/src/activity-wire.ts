// Security-audit contract: as-types/{security_log,main_ports}.rs.
import { byteString, bytes, decimalU64, exact, integer, invalid, oneOf, record } from './wire-json';
import { event, hit, outcome, proposal, terminationReport, type DecisionId, type OriginalEvent,
  type Outcome, type Proposal, type RuleHit, type TerminationReport } from './event-wire';
import { managementCommand, type CommandId } from './request-wire';

export interface LogQuery { before: string | null; limit: number }
export type LogDecision = { state: 'pending' | 'transferred' } | ({ state: 'resolved' } & Proposal);
export type DispatchResult = { Ok: 'observed' | { driver: { outcome: Outcome; termination: TerminationReport } }
  | { execution: { decision_id: DecisionId; outcome: Outcome; termination: TerminationReport } } }
  | { Err: 'unsupported' | 'conflict' | 'forbidden' | 'unauthenticated' | 'unavailable' | 'invalid_input' | 'stopped' | 'busy' };
export interface RuleLog {
  decision_id: DecisionId | null; event: OriginalEvent; hit: RuleHit; decision: LogDecision; result: DispatchResult;
}
export type CloseLog = { kind: 'file_write_close'; reporter_tid: number; path: readonly number[] | null;
  path_error: number | null; device_major: number; device_minor: number; inode: string };
export type LogEntry = RuleLog | CloseLog;
export interface LogRow { id: string; record: LogEntry }
export interface LogPage { records: readonly LogRow[]; next_before: string | null }
export interface SecurityLogData { type: 'security_log'; page: LogPage }
export const logPageSizes = [8, 16] as const;
export const isRuleLog = (entry: LogEntry): entry is RuleLog => !('kind' in entry);
export function logRowId(value: unknown): string {
  const id = decimalU64(value);
  return id === '0' || BigInt(id) > 9223372036854775807n ? invalid() : id;
}
function dispatchResult(value: unknown): DispatchResult {
  const o = record(value);
  if (Object.hasOwn(o, 'Err')) {
    exact(o, ['Err']);
    return { Err: oneOf(o.Err, ['unsupported', 'conflict', 'forbidden', 'unauthenticated', 'unavailable', 'invalid_input', 'stopped', 'busy'] as const) };
  }
  exact(o, ['Ok']);
  if (o.Ok === 'observed') return { Ok: 'observed' };
  const ok = record(o.Ok);
  if (Object.hasOwn(ok, 'driver')) {
    const driver = exact(exact(ok, ['driver']).driver, ['outcome', 'termination']);
    return { Ok: { driver: { outcome: outcome(driver.outcome), termination: terminationReport(driver.termination) } } };
  }
  const execution = exact(exact(ok, ['execution']).execution, ['decision_id', 'outcome', 'termination']);
  return { Ok: { execution: { decision_id: bytes(execution.decision_id, 16),
    outcome: outcome(execution.outcome), termination: terminationReport(execution.termination) } } };
}
export function decodeLogEntry(value: unknown): LogEntry {
  const o = record(value);
  if (!Object.hasOwn(o, 'kind')) {
    exact(o, ['decision_id', 'event', 'hit', 'decision', 'result']);
    const d = record(o.decision);
    let decision: LogDecision;
    if (d.state === 'resolved') {
      exact(d, ['state', 'decision', 'reason']);
      decision = { state: 'resolved', ...proposal({ decision: d.decision, reason: d.reason })! };
    } else {
      exact(d, ['state']); decision = { state: oneOf(d.state, ['pending', 'transferred'] as const) };
    }
    return { decision_id: o.decision_id === null ? null : bytes(o.decision_id, 16), event: event(o.event),
      hit: hit(o.hit), decision, result: dispatchResult(o.result) };
  }
  if (o.kind !== 'file_write_close') return invalid();
  exact(o, ['kind', 'reporter_tid', 'path', 'path_error', 'device_major', 'device_minor', 'inode']);
  return { kind: 'file_write_close', reporter_tid: integer(o.reporter_tid, -0x80000000, 0x7fffffff),
    path: o.path === null ? null : byteString(o.path),
    path_error: o.path_error === null ? null : integer(o.path_error, -4095, -1),
    device_major: integer(o.device_major, 0, 0xffffffff),
    device_minor: integer(o.device_minor, 0, 0xffffffff), inode: decimalU64(o.inode) };
}
export function decodeSecurityLog(value: unknown): SecurityLogData {
  const o = exact(value, ['type', 'page']);
  if (o.type !== 'security_log') return invalid();
  const page = exact(o.page, ['records', 'next_before']);
  if (!Array.isArray(page.records)) return invalid();
  const records = page.records.map(value => {
    const row = exact(value, ['id', 'record']);
    return { id: logRowId(row.id), record: decodeLogEntry(row.record) };
  });
  if (records.some((row, i) => i > 0 && BigInt(row.id) >= BigInt(records[i - 1]!.id))) return invalid();
  const next_before = page.next_before === null ? null : logRowId(page.next_before);
  if (next_before !== null && next_before !== records.at(-1)?.id) return invalid();
  return { type: 'security_log', page: { records, next_before } };
}
export const matchesLogPage = (page: LogPage, query: LogQuery) => page.records.length <= query.limit
  && page.records.every(row => query.before === null || BigInt(row.id) < BigInt(query.before));
export function securityLogCommand(id: CommandId, query: LogQuery): string {
  return managementCommand(id, 'security_log', {
    before: query.before === null ? null : logRowId(query.before), limit: integer(query.limit, 1, 64),
  });
}
