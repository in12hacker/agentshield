import { factObject, type CausalChainModel, type ConfirmedCall } from './causal-chain-model';
import { isMessageId, type ContextView, type MessageId } from './context-wire';
import { sameJsonValue } from './global/json-value';
import type { Facts } from './wire-json';

export interface ChatExcerpt {
  id: MessageId;
  role: 'user' | 'assistant';
  parts: readonly string[];
}

function excerpt(view: ContextView, ref: MessageId, call: ConfirmedCall): ChatExcerpt | null {
  if (!isMessageId(call.message.id) || !sameJsonValue(ref.exchange, call.message.id.exchange)
    || view.gaps.some(gap => gap.at.kind === 'message' && sameJsonValue(gap.at.value, ref))) return null;
  const originals = view.source_records.flatMap(source =>
    source.kind === 'message' && sameJsonValue(source.value.id, ref) ? [source.value] : []);
  const message = originals[0];
  if (!message || originals.some(item => !sameJsonValue(item, message))) return null;
  const isCall = sameJsonValue(ref, call.message.id);
  const role = factObject(message.role).kind;
  if ((role !== 'user' && role !== 'assistant') || !Array.isArray(message.observed_parts)
    || (role === 'user' && message.origin !== 'request_context') || (isCall && role !== 'assistant')) return null;
  const originalParts = message.observed_parts.map(factObject);
  const end = isCall ? originalParts.findIndex(part => part.id === call.part.id && part.type === 'tool_call') : originalParts.length;
  if (end < 0) return null;
  const parts = originalParts.slice(0, end).flatMap(part =>
    part.type === 'text' && part.kind === 'plain' && typeof part.text === 'string' ? [part.text] : []);
  return parts.length ? { id: ref, role, parts } : null;
}

export interface ActivityIntent {
  tool: Facts;
  user: ChatExcerpt | null;
  assistant: ChatExcerpt | null;
}

export function activityIntent(model: CausalChainModel): ActivityIntent | null {
  const { view } = model;
  const upstream = model.steps.find(step => step.stage === 'upstream');
  const operation = model.steps.find(step => step.stage === 'operation');
  if (!view || !model.verified || !upstream || operation?.incoming?.kind !== 'confirmed') return null;
  const { call } = upstream;
  const callId = call.message.id;
  if (!isMessageId(callId)) return null;
  const users = new Map<string, MessageId>();
  for (const record of view.source_records) {
    if (record.kind !== 'message' || !isMessageId(record.value.id)
      || record.value.origin !== 'request_context' || factObject(record.value.role).kind !== 'user'
      || !sameJsonValue(record.value.id.exchange, callId.exchange)
      || factObject(record.value.source).side !== 'client_request') continue;
    const refs = record.value.record_refs;
    if (!Array.isArray(refs) || refs.length !== 1 || !isMessageId(refs[0])
      || !sameJsonValue(refs[0].exchange, callId.exchange)
      || view.gaps.some(gap => gap.at.kind === 'protocol' && sameJsonValue(gap.at.value, refs[0]))) continue;
    users.set(record.value.id.sequence, record.value.id);
  }
  // The activity owner supplies one request excerpt. An ambiguous reply has no user preview.
  const userId = users.size === 1 ? users.values().next().value : undefined;
  return { tool: call.part, user: userId ? excerpt(view, userId, call) : null, assistant: excerpt(view, callId, call) };
}
