import { sameJsonValue } from './global/json-value';
import type { CommandId } from './request-wire';
import { answer, authenticate, availability, canReview, contextStreamCommand, idLabel, parseFrame, protectionOverview, replyOutputUnavailable, type Access } from './wire';
import { sameEventKey, sameId, type EventKey, type OperationResult } from './event-wire';
import type { AskContextResult, ContextResult, BusinessReply, OverviewRead } from './reply-wire';
import { message, type Message } from './messages';
import { decodeChange, rulesCommand, type PolicySnapshot, type RuleChange } from './rules-wire';
import { emptyRules, receiveRulesDiagnostic, receiveRulesReply, type RulesState } from './rules-state';
import { decodeProcessKey, listQuery, processesCommand, type ProcessesQuery, type ProcessKey, type Lifecycle } from './processes-wire';
import { emptyProcesses, pageContext, receiveProcessesReply, type ProcessesState } from './processes-state';
import { groupDelegationsCommand, type GroupDelegation, type GroupQuery } from './group-wire';
import { emptyGroup, groupDenied, groupPage, receiveGroupReply, type GroupState } from './group-state';
import type { Workload } from './workload-wire';
import { securityLogCommand, type LogQuery, type LogRow } from './activity-wire';
import { activityContextCommand, askContextCommand, originalRecordCommand, messageId, isMessageId, type FactReference, type AttributionRequest, type ExchangeKey } from './context-wire';
import { emptyActivity, receiveActivityContext, receiveLogReply, selectionReference,
  type ActivitySelection, type ActivityState } from './activity-state';
import { cancelEvidence, emptyEvidence, loadingEvidence, type EvidenceRead } from './evidence-read';
import { selectedEvidenceSnapshot } from './activity-evidence';

import { emptyAskEvidence, receiveAskContext, retainRecentReviews, reviewRemaining, type AskEvidence, type Review } from './ask-state';
import { emptySettings, ServiceSettings, type SettingsState } from './settings-state';
import type { ConfigDocument, ConfigSnapshot, RuntimeCommand } from './settings-wire';
import { PrivateCredentials } from './credentials-state';
import type { CaInfo, PrivateOperation } from './credentials-wire';
import type { CredentialBinding } from './egress-wire';
import { ChatHistory, chatDenied, emptyChat, type ChatState } from './chat-state';

const contentDenied = (result: BusinessReply['result'] | null) => result?.status === 'failed' && result.code === 'unauthorized';
const keepContentDenial = <Read extends { result: BusinessReply['result'] | null }>(read: Read, empty: Read): Read =>
  contentDenied(read.result) ? read : empty;

