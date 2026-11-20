// Current context.rs / attribution.rs / audit.rs. Validate reads and retain original source objects.
import { arrayOf, bytes, decimalU64, exact, integer, invalid, oneOf, record, string, byteString, type Facts } from './wire-json';
import { decodeAsk, decodeOperationResultFacts, eventKey, nativeError, sameEventKey, type Ask, type EventKey,
  type OperationResultFacts } from './event-wire';
import { decodeProcessKey, type ProcessKey } from './processes-wire';
import { businessCommand, type CommandId } from './request-wire';
import { decodeSessionObservation } from './session-wire';
import { decodeRequestHandoff } from './runtime-observation-wire';
import { sameJsonValue } from './global/json-value';

export type FactReference = { kind: 'event' | 'completion' | 'native_observation'; value: EventKey }
  | { kind: 'process'; value: ProcessKey } | { kind: 'question'; value: readonly number[] }
  | { kind: 'protocol' | 'message' | 'part'; value: Facts };
export interface RequestReturn {
  event: EventKey; operation: 'file_open' | 'execute' | 'connect' | 'send'; decision_id: readonly number[];
  wait_id: string; returned_boottime_ns: string; return_value: number;
}
export interface SyncThread { tgid: number; tid: number; birth_boottime_ns: string }
export interface FutexSelection {
  event: EventKey; waker: SyncThread; waiter: SyncThread; selected_boottime_ns: string;
  wait: { id: string; observed_boottime_ns: string } | null;
}
const feedbackFailures = ['source_unavailable', 'target_not_found', 'ambiguous_target', 'too_late',
  'unsupported_body', 'capacity', 'cancelled', 'transport_error'] as const;
export type RuleFeedbackState = { type: 'prepared' }
  | { type: 'applied'; original: MessageId; modified_body: Facts }
  | { type: 'submitted'; original: MessageId }
  | { type: 'failed'; original: MessageId | null; code: typeof feedbackFailures[number] };
export interface RuleFeedbackFact { denial: EventKey; call: PartReference; state: RuleFeedbackState }

export type SourceRecord = { kind: 'request_return'; value: RequestReturn }
  | { kind: 'deferred'; value: FactReference }
  | { kind: 'protocol_page'; value: { observation: Facts; next_offset: string | null } }
  | { kind: 'futex_selection'; value: FutexSelection }
  | { kind: 'request_handoff'; value: Facts }
  | { kind: 'rule_feedback'; value: RuleFeedbackFact }
  | { kind: 'question'; value: Ask } | { kind: 'operation_result'; value: OperationResultFacts }
  | { kind: 'tool_execution' | 'protocol' | 'message'; value: Facts };
export const relationKinds = ['same_exchange', 'native_tool_reference', 'question_completion', 'tool_execution',
  'native_request_reference', 'native_call_scope', 'same_thread_return_sequence', 'tool_reference_candidate', 'tool_container_candidate'] as const;
export interface EvidenceRelation {
  kind: typeof relationKinds[number]; from: FactReference; to: FactReference; evidence: readonly FactReference[];
}
export function isToolCandidateKind(kind: EvidenceRelation['kind']): boolean {
  return kind === 'tool_reference_candidate' || kind === 'tool_container_candidate';
}
export const gapReasons = ['agent_binding_unavailable', 'command_mapping_unavailable', 'command_input_unavailable',
  'not_provided', 'not_retained', 'source_unavailable', 'no_explicit_relation', 'conflicting_evidence'] as const;
export interface ContextGap { at: FactReference; reason: typeof gapReasons[number] }
export interface ContextView {
  anchor: FactReference; source_records: readonly SourceRecord[]; relations: readonly EvidenceRelation[]; gaps: readonly ContextGap[];
}
export interface ActivityContextData { type: 'activity_context'; view: ContextView; attribution: ToolAttribution | null }
export interface ExchangeKey { flow_instance: readonly number[]; sequence: string }
export interface MessageId { exchange: ExchangeKey; sequence: string }
export interface PartReference { message: MessageId; part_id: number }
export type ActorScope = 'same_process' | 'physical_parent' | 'same_thread';
export type TargetPattern = { type: 'exec_argument'; pointer: string; index: number }
  | { type: 'executable' | 'file_path'; pointer: string }
  | { type: 'network_peer'; address_pointer: string; port_pointer: string };
