import type { CommandId } from './request-wire';
import type { BusinessReply, CommandReply, CommandResult } from './reply-wire';
import { sameId, type EventKey, type OperationResult } from './event-wire';
import { isRuleLog, matchesLogPage, type LogPage, type LogQuery, type LogRow } from './activity-wire';
import { message, type Message } from './messages';
import type { Facts } from './wire-json';
import { matchesAttribution, type FactReference } from './context-wire';
import { sameJsonValue } from './global/json-value';
import { emptyEvidence, type EvidenceRead } from './evidence-read';

export type ActivitySelection = { kind: 'log'; row: LogRow } | { kind: 'completion'; completion: OperationResult }
  | { kind: 'original'; reference: FactReference; bodyOffset: string }
  | { kind: 'native_observation'; observation: EventKey };
export interface ActivityState {
  active: boolean; phase: 'initial' | 'loading' | 'ready' | 'failed'; query: LogQuery;
  page: LogPage | null; pending: CommandId | null; pageNumber: number;
  receipt: { id: CommandId; result: CommandResult } | null; diagnostic: Facts | null; problem: Message | null;
  selection: ActivitySelection | null; evidence: EvidenceRead;
}
export const emptyActivity = (): ActivityState => ({ active: false, phase: 'initial', query: { before: null, limit: 8 },
  page: null, pending: null, pageNumber: 1, receipt: null, diagnostic: null, problem: null, selection: null, evidence: emptyEvidence() });
export function selectionAnchor(selection: ActivitySelection | null): EventKey | null {
  const header = selection?.kind === 'completion' ? selection.completion.event.header
    : selection?.kind === 'log' && isRuleLog(selection.row.record) ? selection.row.record.event.header : null;
  if (!header) return null;
  const { source_instance, sequence_domain, sequence } = header;
  return { source_instance, sequence_domain, sequence };
}
export function selectionReference(selection: ActivitySelection | null): FactReference | null {
  if (selection?.kind === 'original') return selection.reference;
  if (selection?.kind === 'native_observation') return { kind: 'native_observation', value: selection.observation };
  const anchor = selectionAnchor(selection);
  return anchor ? { kind: 'event', value: anchor } : null;
}

export function receiveLogReply(read: ActivityState, reply: CommandReply): ActivityState | null {
  if (!read.active || !read.pending || !sameId(read.pending, reply.id)) return null;
  const { result } = reply;
  if (result.status === 'accepted') return { ...read, receipt: reply };
  if (result.status === 'completed' && result.data.type === 'security_log' && matchesLogPage(result.data.page, read.query)) {
    return { ...read, phase: 'ready', page: result.data.page, pending: null, receipt: reply };
  }
  return { ...read, phase: 'failed', page: null, pending: null, receipt: reply,
    problem: message(result.status === 'failed' ? 'activity.readFailed' : 'activity.wrongReply') };
}

export function receiveActivityContext(read: ActivityState, { id, result }: BusinessReply): ActivityState | null {
  if (!read.active || !read.evidence.pending || !sameId(read.evidence.pending, id)) return null;
  if (result.status === 'failed') {
    return { ...read, evidence: { ...read.evidence, phase: 'returned', pending: null, result, problem: null } };
  }
  const anchor = selectionReference(read.selection);
  if (anchor && result.data.type === 'activity_context' && sameJsonValue(anchor, result.data.view.anchor)
    && (anchor.kind === 'event' ? matchesAttribution(result.data.attribution, read.evidence.attributionRequest, anchor.value)
      : result.data.attribution === null)) {
    return { ...read, evidence: { ...read.evidence, phase: 'returned', pending: null, result: { status: 'completed', data: result.data }, problem: null } };
  }
  return { ...read, evidence: { ...emptyEvidence(), phase: 'failed', problem: message('evidence.wrongReply') } };
}