export interface Snapshot {
  phase: 'locked' | 'connecting' | 'authenticating' | 'authenticated';
  access: Access | null;
  chatContent: boolean;
  reviewDeclared: boolean;
  reviews: readonly Review[];
  askEvidence: AskEvidence;
  notice: Message | null;
  diagnostic: Readonly<Record<string, unknown>> | null;
  overview: OverviewRead;
  rules: RulesState;
  processes: ProcessesState;
  group: GroupState;
  chat: ChatState;
  activity: ActivityState;
  settings: SettingsState;
}
// Narrow injectable browser boundary lets tests exercise actual callback order without a server.
export interface SocketPort {
  readyState: number;
  onopen: (() => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
  onerror: (() => void) | null;
  onclose: (() => void) | null;
  send(text: string): void;
  close(): void;
}
const locked = (notice: Message | null = null): Snapshot => ({ phase: 'locked', access: null, chatContent: false,
  reviewDeclared: false, reviews: [], askEvidence: emptyAskEvidence(), notice, diagnostic: null, overview: { phase: 'initial' },
   rules: emptyRules(), processes: emptyProcesses(), group: emptyGroup(), chat: emptyChat(), activity: emptyActivity(), settings: emptySettings() });

export class ReviewSession {
  private state: Snapshot = locked();
  private listeners = new Set<() => void>();
  private socket: SocketPort | null = null;
  private seenAskIds = new Set<string>();
  private completionSequence = 0;
  private clearCredential: (() => void) | null = null;
  private uiReady = false;
  private rulesActive = false;
  private askContextVisible = false;
  private contextStream: { id: CommandId; phase: 'subscribing' | 'live' | 'unsubscribing' | 'failed';
    unsubscribeId: CommandId | null; cursor: Extract<ReturnType<typeof parseFrame>, { kind: 'chat' }>['cursor'] | null } | null = null;
  private askContextDirty = false;
  private activityContextDirty = false;
  // A monotonic 128-bit command sequence; no retained ID collection or random collision retries.
  private commandSequence = 0n;
  readonly chatHistory = new ChatHistory({
    read: () => this.state.chat,
    publish: chat => this.publishContentState({ ...this.state, chat }, chatDenied(chat)),
    granted: () => this.state.phase === 'authenticated' && this.state.chatContent,
    command: (encode, begin, failed, problem) => this.command(encode, begin, failed, problem),
  });
  private readonly serviceSettings = new ServiceSettings({
    read: () => this.state.settings,
    publish: settings => { this.publish({ ...this.state, settings }); this.credentials.configurationChanged(); },
    access: () => this.state.phase === 'authenticated' ? this.state.access : null,
    command: (encode, begin, failed, problem) => this.command(encode, begin, failed, problem),
    refreshOverview: () => this.refreshOverview(),
  });
  private readonly credentials = new PrivateCredentials({
    access: () => this.state.phase === 'authenticated' ? this.state.access : null,
    settings: () => this.state.settings,
    command: (encode, begin, failed, problem) => this.command(encode, begin, failed, problem),
  });
  constructor(private readonly url: string | null,
    private readonly createSocket: (url: string) => SocketPort,
    private readonly now: () => number = () => performance.now()) {}

  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  private publish(state: Snapshot) {
    if (this.state.askEvidence.ask !== state.askEvidence.ask || state.askEvidence.phase === 'cancelled') this.askContextDirty = false;
    if (this.state.activity.selection !== state.activity.selection || state.activity.evidence.phase === 'cancelled') this.activityContextDirty = false;
    this.state = state;
    this.syncContextStream();
    this.listeners.forEach(listener => listener());
  }
  private contextScopes() {
    const readable = (read: EvidenceRead<ContextResult | AskContextResult>) => read.phase === 'loading'
      || (read.phase === 'returned' && read.result?.status === 'completed');
    const granted = this.state.phase === 'authenticated' && this.state.chatContent;
    return { ask: granted && this.uiReady && this.askContextVisible && !!this.state.askEvidence.ask && readable(this.state.askEvidence),
      activity: granted && this.state.activity.active && !!this.state.activity.selection && readable(this.state.activity.evidence) };
  }
  private syncContextStream() {
    const scopes = this.contextScopes(), wanted = scopes.ask || scopes.activity;
    if (!scopes.ask) this.askContextDirty = false;
    if (!scopes.activity) this.activityContextDirty = false;
    const slot = this.contextStream;
    if (!slot && wanted) {
      this.command(id => contextStreamCommand(id), id => { this.contextStream = { id, phase: 'subscribing', unsubscribeId: null, cursor: null }; },
        () => { if (this.contextStream) this.contextStream.phase = 'failed'; }, message('evidence.sendFailed'));
    } else if (slot && !wanted && slot.phase === 'live') {
      this.command(id => contextStreamCommand(id, slot.id), id => { slot.phase = 'unsubscribing'; slot.unsubscribeId = id; },
        () => { slot.phase = 'failed'; }, message('evidence.sendFailed'));
    } else if (slot?.phase === 'failed' && !wanted) this.contextStream = null;
  }
  private invalidateContext() {
    const scopes = this.contextScopes();
    if (scopes.ask) this.askContextDirty = true;
    if (scopes.activity) this.activityContextDirty = true;
    this.refreshDirtyContext();
  }
  private refreshDirtyContext() {
    const scopes = this.contextScopes();
    if (scopes.ask && this.askContextDirty && !this.state.askEvidence.pending) {
      this.askContextDirty = false;
      const row = this.state.reviews.find(row => row.ask === this.state.askEvidence.ask);
      if (row) this.requestAskContext(row, this.state.askEvidence.attributionRequest, this.state.askEvidence);
    }
    if (scopes.activity && this.activityContextDirty && !this.state.activity.evidence.pending) {
      this.activityContextDirty = false;
      this.requestActivityEvidence(this.state.activity.evidence.attributionRequest, this.state.activity.evidence);
    }
  }

  connect(token: string) {
    if (this.state.phase !== 'locked') return;
    if (!this.url) { this.publish(locked(message('notice.noEndpoint'))); return; }
    if (!/^[0-9a-fA-F]{64}$/.test(token)) {
      this.publish(locked(message('notice.token'))); return;
    }
    let credential = token;
    token = '';
    this.clearCredential = () => { credential = ''; };
    this.uiReady = false;
    this.publish({ ...locked(), phase: 'connecting' });
    let socket: SocketPort;
    try { socket = this.createSocket(this.url); }
    catch { this.disconnect(message('notice.connect')); return; }
    this.socket = socket;
    const current = () => this.socket === socket;
    socket.onopen = () => {
      if (!current()) return;
      this.publish({ ...this.state, phase: 'authenticating' });
      try { socket.send(authenticate(credential)); }
      catch { this.disconnect(message('notice.authSend')); }
      finally { credential = ''; this.clearCredential = null; }
    };
    socket.onmessage = event => {
      if (!current()) return;
      try { this.receive(parseFrame(event.data)); }
      catch { this.disconnect(message('notice.invalidFrame')); }
    };
    socket.onerror = () => { if (current()) this.disconnect(message('notice.socketError')); };
    socket.onclose = () => { if (current()) this.disconnect(message('notice.closed')); };
  }

