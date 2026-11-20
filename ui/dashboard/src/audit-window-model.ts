import { confirmedExecutionCall } from './causal-chain-model';
import { decodeChatMessage, type ChatMessage } from './chat-wire';
import { isMessageId, isPartReference, isToolCandidateKind, type AskAuditContext, type AskMessageWindow, type ContextGap,
  type ContextView, type ExchangeKey, type MessageId } from './context-wire';
import { sameEventKey, sameId, type Ask } from './event-wire';
import { sameJsonValue } from './global/json-value';
import type { Facts } from './wire-json';

export interface AuditWindowMessage {
  reference: MessageId;
  message: ChatMessage | null;
  scopeMismatch: boolean;
  problem: 'missing' | 'conflicting' | 'invalid' | null;
  originals: readonly Facts[];
  gaps: readonly ContextGap[];
}
export interface AuditWindow {
  original: AskMessageWindow;
  kind: 'confirmed' | 'candidate' | 'unverified';
  messages: readonly AuditWindowMessage[];
}

function windowMessage(view: ContextView, reference: MessageId, exchange: ExchangeKey): AuditWindowMessage {
  const originals = view.source_records.flatMap(source =>
    source.kind === 'message' && sameJsonValue(source.value.id, reference) ? [source.value] : []);
  const gaps = view.gaps.filter(gap => gap.at.kind === 'message' && sameJsonValue(gap.at.value, reference)
    || gap.at.kind === 'part' && sameJsonValue(gap.at.value.message, reference));
  const scopeMismatch = !sameJsonValue(reference.exchange, exchange);
  const first = originals[0];
  if (!first) return { reference, message: null, scopeMismatch, problem: 'missing', originals, gaps };
  if (originals.some(original => !sameJsonValue(original, first))) {
    return { reference, message: null, scopeMismatch, problem: 'conflicting', originals, gaps };
  }
  try {
    // The shared decoder validates and returns this exact source object, including its revision.
    return { reference, message: decodeChatMessage(first), scopeMismatch, problem: null, originals, gaps };
  } catch {
    return { reference, message: null, scopeMismatch, problem: 'invalid', originals, gaps };
  }
}

// These are presentation references into the current reply, never another chat history or body copy.
export function askAuditWindows(ask: Ask, context: AskAuditContext): readonly AuditWindow[] {
  const matchesAsk = sameId(ask.decision_id, context.decision_id) && sameEventKey(ask.event.header, context.event)
    && context.view.anchor.kind === 'question' && sameId(context.view.anchor.value, ask.decision_id);
  const call = matchesAsk ? confirmedExecutionCall(ask.event, context.view, { ask, context }) : null;
  const windows: AuditWindow[] = [];
  if (context.window) {
    const original = context.window;
    const messages = original.message_refs.map(reference => windowMessage(context.view, reference, original.exchange));
    const confirmed = !messages.some(entry => entry.scopeMismatch) && call && isMessageId(call.message.id)
      && sameJsonValue(call.message.id.exchange, original.exchange)
      && messages.some(entry => entry.message !== null && sameJsonValue(entry.reference, call.message.id)
        && sameJsonValue(entry.message, call.message));
    windows.push({ original, kind: confirmed ? 'confirmed' : 'unverified', messages });
  }
  for (const original of context.candidate_windows) {
    const messages = original.message_refs.map(reference => windowMessage(context.view, reference, original.exchange));
    const explicitCandidate = context.view.relations.some(link => isToolCandidateKind(link.kind)
      && link.from.kind === 'part' && isPartReference(link.from.value) && sameJsonValue(link.from.value.message.exchange, original.exchange)
      && (link.to.kind === 'part' || (link.to.kind === 'event' && sameEventKey(link.to.value, context.event))));
    const candidate = !messages.some(entry => entry.scopeMismatch) && matchesAsk && (explicitCandidate || (context.attribution
      && sameEventKey(context.attribution.event, context.event)
      && context.attribution.candidates.some(item => item.target_comparison === 'equal'
        && sameJsonValue(item.call.message.exchange, original.exchange))));
    windows.push({ original, kind: candidate ? 'candidate' : 'unverified', messages });
  }
  return windows;
}
