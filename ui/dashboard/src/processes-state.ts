import type { CommandId } from './request-wire';
import type { CommandReply, CommandResult } from './reply-wire';
import { message, type Message } from './messages';
import { listQuery, type ListQuery, type ProcessPage, type ProcessesQuery } from './processes-wire';
import type { Diagnostic } from './wire';
import { sameId } from './event-wire';
import { sameWorkload } from './workload-wire';
import { sameJsonValue } from './global/json-value';

export interface ProcessesState {
  active: boolean;
  phase: 'initial' | 'loading' | 'ready' | 'failed';
  query: ProcessesQuery;
  pending: CommandId | null;
  page: ProcessPage | null;
  pageNumber: number;
  previousContext: PageContext | null;
  changed: boolean;
  problem: Message | null;
  receipt: { id: CommandId; result: CommandResult } | null;
  diagnostic: Diagnostic | null;
}
export const emptyProcesses = (): ProcessesState => ({ active: false, phase: 'initial', query: listQuery(), pending: null,
  page: null, pageNumber: 1, previousContext: null, changed: false, problem: null, receipt: null, diagnostic: null });
// Keep only the original identifiers across a page transition.
interface PageContext {
  instance: readonly number[];
  generation: string;
  evidence_epoch: string;
  tree_revision: string;
}
export function pageContext(page: ProcessPage): PageContext {
  const { instance, generation, evidence_epoch } = page.status;
  return { instance, generation, evidence_epoch, tree_revision: page.tree_revision };
}
function pageChanged(previous: PageContext | null, page: ProcessPage) {
  return previous !== null && (!sameId(previous.instance, page.status.instance)
    || previous.generation !== page.status.generation || previous.evidence_epoch !== page.status.evidence_epoch
    || previous.tree_revision !== page.tree_revision);
}
function matchesList(page: ProcessPage, query: ListQuery): boolean {
  const filter = query.filter;
  return page.records.length <= query.page.limit && page.records.every(record =>
    (filter.lifecycle === null || record.lifecycle === filter.lifecycle)
    && (filter.workload === null || sameWorkload(record.latest.process.workload, filter.workload)));
}

export function receiveProcessesReply(read: ProcessesState, reply: CommandReply): ProcessesState | null {
  if (!read.active || !read.pending || !sameId(read.pending, reply.id)) return null;
  const { result } = reply;
  if (result.status === 'accepted') return { ...read, receipt: reply };
  if (read.query.type === 'list' && result.status === 'completed' && result.data.type === 'processes' && result.data.result.type === 'page'
    && matchesList(result.data.result.data, read.query)) {
    const page = result.data.result.data;
    return { ...read, phase: 'ready', page, pending: null, previousContext: null,
      changed: pageChanged(read.previousContext, page), problem: null, receipt: reply };
  }
  if (read.query.type === 'get' && result.status === 'completed' && result.data.type === 'processes'
    && result.data.result.type === 'details' && sameJsonValue(result.data.result.data.record.key, read.query.key)) {
    return { ...read, phase: 'ready', page: null, pending: null, previousContext: null, changed: false, problem: null, receipt: reply };
  }
  return { ...read, phase: 'failed', page: null, pending: null, previousContext: null, receipt: reply,
    problem: message(result.status === 'failed' ? 'processes.readFailed'
      : read.query.type === 'get' && result.data.type === 'processes' && result.data.result.type === 'details'
        ? 'processes.wrongInstance' : 'processes.wrongReply') };
}
// Details stays in the original receipt; the view does not own another record copy.
export function processDetails(read: ProcessesState) {
  const result = read.receipt?.result;
  return read.active && read.phase === 'ready' && read.query.type === 'get' && result?.status === 'completed'
    && result.data.type === 'processes' && result.data.result.type === 'details'
    && sameJsonValue(read.query.key, result.data.result.data.record.key) ? result.data.result.data : null;
}