  // Only the current connection and a matched live content slot can call this transition.
  private publishContentState(state: Snapshot, denied: boolean) {
    if (!denied) { this.publish(state); return; }
    const { chat, group, askEvidence, activity } = state;
    const clearedChat = emptyChat(chat.active);
    this.publish({ ...state, chatContent: false,
      chat: { ...clearedChat,
        sessions: keepContentDenial(chat.sessions, clearedChat.sessions),
        exchanges: keepContentDenial(chat.exchanges, clearedChat.exchanges),
        messages: keepContentDenial(chat.messages, clearedChat.messages) },
      group: groupDenied(group) ? { ...emptyGroup(group.active), phase: 'failed', reply: group.reply, problem: group.problem }
        : emptyGroup(group.active),
      askEvidence: keepContentDenial(askEvidence, emptyAskEvidence(askEvidence.ask)),
      activity: { ...activity, evidence: keepContentDenial(activity.evidence, emptyEvidence()) },
    });
  }

  private receive(frame: ReturnType<typeof parseFrame>) {
    if (this.state.phase === 'authenticating') {
      if (frame.kind !== 'authenticated') throw new Error('authentication_required');
      this.publish({ ...this.state, phase: 'authenticated', access: frame.access, chatContent: frame.chatContent });
      this.refreshOverview();
      return;
    }
    if (this.state.phase !== 'authenticated' || frame.kind === 'authenticated') throw new Error('unexpected_frame');
    if (frame.kind === 'stream_reply') {
      const slot = this.contextStream, { id, result } = frame.reply;
      if (!slot) return;
      if (slot.phase === 'unsubscribing' && slot.unsubscribeId && sameId(id, slot.unsubscribeId)) {
        if (result.status === 'unsubscribed' && sameId(result.subscription_id, slot.id)) { this.contextStream = null; this.syncContextStream(); }
        else if (result.status === 'failed') { slot.phase = 'failed'; this.publish({ ...this.state, diagnostic: frame.reply }); }
      } else if (sameId(id, slot.id)) {
        if (slot.phase === 'subscribing' && result.status === 'subscribed' && sameId(result.subscription_id, slot.id)) {
          slot.phase = 'live'; this.syncContextStream();
          if (slot.phase === 'live') this.invalidateContext(); // Close the initial SQL/tail-install race once.
        } else if (result.status === 'failed' && (slot.phase === 'subscribing' || slot.phase === 'live')) {
          const scopes = this.contextScopes();
          slot.phase = 'failed';
          this.publishContentState({ ...this.state, diagnostic: frame.reply }, (scopes.ask || scopes.activity)
            && (result.code === 'forbidden' || result.code === 'unauthenticated'));
        }
      }
      return;
    } else if (frame.kind === 'chat') {
      const slot = this.contextStream;
      if (!slot || slot.phase !== 'live' || !sameId(frame.subscriptionId, slot.id) || !this.state.chatContent) return;
      if (slot.cursor && (!sameId(slot.cursor.service_instance, frame.cursor.service_instance)
        || BigInt(frame.cursor.sequence) <= BigInt(slot.cursor.sequence))) return;
      slot.cursor = frame.cursor;
      this.invalidateContext();
      return;
    } else if (frame.kind === 'private_reply') {
      this.credentials.receive(frame.reply);
    } else if (frame.kind === 'business_reply') {
      if (this.chatHistory.receive(frame.reply)) return;
      const group = this.state.chatContent ? receiveGroupReply(this.state.group, frame.reply) : null;
      if (group) { this.publishContentState({ ...this.state, group }, contentDenied(frame.reply.result)); return; }
      const askEvidence = this.state.chatContent ? receiveAskContext(this.state.askEvidence, frame.reply) : null;
      if (askEvidence) { this.publishContentState({ ...this.state, askEvidence }, contentDenied(frame.reply.result)); this.refreshDirtyContext(); return; }
      const activity = receiveActivityContext(this.state.activity, frame.reply);
      if (activity) { this.publishContentState({ ...this.state, activity }, contentDenied(frame.reply.result)); this.refreshDirtyContext(); return; }
      const read = this.state.overview;
      if (read.phase !== 'loading' || !sameId(read.id, frame.reply.id)) return;
      const result = frame.reply.result;
      if (result.status === 'failed') {
        this.publish({ ...this.state, overview: { phase: 'returned', id: read.id, result } });
      } else if (result.data.type === 'protection_overview') {
        this.publish({ ...this.state, overview: { phase: 'returned', id: read.id,
          result: { status: 'completed', data: result.data } } });
      } else {
        this.publish({ ...this.state, overview: { phase: 'failed', id: read.id, message: message('overview.wrongReply') } });
      }
    } else if (frame.kind === 'reply') {
      const reply = { id: frame.id, result: frame.result };
      if (this.serviceSettings.receive(reply)) return;
      const activity = receiveLogReply(this.state.activity, reply);
      if (activity) { this.publish({ ...this.state, activity }); return; }
      const processes = receiveProcessesReply(this.state.processes, reply);
      if (processes) { this.publish({ ...this.state, processes }); return; }
      const previousRules = this.state.rules;
      const rules = receiveRulesReply(previousRules, reply);
      if (rules && rules !== previousRules) {
        this.publish({ ...this.state, rules: this.rulesActive ? rules : { ...rules, phase: 'initial', snapshot: null } });
        if (this.rulesActive && previousRules.pending?.command === 'edit' && rules.pending === null) this.refreshRules();
      }
    } else if (frame.kind === 'driver_status') {
      // The public notification changes neither review state, connection grants nor protection claims.
      return;
    } else if (frame.kind === 'diagnostic') {
      const processes = this.state.processes;
      const diagnostic = frame.diagnostic;
      if (this.credentials.diagnostic(diagnostic)) return;
      if (this.serviceSettings.diagnostic(diagnostic)) return;
      const activity = this.state.activity;
      if (activity.active && replyOutputUnavailable(diagnostic, activity.pending)) {
        this.publish({ ...this.state, activity: { ...activity, phase: 'failed', page: null, pending: null,
          diagnostic, problem: message('activity.outputFailed') } });
        return;
      }
      if (replyOutputUnavailable(diagnostic, processes.pending)) {
        this.publish({ ...this.state, processes: { ...processes, phase: 'failed', page: null, pending: null,
          previousContext: null, diagnostic, problem: message('processes.outputFailed') } });
        return;
      }
      const rules = receiveRulesDiagnostic(this.state.rules, diagnostic);
      if (rules) { this.publish({ ...this.state, rules }); return; }
      // Diagnostics have no DecisionId. They cannot complete or fail a particular Ask.
      this.publish({ ...this.state, diagnostic: frame.diagnostic });
    } else if (frame.kind === 'ask') {
      if (!canReview(this.state.access) || !this.uiReady || !this.state.reviewDeclared) throw new Error('review_not_ready');
      // A repeated Ask never re-arms an attempted answer or renews the observed sample.
      const decisionId = idLabel(frame.ask.decision_id);
      if (this.seenAskIds.has(decisionId)) return;
      this.seenAskIds.add(decisionId);
      const askEvidence = this.state.askEvidence.ask ? this.state.askEvidence : emptyAskEvidence(frame.ask);
      const observedAt = this.now();
      this.publishReviews([...this.state.reviews, { ask: frame.ask, observedAt,
        attempted: false, phase: 'waiting', completion: null, completionOrder: null }], askEvidence);
    } else if (frame.kind === 'operation_result') {
      const completion = frame.completion;
      const id = completion.decision_id;
      if (id === null) return;
      const row = this.state.reviews.find(review => sameId(review.ask.decision_id, id)
        && sameEventKey(review.ask.event.header, completion.event.header));
      if (!row || row.completion) return;
      this.replace(row, { ...row, phase: 'complete', completion, completionOrder: ++this.completionSequence });
    }
  }

