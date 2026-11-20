// Current business.rs / query.rs / flow.rs. Validate once and retain the original message tree.
import { arrayOf, bytes, decimalU64, exact, integer, invalid, oneOf, record, string, decodeChatContent, type Facts } from './wire-json';
import { exchangeKey, messageId, type ExchangeKey, type MessageId } from './context-wire';
import { decodeProcessKey, type ProcessKey } from './processes-wire';
import { businessCommand, type CommandId } from './request-wire';
import { sameJsonValue } from './global/json-value';
import { decodeNativeIdValue, decodeSessionObservation, type SessionObservation } from './session-wire';
import { chatRole, sessionRef, messageCursor, nativeSession, nativeEntry,
  type ChatSessionRef, type ChatMessageCursor, type ChatMessageScope, type NativeChatEntry, type NativeChatSession } from './native-chat-wire';
export type { ChatSessionRef, ChatMessageCursor, ChatMessageScope } from './native-chat-wire';

export type ChatContent = { kind: 'text'; value: string } | { kind: 'json'; value: unknown };
export type ChatPart = { type: 'text'; id: number; kind: 'plain' | 'refusal' | 'visible_reasoning'; text: string }
  | { type: 'tool_call'; id: number; native_id: Facts | null; name: string; arguments: ChatContent | null }
  | { type: 'tool_result'; id: number; native_id: Facts | null; content: ChatContent }
  | { type: 'opaque'; id: number; provider_type: string; reason: string }
  | { type: 'media_ref'; id: number; kind: string; safe_label: string };
export type ChatRole = { kind: 'system' | 'developer' | 'user' | 'assistant' | 'tool' } | { kind: 'other'; value: string };
export interface ChatMessage extends Facts {
  id: MessageId; revision: string; source: Facts; record_refs: readonly MessageId[];
  origin: 'request_context' | 'response_output'; role: ChatRole; candidate_index: number | null; model: string | null;
  observed_parts: readonly ChatPart[]; state: 'streaming' | 'complete' | 'interrupted' | 'failed'; quality: readonly string[];
  provider_status: string | null; finish_reason: string | null; usage: readonly { name: string; value: string }[] | null;
  first_seen_boottime_ns: string; last_seen_boottime_ns: string;
}
export interface ProtocolSource extends Facts { actor: Facts & { process: ProcessKey | null }; origin: string; route_label: string }
export type ChatSession = { kind: 'model_http'; reference: ChatSessionRef; source: ProtocolSource; session: SessionObservation; exchanges: string } | NativeChatSession;
export type ChatMessageEntry = { kind: 'model_http'; message: ChatMessage } | NativeChatEntry;
export interface ChatSessionsData { type: 'chat_sessions'; sessions: readonly ChatSession[]; next_before: ChatSessionRef | null }
export interface ChatExchangesData { type: 'chat_exchanges'; exchanges: readonly ExchangeKey[]; next_after: ExchangeKey | null }
export interface ChatMessagesData { type: 'chat_messages'; messages: readonly ChatMessageEntry[]; next_after: ChatMessageCursor | null }
export type ChatData = ChatExchangesData | ChatMessagesData | ChatSessionsData;
export type SessionsQuery = { type: 'chat_sessions'; before: ChatSessionRef | null; limit: number };
export type ExchangesQuery = { type: 'chat_exchanges'; session: ChatSessionRef | null; after: ExchangeKey | null; limit: number };
export type MessagesQuery = { type: 'chat_messages'; scope: ChatMessageScope; after: ChatMessageCursor | null; limit: number };
export type ChatQuery = ExchangesQuery | MessagesQuery | SessionsQuery;

