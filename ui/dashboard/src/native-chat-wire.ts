import { arrayOf, decimalU64, exact, integer, invalid, oneOf, record, string, decodeChatContent } from './wire-json';
import { gatewayConnection, gatewayField, gatewayReference, rowId, type GatewayConnection, type GatewayFieldReference, type GatewayReference } from './gateway-wire';
import { decodeProcessKey, type ProcessKey } from './processes-wire';
import { messageId, type MessageId, type ExchangeKey } from './context-wire';
import { decodeNativeIdValue } from './session-wire';
import { sameJsonValue } from './global/json-value';
import type { ChatContent, ChatRole } from './chat-wire';
export type NativeIdValue = ReturnType<typeof decodeNativeIdValue>;
export type ChatSessionRef = { kind: 'model_http'; record: MessageId } | { kind: 'native_gateway'; object: GatewayFieldReference };
export type ChatMessageRef = { kind: 'model_http'; message: MessageId } | { kind: 'native_gateway'; object: GatewayFieldReference };
export type ChatMessageScope = { kind: 'model_http'; exchange: ExchangeKey } | { kind: 'native_gateway'; session: ChatSessionRef | null };
export type ChatMessageCursor = { kind: 'model_http'; message: MessageId } | { kind: 'native_gateway'; reference: ChatMessageRef; read_through: string };
export interface GatewaySessionObservation { reference: GatewayFieldReference; session_id: string | null; session_key: string | null;
  run_id: string | null; last_run_id: string | null; agent_id: string | null; request: GatewayReference | null }
export interface SessionGap { reason: string; references: readonly GatewayFieldReference[] }
export type GatewayChatRole = { kind: 'string'; value: ChatRole; original: string; field: GatewayFieldReference }
  | { kind: 'json'; value: unknown; field: GatewayFieldReference } | { kind: 'protocol_input'; value: ChatRole; method: GatewayFieldReference };
export interface GatewayChatObservation { source: GatewayConnection; reference: GatewayFieldReference; wrapper: GatewayFieldReference | null;
  history: GatewayFieldReference | null; session: ChatSessionRef | null; handoff: GatewaySessionObservation | null;
  run_id: string | null; message_run_id: string | null; message_sequence: NativeIdValue | null; wrapper_sequence: NativeIdValue | null;
  record_timestamp_ms?: unknown; transcript_position?: unknown; native_fields: readonly GatewayFieldReference[];
  role: GatewayChatRole | null; content: ChatContent | null; state?: unknown; revision: NativeIdValue | null }
export type GatewayChatOrder = { kind: 'session_sequence'; sequence: NativeIdValue; references: readonly GatewayFieldReference[] }
  | { kind: 'history_window'; window: GatewayFieldReference; index: number } | { kind: 'unresolved'; references: readonly GatewayFieldReference[] };
export interface NativeChatEntry { kind: 'native_gateway'; reference: ChatMessageRef; native_message_id: NativeIdValue | null;
  order: GatewayChatOrder; observations: readonly GatewayChatObservation[]; gaps: readonly SessionGap[] }
export interface NativeChatSession { kind: 'native_gateway'; reference: ChatSessionRef; namespace: { server: ProcessKey; server_port: number };
  source: GatewayConnection; session: GatewaySessionObservation; chat_observations: string; gaps: readonly SessionGap[] }