  // Actual command producers share allocation, current socket and send lifecycle.
  private command(encode: (id: CommandId) => string, begin: (id: CommandId) => void,
    failed: (id: CommandId | null, problem: Message) => void, sendProblem: Message) {
    const socket = this.socket;
    if (this.state.phase !== 'authenticated' || !socket || socket.readyState !== 1) return;
    if (this.commandSequence === (1n << 128n) - 1n) {
      failed(null, message('notice.commandExhausted'));
      return;
    }
    const sequence = ++this.commandSequence;
    const id: CommandId = Array.from({ length: 16 }, (_, index) => Number((sequence >> BigInt((15 - index) * 8)) & 255n));
    begin(id);
    if (this.socket !== socket || this.state.phase !== 'authenticated') return;
    try { socket.send(encode(id)); }
    catch {
      if (this.socket === socket) failed(id, sendProblem);
    }
  }

  refreshOverview = () => {
    this.command(protectionOverview,
      id => this.publish({ ...this.state, overview: { phase: 'loading', id } }),
      (id, problem) => this.publish({ ...this.state, overview: { phase: 'send_failed', id, message: problem } }),
      message('notice.overviewSend'));
  };

  enterSettings = () => this.serviceSettings.enter();
  leaveSettings = () => this.serviceSettings.leave();
  refreshSettings = () => this.serviceSettings.refresh();
  saveConfiguration = (snapshot: ConfigSnapshot, document: ConfigDocument) => this.serviceSettings.save(snapshot, document);
  reloadConfiguration = (snapshot: ConfigSnapshot) => this.serviceSettings.reload(snapshot);
  controlRuntime = (command: RuntimeCommand) => this.serviceSettings.control(command);

