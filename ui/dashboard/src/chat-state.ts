import { emptyEvidence, loadingEvidence, type EvidenceRead } from './evidence-read';
import type { BusinessFailure, BusinessReply } from './reply-wire';
import { chatCommand, matchesChatPage, type ChatData, type ChatQuery, type ChatSession, type ChatSessionRef, type ChatMessageScope } from './chat-wire';
import { exchangeKey, type ExchangeKey } from './context-wire';
import { sameId } from './event-wire';
import { sameJsonValue } from './global/json-value';
import { message, type Message } from './messages';
import type { CommandId } from './request-wire';

type ChatResult = BusinessFailure | { status: 'completed'; data: ChatData };
export interface ChatRead extends EvidenceRead<ChatResult> { query: ChatQuery | null; pageNumber: number }
export interface ChatState { active: boolean; selection: ChatMessageScope | null; sessionSelection: ChatSessionRef | null; sessions: ChatRead; exchanges: ChatRead; messages: ChatRead }
const emptyRead = (query: ChatQuery | null, pageNumber = 1): ChatRead => ({ ...emptyEvidence<ChatResult>(), query, pageNumber });
export const emptyChat = (active = false): ChatState => ({ active, selection: null, sessionSelection: null,
  sessions: emptyRead({ type: 'chat_sessions', before: null, limit: 8 }),
  exchanges: emptyRead({ type: 'chat_exchanges', session: null, after: null, limit: 8 }),
  messages: emptyRead(null) });
export const chatDenied = (state: ChatState) => [state.sessions, state.exchanges, state.messages]
  .some(read => read.result?.status === 'failed' && read.result.code === 'unauthorized');
