// Current contract: protocol.rs, codec.rs, dispatch_event.rs, main_ports.rs, operation.rs.
// EventData remains original read-only facts; this slice does not interpret its payload.
import { arrayOf, boolean, bytes, decimalU64, exact, integer, invalid, oneOf, parseUnique, record, string, type Facts } from './wire-json';
import { decodeAsk, decodeOperationResult, sameId, type Ask, type DecisionId, type OperationResult } from './event-wire';
import { decodeBusinessReply, decodeCommandResult, type BusinessReply, type CommandReply } from './reply-wire';
import { decodeDriverStatus, type DriverStatus } from './driver-wire';
import { message, type Message } from './messages';
import { businessCommand, VERSION, type CommandId } from './request-wire';
import { decodePrivateReply, type PrivateReply } from './credentials-wire';
import { decodeChatMessage, decodeChatPart } from './chat-wire';
import { exchangeKey, messageId } from './context-wire';
const MAX_FRAME_BYTES = 8 * 1024 * 1024;
export type Access = 'read_only' | 'manage_rules' | 'manage_service';
export type Diagnostic = Facts & { type: string; id?: CommandId; code?: string };
export function replyOutputUnavailable(diagnostic: Diagnostic, pending: CommandId | null) {
  return pending !== null && diagnostic.type === 'reply_output_unavailable'
    && diagnostic.id !== undefined && sameId(pending, diagnostic.id)
    && (diagnostic.code === 'output_too_large' || diagnostic.code === 'output_encoding_failed');
}
export type Frame = { kind: 'authenticated'; access: Access; chatContent: boolean }
  | { kind: 'ask'; ask: Ask }
  | { kind: 'operation_result'; completion: OperationResult }
  | { kind: 'driver_status'; snapshot: DriverStatus }
  | { kind: 'business_reply'; reply: BusinessReply }
  | { kind: 'private_reply'; reply: PrivateReply }
  | { kind: 'stream_reply'; reply: { id: CommandId; result: { status: 'subscribed' | 'unsubscribed'; subscription_id: CommandId } | { status: 'failed'; code: string } } }
  | { kind: 'chat'; subscriptionId: CommandId; cursor: { service_instance: readonly number[]; sequence: string }; event: Facts }
  | ({ kind: 'reply' } & CommandReply)
  | { kind: 'diagnostic'; diagnostic: Diagnostic };