  getCredentials = () => this.credentials.getSnapshot();
  subscribeCredentials = (listener: () => void) => this.credentials.subscribe(listener);
  getConfiguredCa = (snapshot: ConfigSnapshot | null, ca: CaInfo | null) =>
    this.state.phase === 'authenticated' && this.socket?.readyState === 1 ? this.credentials.configuredCa(snapshot, ca) : null;
  enterEgress = () => this.credentials.enter();
  leaveEgress = () => this.credentials.leave();
  discardPhantom = () => this.credentials.discard();
  readPhantomValue = () => this.credentials.readValue();
  copyPhantomValue = (write: (value: string) => Promise<void>) => this.credentials.copyValue(write);
  credentialCommand = (operation: PrivateOperation, value?: string) => this.credentials.send(operation, value);
  issuePhantom = (snapshot: ConfigSnapshot, binding: CredentialBinding) => this.credentials.issue(snapshot, binding);

  // One selected Ask only. Initial authorized reads reuse this slot and never change the time sample.
  selectAskContext = (row: Review | undefined) => {
    if (row && !this.state.reviews.includes(row)) return;
    this.askContextVisible = !!row;
    const ask = row?.ask ?? null;
    if (this.state.askEvidence.ask !== ask) this.publish({ ...this.state, askEvidence: emptyAskEvidence(ask) });
    if (row && this.uiReady && this.state.askEvidence.phase === 'initial') this.readAskContext(row);
    this.syncContextStream();
  };
  readAskContext = (row: Review, attribution: AttributionRequest | null = null) => this.requestAskContext(row, attribution);
  private requestAskContext(row: Review, attribution: AttributionRequest | null, previous?: AskEvidence) {
    const read = this.state.askEvidence;
    if (this.state.phase !== 'authenticated' || !this.state.chatContent || !this.state.reviews.includes(row)
      || read.ask !== row.ask || read.phase === 'loading'
      || (read.result?.status === 'failed' && read.result.code === 'unauthorized')) return;
    this.command(id => askContextCommand(id, row.ask.decision_id, attribution),
      id => this.publish({ ...this.state, askEvidence: { ask: row.ask, ...loadingEvidence<AskContextResult>(id, attribution, previous) } }),
      (id, problem) => {
        const current = this.state.askEvidence;
        if (current.ask !== row.ask || (id && (!current.pending || !sameId(id, current.pending)))) return;
        this.publish({ ...this.state, askEvidence: { ...emptyAskEvidence(row.ask), phase: 'failed', problem } });
      }, message('evidence.sendFailed'));
  }
  cancelAskContext = (row: Review, read: AskEvidence) => {
    if (this.state.askEvidence !== read || read.ask !== row.ask || !this.state.reviews.includes(row)) return;
    const cancelled = cancelEvidence(read);
    if (cancelled) this.publish({ ...this.state, askEvidence: { ask: row.ask, ...cancelled } });
  };

  enterActivity = () => {
    if (this.state.phase !== 'authenticated' || this.state.activity.active) return;
    this.publish({ ...this.state, activity: { ...this.state.activity, active: true } });
    if (this.state.activity.selection === null) this.refreshActivity();
  };
  clearActivity = () => {
    if (this.state.activity.active) this.publish({ ...this.state, activity: emptyActivity() });
  };
  private readActivity(query: LogQuery, pageNumber: number) {
    if (this.state.access !== 'manage_service' || !this.state.activity.active || this.state.activity.pending) return;
    this.command(id => securityLogCommand(id, query),
      id => this.publish({ ...this.state, activity: { ...emptyActivity(), active: true, phase: 'loading', query, pageNumber, pending: id } }),
      (id, problem) => {
        const activity = this.state.activity;
        if (!activity.active || (id && (!activity.pending || !sameId(id, activity.pending)))) return;
        this.publish({ ...this.state, activity: { ...activity, phase: 'failed', pending: null, problem } });
      }, message('activity.sendFailed'));
  }
  refreshActivity = (limit = this.state.activity.query.limit) => {
    if (limit !== 8 && limit !== 16) return;
    this.readActivity({ before: null, limit }, 1);
  };
  nextActivity = () => {
    const a = this.state.activity;
    if (a.phase !== 'ready' || !a.page?.next_before || a.pageNumber === Number.MAX_SAFE_INTEGER) return;
    this.readActivity({ before: a.page.next_before, limit: a.query.limit }, a.pageNumber + 1);
  };
  selectActivity = (row: LogRow) => {
    const a = this.state.activity;
    if (!a.active || a.phase !== 'ready' || !a.page?.records.includes(row)) return;
    if (a.selection?.kind === 'log' && a.selection.row === row) return;
    this.publish({ ...this.state, activity: { ...a, selection: { kind: 'log', row }, evidence: emptyEvidence() } });
    this.readActivityEvidence();
  };
  closeActivitySelection = () => {
    const a = this.state.activity;
    if (a.selection) this.publish({ ...this.state, activity: { ...a, selection: null, evidence: emptyEvidence() } });
  };
  openCompletionEvidence = (completion: OperationResult) => {
    if (this.state.phase !== 'authenticated' || !this.state.reviews.some(row => row.completion === completion)) return;
    this.publish({ ...this.state, activity: { ...this.state.activity, active: true,
      selection: { kind: 'completion', completion }, evidence: emptyEvidence() } });
    this.readActivityEvidence();
  };
  readActivityEvidence = (attribution: AttributionRequest | null = null) => this.requestActivityEvidence(attribution);
  private requestActivityEvidence(attribution: AttributionRequest | null, previous?: EvidenceRead) {
    const a = this.state.activity;
    const selection = a.selection;
    const anchor = selectionReference(selection);
    if (!this.state.chatContent || !a.active || !anchor || a.evidence.pending) return;
    if (anchor.kind === 'native_observation' && attribution !== null) return;
    this.command(id => selection?.kind === 'original' ? originalRecordCommand(id, anchor, selection.bodyOffset) : activityContextCommand(id, anchor, attribution),
      id => this.publish({ ...this.state, activity: { ...this.state.activity, evidence: loadingEvidence(id, attribution, previous) } }),
      (id, problem) => {
        const current = this.state.activity;
        if (!current.active || current.selection !== selection
          || (id && (!current.evidence.pending || !sameId(id, current.evidence.pending)))) return;
        this.publish({ ...this.state, activity: { ...current, evidence: { ...emptyEvidence(), phase: 'failed', problem } } });
      }, message('evidence.sendFailed'));
  }
  cancelActivityEvidence = (selection: ActivitySelection, read: EvidenceRead) => {
    const activity = this.state.activity;
    if (!activity.active || activity.selection !== selection || activity.evidence !== read) return;
    const evidence = cancelEvidence(read);
    if (evidence) this.publish({ ...this.state, activity: { ...activity, evidence } });
  };