export function chatRole(value: unknown): ChatRole {
  const o = record(value); if (o.kind === 'other') { exact(o, ['kind', 'value']); string(o.value); }
  else { exact(o, ['kind']); oneOf(o.kind, ['system', 'developer', 'user', 'assistant', 'tool']); } return o as ChatRole;
}
export function sessionRef(value: unknown): ChatSessionRef {
  const o = record(value); if (o.kind === 'model_http') { exact(o, ['kind', 'record']); messageId(o.record); }
  else { exact(o, ['kind', 'object']); if (o.kind !== 'native_gateway') invalid(); gatewayField(o.object); } return o as ChatSessionRef;
}
export function messageRef(value: unknown): ChatMessageRef {
  const o = record(value); if (o.kind === 'model_http') { exact(o, ['kind', 'message']); messageId(o.message); }
  else { exact(o, ['kind', 'object']); if (o.kind !== 'native_gateway') invalid(); gatewayField(o.object); } return o as ChatMessageRef;
}
export function messageCursor(value: unknown): ChatMessageCursor {
  const o = record(value); if (o.kind === 'model_http') { exact(o, ['kind', 'message']); messageId(o.message); }
  else { exact(o, ['kind', 'reference', 'read_through']); if (o.kind !== 'native_gateway' || messageRef(o.reference).kind !== 'native_gateway') invalid(); rowId(o.read_through); }
  return o as ChatMessageCursor;
}
export function sessionObservation(value: unknown): GatewaySessionObservation {
  const o = exact(value, ['reference', 'session_id', 'session_key', 'run_id', 'last_run_id', 'agent_id', 'request']); gatewayField(o.reference);
  for (const f of ['session_id', 'session_key', 'run_id', 'last_run_id', 'agent_id']) if (o[f] !== null) string(o[f]);
  if (o.request !== null) gatewayReference(o.request); return o as unknown as GatewaySessionObservation;
}
export function sessionGaps(value: unknown): readonly SessionGap[] {
  return arrayOf(value, value => { const o = exact(value, ['reason', 'references']);
    oneOf(o.reason, ['unsupported_shape', 'field_type_unsupported', 'session_namespace_unavailable', 'session_identity_unavailable', 'alias_binding_unproven',
      'rpc_unpaired', 'rpc_failed', 'rpc_conflict', 'identity_conflict', 'message_identity_unavailable', 'message_order_unproven', 'message_order_conflict', 'revision_order_unproven']);
    if (!arrayOf(o.references, gatewayField).length) invalid(); return o as unknown as SessionGap; });
}
export function nativeSession(value: unknown): NativeChatSession {
  const o = exact(value, ['kind', 'reference', 'namespace', 'source', 'session', 'chat_observations', 'gaps']);
  if (o.kind !== 'native_gateway' || sessionRef(o.reference).kind !== 'native_gateway') invalid();
  const n = exact(o.namespace, ['server', 'server_port']); decodeProcessKey(n.server); integer(n.server_port, 1, 65535);
  const source = gatewayConnection(o.source), session = sessionObservation(o.session);
  if (!sameJsonValue(n.server, source.server) || n.server_port !== source.server_port || !sameJsonValue(source.capture, session.reference.message.capture)
    || !sameJsonValue(o.reference, { kind: 'native_gateway', object: session.reference })) invalid();
  decimalU64(o.chat_observations); sessionGaps(o.gaps); return o as unknown as NativeChatSession;
}
export function nativeEntry(value: unknown): NativeChatEntry {
  const o = exact(value, ['kind', 'reference', 'native_message_id', 'order', 'observations', 'gaps']);
  if (o.kind !== 'native_gateway' || messageRef(o.reference).kind !== 'native_gateway') invalid();
  if (o.native_message_id !== null) decodeNativeIdValue(o.native_message_id);
  const order = record(o.order);
  if (order.kind === 'session_sequence') { exact(order, ['kind', 'sequence', 'references']); decodeNativeIdValue(order.sequence);
    if (!arrayOf(order.references, gatewayField).length) invalid(); }
  else if (order.kind === 'history_window') { exact(order, ['kind', 'window', 'index']); gatewayField(order.window); integer(order.index, 0, 0xffffffff); }
  else { exact(order, ['kind', 'references']); if (order.kind !== 'unresolved' || !arrayOf(order.references, gatewayField).length) invalid(); }
  const observations = arrayOf(o.observations, value => {
    const a = exact(value, ['source', 'reference', 'wrapper', 'history', 'session', 'handoff', 'run_id', 'message_run_id', 'message_sequence', 'wrapper_sequence', 'native_fields', 'role', 'content', 'revision'],
      ['record_timestamp_ms', 'transcript_position', 'state']);
    const source = gatewayConnection(a.source), ref = gatewayField(a.reference); if (!sameJsonValue(source.capture, ref.message.capture)) invalid();
    for (const f of ['wrapper', 'history']) if (a[f] !== null) gatewayField(a[f]);
    if (a.session !== null && sessionRef(a.session).kind !== 'native_gateway') invalid();
    if (a.handoff !== null) sessionObservation(a.handoff);
    for (const f of ['run_id', 'message_run_id']) if (a[f] !== null) string(a[f]);
    for (const f of ['message_sequence', 'wrapper_sequence', 'revision']) if (a[f] !== null) decodeNativeIdValue(a[f]);
    arrayOf(a.native_fields, gatewayField);
    if (a.content !== null) decodeChatContent(a.content);
    if (a.role !== null) { const role = record(a.role);
      if (role.kind === 'protocol_input') { exact(role, ['kind', 'value', 'method']); if (chatRole(role.value).kind !== 'user') invalid(); gatewayField(role.method); }
      else if (role.kind === 'string') { exact(role, ['kind', 'value', 'original', 'field']); chatRole(role.value); string(role.original); gatewayField(role.field); }
      else { exact(role, ['kind', 'value', 'field']); if (role.kind !== 'json') invalid(); gatewayField(role.field); }
    }
    return a;
  });
  if (!observations.length) invalid(); sessionGaps(o.gaps); return o as unknown as NativeChatEntry;
}
