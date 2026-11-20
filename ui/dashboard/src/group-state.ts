import { sameId } from './event-wire';
import { message, type Message } from './messages';
import type { CommandId } from './request-wire';
import type { BusinessReply } from './reply-wire';
import { matchesGroupPage, type GroupPage, type GroupQuery } from './group-wire';

export interface GroupState {
  active: boolean;
  phase: 'initial' | 'loading' | 'ready' | 'failed';
  query: GroupQuery;
  pending: CommandId | null;
  pageNumber: number;
  reply: BusinessReply | null;
  problem: Message | null;
}
export const emptyGroup = (active = false): GroupState => ({ active, phase: 'initial',
  query: { before: null, limit: 8 }, pending: null, pageNumber: 1, reply: null, problem: null });
export const groupDenied = (read: GroupState) => read.reply?.result.status === 'failed' && read.reply.result.code === 'unauthorized';
export function groupPage(read: GroupState): GroupPage | null {
  const result = read.reply?.result;
  return read.phase === 'ready' && result?.status === 'completed' && result.data.type === 'group_delegations'
    ? result.data.page : null;
}
export function receiveGroupReply(read: GroupState, reply: BusinessReply): GroupState | null {
  if (!read.active || !read.pending || !sameId(read.pending, reply.id)) return null;
  const { result } = reply;
  if (result.status === 'failed') return { ...read, phase: 'failed', pending: null, reply, problem: message('group.readFailed') };
  if (result.data.type !== 'group_delegations' || !matchesGroupPage(result.data.page, read.query)) {
    return { ...read, phase: 'failed', pending: null, reply, problem: message('group.wrongReply') };
  }
  return { ...read, phase: 'ready', pending: null, reply, problem: null };
}