  openNativeObservation = (read: EvidenceRead<ContextResult | AskContextResult>, observation: EventKey): boolean => {
    if (!this.state.chatContent || this.state.phase !== 'authenticated'
      || (read !== this.state.activity.evidence && read !== this.state.askEvidence)
      || read.phase !== 'returned' || read.result?.status !== 'completed') return false;
    const data = read.result.data;
    const view = data.type === 'ask_context' ? data.context.view : data.view;
    const reference = { kind: 'native_observation', value: observation };
    const referenced = sameJsonValue(view.anchor, reference)
      || view.source_records.some(source => source.kind === 'request_handoff' && sameJsonValue(source.value.observation, observation))
      || view.relations.some(link => [link.from, link.to, ...link.evidence].some(ref => sameJsonValue(ref, reference)))
      || view.gaps.some(gap => sameJsonValue(gap.at, reference));
    if (!referenced) return false;
    this.publish({ ...this.state, activity: { ...this.state.activity, active: true,
      selection: { kind: 'native_observation', observation }, evidence: emptyEvidence() } });
    this.readActivityEvidence();
    return true;
  };
  openOriginalRecord = (read: EvidenceRead<ContextResult | AskContextResult>, reference: FactReference, bodyOffset = '0'): boolean => {
    if (!this.state.chatContent || this.state.phase !== 'authenticated'
      || (read !== this.state.activity.evidence && read !== this.state.askEvidence)
      || read.phase !== 'returned' || read.result?.status !== 'completed'
      || (reference.kind !== 'protocol' && reference.kind !== 'native_observation')) return false;
    const data = read.result.data;
    const view = data.type === 'ask_context' ? data.context.view : data.view;
    if (!sameJsonValue(view.anchor, reference) && !view.source_records.some(source => source.kind === 'deferred' && sameJsonValue(source.value, reference))) return false;
    this.publish({ ...this.state, activity: { ...this.state.activity, active: true,
      selection: { kind: 'original', reference, bodyOffset }, evidence: emptyEvidence() } });
    this.readActivityEvidence();
    return true;
  };

  openAuditExchange = (read: EvidenceRead<ContextResult | AskContextResult>, exchange: ExchangeKey): boolean => {
    if (!this.state.chatContent || this.state.phase !== 'authenticated'
      || (read !== this.state.activity.evidence && read !== this.state.askEvidence)
      || read.phase !== 'returned' || read.result?.status !== 'completed') return false;
    const data = read.result.data;
    const attribution = data.type === 'ask_context' ? data.context.attribution : data.attribution;
    const windows = data.type === 'ask_context' ? [data.context.window, ...data.context.candidate_windows] : [];
    const view = data.type === 'ask_context' ? data.context.view : data.view;
    const referenced = view.source_records.some(source => source.kind === 'message' && isMessageId(source.value.id)
        && sameJsonValue(source.value.id.exchange, exchange))
      || attribution?.candidates.some(candidate => sameJsonValue(candidate.call.message.exchange, exchange))
      || windows.some(window => window && sameJsonValue(window.exchange, exchange));
    return !!referenced && this.chatHistory.open(exchange);
  };

  getActivityEvidenceSnapshot = (selection: ActivitySelection, read: EvidenceRead) => {
    const { activity } = this.state;
    if (this.state.phase !== 'authenticated' || this.socket?.readyState !== 1
      || activity.selection !== selection || activity.evidence !== read) return null;
    return selectedEvidenceSnapshot(activity, this.state.chatContent);
  };