export interface AttributionPattern { actor: ActorScope; lookback_ns: string; target: TargetPattern }
export interface AttributionRequest { pattern: AttributionPattern; after: MessageId | null; limit: number }
export type ObservedTarget = { type: 'bytes'; value: readonly number[] }
  | { type: 'network_peer'; address: string; port: number };
export interface ThreadIdentity { tid: number; birth_boottime_ns: string }
export type TargetComparison = 'equal' | 'different' | 'event_target_unavailable' | 'argument_unavailable' | 'source_incomplete';
export interface AttributionCandidate {
  call: PartReference; message_revision: string; response_observed_ns: string; target_comparison: TargetComparison;
}
export interface ToolAttribution {
  event: EventKey; pattern: AttributionPattern; required_connection_process: ProcessKey;
  required_connection_thread: ThreadIdentity | null; event_boottime_ns: string; event_target: ObservedTarget | null;
  candidates: readonly AttributionCandidate[]; next_after: MessageId | null;
}
export interface ToolAttributionData { type: 'tool_attribution'; view: ToolAttribution }
export interface AskMessageWindow {
  exchange: ExchangeKey; message_refs: readonly MessageId[]; earlier_observed_messages: number; source_gaps: readonly Facts[];
}
export interface CommandCheck {
  call: PartReference; operation: EventKey; argument_pointer: string; exec_argument_index: number;
  outcome: 'equal_bytes' | 'different_bytes' | 'not_comparable'; evidence: readonly FactReference[];
}
export interface AskAuditContext {
  decision_id: readonly number[]; event: EventKey; agent: ProcessKey | null; trigger_call: PartReference | null;
  window: AskMessageWindow | null; view: ContextView; command_checks: readonly CommandCheck[];
  attribution: ToolAttribution | null; candidate_windows: readonly AskMessageWindow[];
}
export interface AskContextData { type: 'ask_context'; context: AskAuditContext }
export function exchangeKey(value: unknown): ExchangeKey {
  const o = exact(value, ['flow_instance', 'sequence']);
  return { flow_instance: bytes(o.flow_instance, 16), sequence: decimalU64(o.sequence) };
}
export function messageId(value: unknown): MessageId {
  const o = exact(value, ['exchange', 'sequence']);
  return { exchange: exchangeKey(o.exchange), sequence: decimalU64(o.sequence) };
}
export function partReference(value: unknown): PartReference {
  const o = exact(value, ['message', 'part_id']);
  return { message: messageId(o.message), part_id: integer(o.part_id, 0, 0xffffffff) };
}
function jsonPointer(value: unknown): string {
  const pointer = string(value);
  if ((pointer !== '' && !pointer.startsWith('/')) || /~(?![01])/.test(pointer)) return invalid();
  return pointer;
}
function targetPattern(value: unknown): TargetPattern {
  const o = record(value);
  switch (o.type) {
    case 'exec_argument':
      exact(o, ['type', 'pointer', 'index']); jsonPointer(o.pointer); integer(o.index, 0, 0xffffffff); break;
    case 'executable':
    case 'file_path':
      exact(o, ['type', 'pointer']); jsonPointer(o.pointer); break;
    case 'network_peer':
      exact(o, ['type', 'address_pointer', 'port_pointer']); jsonPointer(o.address_pointer); jsonPointer(o.port_pointer); break;
    default:
      return invalid();
  }
  return o as TargetPattern;
}
export function decodeAttributionPattern(value: unknown): AttributionPattern {
  const o = exact(value, ['actor', 'lookback_ns', 'target']);
  oneOf(o.actor, ['same_process', 'physical_parent', 'same_thread']);
  decimalU64(o.lookback_ns); targetPattern(o.target);
  return o as unknown as AttributionPattern;
}
export function decodeAttributionRequest(value: unknown): AttributionRequest {
  const o = exact(value, ['pattern', 'after', 'limit']);
  decodeAttributionPattern(o.pattern); if (o.after !== null) messageId(o.after); integer(o.limit, 1, 64);
  return o as unknown as AttributionRequest;
}
function ipAddress(value: unknown): string {
  const address = string(value);
  if (address.includes(':')) {
    if (!/^[0-9a-fA-F:.]+$/.test(address)) return invalid();
    try { new URL(`http://[${address}]/`); } catch { return invalid(); }
  } else if (!/^(?:0|[1-9][0-9]{0,2})(?:\.(?:0|[1-9][0-9]{0,2})){3}$/.test(address)
    || address.split('.').some(octet => Number(octet) > 255)) return invalid();
  return address;
}
function observedTarget(value: unknown): ObservedTarget {
  const o = record(value);
  if (o.type === 'bytes') { exact(o, ['type', 'value']); byteString(o.value); }
  else if (o.type === 'network_peer') { exact(o, ['type', 'address', 'port']); ipAddress(o.address); integer(o.port, 0, 0xffff); }
  else return invalid();
  return o as ObservedTarget;
}
export function decodeToolAttribution(value: unknown): ToolAttribution {
  const o = exact(value, ['event', 'pattern', 'required_connection_process', 'required_connection_thread',
    'event_boottime_ns', 'event_target', 'candidates', 'next_after']);
  eventKey(o.event); const pattern = decodeAttributionPattern(o.pattern); decodeProcessKey(o.required_connection_process);
  if (o.required_connection_thread !== null) {
    const thread = exact(o.required_connection_thread, ['tid', 'birth_boottime_ns']);
    integer(thread.tid, 0, 0xffffffff); decimalU64(thread.birth_boottime_ns);
  }
  if ((pattern.actor === 'same_thread') !== (o.required_connection_thread !== null)) return invalid();
  decimalU64(o.event_boottime_ns); if (o.event_target !== null) observedTarget(o.event_target);
  arrayOf(o.candidates, value => {
    const candidate = exact(value, ['call', 'message_revision', 'response_observed_ns', 'target_comparison']);
    partReference(candidate.call); decimalU64(candidate.message_revision); decimalU64(candidate.response_observed_ns);
    oneOf(candidate.target_comparison, ['equal', 'different', 'event_target_unavailable', 'argument_unavailable', 'source_incomplete']);
  });
  if (o.next_after !== null) messageId(o.next_after);
  // A comparison remains its original read-only evidence; no edge, selection or copy is produced.
  return o as unknown as ToolAttribution;
}
export function decodeToolAttributionData(value: unknown): ToolAttributionData {
  const o = exact(value, ['type', 'view']);
  if (o.type !== 'tool_attribution') return invalid();
  decodeToolAttribution(o.view);
  return o as unknown as ToolAttributionData;
}
// Requested evidence can be unavailable; the original ContextView then carries its gap.
// The pending read owns this identity check so a mismatch never disconnects a live Ask.
export function matchesAttribution(result: ToolAttribution | null, request: AttributionRequest | null, event: EventKey): boolean {
  return result === null || (request !== null && sameEventKey(result.event, event) && sameJsonValue(result.pattern, request.pattern));
}
export function isMessageId(value: unknown): value is MessageId & Facts {
  try { messageId(value); return true; } catch { return false; }
}
export function isPartReference(value: unknown): value is PartReference & Facts {
  try { partReference(value); return true; } catch { return false; }
}
export function factReference(value: unknown): FactReference {
  const o = exact(value, ['kind', 'value']);
  if (o.kind === 'event' || o.kind === 'completion' || o.kind === 'native_observation') return { kind: o.kind, value: eventKey(o.value) };
  if (o.kind === 'process') return { kind: o.kind, value: decodeProcessKey(o.value) };
  if (o.kind === 'question') return { kind: o.kind, value: bytes(o.value, 16) };
  return { kind: oneOf(o.kind, ['protocol', 'message', 'part'] as const), value: record(o.value) };
}
function syncThread(value: unknown): SyncThread {
  const o = exact(value, ['tgid', 'tid', 'birth_boottime_ns']);
  return { tgid: integer(o.tgid, 0, 0xffffffff), tid: integer(o.tid, 0, 0xffffffff), birth_boottime_ns: decimalU64(o.birth_boottime_ns) };
}
function feedbackState(value: unknown): RuleFeedbackState {
  const state = record(value);
  switch (state.type) {
    case 'prepared':
      exact(state, ['type']);
      return { type: state.type };
    case 'failed':
      exact(state, ['type', 'original', 'code']);
      return { type: 'failed', original: state.original === null ? null : messageId(state.original),
        code: oneOf(state.code, feedbackFailures) };
    case 'submitted':
      exact(state, ['type', 'original']);
      return { type: 'submitted', original: messageId(state.original) };
    case 'applied': {
      exact(state, ['type', 'original', 'modified_body']);
      const body = exact(state.modified_body, ['original_length', 'segments']);
      if (body.original_length !== null) decimalU64(body.original_length);
      arrayOf(body.segments, value => {
        const segment = record(value);
        if (segment.kind === 'bytes') {
          exact(segment, ['kind', 'offset', 'data']);
          byteString(segment.data);
        } else if (segment.kind === 'missing') {
          exact(segment, ['kind', 'offset', 'length', 'reason']);
          if (segment.length !== null) decimalU64(segment.length);
          string(segment.reason);
        } else return invalid();
        decimalU64(segment.offset);
      });
      return { type: 'applied', original: messageId(state.original), modified_body: body };
    }
    default:
      return invalid();
  }
}

