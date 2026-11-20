// Frozen #452 37eaa413c: process_tree.rs, as-process-tree/model.rs and QueryReply.
// Interpret only consumed ProcessInfo fields; retain every other original field for detail.
import { arrayOf, boolean, byteString, bytes, decimalU64, exact, integer, invalid, numericU64,
  oneOf, record, string, type Facts, type JsonU64 } from './wire-json';
import { decodeWorkload, type Workload } from './workload-wire';
import { managementCommand, type CommandId } from './request-wire';

export interface ProcessKey { boot_id: readonly number[]; host_tgid: number; birth_boottime_ns: string }
export const lifecycles = ['present', 'exit_observed', 'no_longer_present'] as const;
export type Lifecycle = typeof lifecycles[number];
export interface SampleWindow { begin_boottime_ns: string; end_boottime_ns: string }
export type Origin = { kind: 'startup_scan' | 'refresh' } | {
  kind: 'driver_event'; event: { source_instance: readonly number[]; sequence_domain: string; sequence: string };
  actor_tid: number | null; actor_thread_birth_boottime_ns: string | null;
};
export type FieldState = 'known' | 'missing' | 'inaccessible' | 'truncated' | 'stale';
export interface FieldStamp { evidence_epoch: string; origin: Origin; window: SampleWindow; state: FieldState }
export const processFields = ['pid_namespace', 'net_namespace', 'user_namespace', 'mount_namespace',
  'namespace_tgids', 'suid', 'fsuid', 'sgid', 'fsgid', 'name', 'executable', 'argv', 'cwd',
  'uid', 'euid', 'gid', 'egid', 'parent_pid', 'parent_birth', 'parent_name', 'parent_executable',
  'parent_argv', 'workload', 'cgroup', 'exec_cookie', 'kind', 'leader_state'] as const;
export type ProcessField = typeof processFields[number];
export interface ProcessSnapshot {
  process: Facts & { name: readonly number[] | null; executable: readonly number[] | null; workload: Workload | null };
  cgroup_id: string | null; leader_state: number | null;
  stamps: Readonly<Partial<Record<ProcessField, FieldStamp>>>;
}
export interface ProcessRecord {
  key: ProcessKey; initial: ProcessSnapshot; latest: ProcessSnapshot; lifecycle: Lifecycle;
  first_observed: SampleWindow; exit_observed: SampleWindow | null; exit_code: number | null;
  creator: ProcessKey | null; revision: string;
}
export interface ProcessCursor {
  instance: readonly number[]; generation: string; query: readonly number[]; after: ProcessKey; hit_after: string | null;
}
export interface TreeStatus {
  evidence_epoch: string; phase: 'building' | 'ready' | 'uncertain' | 'closed'; instance: readonly number[];
  generation: string; revision: string; nodes: JsonU64; retired: JsonU64; memory_used: JsonU64;
  tracking_epoch: string; marker: string; loss_epoch: string; backend_rejected: string; historical_gap: boolean;
}
export interface ProcessPage {
  records: readonly ProcessRecord[]; next_cursor: ProcessCursor | null; tree_revision: string; status: TreeStatus;
}
export interface ProcessDetails {
  evidence_epoch: string; record: ProcessRecord; initial_parent: Facts; latest_parent: Facts; children: JsonU64;
}
// Other current variants remain accepted as original bounded results, without new requests/state.
export type ProcessQueryReply = { type: 'page'; data: ProcessPage } | { type: 'status'; data: TreeStatus }
  | { type: 'details'; data: ProcessDetails } | { type: 'pid'; data: ProcessKey | null }
  | { type: 'ancestors' | 'hits'; data: Facts };