  private readProcesses(query: ProcessesQuery, pageNumber: number) {
    const previous = query.type === 'list' && query.page.cursor && this.state.processes.page ? pageContext(this.state.processes.page) : null;
    this.command(id => processesCommand(id, query),
      id => this.publish({ ...this.state, processes: { ...emptyProcesses(), active: true, phase: 'loading', query,
        pending: id, pageNumber, previousContext: previous } }),
      (id, problem) => {
        if (id && (!this.state.processes.pending || !sameId(id, this.state.processes.pending))) return;
        this.publish({ ...this.state, processes: { ...emptyProcesses(), active: true, phase: 'failed', query, pageNumber, problem } });
      }, message('processes.sendFailed'));
  }
  enterProcesses = () => {
    if (this.state.phase !== 'authenticated' || this.state.processes.active) return;
    this.readProcesses(this.state.processes.query, 1);
  };
  openProcess = (key: ProcessKey) => {
    if (this.state.phase !== 'authenticated') return;
    try { decodeProcessKey(key); } catch { return; }
    const query = { type: 'get', key } as const;
    if (this.state.processes.active) this.readProcesses(query, 1);
    else this.publish({ ...this.state, processes: { ...emptyProcesses(), query } });
  };
  reloadProcess = () => {
    const read = this.state.processes;
    if (read.active && read.query.type === 'get' && !read.pending) this.readProcesses(read.query, 1);
  };
  refreshProcesses = (limit?: number, lifecycle?: Lifecycle | null, workload?: Workload | null) => {
    const previous = this.state.processes.query.type === 'list' ? this.state.processes.query : listQuery();
    this.readProcesses(listQuery(limit ?? previous.page.limit, lifecycle === undefined ? previous.filter.lifecycle : lifecycle,
      workload === undefined ? previous.filter.workload : workload), 1);
  };
  nextProcesses = () => {
    const read = this.state.processes;
    if (!read.active || read.query.type !== 'list' || read.phase !== 'ready' || !read.page?.next_cursor || read.pending || read.pageNumber === Number.MAX_SAFE_INTEGER) return;
    this.readProcesses({ ...read.query, page: { limit: read.query.page.limit, cursor: read.page.next_cursor } }, read.pageNumber + 1);
  };
  clearProcesses = () => {
    const read = this.state.processes;
    if (read.active || read.phase !== 'initial' || read.query.type === 'get') this.publish({ ...this.state, processes: emptyProcesses() });
  };

  enterGroupDelegations = () => {
    if (this.state.phase !== 'authenticated' || this.state.group.active) return;
    this.publish({ ...this.state, group: emptyGroup(true) });
    this.refreshGroupDelegations();
  };
  leaveGroupDelegations = () => {
    if (this.state.group.active) this.publish({ ...this.state, group: emptyGroup() });
  };
  private readGroupDelegations(query: GroupQuery, pageNumber: number) {
    const read = this.state.group;
    if (this.state.phase !== 'authenticated' || !this.state.chatContent || !read.active || read.pending || groupDenied(read)) return;
    this.command(id => groupDelegationsCommand(id, query),
      id => this.publish({ ...this.state, group: { ...emptyGroup(true), phase: 'loading', query, pending: id, pageNumber } }),
      (id, problem) => {
        const current = this.state.group;
        if (!current.active || (id && (!current.pending || !sameId(current.pending, id)))) return;
        this.publish({ ...this.state, group: { ...current, phase: 'failed', pending: null, problem } });
      }, message('group.sendFailed'));
  }
  refreshGroupDelegations = () => this.readGroupDelegations({ before: null, limit: 8 }, 1);
  nextGroupDelegations = () => {
    const read = this.state.group;
    const page = groupPage(read);
    if (!page?.next_before || read.pageNumber === Number.MAX_SAFE_INTEGER) return;
    this.readGroupDelegations({ before: page.next_before, limit: read.query.limit }, read.pageNumber + 1);
  };
  openGroupReceipt = (row: GroupDelegation): boolean => {
    if (this.state.phase !== 'authenticated' || !this.state.chatContent || !this.state.group.active
      || !groupPage(this.state.group)?.records.includes(row) || row.kind !== 'openclaw_tool_receipt'
      || row.receipt.kind !== 'protocol') return false;
    return this.chatHistory.open(messageId(row.receipt.value).exchange);
  };