function sourceRecord(value: unknown): SourceRecord {
  const o = exact(value, ['kind', 'value']);
  if (o.kind === 'deferred') return { kind: o.kind, value: factReference(o.value) };
  if (o.kind === 'protocol_page') {
    const value = exact(o.value, ['observation', 'next_offset']);
    return { kind: o.kind, value: { observation: record(value.observation),
      next_offset: value.next_offset === null ? null : decimalU64(value.next_offset) } };
  }
  if (o.kind === 'request_handoff') return { kind: o.kind, value: decodeRequestHandoff(o.value) };
  if (o.kind === 'question') return { kind: o.kind, value: decodeAsk(o.value) };
  if (o.kind === 'operation_result') return { kind: o.kind, value: decodeOperationResultFacts(o.value) };
  if (o.kind === 'rule_feedback') {
    const v = exact(o.value, ['denial', 'call', 'state']);
    return { kind: 'rule_feedback', value: { denial: eventKey(v.denial), call: partReference(v.call), state: feedbackState(v.state) } };
  }
  if (o.kind === 'request_return') {
    const v = exact(o.value, ['event', 'operation', 'decision_id', 'wait_id', 'returned_boottime_ns', 'return_value']);
    return { kind: o.kind, value: { event: eventKey(v.event),
      operation: oneOf(v.operation, ['file_open', 'execute', 'connect', 'send'] as const), decision_id: bytes(v.decision_id, 16),
      wait_id: decimalU64(v.wait_id), returned_boottime_ns: decimalU64(v.returned_boottime_ns), return_value: nativeError(v.return_value) } };
  }
  if (o.kind === 'futex_selection') {
    const v = exact(o.value, ['event', 'waker', 'waiter', 'selected_boottime_ns', 'wait']);
    const wait = v.wait === null ? null : exact(v.wait, ['id', 'observed_boottime_ns']);
    return { kind: o.kind, value: { event: eventKey(v.event), waker: syncThread(v.waker), waiter: syncThread(v.waiter),
      selected_boottime_ns: decimalU64(v.selected_boottime_ns), wait: wait === null ? null
        : { id: decimalU64(wait.id), observed_boottime_ns: decimalU64(wait.observed_boottime_ns) } } };
  }
  const kind = oneOf(o.kind, ['tool_execution', 'protocol', 'message'] as const);
  const original = record(o.value);
  if (kind === 'protocol' && original.request_session !== null) decodeSessionObservation(original.request_session);
  if (kind === 'tool_execution') { eventKey(original.event); record(original.context); }
  return { kind, value: original };
}
export function decodeActivityContext(value: unknown): ActivityContextData {
  const o = exact(value, ['type', 'view', 'attribution']);
  if (o.type !== 'activity_context') return invalid();
  decodeContextView(o.view);
  if (o.attribution !== null) decodeToolAttribution(o.attribution);
  return o as unknown as ActivityContextData;
}
export function decodeContextView(value: unknown): ContextView {
  const v = exact(value, ['anchor', 'source_records', 'relations', 'gaps']);
  factReference(v.anchor);
  arrayOf(v.source_records, sourceRecord);
  arrayOf(v.relations, value => {
    const r = exact(value, ['kind', 'from', 'to', 'evidence']);
    const evidence = arrayOf(r.evidence, factReference);
    if (evidence.length === 0) return invalid();
    const kind = oneOf(r.kind, relationKinds), from = factReference(r.from), to = factReference(r.to);
    if (isToolCandidateKind(kind)) {
      if (from.kind !== 'part' || (to.kind !== 'event' && to.kind !== 'part')) return invalid();
      if (kind === 'tool_container_candidate' && to.kind !== 'event') return invalid();
      partReference(from.value);
      if (to.kind === 'part') partReference(to.value);
    }
    if (kind === 'native_request_reference' && (from.kind !== 'native_observation' || to.kind !== 'native_observation')) return invalid();
    if (kind === 'native_call_scope' && (from.kind !== 'event' || to.kind !== 'native_observation')) return invalid();
    if (kind === 'same_thread_return_sequence' && (from.kind !== 'event' || to.kind !== 'native_observation')) return invalid();
    return { kind, from, to, evidence };
  });
  arrayOf(v.gaps, value => {
    const g = exact(value, ['at', 'reason']);
    return { at: factReference(g.at), reason: oneOf(g.reason, gapReasons) };
  });
  // Validation must not rebuild, reorder or trim the original evidence.
  return v as unknown as ContextView;
}
export function activityContextCommand(id: CommandId, anchor: FactReference, attribution: AttributionRequest | null = null): string {
  factReference(anchor);
  if (anchor.kind !== 'event' && anchor.kind !== 'native_observation') return invalid();
  if (anchor.kind === 'native_observation' && attribution !== null) return invalid();
  if (attribution !== null) decodeAttributionRequest(attribution);
  return businessCommand(id, { type: 'activity_context', anchor, attribution });
}
export function originalRecordCommand(id: CommandId, reference: FactReference, bodyOffset = '0'): string {
  factReference(reference); decimalU64(bodyOffset);
  if (reference.kind !== 'protocol' && reference.kind !== 'native_observation') return invalid();
  return businessCommand(id, { type: 'original_record', reference, body_offset: bodyOffset, body_limit: 65536 });
}
function messageWindow(value: unknown): AskMessageWindow {
  const o = exact(value, ['exchange', 'message_refs', 'earlier_observed_messages', 'source_gaps']);
  exchangeKey(o.exchange);
  if (arrayOf(o.message_refs, messageId).length > 20) return invalid();
  integer(o.earlier_observed_messages, 0, 0xffffffff); arrayOf(o.source_gaps, record);
  return o as unknown as AskMessageWindow;
}
export function decodeAskContext(value: unknown): AskContextData {
  const o = exact(value, ['type', 'context']);
  if (o.type !== 'ask_context') return invalid();
  const c = exact(o.context, ['decision_id', 'event', 'agent', 'trigger_call', 'window', 'view',
    'command_checks', 'attribution', 'candidate_windows']);
  bytes(c.decision_id, 16); eventKey(c.event); decodeContextView(c.view);
  if (c.agent !== null) decodeProcessKey(c.agent);
  if (c.trigger_call !== null) partReference(c.trigger_call);
  if (c.window !== null) messageWindow(c.window);
  arrayOf(c.command_checks, value => {
    const check = exact(value, ['call', 'operation', 'argument_pointer', 'exec_argument_index', 'outcome', 'evidence']);
    partReference(check.call); eventKey(check.operation); string(check.argument_pointer);
    integer(check.exec_argument_index, 0, 0xffffffff);
    oneOf(check.outcome, ['equal_bytes', 'different_bytes', 'not_comparable']);
    arrayOf(check.evidence, factReference);
  });
  if (c.attribution !== null) decodeToolAttribution(c.attribution);
  arrayOf(c.candidate_windows, messageWindow);
  return o as unknown as AskContextData;
}
export function askContextCommand(id: CommandId, decisionId: readonly number[], attribution: AttributionRequest | null = null): string {
  if (attribution !== null) decodeAttributionRequest(attribution);
  return businessCommand(id, { type: 'ask_context', decision_id: bytes(decisionId, 16), attribution });
}