export const chatData = (read: ChatRead): ChatData | null => read.phase === 'returned' && read.result?.status === 'completed' ? read.result.data : null;
interface ChatPorts {
  read: () => ChatState; publish: (state: ChatState) => void; granted: () => boolean;
  command: (encode: (id: CommandId) => string, begin: (id: CommandId) => void,
    failed: (id: CommandId | null, problem: Message) => void, problem: Message) => void;
}
// Current directory and page pair share the existing session and command allocator.
export class ChatHistory {
  constructor(private readonly ports: ChatPorts) {}
  enter = () => {
    const state = this.ports.read();
    if (state.active) return;
    this.ports.publish({ ...emptyChat(true), selection: state.selection });
    if (state.selection) this.refreshMessages();
    else this.refreshExchanges();
  };
  leave = () => {
    const state = this.ports.read();
    if (state.active || state.selection) this.ports.publish(emptyChat());
  };
  open = (exchange: ExchangeKey): boolean => {
    const state = this.ports.read();
    if (!this.ports.granted() || chatDenied(state)) return false;
    try { exchangeKey(exchange); } catch { return false; }
    this.ports.publish({ ...emptyChat(state.active), selection: { kind: 'model_http', exchange } });
    if (state.active) this.refreshMessages();
    return true;
  };
  private allowed() { const state = this.ports.read(); return state.active && this.ports.granted() && !chatDenied(state); }
  private read(query: ChatQuery, pageNumber: number) {
    const state = this.ports.read();
    const slot = query.type === 'chat_sessions' ? 'sessions' : query.type === 'chat_exchanges' ? 'exchanges' : 'messages';
    if (!this.allowed() || state[slot].pending) return;
    this.ports.command(id => chatCommand(id, query), id => {
      const current = this.ports.read();
      this.ports.publish({ ...current, ...(slot === 'exchanges' ? { selection: null, messages: emptyChat().messages } : {}),
        ...(slot === 'sessions' && current.sessionSelection ? { sessionSelection: null, selection: null,
          exchanges: emptyChat().exchanges, messages: emptyChat().messages } : {}),
        [slot]: { ...loadingEvidence<ChatResult>(id), query, pageNumber } });
    }, (id, problem) => {
      const current = this.ports.read(), read = current[slot];
      if (!current.active || (id && (!read.pending || !sameId(id, read.pending)))) return;
      this.ports.publish({ ...current, [slot]: { ...emptyRead(query, pageNumber), phase: 'failed', problem } });
    }, message('chat.sendFailed'));
  }
  refreshSessions = (limit = this.ports.read().sessions.query?.limit ?? 8) => {
    if (limit === 8 || limit === 16) this.read({ type: 'chat_sessions', before: null, limit }, 1);
  };
  nextSessions = () => {
    const state = this.ports.read(), read = state.sessions, data = chatData(read);
    if (!this.allowed() || read.pending || data?.type !== 'chat_sessions' || !data.next_before || read.pageNumber === Number.MAX_SAFE_INTEGER) return;
    this.ports.publish({ ...state, sessionSelection: null, selection: null,
      exchanges: emptyChat().exchanges, messages: emptyChat().messages });
    this.read({ type: 'chat_sessions', before: data.next_before, limit: read.query?.limit ?? 8 }, read.pageNumber + 1);
  };
  selectSession = (item: ChatSession | null) => {
    const state = this.ports.read(), data = chatData(state.sessions);
    if (!this.allowed() || (item && (data?.type !== 'chat_sessions' || !data.sessions.includes(item)
       || (item.kind === 'model_http' && item.session.issue !== null)))) return;
    const reference = item?.reference ?? null;
    if (sameJsonValue(state.sessionSelection, reference) && (item?.kind === 'native_gateway'
      ? state.selection?.kind === 'native_gateway' && sameJsonValue(state.selection.session, reference)
      : state.exchanges.phase !== 'initial')) return;
    this.ports.publish({ ...state, sessionSelection: reference, selection: null,
      exchanges: emptyChat().exchanges, messages: emptyChat().messages });
    if (item?.kind === 'native_gateway') this.selectNative(reference);
    else this.refreshExchanges();
  };
  selectNative = (session: ChatSessionRef | null = null) => {
    const state = this.ports.read(); if (!this.allowed() || session?.kind === 'model_http') return;
    this.ports.publish({ ...state, sessionSelection: session, selection: { kind: 'native_gateway', session }, messages: emptyChat().messages, exchanges: emptyChat().exchanges });
    this.refreshMessages();
  };
  refreshExchanges = (limit = this.ports.read().exchanges.query?.limit ?? 8) => {
    if (limit === 8 || limit === 16) this.read({ type: 'chat_exchanges', session: this.ports.read().sessionSelection, after: null, limit }, 1);
  };
  nextExchanges = () => {
    const read = this.ports.read().exchanges, data = chatData(read);
    if (data?.type === 'chat_exchanges' && data.next_after && read.pageNumber < Number.MAX_SAFE_INTEGER)
      this.read({ type: 'chat_exchanges', session: this.ports.read().sessionSelection, after: data.next_after, limit: read.query?.limit ?? 8 }, read.pageNumber + 1);
  };
  select = (exchange: ExchangeKey) => {
    const state = this.ports.read(), data = chatData(state.exchanges);
    if (!this.allowed() || data?.type !== 'chat_exchanges' || !data.exchanges.includes(exchange)
       || sameJsonValue(state.selection, { kind: 'model_http', exchange })) return;
    this.ports.publish({ ...state, selection: { kind: 'model_http', exchange }, messages: emptyChat().messages });
    this.refreshMessages();
  };
  close = () => {
    const state = this.ports.read();
    if (state.selection) this.ports.publish({ ...state, selection: null, messages: emptyChat().messages });
  };
  refreshMessages = (limit = this.ports.read().messages.query?.limit ?? 8) => {
    const scope = this.ports.read().selection;
    if (scope && (limit === 8 || limit === 16)) this.read({ type: 'chat_messages', scope, after: null, limit }, 1);
  };
  nextMessages = () => {
    const state = this.ports.read(), read = state.messages, data = chatData(read);
    if (state.selection && data?.type === 'chat_messages' && data.next_after && read.pageNumber < Number.MAX_SAFE_INTEGER)
      this.read({ type: 'chat_messages', scope: state.selection, after: data.next_after, limit: read.query?.limit ?? 8 }, read.pageNumber + 1);
  };
  receive(reply: BusinessReply): boolean {
    const state = this.ports.read();
    if (!state.active || !this.ports.granted()) return false;
    const slot = (['sessions', 'exchanges', 'messages'] as const).find(slot => state[slot].pending && sameId(state[slot].pending!, reply.id));
    if (!slot) return false;
    const read = state[slot], result = reply.result;
    if (result.status === 'failed' || ((result.data.type === 'chat_sessions' || result.data.type === 'chat_exchanges' || result.data.type === 'chat_messages')
      && read.query && matchesChatPage(result.data, read.query))) {
      this.ports.publish({ ...state, [slot]: { ...read, phase: 'returned', pending: null, result } });
    } else this.ports.publish({ ...state, [slot]: { ...emptyRead(read.query, read.pageNumber), phase: 'failed', problem: message('chat.wrongReply') } });
    return true;
  }
}