  enterRules = () => {
    if (this.state.phase !== 'authenticated' || this.rulesActive) return;
    this.rulesActive = true;
    this.refreshRules();
  };
  leaveRules = () => {
    this.rulesActive = false;
    const rules = this.state.rules;
    // Get has no remaining consumer. Edit still owns its exact waiter and real receipts.
    this.publish({ ...this.state, rules: { ...rules, phase: 'initial', snapshot: null,
      pending: rules.pending?.command === 'edit' ? rules.pending : null,
      readFailure: null, problem: null } });
  };
  refreshRules = () => {
    if (this.state.phase !== 'authenticated') return;
    if (this.state.rules.pending?.command === 'edit') return;
    // An explicit read also opens the read lifecycle for non-React session consumers.
    this.rulesActive = true;
    this.command(id => rulesCommand(id, { type: 'get' }),
      id => this.publish({ ...this.state, rules: { ...this.state.rules, phase: 'loading', snapshot: null,
        pending: { id, command: 'get' },
        problem: null, readFailure: null } }),
      (_id, problem) => this.publish({ ...this.state, rules: { ...this.state.rules, phase: 'failed', snapshot: null, pending: null, problem } }),
      message('rules.readSendFailed'));
  };

  editRules = (snapshot: PolicySnapshot, proposed: RuleChange) => {
    const rules = this.state.rules;
    if (this.state.phase !== 'authenticated' || !this.rulesActive || !canReview(this.state.access) || rules.phase !== 'ready'
      || rules.snapshot !== snapshot || rules.pending) return;
    let change: RuleChange;
    try { change = decodeChange(proposed); } catch { return; }
    this.command(id => rulesCommand(id, { type: 'edit', expected_version: snapshot.version, change }),
      id => this.publish({ ...this.state, rules: { ...rules, pending: { id, command: 'edit' }, problem: null, receipt: null, conflict: false } }),
      (_id, problem) => this.publish({ ...this.state, rules: { ...this.state.rules, phase: 'failed', snapshot: null,
        pending: null, problem } }), message('rules.sendFailed'));
  };

  setUiReady(ready: boolean) {
    this.uiReady = ready;
    if (!ready && this.state.askEvidence.ask) this.publish({ ...this.state, askEvidence: emptyAskEvidence() });
    if (this.state.phase !== 'authenticated' || !canReview(this.state.access)) return;
    if (this.state.reviewDeclared === ready) return;
    if (!this.socket || this.socket.readyState !== 1) { this.disconnect(message('notice.unavailable')); return; }
    try {
      this.socket.send(availability(ready));
      this.publish({ ...this.state, reviewDeclared: ready });
    } catch { this.disconnect(message('notice.availability')); }
  }

  remaining(row: Review, at = this.now()) { return reviewRemaining(row, at); }
  closeReview = (row: Review) => {
    if (!row.completion || !this.state.reviews.includes(row)) return;
    this.publishReviews(this.state.reviews.filter(value => value !== row),
      this.state.askEvidence.ask === row.ask ? emptyAskEvidence() : this.state.askEvidence);
  };
  submit(row: Review, choiceIndex: number) {
    const socket = this.socket;
    if (this.state.phase !== 'authenticated' || !canReview(this.state.access) || !this.uiReady
      || !this.state.reviewDeclared || !socket || socket.readyState !== 1
      || !this.state.reviews.includes(row) || row.attempted || row.completion || this.remaining(row) <= 0) return;
    let wire: string;
    try { wire = answer(row.ask, choiceIndex); } catch { return; }
    const sending: Review = { ...row, attempted: true, phase: 'sending' };
    // Consume the UI's one send attempt synchronously, before send and React rendering.
    this.replace(row, sending);
    try {
      socket.send(wire);
      if (this.socket === socket) this.replace(sending, { ...sending, phase: 'sent' });
    } catch {
      if (this.socket === socket) this.replace(sending, { ...sending, phase: 'send_failed' });
    }
  }

  private replace(before: Review, after: Review) {
    if (!this.state.reviews.includes(before)) return;
    const read = this.state.askEvidence;
    const evidence = after.completion && !before.completion && read.ask === before.ask ? emptyAskEvidence(before.ask) : read;
    this.publishReviews(this.state.reviews.map(row => row === before ? after : row), evidence);
  }
  private publishReviews(reviews: readonly Review[], askEvidence = this.state.askEvidence) {
    const recent = retainRecentReviews(reviews);
    this.publish({ ...this.state, reviews: recent,
      askEvidence: recent === reviews || recent.some(row => row.ask === askEvidence.ask) ? askEvidence : emptyAskEvidence() });
  }
  disconnect = (notice: Message = message('notice.locked')) => {
    const settingsOperation = this.state.settings.pending?.operation;
    if (settingsOperation && settingsOperation !== 'get') notice = message('settings.unconfirmed');
    if (this.credentials.reset()) notice = message('egress.unconfirmed');
    this.clearCredential?.();
    this.clearCredential = null;
    this.uiReady = false;
    this.askContextVisible = false;
    this.contextStream = null;
    this.askContextDirty = false;
    this.activityContextDirty = false;
    this.rulesActive = false;
    this.seenAskIds.clear();
    this.completionSequence = 0;
    const socket = this.socket;
    this.socket = null;
    if (socket) {
      socket.onopen = null; socket.onmessage = null; socket.onerror = null; socket.onclose = null;
      try { socket.close(); } catch { /* Local scope is already closed. No Answer or retry. */ }
    }
    this.publish(locked(notice));
  };
}