export interface ProcessesReplyData { type: 'processes'; result: ProcessQueryReply }
export interface ListQuery {
  type: 'list';
  filter: { name: null; executable: null; lifecycle: Lifecycle | null; kind: null; workload: Workload | null };
  page: { limit: number; cursor: ProcessCursor | null };
}
export interface GetQuery { type: 'get'; key: ProcessKey }
export type ProcessesQuery = ListQuery | GetQuery;
export const processPageSizes = [1, 4, 8, 16, 32] as const;
const nullable = <T>(value: unknown, decode: (v: unknown) => T): T | null => value === null ? null : decode(value);
const u32 = (value: unknown) => integer(value, 0, 0xffffffff);
export function decodeProcessKey(value: unknown): ProcessKey {
  const o = exact(value, ['boot_id', 'host_tgid', 'birth_boottime_ns']);
  return { boot_id: bytes(o.boot_id, 16), host_tgid: u32(o.host_tgid), birth_boottime_ns: decimalU64(o.birth_boottime_ns) };
}
function window(value: unknown): SampleWindow {
  const o = exact(value, ['begin_boottime_ns', 'end_boottime_ns']);
  return { begin_boottime_ns: decimalU64(o.begin_boottime_ns), end_boottime_ns: decimalU64(o.end_boottime_ns) };
}
function origin(value: unknown): Origin {
  const o = record(value);
  if (o.kind === 'startup_scan' || o.kind === 'refresh') { exact(o, ['kind']); return { kind: o.kind }; }
  exact(o, ['kind', 'event', 'actor_tid', 'actor_thread_birth_boottime_ns']);
  if (o.kind !== 'driver_event') return invalid();
  const e = exact(o.event, ['source_instance', 'sequence_domain', 'sequence']);
  return { kind: 'driver_event', event: { source_instance: bytes(e.source_instance, 16),
    sequence_domain: decimalU64(e.sequence_domain), sequence: decimalU64(e.sequence) },
  actor_tid: nullable(o.actor_tid, u32), actor_thread_birth_boottime_ns: nullable(o.actor_thread_birth_boottime_ns, decimalU64) };
}
function stamp(value: unknown): FieldStamp {
  const o = exact(value, ['evidence_epoch', 'origin', 'window', 'state']);
  return { evidence_epoch: decimalU64(o.evidence_epoch), origin: origin(o.origin), window: window(o.window),
    state: oneOf(o.state, ['known', 'missing', 'inaccessible', 'truncated', 'stale'] as const) };
}
function snapshot(value: unknown): ProcessSnapshot {
  const o = exact(value, ['process', 'cgroup_id', 'leader_state', 'stamps']);
  const p = record(o.process);
  const fields = exact(o.stamps, [], processFields);
  return { process: { ...p, name: nullable(p.name, byteString), executable: nullable(p.executable, byteString),
    workload: nullable(p.workload, decodeWorkload) }, cgroup_id: nullable(o.cgroup_id, decimalU64),
  leader_state: nullable(o.leader_state, v => integer(v, 0, 255)),
  stamps: Object.fromEntries(Object.entries(fields).map(([field, value]) => [field, stamp(value)])) };
}
function processRecord(value: unknown): ProcessRecord {
  const o = exact(value, ['key', 'initial', 'latest', 'lifecycle', 'first_observed', 'exit_observed', 'exit_code', 'creator', 'revision']);
  return { key: decodeProcessKey(o.key), initial: snapshot(o.initial), latest: snapshot(o.latest),
    lifecycle: oneOf(o.lifecycle, lifecycles), first_observed: window(o.first_observed),
    exit_observed: nullable(o.exit_observed, window), exit_code: nullable(o.exit_code, v => integer(v, -0x80000000, 0x7fffffff)),
    creator: nullable(o.creator, decodeProcessKey), revision: decimalU64(o.revision) };
}
function cursor(value: unknown): ProcessCursor {
  const o = exact(value, ['instance', 'generation', 'query', 'after', 'hit_after']);
  // Validate without modifying this opaque value; the next request returns it whole.
  bytes(o.instance, 16); decimalU64(o.generation); bytes(o.query, 32); decodeProcessKey(o.after);
  nullable(o.hit_after, string);
  return o as unknown as ProcessCursor;
}
function status(value: unknown): TreeStatus {
  const o = exact(value, ['evidence_epoch', 'phase', 'instance', 'generation', 'revision', 'nodes', 'retired',
    'memory_used', 'tracking_epoch', 'marker', 'loss_epoch', 'backend_rejected', 'historical_gap']);
  return { evidence_epoch: decimalU64(o.evidence_epoch), phase: oneOf(o.phase, ['building', 'ready', 'uncertain', 'closed'] as const),
    instance: bytes(o.instance, 16), generation: decimalU64(o.generation), revision: decimalU64(o.revision),
    nodes: numericU64(o.nodes), retired: numericU64(o.retired), memory_used: numericU64(o.memory_used),
    tracking_epoch: decimalU64(o.tracking_epoch), marker: decimalU64(o.marker), loss_epoch: decimalU64(o.loss_epoch),
    backend_rejected: decimalU64(o.backend_rejected), historical_gap: boolean(o.historical_gap) };
}
export function decodeProcessesData(value: unknown): ProcessesReplyData {
  const o = exact(value, ['type', 'result']);
  if (o.type !== 'processes') return invalid();
  const r = exact(o.result, ['type', 'data']);
  let result: ProcessQueryReply;
  if (r.type === 'page') {
    const p = exact(r.data, ['records', 'next_cursor', 'tree_revision', 'status']);
    result = { type: 'page', data: { records: arrayOf(p.records, processRecord), next_cursor: nullable(p.next_cursor, cursor),
      tree_revision: decimalU64(p.tree_revision), status: status(p.status) } };
  } else if (r.type === 'status') result = { type: 'status', data: status(r.data) };
  else if (r.type === 'pid') result = { type: 'pid', data: nullable(r.data, decodeProcessKey) };
  else if (r.type === 'details') {
    const d = exact(r.data, ['evidence_epoch', 'record', 'initial_parent', 'latest_parent', 'children']);
    result = { type: 'details', data: { evidence_epoch: decimalU64(d.evidence_epoch), record: processRecord(d.record),
      initial_parent: record(d.initial_parent), latest_parent: record(d.latest_parent), children: numericU64(d.children) } };
  } else {
    const type = oneOf(r.type, ['ancestors', 'hits'] as const);
    const d = record(r.data);
    exact(d, type === 'hits' ? ['records', 'next_cursor', 'incomplete'] : ['records', 'incomplete']);
    arrayOf(d.records, record); boolean(d.incomplete);
    if (type === 'hits') nullable(d.next_cursor, cursor);
    result = { type, data: d };
  }
  return { type: 'processes', result };
}
export function listQuery(limit = 8, lifecycle: Lifecycle | null = null, workload: Workload | null = null): ListQuery {
  integer(limit, 1, 32);
  if (!processPageSizes.some(size => size === limit)) return invalid();
  if (lifecycle !== null) oneOf(lifecycle, lifecycles);
  return { type: 'list', filter: { name: null, executable: null, lifecycle, kind: null,
    workload: workload === null ? null : decodeWorkload(workload) }, page: { limit, cursor: null } };
}
export function processesCommand(id: CommandId, query: ProcessesQuery): string {
  if (query.type === 'get') { exact(query, ['type', 'key']); decodeProcessKey(query.key); }
  return managementCommand(id, 'processes', { type: 'query', query });
}