function nativeId(value: unknown) {
  if (value === null) return;
  const o = exact(value, ['namespace', 'field_path', 'value']);
  oneOf(o.namespace, ['request', 'response', 'message', 'conversation', 'previous_response', 'tool_call', 'tool_result_reference', 'provider_event']);
  string(o.field_path); decodeNativeIdValue(o.value);
}
export function decodeChatPart(value: unknown): ChatPart {
  const o = record(value);
  integer(o.id, 0, 0xffffffff);
  if (o.type === 'text') {
    exact(o, ['type', 'id', 'kind', 'text']); oneOf(o.kind, ['plain', 'refusal', 'visible_reasoning']); string(o.text);
  } else if (o.type === 'tool_call') {
    exact(o, ['type', 'id', 'native_id', 'name', 'arguments']); nativeId(o.native_id); string(o.name);
    if (o.arguments !== null) decodeChatContent(o.arguments);
  } else if (o.type === 'tool_result') {
    exact(o, ['type', 'id', 'native_id', 'content']); nativeId(o.native_id); decodeChatContent(o.content);
  } else if (o.type === 'opaque') {
    exact(o, ['type', 'id', 'provider_type', 'reason']); string(o.provider_type); string(o.reason);
  } else if (o.type === 'media_ref') {
    exact(o, ['type', 'id', 'kind', 'safe_label']); string(o.kind); string(o.safe_label);
  } else return invalid();
  return o as ChatPart;
}
function source(value: unknown) {
  const o = exact(value, ['actor', 'channel', 'side', 'request_traceparent', 'profile', 'origin', 'route_label',
    'config_version', 'observed_at_utc_ms', 'actual_auth_scope', 'credential_use']);
  const actor = exact(o.actor, ['process', 'info', 'tid', 'thread_birth_boottime_ns', 'cgroup_id', 'observed_boottime_ns']);
  if (actor.process !== null) decodeProcessKey(actor.process);
  record(actor.info);
  if (actor.tid !== null) integer(actor.tid, 0, 0xffffffff);
  for (const key of ['thread_birth_boottime_ns', 'cgroup_id']) if (actor[key] !== null) decimalU64(actor[key]);
  decimalU64(actor.observed_boottime_ns);
  if (o.channel !== null) {
    const c = exact(o.channel, ['kind', 'source_instance', 'object_id', 'generation']);
    if (c.kind !== 'socket') invalid();
    bytes(c.source_instance, 16); decimalU64(c.object_id); decimalU64(c.generation);
  }
  oneOf(o.side, ['client_request', 'upstream_request', 'upstream_response', 'client_response']);
  oneOf(o.profile, ['open_ai_responses', 'open_ai_chat_completions', 'anthropic_messages']);
  string(o.origin); string(o.route_label); bytes(o.config_version, 32);
  if (o.request_traceparent !== null) string(o.request_traceparent);
  if (o.actual_auth_scope !== null) bytes(o.actual_auth_scope, 32);
  if (o.credential_use !== null) messageId(o.credential_use);
  if (o.observed_at_utc_ms !== null) {
    const n = o.observed_at_utc_ms;
    if ((typeof n !== 'number' || !Number.isSafeInteger(n)) && typeof n !== 'bigint') invalid();
    if (BigInt(n as number | bigint) < -9223372036854775808n || BigInt(n as number | bigint) > 9223372036854775807n) invalid();
  }
}
export function decodeChatMessage(value: unknown): ChatMessage {
  const o = exact(value, ['id', 'revision', 'source', 'record_refs', 'origin', 'role', 'candidate_index', 'model',
    'observed_parts', 'state', 'quality', 'provider_status', 'finish_reason', 'usage', 'first_seen_boottime_ns', 'last_seen_boottime_ns']);
  messageId(o.id); decimalU64(o.revision); source(o.source); arrayOf(o.record_refs, messageId);
  oneOf(o.origin, ['request_context', 'response_output']);
  chatRole(o.role);
  if (o.candidate_index !== null) integer(o.candidate_index, 0, 0xffffffff);
  if (o.model !== null) string(o.model);
  const parts = arrayOf(o.observed_parts, decodeChatPart);
  if (new Set(parts.map(p => p.id)).size !== parts.length) invalid();
  oneOf(o.state, ['streaming', 'complete', 'interrupted', 'failed']);
  arrayOf(o.quality, v => oneOf(v, ['truncated', 'gap', 'attach_mid_stream', 'order_unknown', 'identity_uncertain']));
  for (const key of ['provider_status', 'finish_reason']) if (o[key] !== null) string(o[key]);
  if (o.usage !== null) arrayOf(o.usage, v => {
    const u = exact(v, ['name', 'value']); string(u.name); decimalU64(u.value);
  });
  decimalU64(o.first_seen_boottime_ns); decimalU64(o.last_seen_boottime_ns);
  return o as ChatMessage;
}
export function decodeChatData(value: unknown): ChatData {
  const o = record(value);
  if (o.type === 'chat_sessions') {
    exact(o, ['type', 'sessions', 'next_before']);
    arrayOf(o.sessions, item => {
      if (record(item).kind === 'native_gateway') return nativeSession(item);
      const session = exact(item, ['kind', 'reference', 'source', 'session', 'exchanges']);
      if (session.kind !== 'model_http' || sessionRef(session.reference).kind !== 'model_http') invalid();
      source(session.source); decodeSessionObservation(session.session); decimalU64(session.exchanges);
      return session;
    });
    if (o.next_before !== null) sessionRef(o.next_before);
    return o as unknown as ChatSessionsData;
  }
  if (o.type === 'chat_exchanges') {
    exact(o, ['type', 'exchanges', 'next_after']); arrayOf(o.exchanges, exchangeKey);
    if (o.next_after !== null) exchangeKey(o.next_after);
    return o as unknown as ChatExchangesData;
  }
  exact(o, ['type', 'messages', 'next_after']);
  if (o.type !== 'chat_messages') return invalid();
  arrayOf(o.messages, value => {
    if (record(value).kind === 'native_gateway') return nativeEntry(value);
    const entry = exact(value, ['kind', 'message']); if (entry.kind !== 'model_http') invalid(); decodeChatMessage(entry.message); return entry;
  }); if (o.next_after !== null) messageCursor(o.next_after);
  return o as unknown as ChatMessagesData;
}
export function matchesChatPage(data: ChatData, query: ChatQuery): boolean {
  if (data.type !== query.type) return false;
  const ids = data.type === 'chat_exchanges' ? data.exchanges : data.type === 'chat_messages'
    ? data.messages.map(m => m.kind === 'model_http' ? { kind: 'model_http', message: m.message.id } : m.reference) : data.sessions.map(session => session.reference);
  const boundary = query.type === 'chat_sessions' ? query.before : query.type === 'chat_messages' && query.after?.kind === 'native_gateway' ? query.after.reference : query.after;
  const cursor = data.type === 'chat_sessions' ? data.next_before : data.next_after;
  const next = data.type === 'chat_messages' && data.next_after?.kind === 'native_gateway' ? data.next_after.reference : cursor;
  return ids.length <= query.limit && ids.every((id, index) => !sameJsonValue(id, boundary)
    && !ids.slice(0, index).some(previous => sameJsonValue(previous, id)))
    && (next === null || sameJsonValue(next, ids.at(-1)))
    && (query.type !== 'chat_messages' || (data.type === 'chat_messages'
      && data.messages.every(m => query.scope.kind === 'model_http'
        ? m.kind === 'model_http' && sameJsonValue(m.message.id.exchange, query.scope.exchange)
        : m.kind === 'native_gateway' && m.observations.every(o => sameJsonValue(o.session, query.scope.kind === 'native_gateway' ? query.scope.session : null)))
      && (data.next_after === null || (query.scope.kind === 'model_http' ? data.next_after.kind === 'model_http'
        && sameJsonValue(data.next_after.message.exchange, query.scope.exchange) : data.next_after.kind === 'native_gateway'
        && (query.after === null || (query.after.kind === 'native_gateway' && query.after.read_through === data.next_after.read_through))))));
}
export function chatCommand(id: CommandId, query: ChatQuery): string {
  integer(query.limit, 1, 64);
  if (query.type === 'chat_sessions') { if (query.before !== null) sessionRef(query.before); }
  else if (query.type === 'chat_exchanges') {
    if (query.after !== null) exchangeKey(query.after);
    if (query.session !== null) sessionRef(query.session);
  }
  else {
    if (query.scope.kind === 'model_http') {
      exchangeKey(query.scope.exchange);
      if (query.after !== null && (messageCursor(query.after).kind !== 'model_http' || query.after.kind !== 'model_http'
        || !sameJsonValue(query.after.message.exchange, query.scope.exchange))) return invalid();
    } else {
      if (query.scope.session !== null && sessionRef(query.scope.session).kind !== 'native_gateway') invalid();
      if (query.after !== null && messageCursor(query.after).kind !== 'native_gateway') invalid();
    }
  }
  return businessCommand(id, query);
}
