// #452 R7 GroupDelegations: the wire flattens GroupPage into BusinessData.
import { arrayOf, boolean, bytes, exact, integer, invalid, numericU64, oneOf, record, string, type Facts } from './wire-json';
import { decodeProcessKey, type ProcessKey } from './processes-wire';
import { businessCommand, type CommandId } from './request-wire';
import { factReference, messageId, partReference, type FactReference } from './context-wire';
import { gatewayReference, rowId, type GatewayReference } from './gateway-wire';

export interface GroupQuery { before: string | null; limit: number }
export const delegationGaps = ['original_task_unavailable', 'task_terminal_unavailable', 'trace_handoff_unavailable',
  'parent_tool_facts_unavailable', 'child_model_exchange_unavailable'] as const;
export interface Delegation extends Facts {
  gap: typeof delegationGaps[number] | null;
  tool_call: readonly FactReference[]; tool_result: readonly FactReference[]; child_responses: readonly FactReference[];
}
export const gatewayGaps = ['gateway_receipt_unobserved', 'gateway_receipt_conflict', 'child_terminal_unobserved',
  'child_terminal_conflict', 'parent_publication_unobserved', 'parent_publication_unverified', 'parent_publication_conflict'] as const;
export interface GatewayObservation {
  reference: GatewayReference;
  fact: Facts;
}
export interface GatewayEvidence {
  accepted_spawns: readonly GatewayObservation[]; child_terminals: readonly GatewayObservation[]; parent_publications: readonly GatewayObservation[];
}
export type GroupDelegation = {
  id: string; kind: 'a2a'; capture_id: readonly number[]; parent: ProcessKey; child: ProcessKey; delegation: Delegation;
} | {
  id: string; kind: 'openclaw_tool_receipt'; actor: ProcessKey; receipt: FactReference;
  call_index: number; output_index: number; tool_call_id: string; child_run_id: string; child_session_key: string;
  gateway: GatewayEvidence; gaps: readonly typeof gatewayGaps[number][];
};
export interface GroupPage { records: readonly GroupDelegation[]; next_before: string | null }
export interface GroupData { type: 'group_delegations'; page: GroupPage }

function reference(value: unknown): FactReference {
  const ref = factReference(value);
  if (ref.kind === 'protocol' || ref.kind === 'message') messageId(ref.value);
  if (ref.kind === 'part') partReference(ref.value);
  return ref;
}
function delegation(value: unknown): Delegation {
  const d = exact(value, ['request_id', 'message_id', 'task_id', 'context_id', 'terminal_state', 'parent_tool_id',
    'parent_tool_span', 'client_span', 'server_span', 'trace_id', 'tool_call', 'tool_result', 'child_responses', 'gap']);
  string(d.message_id);
  for (const field of ['task_id', 'context_id', 'terminal_state', 'parent_tool_id', 'parent_tool_span', 'client_span', 'server_span', 'trace_id']) {
    if (d[field] !== null) string(d[field]);
  }
  for (const field of ['tool_call', 'tool_result', 'child_responses']) arrayOf(d[field], reference);
  if (d.gap !== null) oneOf(d.gap, delegationGaps);
  return d as Delegation;
}

function gatewayObservation(value: unknown, kind: 'accepted_spawn' | 'child_terminal' | 'parent_publication'): GatewayObservation {
  const observation = exact(value, ['reference', 'fact']);
  const ref = gatewayReference(observation.reference);
  if (ref.from_client !== false) invalid();
  const fact = kind === 'accepted_spawn' ? exact(observation.fact,
    ['parent_run_id', 'parent_session_key', 'parent_agent_id', 'tool_call_id', 'child_run_id', 'child_session_key'], ['event_sequence', 'sequence'])
    : kind === 'child_terminal' ? exact(observation.fact, ['source', 'run_id', 'session_key'],
      ['agent_id', 'session_id', 'event_sequence', 'sequence', 'phase', 'status', 'outcome', 'aborted', 'stop_reason', 'execution_settled'])
      : exact(observation.fact, ['run_id', 'session_key'], ['agent_id', 'event_sequence', 'sequence', 'yielded']);
  for (const key of ['parent_run_id', 'parent_session_key', 'parent_agent_id', 'tool_call_id', 'child_run_id', 'child_session_key',
    'run_id', 'session_key', 'agent_id', 'session_id', 'phase']) if (Object.hasOwn(fact, key)) string(fact[key]);
  for (const key of ['sequence', 'event_sequence']) if (Object.hasOwn(fact, key)) numericU64(fact[key]);
  if (Object.hasOwn(fact, 'execution_settled')) boolean(fact.execution_settled);
  if (kind === 'child_terminal') oneOf(fact.source, ['lifecycle', 'session_projection']);
  return observation as unknown as GatewayObservation;
}
function gateway(value: unknown): GatewayEvidence {
  const evidence = exact(value, ['accepted_spawns', 'child_terminals', 'parent_publications']);
  arrayOf(evidence.accepted_spawns, item => gatewayObservation(item, 'accepted_spawn'));
  arrayOf(evidence.child_terminals, item => gatewayObservation(item, 'child_terminal'));
  arrayOf(evidence.parent_publications, item => gatewayObservation(item, 'parent_publication'));
  return evidence as unknown as GatewayEvidence;
}
export function decodeGroupData(value: unknown): GroupData {
  const page = exact(value, ['type', 'records', 'next_before']);
  if (page.type !== 'group_delegations') return invalid();
  const records = arrayOf(page.records, value => {
    const row = record(value);
    rowId(row.id);
    if (row.kind === 'a2a') {
      exact(row, ['id', 'kind', 'capture_id', 'parent', 'child', 'delegation']);
      bytes(row.capture_id, 32); decodeProcessKey(row.parent); decodeProcessKey(row.child); delegation(row.delegation);
    } else {
      exact(row, ['id', 'kind', 'actor', 'receipt', 'call_index', 'output_index', 'tool_call_id', 'child_run_id', 'child_session_key', 'gateway', 'gaps']);
      if (row.kind !== 'openclaw_tool_receipt') return invalid();
      decodeProcessKey(row.actor);
      if (reference(row.receipt).kind !== 'protocol') return invalid();
      integer(row.call_index, 0, 0xffffffff); integer(row.output_index, 0, 0xffffffff);
      string(row.tool_call_id); string(row.child_run_id); string(row.child_session_key);
      gateway(row.gateway);
      const gaps = arrayOf(row.gaps, gap => oneOf(gap, gatewayGaps));
      if (new Set(gaps).size !== gaps.length) invalid();
    }
    return row as GroupDelegation;
  });
  return { type: 'group_delegations', page: { records,
    next_before: page.next_before === null ? null : rowId(page.next_before) } };
}
export function matchesGroupPage(page: GroupPage, query: GroupQuery): boolean {
  return page.records.length <= query.limit
    && page.records.every((row, index) => (query.before === null || BigInt(row.id) < BigInt(query.before))
      && (index === 0 || BigInt(row.id) < BigInt(page.records[index - 1]!.id)))
    && (page.next_before === null || page.next_before === page.records.at(-1)?.id);
}
export function groupDelegationsCommand(id: CommandId, query: GroupQuery): string {
  return businessCommand(id, { type: 'group_delegations',
    before: query.before === null ? null : rowId(query.before), limit: integer(query.limit, 1, 64) });
}