export function parseFrame(text: unknown): Frame {
  if (typeof text !== 'string' || text.length > MAX_FRAME_BYTES
    || new TextEncoder().encode(text).length > MAX_FRAME_BYTES) return invalid();
  const envelope = record(parseUnique(text));
  const lanes = Object.keys(envelope).filter(key => key !== 'version');
  if (envelope.version !== VERSION || lanes.length !== 1) return invalid();
  const lane = lanes[0];
  if (lane === 'authenticated') {
    const auth = exact(envelope.authenticated, ['access', 'chat_content']);
    return { kind: 'authenticated', access: oneOf(auth.access,
      ['read_only', 'manage_rules', 'manage_service'] as const), chatContent: boolean(auth.chat_content) };
  }
  if (lane === 'diagnostic') {
    const info = exact(envelope.diagnostic, ['type'], ['id', 'code']);
    if (info.type === 'private_not_authorized' && Object.hasOwn(info, 'id')) return invalid();
    return { kind: 'diagnostic', diagnostic: { type: string(info.type),
      ...(Object.hasOwn(info, 'id') ? { id: bytes(info.id, 16) } : {}),
      ...(Object.hasOwn(info, 'code') ? { code: string(info.code) } : {}) } };
  }
  if (lane === 'business_reply') return { kind: 'business_reply', reply: decodeBusinessReply(envelope.business_reply) };
  if (lane === 'private_reply') return { kind: 'private_reply', reply: decodePrivateReply(envelope.private_reply) };
  if (lane === 'stream_reply') {
    const reply = exact(envelope.stream_reply, ['id', 'result']);
    const result = record(reply.result);
    if (result.status === 'failed') { exact(result, ['status', 'code']); string(result.code); }
    else { exact(result, ['status', 'subscription_id']); oneOf(result.status, ['subscribed', 'unsubscribed']); bytes(result.subscription_id, 16); }
    bytes(reply.id, 16);
    return { kind: 'stream_reply', reply: reply as Extract<Frame, { kind: 'stream_reply' }>['reply'] };
  }
  if (lane !== 'message') return invalid();
  const message = record(envelope.message);
  if (message.type === 'chat') {
    exact(message, ['type', 'subscription_id', 'envelope']);
    const chat = exact(message.envelope, ['cursor', 'event']);
    const cursor = exact(chat.cursor, ['service_instance', 'sequence']);
    bytes(cursor.service_instance, 16); decimalU64(cursor.sequence);
    const event = exact(chat.event, ['type', 'data']);
    const data = record(event.data);
    switch (event.type) {
      case 'started': case 'finished': decodeChatMessage(data); break;
      case 'delta':
        exact(data, ['id', 'base_revision', 'revision', 'changes']); messageId(data.id);
        decimalU64(data.base_revision); decimalU64(data.revision);
        arrayOf(data.changes, value => {
          const change = record(value);
          if (change.kind === 'append_text') { exact(change, ['kind', 'part_id', 'text']); integer(change.part_id, 0, 0xffffffff); string(change.text); }
          else if (change.kind === 'put_part') { exact(change, ['kind', 'part']); decodeChatPart(change.part); }
          else invalid();
        }); break;
      case 'exchange_failed':
        exact(data, ['exchange', 'http_status', 'reason']); exchangeKey(data.exchange);
        if (data.http_status !== null) integer(data.http_status, 0, 65535); string(data.reason); break;
      // These original fact objects are only invalidation signals; their content stays with SQL owners.
      case 'observation': case 'credential_use': case 'rule_feedback': case 'gap': case 'status': break;
      default: return invalid();
    }
    return { kind: 'chat', subscriptionId: bytes(message.subscription_id, 16),
      cursor: cursor as Extract<Frame, { kind: 'chat' }>['cursor'], event };
  }
  if (message.type === 'reply') {
    exact(message, ['type', 'id', 'result']);
    return { kind: 'reply', id: bytes(message.id, 16), result: decodeCommandResult(message.result) };
  }
  if (message.type === 'driver_status') {
    // Formal Up::DriverStatus, not an extension/unknown-message fallback.
    exact(message, ['type', 'snapshot']);
    return { kind: 'driver_status', snapshot: decodeDriverStatus(message.snapshot) };
  }
  if (message.type === 'ask') {
    return { kind: 'ask', ask: decodeAsk(message) };
  }
  if (message.type !== 'operation_result') return invalid();
  return { kind: 'operation_result', completion: decodeOperationResult(message) };
}

export const canReview = (access: Access | null) => access === 'manage_rules' || access === 'manage_service';
export const idLabel = (id: DecisionId) => id.map(byte => byte.toString(16).padStart(2, '0')).join('');
export const availability = (available: boolean) => JSON.stringify({ version: VERSION,
  message: { type: 'review_availability', available } });
export const answer = (ask: Ask, index: number) => {
  const choice = ask.choices[index];
  if (!choice || !Number.isInteger(index)) return invalid();
  return JSON.stringify({ version: VERSION, message: { type: 'answer', decision_id: ask.decision_id, decision: choice } });
};
export const authenticate = (token: string) => JSON.stringify({ version: VERSION, authenticate: { token } });
export const protectionOverview = (id: CommandId) => businessCommand(id, { type: 'protection_overview' });
export const contextStreamCommand = (id: CommandId, subscriptionId: CommandId | null = null) => JSON.stringify({ version: VERSION,
  stream: { id: bytes(id, 16), command: subscriptionId === null ? { type: 'subscribe', filter: {}, after: null }
    : { type: 'unsubscribe', subscription_id: bytes(subscriptionId, 16) } } });

export function endpoint(configured: string | undefined): { url: string | null; problem: Message | null } {
  if (!configured) return { url: null, problem: message('endpoint.missing') };
  // Literal loopback only. An omitted root slash has the same WHATWG URL path
  // as '/'; no other path, DNS, userinfo, query or fragment is admitted.
  const match = /^ws:\/\/(127\.(?:0|[1-9]\d{0,2})\.(?:0|[1-9]\d{0,2})\.(?:0|[1-9]\d{0,2})|\[::1\]):([1-9]\d{0,4})\/?(?![\s\S])/.exec(configured);
  if (match) {
    const host = match[1] ?? '';
    const port = Number(match[2]);
    if (port <= 65535 && (host === '[::1]' || host.split('.').every(part => Number(part) <= 255))) {
      return { url: configured.endsWith('/') ? configured : configured + '/', problem: null };
    }
  }
  return { url: null, problem: message('endpoint.invalid') };
}
