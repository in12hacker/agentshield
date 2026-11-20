import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach,describe,expect,it,vi } from 'vitest';
import {
allow,
anotherExchange,
ask,
askContext,askContextResult,
childKey,
completed,
configDocument,configSnapshot,
containerInstance,
contextResult,
contextView,
delegationPage,
delegationRow,
deny,
driverRead,
driverStatus,
egressBinding,egressRouteFact,egressSnapshot,
eventAnchor,
exchangesResult,
feature,
flowRead,
flowStatus,
groupResult,
historyExchange,
historyMessage,
historyReceipt,
id,
instanceDetails,
linkedContext,
logPage,
messagesResult,
originalSessions,
otherBootKey,
overviewResult,
parentKey,
phantomData,
processStamp,
ruleLog,
rulePolicy,
savedConfiguration,
savedRule,
syntheticCa,
syntheticPhantom,
syntheticSecret,
testToken,
trackedPage,
trackedRecord,
wire
} from '../test-support/contract-supply';
import { ActivityView } from './activity';
import { evidenceBlob,evidenceFilename,selectedEvidenceSnapshot } from './activity-evidence';
import { selectionAnchor } from './activity-state';
import { decodeLogEntry,decodeSecurityLog,logRowId,matchesLogPage,securityLogCommand,type RuleLog } from './activity-wire';
import { AskDialog } from './ask';
import { selectCurrentReview } from './ask-state';
import { CausalChain, NativeEvidence } from './causal-chain';
import { confirmedExecutionCall,projectCausalChain } from './causal-chain-model';
import { ChatHistoryView } from './chat-history';
import { OriginalChatMessage } from './chat-message';
import { chatData } from './chat-state';
import { chatCommand,decodeChatData,type ChatMessage } from './chat-wire';
import {
activityContextCommand,askContextCommand,decodeActivityContext,decodeAskContext,
type AskAuditContext,
type AttributionRequest,
type ContextView,
type ToolAttribution
} from './context-wire';
import { MAX_SECRET_BYTES,credentialErrors,privateCommand,validSecret,type PrivateOperation } from './credentials-wire';
import { EgressView } from './egress';
import { configuredSubjects, readEgressConfig } from './egress-wire';
import type { Ask,Outcome } from './event-wire';
import type { EvidenceRead } from './evidence-read';
import { byteText } from './global/byte-text';
import { sameJsonValue } from './global/json-value';
import { GroupDelegationsView } from './group';
import { groupPage } from './group-state';
import { decodeGroupData,groupDelegationsCommand,type GroupDelegation,type GroupPage } from './group-wire';
import { I18nProvider } from './i18n';
import { translate } from './messages';
import { driverFacts,featureView,flowFacts,flowView,overviewView } from './overview-model';
import { ProcessesView } from './processes';
import { containerIdentity,groupProcesses,observationState,observedValue,processIdentity } from './processes-model';
import { processDetails } from './processes-state';
import {
listQuery,processesCommand,
type ProcessCursor,
type ProcessPage
} from './processes-wire';
import { adoptionConfirmed,savedFacts } from './rules-state';
import { decodeRuleDefinition,definition,operationNames,ruleKinds,rulesCommand,type RuleChange,type RulesSaved } from './rules-wire';
import { ReviewSession,type SocketPort } from './session';
import { decodeSessionObservation } from './session-wire';
import { SettingsView } from './settings';
import {
changedConfigSections,configurationCommand,decodeConfigDocument,decodeConfigSnapshot,
runtimeCommand,type ConfigDocument,type ConfigSnapshot
} from './settings-wire';
import { endpoint,parseFrame,protectionOverview } from './wire';
import { factsJson,integer,numericU64,parseUnique,record,wireJson } from './wire-json';
import { decodeContainerInstance } from './workload-wire';

// Contract-shaped test supply, not Linux/driver acceptance. No real credential is used.
// Shapes: frozen protocol.rs + main_ports.rs + operation.rs + dispatch_event.rs;
// 16-byte DecisionId and flattened OperationResult also used by R68 frontend.py:219–242.
// Frozen driver_control.rs DriverStatus; plain u64 status counters are JSON numbers,
// unlike the decimal strings in EventKey. They are not used for Ask correlation.
class FakeSocket implements SocketPort {
  readyState = 0;
  onopen: SocketPort['onopen'] = null;
  onmessage: SocketPort['onmessage'] = null;
  onerror: SocketPort['onerror'] = null;
  onclose: SocketPort['onclose'] = null;
  sent: string[] = [];
  failSend = false;
  send(text: string) { if (this.failSend) throw new Error('fixture_send_failure'); this.sent.push(text); }
  close() { this.readyState = 3; }
  open() { this.readyState = 1; this.onopen?.(); }
  receive(text: string) { this.onmessage?.({ data: text }); }
}
function setup(access = 'manage_rules', content = false) {
  let now = 100;
  const sockets: FakeSocket[] = [];
  const session = new ReviewSession('ws://127.0.0.1:21854/', () => {
    const socket = new FakeSocket(); sockets.push(socket); return socket;
  }, () => now);
  session.connect(testToken);
  const socket = sockets[0]!;
  socket.open();
  socket.receive(JSON.stringify({ version: 2, authenticated: { access, chat_content: content } }));
  return { session, socket, sockets, advance: (ms: number) => { now += ms; } };
}
afterEach(() => { vi.useRealTimers(); });

const ingressStatus = { generation: '18446744073709551615', accepting: false, admitted: '9007199254740993', rejected: '0' };
const withoutKey = (object: object, key: string) => Object.fromEntries(Object.entries(object).filter(([name]) => name !== key));
const businessWire = (commandId: unknown, result: unknown = overviewResult()) => JSON.stringify({
  version: 2, business_reply: { id: commandId, result },
});
function queryId(socket: FakeSocket): readonly number[] {
  return (JSON.parse(socket.sent.filter(text => Object.hasOwn(JSON.parse(text), 'business')).at(-1)!) as {
    business: { id: number[] };
  }).business.id;
}

describe('current R68 wire', () => {
  it('accepts the original Ask and preserves missing, empty, zero and false facts', () => {
    const frame = parseFrame(wire(ask));
    expect(frame).toEqual({ kind: 'ask', ask });
    expect(() => parseFrame(wire({ ...ask, event: withoutKey(ask.event, 'user_stack') }))).toThrow();
    expect(parseFrame(wire(completed({ failed: { native_error: -13 } })))).toEqual({
      kind: 'operation_result', completion: completed({ failed: { native_error: -13 } }),
    });
  });
  it('explicitly decodes the formal driver_status envelope through the shared status boundary', () => {
    expect(parseFrame(wire(driverStatus))).toEqual({ kind: 'driver_status', snapshot: driverStatus.snapshot });
  });
  it.each([
    { source_instance: Array(15).fill(3) }, { source_instance: [...Array(15).fill(3), 256] },
    { sequence_domain: 1 }, { sequence_domain: '0' }, { sequence_domain: '18446744073709551616' },
    { sequence_domain: null }, { sequence_domain: '01' },
    { sequence: 9007199254740992 }, { sequence: '0' }, { sequence: '-1' },
    { sequence: '1.0' }, { sequence: '1e3' }, { sequence: '18446744073709551616' }, { sequence: null },
  ])('rejects malformed/old EventKey formats through the same Ask/result decoder %#', changed => {
    const event = { ...ask.event, header: { ...ask.event.header, ...changed } };
    expect(() => parseFrame(wire({ ...ask, event }))).toThrow();
    expect(() => parseFrame(wire({ ...completed(), event }))).toThrow();
  });
  it.each([
    JSON.stringify({ version: 1, message: ask }),
    JSON.stringify({ version: 2, message: ask, stream: null }),
    JSON.stringify({ version: 2, authenticated: { access: 'ManageRules', chat_content: false } }),
    JSON.stringify({ version: 2, message: null }),
    wire({ type: 'driver_status' }), wire({ type: 'driver_status', snapshot: null }),
    wire({ type: 'driver_status', snapshot: [] }), wire({ type: 'driver_status', snapshot: 1 }),
    wire({ ...driverStatus, extra: null }), wire({ type: 'future_notification', snapshot: driverStatus.snapshot }),
    JSON.stringify({ version: 1, message: driverStatus }),
    JSON.stringify({ version: 2, message: driverStatus, diagnostic: null }),
    `{"version":2,"version":2,"message":${JSON.stringify(ask)}}`,
    `{"version":2,"message":${JSON.stringify(ask)},"messa\\u0067e":null}`,
    `[${wire(ask)}]`, wire({ ...ask, decision_id: 'not-an-original-id' }),
    wire({ ...ask, choices: [{ type: 'deny' }] }),
    wire({ ...ask, choices: [{ type: 'allow', terminate: 'none' }] }),
    wire({ ...ask, choices: [{ type: 'deny', terminate: 'tree' }] }),
    wire({ ...ask, remaining_ms: '5000' }),
    wire({ ...completed(), result: { type: 'receipt', proposal: null, outcome: 'success', termination: {} } }),
    wire({ ...completed(), result: { type: 'receipt', proposal: null, outcome: { type: 'failed', native_error: -13 }, termination: {} } }),
  ])('rejects incompatible/ambiguous frames %#', text => { expect(() => parseFrame(text)).toThrow(); });
  it('accepts only a configured literal loopback ws root endpoint', () => {
    for (const root of ['ws://127.0.0.1:21854', 'ws://127.9.0.2:80', 'ws://[::1]:21854']) {
      for (const url of [root, root + '/']) {
        expect(new URL(url).pathname).toBe('/');
        expect(endpoint(url)).toEqual({ url: root + '/', problem: null });
      }
    }
    for (const url of [undefined, '', 'ws://localhost:21854/', 'wss://127.0.0.1:21854/',
      'ws://127.0.0.1:21854/?', 'ws://127.0.0.1:21854/#', 'ws://127.0.0.1:21854?', 'ws://127.0.0.1:21854#',
      'ws://127.0.0.1:21854/api/v1/ws', 'ws://a@127.0.0.1:21854/', 'ws://0.0.0.0:21854/',
      'ws://127.0.0.1:21854/\n', 'ws://127.0.0.1:21854//', 'ws://127.0.0.1:0/',
      'ws://127.0.0.1:65536/', 'ws://127.300.0.1:12/']) expect(endpoint(url).url).toBeNull();
  });
});

describe('connection-scoped original review', () => {
  it('requires the original timeout Decision and allows closing only real double-identity completions', () => {
    for (const changed of [withoutKey(ask, 'timeout'), { ...ask, timeout: null }, { ...ask, timeout: { type: 'deny' } }])
      expect(() => parseFrame(wire(changed))).toThrow();
    const { session, socket, advance } = setup('manage_rules', true);
    session.setUiReady(true); socket.receive(wire(ask));
    const a = session.getSnapshot().reviews[0]!;
    session.closeReview(a);
    expect(session.getSnapshot().reviews).toEqual([a]);
    advance(5001); session.closeReview(a); session.submit(a, 0);
    expect(session.getSnapshot().reviews[0]).toBe(a);
    expect(a.completion).toBeNull();
    expect(socket.sent.some(text => JSON.parse(text).message?.type === 'answer')).toBe(false);
    socket.receive(wire({ ...completed(), event: { ...ask.event, header: { ...ask.event.header, sequence: '1' } } }));
    expect(session.getSnapshot().reviews[0]).toBe(a);
    socket.receive(wire(completed('unknown')));
    const finished = session.getSnapshot().reviews[0]!;
    session.openCompletionEvidence(finished.completion!); session.closeReview(finished);
    expect(session.getSnapshot().reviews).toEqual([]);
    expect(selectionAnchor(session.getSnapshot().activity.selection)).toEqual({
      source_instance: ask.event.header.source_instance, sequence_domain: ask.event.header.sequence_domain, sequence: ask.event.header.sequence,
    });
    socket.receive(wire(ask)); // Closed identities cannot re-arm.
    expect(session.getSnapshot().reviews).toEqual([]);
    session.disconnect();
  });
  it('keeps B’s early completion while A is answerable and exposes no hide or skip controls', () => {
    const { session, socket } = setup();
    session.setUiReady(true); socket.receive(wire(ask));
    const a = session.getSnapshot().reviews[0]!;
    const b: Ask = { ...ask, decision_id: Array(16).fill(23), event: { ...ask.event, header: { ...ask.event.header, sequence: '7' } } };
    socket.receive(wire(b)); socket.receive(wire({ ...completed(), decision_id: b.decision_id, event: b.event }));
    expect(selectCurrentReview(session.getSnapshot().reviews, a.ask, 100)).toBe(a);
    const render = () => renderToStaticMarkup(createElement(I18nProvider, { children: createElement(AskDialog, {
      row: a, onClose: () => {}, onInspect: () => {}, onEvidence: () => {}, reviews: session.getSnapshot().reviews, now: 100,
      session, evidence: session.getSnapshot().askEvidence, chatContent: false,
    }) }));
    expect(render()).not.toContain(translate('ja-JP', 'ask.closeCompleted'));
    expect(render()).toContain('class="ask-intent"');
    expect(render()).not.toContain('class="ask-intent" open=""');
    expect(render()).toContain('class="causal-chain stack-tight"');
    socket.failSend = true; session.submit(a, 0);
    const failed = session.getSnapshot().reviews[0]!;
    expect(failed.phase).toBe('send_failed');
    expect(selectCurrentReview(session.getSnapshot().reviews, a.ask, 100)).toBe(failed);
    socket.failSend = false;
    socket.receive(wire({ ...ask, decision_id: Array(16).fill(24) }));
    expect(selectCurrentReview(session.getSnapshot().reviews, a.ask, 100)?.ask.decision_id).toEqual(Array(16).fill(24));
    session.disconnect();
  });
  it('retains answerable A when B arrives, then automatically selects B after A completes', () => {
    const { session, socket } = setup();
    session.setUiReady(true); socket.receive(wire(ask));
    const a = session.getSnapshot().reviews[0]!;
    const b = { ...ask, decision_id: Array(16).fill(23),
      event: { ...ask.event, header: { ...ask.event.header, sequence: '7' } } };
    socket.receive(wire(b));
    expect(selectCurrentReview(session.getSnapshot().reviews, a.ask, 100)?.ask).toBe(a.ask);
    const receivedB = session.getSnapshot().reviews[1]!;
    expect(receivedB.ask.decision_id).toEqual(b.decision_id);
    socket.receive(wire(completed()));
    expect(selectCurrentReview(session.getSnapshot().reviews, a.ask, 100)).toBe(receivedB);
    expect(receivedB.observedAt).toBe(100);
    expect(receivedB.ask.remaining_ms).toBe(ask.remaining_ms);
    session.submit(receivedB, 0); session.submit(receivedB, 0);
    expect(socket.sent.filter(text => JSON.parse(text).message?.type === 'answer')).toHaveLength(1);
    session.disconnect();
  });
  it('advances in receipt order on an attempt or local exhaustion and retains the sole receipt', () => {
    const { session, socket, advance } = setup();
    session.setUiReady(true); socket.receive(wire({ ...ask, remaining_ms: 10 }));
    const a = session.getSnapshot().reviews[0]!;
    advance(1);
    socket.receive(wire({ ...ask, decision_id: Array(16).fill(23), remaining_ms: 100 }));
    socket.receive(wire({ ...ask, decision_id: Array(16).fill(2), remaining_ms: 100 }));
    const rows = session.getSnapshot().reviews;
    expect(selectCurrentReview(rows, a.ask, 110)).toBe(rows[1]);
    session.submit(a, 0);
    expect(selectCurrentReview(session.getSnapshot().reviews, a.ask, 101)).toBe(rows[1]);
    socket.receive(wire(completed()));
    const finished = session.getSnapshot().reviews[0]!;
    expect(selectCurrentReview([finished], a.ask, 1000)).toBe(finished);
    session.closeReview(finished);
    expect(session.getSnapshot().reviews).not.toContain(finished);
    session.disconnect();
  });
  it('lets formal driver_status pass ReadOnly and review sessions without changing any grants or Ask state', () => {
    for (const access of ['read_only', 'manage_rules']) {
      const { session, socket } = setup(access);
      if (access === 'manage_rules') { session.setUiReady(true); socket.receive(wire(ask)); }
      const before = session.getSnapshot();
      const sent = socket.sent.length;
      socket.receive(wire(driverStatus));
      expect(session.getSnapshot()).toBe(before);
      expect(socket.sent).toHaveLength(sent);
      if (access === 'manage_rules') {
        session.submit(session.getSnapshot().reviews[0]!, 0);
        socket.receive(wire(driverStatus));
        socket.receive(wire(completed('unknown')));
        expect(session.getSnapshot().reviews[0]?.completion?.result).toEqual(completed('unknown').result);
      }
      session.disconnect();
    }
  });
  it('requires actual ManageRules (including ManageService) AND a mounted UI, independent of chat scope', () => {
    const read = setup('read_only', true);
    read.session.setUiReady(true);
    expect(JSON.parse(read.socket.sent[0]!)).toEqual({ version: 2, authenticate: { token: testToken } });
    expect(JSON.parse(read.socket.sent[1]!).business.query).toEqual({ type: 'protection_overview' });
    read.socket.receive(wire(ask));
    read.session.submit({ ask, observedAt: 100, attempted: false, phase: 'waiting', completion: null, completionOrder: null }, 0);
    expect(read.socket.sent).toHaveLength(2);
    expect(read.session.getSnapshot().phase).toBe('locked');
    for (const access of ['manage_rules', 'manage_service']) {
      const { session, socket } = setup(access, false);
      expect(socket.sent).toHaveLength(2);
      session.setUiReady(true);
      expect(JSON.parse(socket.sent[2]!)).toEqual({ version: 2, message: { type: 'review_availability', available: true } });
      session.disconnect();
    }
  });
  it('sends the original choice exactly once and waits for the original result, never claims execution on send', () => {
    const { session, socket } = setup();
    session.setUiReady(true); socket.receive(wire({ ...ask, choices: [deny] }));
    const row = session.getSnapshot().reviews[0]!;
    session.submit(row, 1); expect(socket.sent).toHaveLength(3); // absent Allow cannot be submitted
    session.submit(row, 0); session.submit(row, 0);
    const sent = session.getSnapshot().reviews[0]!;
    session.submit(sent, 0);
    expect(socket.sent.slice(3).map(text => JSON.parse(text))).toEqual([
      { version: 2, message: { type: 'answer', decision_id: id, decision: deny } },
    ]);
    expect(sent.phase).toBe('sent'); expect(sent.completion).toBeNull();
    socket.receive(wire(ask)); // repeated Ask cannot renew or re-arm
    expect(session.getSnapshot().reviews[0]).toBe(sent);
    socket.receive(wire(completed('denied')));
    expect(session.getSnapshot().reviews[0]?.completion?.result).toEqual(completed('denied').result);
    session.disconnect();
  });
  it('does not correlate null/different IDs by the identical path, PID or event', () => {
    const { session, socket } = setup();
    session.setUiReady(true); socket.receive(wire(ask));
    const row = session.getSnapshot().reviews[0];
    for (const foreign of [null, Array(16).fill(99)]) {
      socket.receive(wire(completed('allowed', foreign)));
      expect(session.getSnapshot().reviews[0]).toBe(row);
    }
    socket.receive(JSON.stringify({ version: 2, diagnostic: { type: 'source_unavailable' } }));
    expect(session.getSnapshot().reviews[0]).toBe(row);
    session.disconnect();
  });
  it('does not complete the same DecisionId for another original EventKey, including u64 values that Number would conflate', () => {
    const { session, socket } = setup();
    session.setUiReady(true); socket.receive(wire(ask));
    const row = session.getSnapshot().reviews[0]!;
    for (const changed of [
      { source_instance: Array(16).fill(9) },
      { sequence_domain: '18446744073709551614' },
      { sequence: '9007199254740992' },
    ]) {
      // The DecisionId, path, TGID/TID and every other event fact remain identical.
      const event = { ...ask.event, header: { ...ask.event.header, ...changed } };
      socket.receive(wire({ ...completed('allowed'), event }));
      expect(session.getSnapshot().reviews[0]).toBe(row);
      expect(session.getSnapshot().phase).toBe('authenticated');
      expect(row.completion).toBeNull();
    }
    socket.receive(wire(completed('denied')));
    expect(session.getSnapshot().reviews[0]?.completion).toEqual(completed('denied'));
    session.disconnect();
  });
  it.each(['unknown', 'cancelled', { failed: { native_error: -13 } }] satisfies Outcome[])('preserves non-success physical outcome %j and the first result', outcome => {
    const { session, socket } = setup();
    session.setUiReady(true); socket.receive(wire(ask)); socket.receive(wire(completed(outcome)));
    const row = session.getSnapshot().reviews[0]!;
    expect(row.completion?.result).toEqual(completed(outcome).result);
    socket.receive(wire(completed('allowed')));
    expect(session.getSnapshot().reviews[0]).toBe(row);
    session.disconnect();
  });
  it('shows a genuine report failure without turning it into a deny/allow receipt', () => {
    const { session, socket } = setup(); session.setUiReady(true); socket.receive(wire(ask));
    const failure = { ...completed(), result: { type: 'failed', proposal: null, code: 'unavailable', message: '' } };
    socket.receive(wire(failure));
    expect(session.getSnapshot().reviews[0]?.completion).toEqual(failure);
    session.disconnect();
  });
  it('does not answer on local sample expiry and accepts a matching late result', () => {
    const { session, socket, advance } = setup(); session.setUiReady(true); socket.receive(wire(ask));
    const row = session.getSnapshot().reviews[0]!; advance(6000); session.submit(row, 0);
    expect(socket.sent).toHaveLength(3); expect(row.completion).toBeNull();
    socket.receive(wire(completed('cancelled')));
    expect(session.getSnapshot().reviews[0]?.phase).toBe('complete');
    session.disconnect();
  });
  it('does not retry a failed send, and can still consume the actual original result', () => {
    const { session, socket } = setup(); session.setUiReady(true); socket.receive(wire(ask));
    socket.failSend = true; session.submit(session.getSnapshot().reviews[0]!, 0);
    const failed = session.getSnapshot().reviews[0]!;
    expect(failed.phase).toBe('send_failed'); expect(failed.completion).toBeNull();
    socket.failSend = false; session.submit(failed, 0); expect(socket.sent).toHaveLength(3);
    socket.receive(wire(completed('unknown'))); expect(session.getSnapshot().reviews[0]?.completion).not.toBeNull();
    session.disconnect();
  });
  it('locks on disconnect, drops old Ask/auth scope, ignores stale callbacks, and only reconnects explicitly', () => {
    const { session, socket, sockets } = setup(); session.setUiReady(true); socket.receive(wire(ask));
    const oldRow = session.getSnapshot().reviews[0]!;
    const lateCallback = socket.onmessage!;
    socket.onclose?.();
    expect(session.getSnapshot()).toMatchObject({ phase: 'locked', access: null, reviews: [], reviewDeclared: false });
    expect(sockets).toHaveLength(1); expect(socket.sent).toHaveLength(3);
    session.connect('b'.repeat(64));
    const next = sockets[1]!; next.open();
    lateCallback({ data: wire(completed('allowed')) }); session.submit(oldRow, 0);
    expect(next.sent.map(text => JSON.parse(text))).toEqual([{ version: 2, authenticate: { token: 'b'.repeat(64) } }]);
    next.receive(JSON.stringify({ version: 2, authenticated: { access: 'read_only', chat_content: false } }));
    session.setUiReady(true); session.submit(oldRow, 0);
    expect(next.sent).toHaveLength(2); expect(session.getSnapshot().reviews).toEqual([]);
    expect(JSON.stringify(session.getSnapshot())).not.toContain(testToken);
    session.disconnect();
  });
  it('cancelling a connecting scope clears the credential before a stale open callback', () => {
    const socket = new FakeSocket();
    const session = new ReviewSession('ws://127.0.0.1:21854/', () => socket);
    session.connect(testToken); const lateOpen = socket.onopen!; session.disconnect(); lateOpen();
    expect(socket.sent).toEqual([]); expect(session.getSnapshot().phase).toBe('locked');
  });
  it('waits for real authentication without an extra deadline; unmount/lock never sends an Answer', () => {
    vi.useFakeTimers();
    const socket = new FakeSocket(); const session = new ReviewSession('ws://127.0.0.1:21854/', () => socket);
    session.connect(testToken); socket.open(); vi.advanceTimersByTime(60000);
    expect(session.getSnapshot().phase).toBe('authenticating'); expect(socket.sent).toHaveLength(1);
    expect(vi.getTimerCount()).toBe(0);
    socket.receive(JSON.stringify({ version: 2, authenticated: { access: 'read_only', chat_content: false } }));
    expect(session.getSnapshot().phase).toBe('authenticated');
    session.disconnect();
    const ready = setup(); ready.session.setUiReady(true); ready.socket.receive(wire(ask));
    ready.session.setUiReady(false); ready.session.disconnect();
    expect(ready.socket.sent.map(text => JSON.parse(text).message?.type).filter(Boolean))
      .toEqual(['review_availability', 'review_availability']);
  });
});

describe('same-connection protection overview', () => {
  it('queries immediately after ReadOnly authentication without review UI or chat_content', () => {
    const { session, socket, sockets } = setup('read_only', false);
    const commandId = queryId(socket);
    expect(commandId).toHaveLength(16);
    expect(commandId.every(byte => Number.isInteger(byte) && byte >= 0 && byte <= 255)).toBe(true);
    expect(socket.sent.map(text => JSON.parse(text))).toEqual([
      { version: 2, authenticate: { token: testToken } },
      { version: 2, business: { id: commandId, query: { type: 'protection_overview' } } },
    ]);
    expect(JSON.parse(protectionOverview(commandId))).toEqual(JSON.parse(socket.sent[1]!));
    expect(session.getSnapshot().overview).toEqual({ phase: 'loading', id: commandId });
    session.setUiReady(true);
    socket.receive(businessWire(commandId));
    expect(session.getSnapshot()).toMatchObject({ phase: 'authenticated', access: 'read_only',
      reviewDeclared: false, reviews: [], overview: { phase: 'returned', result: overviewResult() } });
    expect(sockets).toHaveLength(1);
    expect(socket.sent).toHaveLength(2);
    session.disconnect();
  });

  it.each([id.slice(1), [...id.slice(1), 256], '0102030405060708090a0b0c0d0e0f10', null])(
    'rejects non-current business CommandId shapes %j', commandId => {
      expect(() => parseFrame(businessWire(commandId))).toThrow();
      expect(() => protectionOverview(commandId as readonly number[])).toThrow();
    });

  it('rejects old lanes, missing composite members, duplicate IDs and string-encoded Driver counters', () => {
    for (const text of [
      JSON.stringify({ version: 1, business_reply: { id, result: overviewResult() } }),
      JSON.stringify({ version: 2, message: { type: 'protection_overview', id } }),
      JSON.stringify({ version: 2, business_reply: { id, result: overviewResult() }, message: null }),
      businessWire(id, { status: 'completed', data: { type: 'protection_overview', driver: driverRead } }),
      `{"version":2,"business_reply":{"id":${JSON.stringify(id)},"i\\u0064":${JSON.stringify(id)},"result":${JSON.stringify(overviewResult())}}`,
      businessWire(id, overviewResult({ ...driverRead, data: { ...driverStatus,
        snapshot: { ...driverStatus.snapshot, dropped: '9007199254740993' } } })),
      businessWire(id, overviewResult(driverRead, { status: 'completed', data: { ...flowStatus,
        status: { ...flowStatus.status, active_exchanges: 1 } } })),
    ]) expect(() => parseFrame(text)).toThrow();
  });

  it('requires every serialized DriverStatus field on both push and composite reply, including Option fields', () => {
    for (const key of Object.keys(driverStatus.snapshot)) {
      const data = { ...driverStatus, snapshot: withoutKey(driverStatus.snapshot, key) };
      expect(() => parseFrame(wire(data)), `push missing ${key}`).toThrow();
      expect(() => parseFrame(businessWire(id, overviewResult({ status: 'completed', data }))), `reply missing ${key}`).toThrow();
    }
    for (const family of ['process', 'file', 'network']) {
      for (const key of Object.keys(feature)) {
        const data = { ...driverStatus, snapshot: { ...driverStatus.snapshot, [family]: withoutKey(feature, key) } };
        expect(() => parseFrame(wire(data)), `${family} missing ${key}`).toThrow();
        expect(() => parseFrame(businessWire(id, overviewResult({ status: 'failed', code: 'failed', message: '', facts: data })))).toThrow();
      }
    }
    for (const key of Object.keys(driverStatus.snapshot.startup_protection)) {
      const data = { ...driverStatus, snapshot: { ...driverStatus.snapshot,
        startup_protection: withoutKey(driverStatus.snapshot.startup_protection, key) } };
      expect(() => parseFrame(wire(data))).toThrow();
    }
  });

  it('requires every FlowStatus field and the ingress field instead of accepting incomplete success facts', () => {
    for (const key of Object.keys(flowStatus.status)) {
      const data = { ...flowStatus, status: withoutKey(flowStatus.status, key) };
      expect(() => parseFrame(businessWire(id, overviewResult(driverRead, { status: 'completed', data }))), `Flow missing ${key}`).toThrow();
    }
    const data = withoutKey(flowStatus, 'ingress');
    expect(() => parseFrame(businessWire(id, overviewResult(driverRead, { status: 'completed', data })))).toThrow();
    expect(() => parseFrame(businessWire(id, overviewResult(driverRead, {
      status: 'failed', code: 'unavailable', message: '',
    })))).toThrow(); // facts is required even when it is null.
  });

  it('accepts explicit Option nulls and retains empty, false and zero values in the unique current shape', () => {
    const snapshot = { ...driverStatus.snapshot, driver_config_version: null, rule_version: null,
      startup_protection: null, error: null, pending: 0, process: { ...feature, startup_default: false } };
    const driver = { status: 'completed', data: { type: 'driver_status', snapshot } };
    expect(parseFrame(wire(driver.data))).toEqual({ kind: 'driver_status', snapshot });
    expect(parseFrame(businessWire(id, overviewResult(driver)))).toMatchObject({ kind: 'business_reply',
      reply: { result: { data: { driver, flow: flowRead } } } });
    for (const key of ['process', 'file', 'network', 'source_state', 'self_protection', 'dropped']) {
      expect(() => parseFrame(wire({ ...driverStatus, snapshot: { ...driverStatus.snapshot, [key]: null } }))).toThrow();
    }
    expect(() => parseFrame(businessWire(id, overviewResult(driverRead, { status: 'completed', data: {
      ...flowStatus, status: null,
    } })))).toThrow();
  });

  it('validates non-null TlsIngressStatus with required decimal generation/admitted/rejected and boolean accepting', () => {
    const data = { ...flowStatus, ingress: ingressStatus };
    expect(parseFrame(businessWire(id, overviewResult(driverRead, { status: 'completed', data })))).toMatchObject({
      kind: 'business_reply', reply: { result: { data: { flow: { status: 'completed', data } } } },
    });
    for (const key of Object.keys(ingressStatus)) {
      expect(() => parseFrame(businessWire(id, overviewResult(driverRead, { status: 'completed', data: {
        ...data, ingress: withoutKey(ingressStatus, key),
      } }))), `ingress missing ${key}`).toThrow();
    }
    for (const ingress of [[], {}, { ...ingressStatus, generation: 1 }, { ...ingressStatus, accepting: 'false' },
      { ...ingressStatus, admitted: null }, { ...ingressStatus, admitted: '-1' },
      { ...ingressStatus, rejected: '18446744073709551616' }, { ...ingressStatus, rejected: '00' },
      { ...ingressStatus, extra: 0 }]) {
      expect(() => parseFrame(businessWire(id, overviewResult(driverRead, { status: 'completed', data: {
        ...data, ingress,
      } })))).toThrow();
    }
  });

  it('preserves driver completed / Flow failed independently of the outer completed envelope', () => {
    const { session, socket } = setup('read_only', false);
    const failure = { status: 'failed', code: 'unavailable', message: '', facts: null };
    socket.receive(businessWire(queryId(socket), overviewResult(driverRead, failure)));
    const read = session.getSnapshot().overview;
    expect(read).toMatchObject({ phase: 'returned', result: overviewResult(driverRead, failure) });
    expect(driverFacts(read)).toEqual(driverStatus.snapshot);
    expect(flowFacts(read)).toBeUndefined();
    expect(overviewView(read).label).toBe('overview.partial');
    expect(session.getSnapshot().phase).toBe('authenticated');
    session.disconnect();
  });

  it('uses effective category states, not startup defaults, overrides, counts or source lifecycle', () => {
    const { session, socket } = setup();
    const snapshot = { ...driverStatus.snapshot, source_state: 'failed', pending: 0, error: '',
      process: { startup_default: false, runtime_override: 'disabled', effective_state: 'enabled' },
      file: { startup_default: true, runtime_override: 'enabled', effective_state: 'disabled' },
      network: { startup_default: true, runtime_override: 'default', effective_state: 'unavailable' },
      self_protection: 'disabled' };
    socket.receive(businessWire(queryId(socket), overviewResult({ status: 'completed', data: { type: 'driver_status', snapshot } })));
    const data = driverFacts(session.getSnapshot().overview)!;
    expect(data).toEqual(snapshot);
    expect(featureView(data.process?.effective_state).label).toBe('feature.enabled');
    expect(featureView(data.file?.effective_state).label).toBe('feature.disabled');
    expect(featureView(data.network?.effective_state).label).toBe('feature.unavailable');
    expect(featureView(data.self_protection).label).toBe('feature.disabled');
    expect(data.error).toBe('');
    session.refreshOverview();
    socket.receive(businessWire(queryId(socket), overviewResult({
      status: 'failed', code: 'unavailable', message: '', facts: null,
    })));
    expect(featureView(driverFacts(session.getSnapshot().overview)?.process?.effective_state).label).toBe('common.unknown');
    expect(featureView(driverFacts(session.getSnapshot().overview)?.self_protection).label).toBe('common.unknown');
    session.disconnect();
  });

  it.each([
    ['running', 'flow.running'], ['source_closed', 'flow.sourceClosed'], ['stopped', 'flow.stopped'],
  ])('reports actual Flow intake %s without converting it to whole-exchange completion', (state, label) => {
    const { session, socket } = setup('read_only');
    socket.receive(businessWire(queryId(socket), overviewResult(driverRead, { status: 'completed', data: {
      ...flowStatus, status: { ...flowStatus.status, intake_state: state },
    } })));
    expect(flowView(flowFacts(session.getSnapshot().overview)?.intake_state).label).toBe(label);
    expect(flowFacts(session.getSnapshot().overview)?.active_exchanges).toBe('9007199254740993');
    session.disconnect();
  });

  it('retains accepted and failed facts as such, and treats wrong ReplyData types as unknown', () => {
    const { session, socket } = setup('read_only');
    const accepted = { status: 'accepted', data: driverStatus };
    const failed = { status: 'failed', code: 'failed', message: 'partial status', facts: flowStatus };
    socket.receive(businessWire(queryId(socket), overviewResult(accepted, failed)));
    expect(session.getSnapshot().overview).toMatchObject({ result: overviewResult(accepted, failed) });
    expect(driverFacts(session.getSnapshot().overview)).toEqual(driverStatus.snapshot);
    expect(flowFacts(session.getSnapshot().overview)).toEqual(flowStatus.status);
    expect(overviewView(session.getSnapshot().overview).label).toBe('overview.partial');
    session.refreshOverview();
    socket.receive(businessWire(queryId(socket), overviewResult(flowRead, driverRead)));
    expect(driverFacts(session.getSnapshot().overview)).toBeUndefined();
    expect(flowFacts(session.getSnapshot().overview)).toBeUndefined();
    expect(overviewView(session.getSnapshot().overview).label).toBe('overview.unknown');
    session.refreshOverview();
    socket.receive(businessWire(queryId(socket), overviewResult(driverRead, {
      status: 'failed', code: 'unavailable', message: '', facts: null,
    })));
    expect(flowView(flowFacts(session.getSnapshot().overview)?.intake_state).label).toBe('common.unknown');
    expect(session.getSnapshot().phase).toBe('authenticated');
    session.disconnect();
  });

  it('preserves numeric Driver u64 and string Flow u64 at full decimal precision through push and reply', () => {
    const { session, socket } = setup('read_only');
    const raw = businessWire(queryId(socket)).replace('"dropped":0', '"dropped":18446744073709551615')
      .replace('"persistent_generation":1', '"persistent_generation":9007199254740993');
    socket.receive(raw);
    const read = session.getSnapshot().overview;
    expect(driverFacts(read)?.dropped).toBe(18446744073709551615n);
    expect(driverFacts(read)?.persistent_generation).toBe(9007199254740993n);
    expect(flowFacts(read)?.parse_failures).toBe('18446744073709551615');
    const rendered = factsJson(read);
    expect(rendered).toContain('"dropped": 18446744073709551615');
    expect(rendered).toContain('"active_exchanges": "9007199254740993"');
    const push = parseFrame(wire(driverStatus).replace('"dropped":0', '"dropped":18446744073709551615'));
    expect(push.kind === 'driver_status' && push.snapshot.dropped).toBe(18446744073709551615n);
    for (const tooLarge of ['18446744073709551616', '-1', '1.5']) {
      expect(() => parseFrame(raw.replace('"dropped":18446744073709551615', `"dropped":${tooLarge}`))).toThrow();
    }
    session.disconnect();
  });

  it('replaces a refresh slot, rejects foreign/old/duplicate IDs and never fills it from a push', () => {
    const { session, socket } = setup();
    const first = queryId(socket);
    session.refreshOverview();
    const second = queryId(socket);
    expect(second).not.toEqual(first);
    const pending = session.getSnapshot();
    socket.receive(businessWire(first));
    socket.receive(businessWire(Array(16).fill(99)));
    socket.receive(wire(driverStatus));
    expect(session.getSnapshot()).toBe(pending);
    socket.receive(businessWire(second));
    const returned = session.getSnapshot();
    socket.receive(businessWire(second, { status: 'failed', code: 'failed', message: 'duplicate' }));
    expect(session.getSnapshot()).toBe(returned);
    session.refreshOverview();
    expect(session.getSnapshot().overview).toEqual({ phase: 'loading', id: queryId(socket) });
    expect(driverFacts(session.getSnapshot().overview)).toBeUndefined();
    session.disconnect();
  });

  it('clears facts on disconnect and ignores both old callbacks and old IDs on the next connection', () => {
    const { session, socket, sockets } = setup('read_only');
    const oldId = queryId(socket);
    socket.receive(businessWire(oldId));
    const late = socket.onmessage!;
    socket.onclose?.();
    expect(session.getSnapshot().overview).toEqual({ phase: 'initial' });
    session.connect(testToken);
    const next = sockets[1]!;
    next.open(); next.receive(JSON.stringify({ version: 2, authenticated: { access: 'read_only', chat_content: false } }));
    const pending = session.getSnapshot();
    expect(queryId(next)).not.toEqual(oldId);
    late({ data: businessWire(oldId) });
    next.receive(businessWire(oldId));
    expect(session.getSnapshot()).toBe(pending);
    next.receive(businessWire(queryId(next)));
    expect(session.getSnapshot().overview.phase).toBe('returned');
    session.disconnect();
  });

  it('leaves a live Ask and one-shot Answer isolated from partial, accepted and failed overview replies', () => {
    const { session, socket, sockets } = setup('manage_rules', false);
    session.setUiReady(true); socket.receive(wire(ask));
    const row = session.getSnapshot().reviews[0]!;
    socket.receive(businessWire(queryId(socket), overviewResult(driverRead, {
      status: 'failed', code: 'unavailable', message: '', facts: null,
    })));
    expect(session.getSnapshot().reviews[0]).toBe(row);
    session.refreshOverview();
    socket.receive(businessWire(queryId(socket), overviewResult({ status: 'accepted', data: driverStatus }, flowRead)));
    session.refreshOverview();
    socket.receive(businessWire(queryId(socket), { status: 'failed', code: 'unauthorized', message: '' }));
    expect(session.getSnapshot()).toMatchObject({ phase: 'authenticated', reviewDeclared: true });
    expect(session.getSnapshot().reviews[0]).toBe(row);
    session.submit(row, 1); session.submit(row, 1);
    const answers = socket.sent.map(text => JSON.parse(text)).filter(frame => frame.message?.type === 'answer');
    expect(answers).toEqual([{ version: 2, message: { type: 'answer', decision_id: id, decision: deny } }]);
    socket.receive(wire(completed('denied')));
    expect(session.getSnapshot().reviews[0]?.completion).toEqual(completed('denied'));
    expect(sockets).toHaveLength(1);
    session.disconnect();
  });

  it('reports a local read send failure without clearing Ask or automatically retrying', () => {
    const { session, socket } = setup();
    session.setUiReady(true); socket.receive(wire(ask));
    const row = session.getSnapshot().reviews[0]!;
    socket.failSend = true;
    session.refreshOverview();
    expect(session.getSnapshot().overview.phase).toBe('send_failed');
    expect(session.getSnapshot().phase).toBe('authenticated');
    expect(session.getSnapshot().reviews[0]).toBe(row);
    socket.failSend = false;
    session.submit(row, 0);
    socket.receive(wire(completed('unknown')));
    expect(session.getSnapshot().reviews[0]?.completion).toEqual(completed('unknown'));
    session.disconnect();
  });
});

// Current R58/MVP management shapes: native source, original typed rule, RulesSaved facts.
function ruleCommand(socket: FakeSocket) {
  return JSON.parse(socket.sent.filter(text => JSON.parse(text).message?.type === 'rules').at(-1)!).message as {
    type: 'rules'; id: readonly number[]; command: { type: string; expected_version?: readonly number[]; change?: unknown };
  };
}
const ruleReply = (id: readonly number[], result: unknown) => factsJson({ version: 2, message: { type: 'reply', id, result } });
function readPolicy(session: ReviewSession, socket: FakeSocket, snapshot = rulePolicy) {
  session.refreshRules();
  socket.receive(ruleReply(ruleCommand(socket).id, { status: 'completed', data: { type: 'rules', snapshot } }));
  return session.getSnapshot().rules.snapshot!;
}
const editCases: RuleChange[] = [
  { type: 'create_group', group: { ...rulePolicy.document.groups[0]!, id: 'new-group', rules: [] } },
  { type: 'replace_group', group_id: 'saved-decisions', group: { ...rulePolicy.document.groups[0]!, enabled: false } },
  { type: 'delete_group', group_id: 'saved-decisions' },
  { type: 'create_rule', group_id: 'saved-decisions', rule: definition('file', { ...rulePolicy.document.groups[0]!.rules[0]!, id: 'new-rule' }) },
  { type: 'replace_rule', rule_id: 'saved-open', rule: definition('file', { ...rulePolicy.document.groups[0]!.rules[0]!, enabled: false }) },
  { type: 'delete_rule', rule_id: 'saved-open' },
];

describe('rule workspace read and write lifetimes', () => {
  it('does not prefetch rules, reads once on entry and invalidates a late Get on leave/reentry', () => {
    const { session, socket } = setup('read_only');
    expect(socket.sent.some(text => JSON.parse(text).message?.type === 'rules')).toBe(false);
    session.enterRules();
    const oldId = ruleCommand(socket).id, count = socket.sent.length;
    session.enterRules();
    expect(socket.sent).toHaveLength(count);
    session.leaveRules();
    const left = session.getSnapshot();
    socket.receive(ruleReply(oldId, { status: 'completed', data: { type: 'rules', snapshot: rulePolicy } }));
    expect(session.getSnapshot()).toBe(left);
    session.enterRules();
    const freshId = ruleCommand(socket).id, entering = session.getSnapshot();
    expect(freshId).not.toEqual(oldId);
    socket.receive(ruleReply(oldId, { status: 'completed', data: { type: 'rules', snapshot: rulePolicy } }));
    expect(session.getSnapshot()).toBe(entering);
    socket.receive(ruleReply(freshId, { status: 'completed', data: { type: 'rules', snapshot: rulePolicy } }));
    const fresh = session.getSnapshot().rules.snapshot!;
    const beforeWrite = socket.sent.length;
    session.editRules(fresh, editCases[0]!);
    expect(socket.sent).toHaveLength(beforeWrite);
    session.disconnect();
  });

  it.each([false, true])('retains an actual Edit waiter on leave, only rereads on return (return before completion=%s)', returnEarly => {
    const { session, socket } = setup();
    session.enterRules();
    socket.receive(ruleReply(ruleCommand(socket).id, { status: 'completed', data: { type: 'rules', snapshot: rulePolicy } }));
    const original = session.getSnapshot().rules.snapshot!;
    session.editRules(original, editCases[1]!);
    const pending = session.getSnapshot().rules.pending!, count = socket.sent.length;
    session.leaveRules();
    expect(session.getSnapshot().rules).toMatchObject({ snapshot: null, pending });
    socket.receive(ruleReply(pending.id, { status: 'accepted', data: savedRule }));
    expect(session.getSnapshot().rules.pending).toBe(pending);
    expect(session.getSnapshot().rules.receipt?.result.status).toBe('accepted');
    if (returnEarly) session.enterRules();
    expect(socket.sent).toHaveLength(count);
    const failure = { status: 'failed', code: 'conflict', message: '', facts: savedRule };
    socket.receive(ruleReply(pending.id, failure));
    expect(session.getSnapshot().rules.receipt?.result).toEqual(failure);
    if (!returnEarly) {
      expect(socket.sent).toHaveLength(count);
      expect(session.getSnapshot().rules).toMatchObject({ phase: 'initial', pending: null, snapshot: null });
      session.enterRules();
    }
    expect(socket.sent).toHaveLength(count + 1);
    expect(ruleCommand(socket).command).toEqual({ type: 'get' });
    const beforeDuplicate = session.getSnapshot();
    socket.receive(ruleReply(pending.id, { status: 'completed', data: savedRule }));
    expect(session.getSnapshot()).toBe(beforeDuplicate);
    socket.receive(ruleReply(ruleCommand(socket).id, { status: 'completed', data: { type: 'rules', snapshot: rulePolicy } }));
    const beforeOldDraft = socket.sent.length;
    session.editRules(original, editCases[1]!);
    expect(socket.sent).toHaveLength(beforeOldDraft);
    expect(session.getSnapshot().rules.receipt?.result).toEqual(failure);
    session.disconnect();
  });
});

describe('current same-connection rule management', () => {
  it('retains the explicit self-protection mount condition through the same rule codec', () => {
    const original = rulePolicy.document.groups[0]!.rules[0]!;
    const mount = definition('file', { ...original, operations: ['mount'], target: { mount_effect: 'independent_view' } });
    expect(decodeRuleDefinition(parseUnique(factsJson(mount)))).toEqual(mount);
  });
  it('reads the full known protocol operation set without applying author input restrictions', () => {
    const original = rulePolicy.document.groups[0]!.rules[0]!;
    for (const kind of ruleKinds) {
      const known = definition(kind, { ...original, target: {}, operations: operationNames[kind] });
      expect(decodeRuleDefinition(parseUnique(factsJson(known)))).toEqual(known);
    }
  });
  it('retains an explicit zero mask in the shape codec without claiming it is a valid save candidate', () => {
    const rule = rulePolicy.document.groups[0]!.rules[0]!;
    const zeroMask = definition('file', { ...rule, target: { ...rule.target, access_mask: 0 } });
    // Business validation rejects zero. The lossless codec must not turn it into null or a nonzero value.
    expect(decodeRuleDefinition(parseUnique(factsJson(zeroMask)))).toEqual(zeroMask);
  });
  it('allows ReadOnly Get with complete details and sends no Edit', () => {
    const { session, socket } = setup('read_only');
    const snapshot = readPolicy(session, socket);
    expect(snapshot).toEqual(rulePolicy);
    const sent = socket.sent.length;
    for (const change of editCases) session.editRules(snapshot, change);
    expect(socket.sent).toHaveLength(sent);
    expect(session.getSnapshot().rules.snapshot).toBe(snapshot);
    session.disconnect();
  });

  it.each(editCases)('sends only the current typed Edit for $type and keeps adoption unconfirmed on a save alone', change => {
    const { session, socket } = setup('manage_service');
    const snapshot = readPolicy(session, socket);
    session.editRules(snapshot, change);
    const command = ruleCommand(socket);
    expect(command.command.type).toBe('edit');
    expect(command.command.expected_version).toEqual(snapshot.version);
    expect(decodeRuleDefinition(parseUnique(factsJson(definition('file', snapshot.document.groups[0]!.rules[0]!)))).rule.target)
      .toEqual(snapshot.document.groups[0]!.rules[0]!.target);
    expect(socket.sent.at(-1)).toBe(rulesCommand(command.id, { type: 'edit', expected_version: snapshot.version, change }));
    const sent = socket.sent.length;
    session.editRules(snapshot, change); expect(socket.sent).toHaveLength(sent);
    socket.receive(ruleReply(command.id, { status: 'completed', data: savedRule }));
    expect(session.getSnapshot().rules.receipt?.result).toEqual({ status: 'completed', data: savedRule });
    expect(adoptionConfirmed(savedRule)).toBe(false);
    expect(session.getSnapshot().rules).toMatchObject({ phase: 'loading', snapshot: null });
    expect(ruleCommand(socket).command.type).toBe('get');
    session.editRules(snapshot, change); expect(ruleCommand(socket).command.type).toBe('get');
    session.disconnect();
  });

  it('requires the exact rule ID reply on the same connection and does not consume overview IDs', () => {
    const { session, socket } = setup();
    session.refreshRules();
    const pending = session.getSnapshot();
    const id = ruleCommand(socket).id;
    expect(id).not.toEqual(queryId(socket));
    socket.receive(ruleReply(queryId(socket), { status: 'completed', data: { type: 'rules', snapshot: rulePolicy } }));
    socket.receive(ruleReply(Array(16).fill(99), { status: 'completed', data: { type: 'rules', snapshot: rulePolicy } }));
    expect(session.getSnapshot()).toBe(pending);
    socket.receive(ruleReply(id, { status: 'completed', data: { type: 'rules', snapshot: rulePolicy } }));
    const current = session.getSnapshot();
    socket.receive(ruleReply(id, { status: 'failed', code: 'failed', message: '', facts: null }));
    expect(session.getSnapshot()).toBe(current);
    socket.receive(businessWire(queryId(socket)));
    expect(session.getSnapshot().overview.phase).toBe('returned');
    expect(session.getSnapshot().rules).toBe(current.rules);
    session.disconnect();
  });

  it('rereads on Conflict, rejects the old draft even after reread, and never replays it', () => {
    const { session, socket } = setup();
    const original = readPolicy(session, socket);
    session.editRules(original, editCases[5]!);
    const writeId = ruleCommand(socket).id;
    socket.receive(ruleReply(writeId, { status: 'failed', code: 'conflict', message: 'source changed', facts: null }));
    expect(session.getSnapshot().rules).toMatchObject({ conflict: true, phase: 'loading', snapshot: null });
    const read = ruleCommand(socket);
    expect(read.command.type).toBe('get');
    socket.receive(ruleReply(read.id, { status: 'completed', data: { type: 'rules', snapshot: { ...rulePolicy, version: Array(32).fill(4) } } }));
    const sent = socket.sent.length;
    session.editRules(original, editCases[5]!);
    expect(socket.sent).toHaveLength(sent);
    const fresh = session.getSnapshot().rules.snapshot!;
    session.editRules(fresh, editCases[5]!);
    expect(ruleCommand(socket).command.expected_version).toEqual(fresh.version);
    session.disconnect();
  });

  it('preserves real failure and RulesSaved facts even if the subsequent read also fails', () => {
    const { session, socket } = setup();
    const original = readPolicy(session, socket);
    session.editRules(original, editCases[1]!);
    const failure = { status: 'failed', code: 'failed', message: 'directory sync failed', facts: savedRule } as const;
    socket.receive(ruleReply(ruleCommand(socket).id, failure));
    expect(session.getSnapshot().rules.receipt?.result).toEqual(failure);
    expect(savedFacts(session.getSnapshot().rules.receipt!.result)).toEqual(savedRule);
    expect(adoptionConfirmed(savedRule)).toBe(false);
    socket.receive(ruleReply(ruleCommand(socket).id, { status: 'failed', code: 'unavailable', message: '', facts: null }));
    expect(session.getSnapshot().rules.receipt?.result).toEqual(failure);
    expect(session.getSnapshot().rules.phase).toBe('failed');
    expect(adoptionConfirmed({ ...savedRule, applied: { ...driverStatus.snapshot, source_state: 'running',
      rule_version: savedRule.receipt.saved_version } } as RulesSaved)).toBe(true);
    expect(adoptionConfirmed({ ...savedRule, applied: driverStatus.snapshot } as RulesSaved)).toBe(false);
    session.disconnect();
  });

  it('keeps accepted writes pending and clears uncertain drafts after a send error', () => {
    const { session, socket } = setup();
    const original = readPolicy(session, socket);
    session.editRules(original, editCases[2]!);
    socket.receive(ruleReply(ruleCommand(socket).id, { status: 'accepted', data: savedRule }));
    const sent = socket.sent.length;
    session.refreshRules(); session.editRules(original, editCases[2]!);
    expect(socket.sent).toHaveLength(sent);
    socket.receive(ruleReply(ruleCommand(socket).id, { status: 'completed', data: savedRule }));
    socket.receive(ruleReply(ruleCommand(socket).id, { status: 'completed', data: { type: 'rules', snapshot: rulePolicy } }));
    socket.failSend = true;
    session.editRules(session.getSnapshot().rules.snapshot!, editCases[2]!);
    expect(session.getSnapshot().rules).toMatchObject({ phase: 'failed', snapshot: null, pending: null });
    session.disconnect();
  });

  it('discards rule reads and writable drafts across connections and ignores old callbacks and IDs', () => {
    const { session, socket, sockets } = setup();
    const original = readPolicy(session, socket);
    const oldId = ruleCommand(socket).id;
    const late = socket.onmessage!;
    session.disconnect();
    expect(session.getSnapshot().rules).toMatchObject({ phase: 'initial', snapshot: null, receipt: null });
    session.connect(testToken);
    const next = sockets[1]!; next.open();
    next.receive(JSON.stringify({ version: 2, authenticated: { access: 'manage_rules', chat_content: false } }));
    session.refreshRules();
    const current = session.getSnapshot();
    late({ data: ruleReply(oldId, { status: 'completed', data: { type: 'rules', snapshot: rulePolicy } }) });
    next.receive(ruleReply(oldId, { status: 'completed', data: { type: 'rules', snapshot: rulePolicy } }));
    session.editRules(original, editCases[0]!);
    expect(session.getSnapshot()).toBe(current);
    expect(ruleCommand(next).id).not.toEqual(oldId);
    session.disconnect();
  });

  it('shares the existing WS with overview and one-shot Ask without changing either result', () => {
    const { session, socket, sockets } = setup();
    session.setUiReady(true); socket.receive(wire(ask));
    const row = session.getSnapshot().reviews[0]!;
    const original = readPolicy(session, socket);
    socket.receive(businessWire(queryId(socket)));
    const overview = session.getSnapshot().overview;
    session.editRules(original, editCases[4]!);
    socket.receive(ruleReply(ruleCommand(socket).id, { status: 'failed', code: 'invalid', message: 'maintenance group', facts: null }));
    expect(session.getSnapshot().reviews[0]).toBe(row);
    expect(session.getSnapshot().overview).toBe(overview);
    session.submit(row, 1); socket.receive(wire(completed('denied')));
    expect(session.getSnapshot().reviews[0]?.completion).toEqual(completed('denied'));
    expect(sockets).toHaveLength(1);
    expect(session.getSnapshot().rules.receipt?.result).toMatchObject({ status: 'failed', code: 'invalid' });
    session.disconnect();
  });
});

// Frozen process-tree shapes, including the R61 fixed-field stamp map. Test supply only.
const processReply = (id: readonly number[], page = trackedPage()) => ruleReply(id, {
  status: 'completed', data: { type: 'processes', result: { type: 'page', data: page } },
});
function processCommand(socket: FakeSocket) {
  return JSON.parse(socket.sent.filter(text => JSON.parse(text).message?.type === 'processes').at(-1)!).message as {
    type: 'processes'; id: readonly number[]; command: { type: 'query'; query: ReturnType<typeof listQuery> };
  };
}

describe('typed processes on the original session', () => {
  it('uses the whole ordinary envelope, starts only on demand and preserves the real page shape', () => {
    const { session, socket, sockets } = setup('read_only', false);
    expect(socket.sent).toHaveLength(2);
    expect(session.getSnapshot().processes.phase).toBe('initial');
    session.refreshProcesses();
    const command = processCommand(socket);
    expect(JSON.parse(socket.sent.at(-1)!)).toEqual({ version: 2, message: {
      type: 'processes', id: command.id, command: { type: 'query', query: {
        type: 'list', filter: { name: null, executable: null, lifecycle: null, kind: null, workload: null },
        page: { limit: 8, cursor: null },
      } },
    } });
    expect(JSON.parse(processesCommand(command.id, listQuery()))).toEqual(JSON.parse(socket.sent.at(-1)!));
    socket.receive(processReply(command.id));
    const read = session.getSnapshot().processes;
    expect(read).toMatchObject({ phase: 'ready', pending: null, page: trackedPage() });
    expect(read.page?.records[0]?.latest.process.argv).toEqual([[], [255]]);
    expect(Object.keys(read.page!.records[0]!.latest.stamps)).toHaveLength(27);
    expect(read.page!.status.memory_used).toBe(18446744073709551615n);
    expect(observedValue(read.page!.records[0]!, 'workload', '0')).toEqual({ kind: 'container', instance: containerInstance });
    expect(session.getSnapshot().reviewDeclared).toBe(false);
    expect(sockets).toHaveLength(1);
    session.disconnect();
  });

  it('rejects malformed consumed identities, cursor, status, workload and observations without accepting old DTOs', () => {
    const page = trackedPage();
    const record = trackedRecord();
    for (const changed of [
      { ...record, key: { ...record.key, birth_boottime_ns: 1 } },
      { ...record, key: { ...record.key, boot_id: [1] } },
      { ...record, latest: { ...record.latest, process: { ...record.latest.process, name: 'proc' } } },
      { ...record, latest: { ...record.latest, process: { ...record.latest.process,
        workload: { kind: 'container', instance: { runtime: 'docker', id: 'short' } } } } },
      { ...record, latest: { ...record.latest, stamps: { name: { ...processStamp, state: 'live' } } } },
      { ...record, latest: { ...record.latest, stamps: { unknown_field: processStamp } } },
    ]) expect(() => parseFrame(processReply(id, { ...page, records: [changed] } as unknown as ProcessPage))).toThrow();
    expect(() => parseFrame(processReply(id, { ...page, status: { ...page.status, evidence_epoch: '00' } }))).toThrow();
    const cursor = { instance: page.status.instance, generation: '1', query: Array(32).fill(7), after: record.key, hit_after: null };
    expect(() => parseFrame(processReply(id, { ...page, next_cursor: { ...cursor, generation: 1 } } as unknown as ProcessPage))).toThrow();
    for (const result of [{ type: 'status', data: page.status }, { type: 'pid', data: record.key },
      { type: 'ancestors', data: { records: [record], incomplete: true } },
      { type: 'hits', data: { records: [], next_cursor: null, incomplete: true } },
      { type: 'details', data: { evidence_epoch: '0', record, initial_parent: { kind: 'root' },
        latest_parent: { kind: 'unresolved', observed_pid: null }, children: 0 } }]) {
      expect(parseFrame(ruleReply(id, { status: 'completed', data: { type: 'processes', result } })).kind).toBe('reply');
    }
  });

  it('returns the complete original cursor, replaces pages, resets filters/size, and makes expiration explicit', () => {
    const { session, socket } = setup('read_only');
    session.refreshProcesses();
    const page = trackedPage();
    const cursor: ProcessCursor = { instance: page.status.instance, generation: '1', query: Array(32).fill(7),
      after: page.records[0]!.key, hit_after: null };
    socket.receive(processReply(processCommand(socket).id, { ...page, next_cursor: cursor }));
    session.nextProcesses();
    const next = processCommand(socket);
    expect(next.command.query.page).toEqual({ limit: 8, cursor });
    expect(session.getSnapshot().processes.page).toBeNull();
    const sent = socket.sent.length;
    session.nextProcesses(); expect(socket.sent).toHaveLength(sent);
    const changedPage = { ...trackedPage([]), status: { ...page.status, evidence_epoch: '1', phase: 'uncertain' as const,
      historical_gap: true }, tree_revision: '9007199254740994' };
    socket.receive(processReply(next.id, changedPage));
    expect(session.getSnapshot().processes).toMatchObject({ phase: 'ready', pageNumber: 2, changed: true, page: changedPage });
    session.refreshProcesses(1, 'present', { kind: 'container', instance: containerInstance });
    expect(processCommand(socket).command.query).toEqual(listQuery(1, 'present', { kind: 'container', instance: containerInstance }));
    socket.receive(processReply(processCommand(socket).id, { ...page, next_cursor: cursor }));
    session.nextProcesses();
    socket.receive(ruleReply(processCommand(socket).id, { status: 'failed', code: 'cursor_expired', message: 'tree changed', facts: null }));
    expect(session.getSnapshot().processes).toMatchObject({ phase: 'failed', page: null, pending: null,
      receipt: { result: { code: 'cursor_expired' } } });
    const expired = socket.sent.length;
    session.nextProcesses(); expect(socket.sent).toHaveLength(expired);
    session.refreshProcesses();
    expect(processCommand(socket).command.query.page).toEqual({ limit: 1, cursor: null });
    session.disconnect();
  });

  it('isolates old IDs, other commands and wrong query results; accepted is not a completed page', () => {
    const { session, socket } = setup();
    session.refreshProcesses(); const old = processCommand(socket).id;
    session.refreshProcesses(4); const current = processCommand(socket).id;
    session.refreshRules();
    expect(current).not.toEqual(queryId(socket)); expect(current).not.toEqual(ruleCommand(socket).id);
    const pending = session.getSnapshot().processes;
    socket.receive(processReply(old)); socket.receive(processReply(Array(16).fill(99)));
    socket.receive(processReply(queryId(socket)));
    expect(session.getSnapshot().processes).toBe(pending);
    socket.receive(ruleReply(current, { status: 'accepted', data: { type: 'processes', result: { type: 'page', data: trackedPage() } } }));
    expect(session.getSnapshot().processes).toMatchObject({ phase: 'loading', page: null, pending: current });
    socket.receive(processReply(current)); const returned = session.getSnapshot().processes;
    socket.receive(ruleReply(current, { status: 'failed', code: 'unavailable', message: '', facts: null }));
    expect(session.getSnapshot().processes).toBe(returned);
    session.refreshProcesses();
    socket.receive(ruleReply(processCommand(socket).id, { status: 'completed', data: { type: 'processes',
      result: { type: 'status', data: trackedPage().status } } }));
    expect(session.getSnapshot().processes).toMatchObject({ phase: 'failed', pending: null, page: null,
      problem: { key: 'processes.wrongReply' } });
    session.disconnect();
  });

  it.each(['output_too_large', 'output_encoding_failed'])('ends loading only for the matching output diagnostic: %s', code => {
    const { session, socket } = setup(); session.setUiReady(true); socket.receive(wire(ask));
    const row = session.getSnapshot().reviews[0]!;
    session.refreshProcesses(); const command = processCommand(socket);
    const diagnostic = (type: string, commandId = command.id) => JSON.stringify({ version: 2, diagnostic: { type, id: commandId, code } });
    socket.receive(diagnostic('reply_output_unavailable', Array(16).fill(99)));
    socket.receive(diagnostic('request_rejected'));
    expect(session.getSnapshot().processes.phase).toBe('loading');
    const global = session.getSnapshot().diagnostic;
    socket.receive(diagnostic('reply_output_unavailable'));
    expect(session.getSnapshot().processes).toMatchObject({ phase: 'failed', pending: null, page: null,
      diagnostic: { type: 'reply_output_unavailable', id: command.id, code } });
    expect(session.getSnapshot().diagnostic).toBe(global);
    socket.receive(processReply(command.id)); expect(session.getSnapshot().processes.phase).toBe('failed');
    expect(session.getSnapshot().reviews[0]).toBe(row);
    session.submit(row, 1); socket.receive(wire(completed('denied')));
    expect(session.getSnapshot().reviews[0]?.completion).toEqual(completed('denied'));
    session.disconnect();
  });

  it('separates service/send failures from empty pages and never accepts failure facts as a successful list', () => {
    const { session, socket } = setup('read_only');
    session.refreshProcesses();
    socket.receive(ruleReply(processCommand(socket).id, { status: 'failed', code: 'invalid', message: 'page limit is 1',
      facts: { type: 'processes', result: { type: 'page', data: trackedPage() } } }));
    expect(session.getSnapshot().processes).toMatchObject({ phase: 'failed', page: null, receipt: { result: { message: 'page limit is 1' } } });
    session.refreshProcesses(1); socket.receive(processReply(processCommand(socket).id, trackedPage([])));
    expect(session.getSnapshot().processes).toMatchObject({ phase: 'ready', page: { records: [] } });
    socket.failSend = true; session.refreshProcesses();
    expect(session.getSnapshot().processes).toMatchObject({ phase: 'failed', page: null, pending: null,
      problem: { key: 'processes.sendFailed' } });
    expect(session.getSnapshot().phase).toBe('authenticated');
    session.disconnect();
  });

  it('releases page and cursor on leaving/lock and ignores old requests and socket callbacks', () => {
    const { session, socket, sockets } = setup('read_only'); session.refreshProcesses();
    const oldId = processCommand(socket).id; const late = socket.onmessage!;
    socket.receive(processReply(oldId)); session.clearProcesses();
    socket.receive(processReply(oldId)); expect(session.getSnapshot().processes.phase).toBe('initial');
    session.refreshProcesses(); session.disconnect();
    expect(session.getSnapshot().processes).toMatchObject({ phase: 'initial', page: null, pending: null });
    session.connect(testToken); const next = sockets[1]!; next.open();
    next.receive(JSON.stringify({ version: 2, authenticated: { access: 'read_only', chat_content: false } }));
    session.refreshProcesses(); const pending = session.getSnapshot();
    late({ data: processReply(oldId) }); next.receive(processReply(oldId));
    expect(session.getSnapshot()).toBe(pending);
    expect(processCommand(next).id).not.toEqual(oldId);
    next.receive(processReply(processCommand(next).id)); expect(session.getSnapshot().processes.phase).toBe('ready');
    session.disconnect();
  });
});

// Current tagged Group test supply. Process endpoints are not Agent identities.

describe('Group original calls', () => {
  it('strictly consumes mixed tagged evidence, original A2A gaps and an OpenClaw actor without inventing a child', () => {
    const a2a = delegationRow('10');
    a2a.delegation = { ...a2a.delegation, gap: 'child_model_exchange_unavailable', child_responses: [{ kind: 'protocol', value: {
      exchange: { flow_instance: Array(16).fill(9), sequence: '18446744073709551615' }, sequence: '1',
    } }] };
    const receipt: GroupDelegation = { id: '9', kind: 'openclaw_tool_receipt', actor: parentKey,
      receipt: { kind: 'protocol', value: { exchange: { flow_instance: Array(16).fill(9), sequence: '2' }, sequence: '0' } },
      call_index: 0, output_index: 4294967295, tool_call_id: '', child_run_id: 'run-original', child_session_key: 'session-original',
      gateway: { accepted_spawns: [], child_terminals: [], parent_publications: [] },
      gaps: ['gateway_receipt_unobserved', 'child_terminal_unobserved', 'parent_publication_unobserved'] };
    const page: GroupPage = { records: [a2a, receipt], next_before: null };
    expect(decodeGroupData({ type: 'group_delegations', ...page }).page.records).toEqual(page.records);
    for (const row of [withoutKey(a2a, 'kind'), { ...a2a, kind: 'legacy' }, { ...a2a, delegation: { ...a2a.delegation, gap: 'unknown' } },
      { ...receipt, child: childKey }, { ...receipt, gaps: ['unknown'] }, { ...receipt, output_index: -1 }, { ...receipt, receipt: { kind: 'protocol', value: {} } }])
      expect(() => decodeGroupData({ type: 'group_delegations', records: [row], next_before: null })).toThrow();
    const { session, socket } = setup('read_only', true);
    session.enterGroupDelegations(); socket.receive(businessWire(queryId(socket), groupResult(page)));
    const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(GroupDelegationsView, {
      read: session.getSnapshot().group, chatContent: true, session,
    }) }));
    expect(markup.match(/class="group-node stack-tight"/g)).toHaveLength(3);
    expect(markup).toContain('child_model_exchange_unavailable');
    expect(markup).toContain('child_terminal_unobserved'); expect(markup).toContain('parent_publication_unobserved');
    expect(markup).toContain(translate('ja-JP', 'group.childRun'));
    session.disconnect();
  });
  it('consumes typed public Gateway observations and preserves gaps and original scalar values', () => {
    const reference = { id: '1', capture: Array(32).fill(3), from_client: false, frames: [{ offset: '4294967297', length: '81' }] };
    const gateway = {
      accepted_spawns: [{ reference, fact: { parent_run_id: 'parent', parent_session_key: 'parent-session', parent_agent_id: 'main',
        tool_call_id: 'original|compound', child_run_id: 'child', child_session_key: 'child-session' } }],
      child_terminals: [{ reference: { ...reference, id: '2' }, fact: { source: 'lifecycle', run_id: 'child', session_key: 'child-session',
        phase: 'end', execution_settled: true, status: null, outcome: { empty: '', values: [0, false, []] }, sequence: 18446744073709551615n } }],
      parent_publications: [{ reference: { ...reference, id: '3' }, fact: { run_id: 'announce:v1:child-session:child', session_key: 'parent-session', yielded: false } }],
    };
    const row = { id: '9', kind: 'openclaw_tool_receipt', actor: parentKey,
      receipt: { kind: 'protocol', value: { exchange: historyExchange, sequence: '0' } },
      call_index: 0, output_index: 1, tool_call_id: 'original', child_run_id: 'child', child_session_key: 'child-session', gateway, gaps: [] };
    const decode = (value: unknown) => decodeGroupData({ type: 'group_delegations', records: [value], next_before: null });
    expect(decode(row).page.records[0]).toEqual(row);
    expect(() => decode({ ...row, gateway: { ...gateway, accepted_spawns: [{ ...gateway.accepted_spawns[0], reference: { ...reference, from_client: true } }] } })).toThrow();
    expect(() => decode({ ...row, gateway: { ...gateway, child_terminals: [{ reference, fact: { source: 'inferred', run_id: 'child', session_key: 'child-session' } }] } })).toThrow();
    expect(() => decode({ ...row, gaps: ['child_terminal_unobserved', 'child_terminal_unobserved'] })).toThrow();
  });
  it('uses one content-authorized connection, replaces two pages and never merges TGID reuse across birth or boot', () => {
    const { session, socket, sockets } = setup('read_only', true);
    expect(session.getSnapshot().group.active).toBe(false);
    expect(socket.sent).toHaveLength(2);
    session.enterGroupDelegations();
    const first = queryId(socket);
    expect(JSON.parse(socket.sent.at(-1)!)).toEqual({ version: 2, business: {
      id: first, query: { type: 'group_delegations', before: null, limit: 8 },
    } });
    expect(JSON.parse(groupDelegationsCommand(first, { before: null, limit: 8 }))).toEqual(JSON.parse(socket.sent.at(-1)!));
    const sent = socket.sent.length;
    session.enterGroupDelegations(); session.refreshGroupDelegations(); session.nextGroupDelegations();
    expect(socket.sent).toHaveLength(sent);
    socket.receive(businessWire(first, groupResult()));
    const page = groupPage(session.getSnapshot().group)!;
    expect(page).toEqual(delegationPage());
    expect(page.records[0]).toMatchObject({ parent: parentKey, child: childKey });
    expect(page.records[1]).toMatchObject({ parent: parentKey, child: otherBootKey });
    session.nextGroupDelegations();
    const second = queryId(socket);
    expect(second).not.toEqual(first);
    expect(JSON.parse(socket.sent.at(-1)!).business.query).toEqual({ type: 'group_delegations', before: '9007199254740993', limit: 8 });
    expect(groupPage(session.getSnapshot().group)).toBeNull();
    const nextPending = session.getSnapshot().group;
    socket.receive(businessWire(first, groupResult()));
    expect(session.getSnapshot().group).toBe(nextPending);
    socket.receive(businessWire(second, groupResult({ records: [delegationRow('9007199254740992', childKey, childKey)], next_before: null })));
    expect(session.getSnapshot().group.pageNumber).toBe(2);
    const secondPage = groupPage(session.getSnapshot().group)!;
    expect(secondPage.records).toHaveLength(1);
    expect(secondPage.records[0]).toMatchObject({ parent: childKey, child: childKey });
    const ended = socket.sent.length; session.nextGroupDelegations(); expect(socket.sent).toHaveLength(ended);
    session.refreshGroupDelegations();
    expect(JSON.parse(socket.sent.at(-1)!).business.query.before).toBeNull();
    expect(session.getSnapshot().group.pageNumber).toBe(1);
    expect(session.getSnapshot().reviewDeclared).toBe(false);
    expect(sockets).toHaveLength(1);
    session.disconnect();
  });

  it.each(['read_only', 'manage_rules', 'manage_service'])('sends no Group frames without content authorization: %s', access => {
    const { session, socket } = setup(access, false);
    const sent = socket.sent.length;
    session.enterGroupDelegations(); session.refreshGroupDelegations(); session.nextGroupDelegations();
    expect(socket.sent).toHaveLength(sent);
    expect(session.getSnapshot().group).toMatchObject({ active: true, phase: 'initial', pending: null, reply: null });
    session.refreshProcesses(); socket.receive(processReply(processCommand(socket).id));
    expect(session.getSnapshot().processes.phase).toBe('ready');
    session.leaveGroupDelegations();
    expect(session.getSnapshot().group.active).toBe(false);
    session.disconnect();
  });

  it('isolates overview, Processes and original Ask; rejects old IDs and stops consuming immediately on leaving the partition', () => {
    const { session, socket } = setup('manage_rules', true);
    const overview = queryId(socket);
    session.setUiReady(true); socket.receive(wire(ask));
    const review = session.getSnapshot().reviews[0]!;
    session.refreshProcesses(); const processes = processCommand(socket).id;
    session.enterGroupDelegations(); const old = queryId(socket);
    session.leaveGroupDelegations();
    socket.receive(businessWire(old, groupResult()));
    expect(session.getSnapshot().group).toMatchObject({ active: false, phase: 'initial', reply: null, pending: null });
    session.refreshGroupDelegations(); const sent = socket.sent.length;
    session.enterGroupDelegations(); const current = queryId(socket);
    expect(socket.sent).toHaveLength(sent + 1);
    expect(current).not.toEqual(old);
    const pending = session.getSnapshot().group;
    socket.receive(businessWire(old, groupResult()));
    socket.receive(businessWire(Array(16).fill(99), groupResult()));
    socket.receive(businessWire(overview)); socket.receive(processReply(processes));
    expect(session.getSnapshot().group).toBe(pending);
    expect(session.getSnapshot().processes.phase).toBe('ready');
    expect(session.getSnapshot().overview.phase).toBe('returned');
    socket.receive(businessWire(current, groupResult()));
    const returned = session.getSnapshot().group;
    socket.receive(businessWire(current, { status: 'failed', code: 'busy', message: 'late' }));
    expect(session.getSnapshot().group).toBe(returned);
    session.leaveGroupDelegations();
    expect(session.getSnapshot().processes.phase).toBe('ready');
    expect(session.getSnapshot().reviews[0]).toBe(review);
    expect(session.remaining(review)).toBe(5000);
    session.submit(review, 0); socket.receive(wire(completed()));
    expect(session.getSnapshot().reviews[0]?.completion).toEqual(completed());
    session.disconnect();
  });

  it('keeps busy, output, unauthorized and send failures distinct from an empty successful page without replay', () => {
    const { session, socket } = setup('read_only', true);
    session.enterGroupDelegations();
    for (const [code, originalMessage] of [['busy', 'query capacity'], ['unavailable', 'output_too_large']] as const) {
      socket.receive(businessWire(queryId(socket), { status: 'failed', code, message: originalMessage }));
      expect(session.getSnapshot().group).toMatchObject({ phase: 'failed', pending: null, reply: { result: { code, message: originalMessage } } });
      expect(groupPage(session.getSnapshot().group)).toBeNull();
      const sent = socket.sent.length; session.nextGroupDelegations(); expect(socket.sent).toHaveLength(sent);
      session.refreshGroupDelegations();
    }
    socket.receive(businessWire(queryId(socket), groupResult({ records: [], next_before: null })));
    expect(session.getSnapshot().group.phase).toBe('ready');
    expect(groupPage(session.getSnapshot().group)?.records).toEqual([]);
    socket.failSend = true; session.refreshGroupDelegations();
    expect(session.getSnapshot().group).toMatchObject({ phase: 'failed', pending: null, problem: { key: 'group.sendFailed' } });
    socket.failSend = false; session.refreshGroupDelegations();
    socket.receive(businessWire(queryId(socket), { status: 'failed', code: 'unauthorized', message: 'content grant withdrawn' }));
    const sent = socket.sent.length; session.refreshGroupDelegations(); session.nextGroupDelegations();
    expect(socket.sent).toHaveLength(sent);
    expect(session.getSnapshot().phase).toBe('authenticated');
    session.disconnect();
  });

  it('ends only the matching Group read for a wrong business type or exclusive pagination boundary', () => {
    const { session, socket } = setup('read_only', true);
    session.enterGroupDelegations();
    socket.receive(businessWire(queryId(socket)));
    expect(session.getSnapshot().group).toMatchObject({ phase: 'failed', pending: null, problem: { key: 'group.wrongReply' } });
    session.refreshGroupDelegations(); socket.receive(businessWire(queryId(socket), groupResult()));
    session.nextGroupDelegations();
    socket.receive(businessWire(queryId(socket), groupResult({ records: [delegationRow('9007199254740993')], next_before: null })));
    expect(session.getSnapshot().group.phase).toBe('failed');
    session.refreshGroupDelegations();
    socket.receive(businessWire(queryId(socket), groupResult({ records: [delegationRow('2'), delegationRow('3')], next_before: '3' })));
    expect(session.getSnapshot().group).toMatchObject({ phase: 'failed', pending: null });
    session.refreshGroupDelegations();
    socket.receive(businessWire(queryId(socket), groupResult({ records: [delegationRow('2')], next_before: '1' })));
    expect(groupPage(session.getSnapshot().group)).toBeNull();
    expect(session.getSnapshot().group.problem?.key).toBe('group.wrongReply');
    expect(session.getSnapshot().phase).toBe('authenticated');
    session.disconnect();
  });

  it('discards Group records on lock and rejects old-socket callbacks after reauthentication', () => {
    const { session, socket, sockets } = setup('read_only', true);
    session.enterGroupDelegations(); const old = queryId(socket); const late = socket.onmessage!;
    socket.receive(businessWire(old, groupResult())); session.disconnect();
    expect(groupPage(session.getSnapshot().group)).toBeNull();
    session.connect(testToken); const next = sockets[1]!; next.open();
    next.receive(JSON.stringify({ version: 2, authenticated: { access: 'read_only', chat_content: true } }));
    session.enterGroupDelegations(); const current = queryId(next); const pending = session.getSnapshot();
    late({ data: businessWire(old, groupResult()) }); next.receive(businessWire(old, groupResult()));
    expect(session.getSnapshot()).toBe(pending);
    expect(current).not.toEqual(old);
    next.receive(businessWire(current, groupResult()));
    expect(session.getSnapshot().group.phase).toBe('ready');
    session.disconnect();
  });

  it('accepts only the R7 page and exact original identity types while retaining the delegation object', () => {
    const page = delegationPage();
    const decoded = decodeGroupData({ type: 'group_delegations', ...page });
    expect(decoded.page.records[0]).toBe(page.records[0]);
    for (const changed of [
      { ...page.records[0], id: '0' }, { ...page.records[0], id: '01' }, { ...page.records[0], id: 1 },
      { ...page.records[0], capture_id: Array(31).fill(6) }, { ...page.records[0], parent: { ...parentKey, birth_boottime_ns: 1 } },
      { ...page.records[0], child: { ...childKey, boot_id: [1] } }, { ...page.records[0], delegation: null },
    ]) expect(() => parseFrame(businessWire(id, groupResult({ records: [changed], next_before: null } as unknown as GroupPage)))).toThrow();
    expect(() => parseFrame(businessWire(id, { status: 'completed', data: { type: 'group_delegations', page } }))).toThrow();
  });

  it.each(['ja-JP', 'zh-CN', 'en-US'] as const)('renders real workspace labels, full keys, limits and no-grant states: %s', locale => {
    vi.stubGlobal('localStorage', { getItem: () => locale });
    const { session, socket } = setup('read_only', true);
    try {
      const entry = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(ProcessesView, {
        read: session.getSnapshot().processes, session,
      }) }));
      expect(entry).not.toContain('Group');
      expect(socket.sent).toHaveLength(2);
      session.enterGroupDelegations(); socket.receive(businessWire(queryId(socket), groupResult()));
      const render = (chatContent: boolean) => renderToStaticMarkup(createElement(I18nProvider, {
        children: createElement(GroupDelegationsView, { read: session.getSnapshot().group, chatContent, session }),
      }));
      const markup = render(true);
      expect(markup).toContain(translate(locale, 'group.observedCall'));
      expect(markup).toContain(translate(locale, 'group.observedProcess'));
      expect(markup).toContain(translate(locale, 'group.scope'));
      expect(markup).toContain(parentKey.birth_boottime_ns);
      expect(markup.match(/class="group-node stack-tight"/g)).toHaveLength(4);
      expect(markup).toContain(childKey.birth_boottime_ns);
      const denied = render(false);
      expect(denied).toContain(translate(locale, 'group.noGrant'));
      expect(denied).toContain('disabled=""');
      expect(denied).not.toContain('class="group-node');
    } finally { session.disconnect(); vi.unstubAllGlobals(); }
  });
});

// Simulated activity supply, using the same original event/receipt fixtures above.
const logReply = (commandId: readonly number[], page = logPage()) => wire({ type: 'reply', id: commandId,
  result: { status: 'completed', data: { type: 'security_log', page } } });
function logCommand(socket: FakeSocket): { id: number[]; command: { before: string | null; limit: number } } {
  return JSON.parse(socket.sent.filter(text => JSON.parse(text).message?.type === 'security_log').at(-1)!).message;
}

describe('selected-event evidence download boundary', () => {
  function loadedLog() {
    const supply = setup('manage_service', true);
    const { session, socket } = supply;
    session.enterActivity(); socket.receive(logReply(logCommand(socket).id));
    session.selectActivity(session.getSnapshot().activity.page!.records[0]!);
    session.readActivityEvidence(); socket.receive(businessWire(queryId(socket), contextResult()));
    return supply;
  }
  function snapshot(session: ReviewSession) {
    const { selection, evidence } = session.getSnapshot().activity;
    return selection ? session.getActivityEvidenceSnapshot(selection, evidence) : null;
  }

  it('cancels only the current evidence wait, ignores old IDs, and reads again only on an explicit action', () => {
    const { session, socket } = loadedLog();
    const loaded = session.getSnapshot().activity;
    session.readActivityEvidence();
    const pending = session.getSnapshot().activity, oldId = queryId(socket), before = socket.sent.length;
    session.cancelActivityEvidence(pending.selection!, pending.evidence);
    const cancelled = session.getSnapshot();
    expect(cancelled.activity.evidence).toEqual({ phase: 'cancelled', pending: null, attributionRequest: null, result: null, problem: null });
    expect(cancelled.activity.selection).toBe(pending.selection);
    expect(cancelled.access).toBe('manage_service'); expect(cancelled.chatContent).toBe(true);
    expect(session.getActivityEvidenceSnapshot(loaded.selection!, loaded.evidence)).toBeNull();
    expect(snapshot(session)).toBeNull();
    socket.receive(businessWire(oldId, contextResult()));
    expect(session.getSnapshot()).toBe(cancelled);
    expect(socket.sent).toHaveLength(before);
    session.readActivityEvidence();
    const current = session.getSnapshot(), newId = queryId(socket);
    expect(newId).not.toEqual(oldId); expect(socket.sent).toHaveLength(before + 1);
    session.cancelActivityEvidence(pending.selection!, pending.evidence); // A stale button cannot cancel the new read.
    socket.receive(businessWire(oldId, { status: 'failed', code: 'unavailable', message: 'late' }));
    expect(session.getSnapshot()).toBe(current);
    socket.receive(businessWire(newId, contextResult()));
    expect(snapshot(session)).not.toBeNull();
    session.disconnect();
  });

  it('round-trips raw numeric wire tokens, all original view order and only the selected source identity', async () => {
    const { session, socket } = loadedLog();
    session.readActivityEvidence();
    // Literal JSON numbers, never constructed as already-rounded JS numbers.
    // Simulated original ChatMessage/ToolCall JSON content, not a real captured message.
    const exchange = { flow_instance: Array(16).fill(7), sequence: '18446744073709551615' };
    const argumentsWire = '{"u64":18446744073709551615,"i64":-9223372036854775808,"above_safe":9007199254740993,"small":0.10000000000000001,"large":9007199254740993.1,"exponent":9.007199254740993e15,"overflow":1e400,"underflow":1e-400,"negative_zero":-0,"safe":9007199254740991,"empty":"","items":["",[],null,0,false],"bytes":[0,255],"decimal":"18446744073709551615"}';
    const messageWire = '{"id":' + JSON.stringify({ exchange, sequence: '9007199254740993' })
      + ',"source":{"origin":"https://provider.example","observed_at_utc_ms":0},"origin":"response_output","state":"complete","record_refs":[],"observed_parts":[{"type":"tool_call","id":0,"native_id":null,"name":"","arguments":{"kind":"json","value":'
      + argumentsWire + '}}]}';
    // Deliberately non-projection key order, including kind/value and gap/reference order.
    const rawView = '{"gaps":' + JSON.stringify(contextView().gaps)
      + ',"relations":[],"source_records":[{"value":' + messageWire + ',"kind":"message"},'
      + JSON.stringify(contextView().source_records[0]) + '],"anchor":{"value":' + JSON.stringify(eventAnchor) + ',"kind":"event"}}';
    socket.receive('{"version":2,"business_reply":{"id":' + JSON.stringify(queryId(socket))
      + ',"result":{"status":"completed","data":{"type":"activity_context","attribution":null,"view":' + rawView + '}}}}');
    const activity = session.getSnapshot().activity;
    const result = snapshot(session)!;
    expect(result.view).toBe(activity.evidence.result?.status === 'completed' ? activity.evidence.result.data.view : null);
    expect(result.selection).toEqual({ kind: 'log', id: '9223372036854775807', decision_id: null });
    expect(result.event_key).toEqual(eventAnchor);
    expect(Object.keys(result)).toEqual(['schema', 'version', 'event_key', 'selection', 'view']);
    const before = socket.sent.length;
    const blob = evidenceBlob(result), text = await blob.text();
    expect(blob.type).toBe('application/json');
    expect(text.endsWith('"view":' + rawView + '}')).toBe(true);
    expect(text).toContain(argumentsWire);
    expect(wireJson(parseUnique(factsJson(result)))).toBe(text);
    expect(sameJsonValue(result, parseUnique(text))).toBe(true);
    expect(parseUnique(text)).toEqual(result);
    expect(wireJson(decodeActivityContext({ type: 'activity_context', attribution: null, view: parseUnique(rawView) }).view)).toBe(rawView);
    expect(text).not.toContain(testToken);
    for (const field of ['business_reply', 'candidate_windows', 'pending', 'chatContent', 'next_before', 'socket', 'query'])
      expect(Object.hasOwn(parseUnique(text) as object, field)).toBe(false);
    expect(socket.sent).toHaveLength(before); // Export does not query or transmit.
    expect(evidenceFilename(result.event_key)).toBe('agentshield-event-' + '03'.repeat(16) + '-18446744073709551615-9007199254740993.json');
    expect(() => evidenceFilename({ ...eventAnchor, sequence: '../capture' })).toThrow();
    session.disconnect();
  });

  it('allows ManageRules with content permission through the actual matching completion, without log access', () => {
    const { session, socket } = setup('manage_rules', true);
    session.setUiReady(true); socket.receive(wire(ask)); socket.receive(wire(completed()));
    session.openCompletionEvidence(session.getSnapshot().reviews[0]!.completion!);
    expect(snapshot(session)).toBeNull();
    session.readActivityEvidence(); socket.receive(businessWire(queryId(socket), contextResult()));
    expect(snapshot(session)?.selection).toEqual({ kind: 'completion', decision_id: id });
    expect(socket.sent.some(text => JSON.parse(text).message?.type === 'security_log')).toBe(false);
    session.disconnect();
  });

  it('rejects retained completed data during loading, failure, pending, missing grants and every mismatched key component', () => {
    const { session, socket } = loadedLog(), a = session.getSnapshot().activity;
    const read = a.evidence;
    const forbidden: EvidenceRead[] = [
      { ...read, phase: 'loading' }, { ...read, phase: 'cancelled' }, { ...read, phase: 'failed' }, { ...read, pending: id },
      { ...read, problem: { key: 'evidence.sendFailed' } },
    ];
    for (const evidence of forbidden) expect(selectedEvidenceSnapshot({ ...a, evidence }, true)).toBeNull();
    expect(selectedEvidenceSnapshot(a, false)).toBeNull();
    expect(selectedEvidenceSnapshot({ ...a, active: false }, true)).toBeNull();
    for (const changed of [{ source_instance: Array(16).fill(9) }, { sequence_domain: '1' }, { sequence: '1' }]) {
      const view: ContextView = { ...contextView(), anchor: { kind: 'event', value: { ...eventAnchor, ...changed } } };
      expect(selectedEvidenceSnapshot({ ...a, evidence: { ...read,
        result: { status: 'completed', data: { type: 'activity_context', attribution: null, view } } } }, true)).toBeNull();
    }
    socket.readyState = 3; expect(snapshot(session)).toBeNull();
    session.disconnect();
  });

  it('clears a successful result synchronously on explicit reread and never revives it on old, wrong-lane or failed replies', () => {
    const { session, socket } = loadedLog();
    const old = session.getSnapshot().activity;
    session.readActivityEvidence(); const currentId = queryId(socket);
    expect(session.getSnapshot().activity.evidence).toMatchObject({ phase: 'loading', result: null });
    expect(session.getActivityEvidenceSnapshot(old.selection!, old.evidence)).toBeNull();
    expect(snapshot(session)).toBeNull();
    socket.receive(logReply(currentId)); expect(snapshot(session)).toBeNull();
    socket.receive(businessWire(currentId, { status: 'failed', code: 'unavailable', message: 'simulated' }));
    expect(snapshot(session)).toBeNull();
    socket.receive(businessWire(currentId, contextResult())); expect(snapshot(session)).toBeNull();
    session.readActivityEvidence(); socket.receive(businessWire(queryId(socket), overviewResult()));
    expect(snapshot(session)).toBeNull();
    session.readActivityEvidence(); socket.receive(businessWire(queryId(socket), contextResult()));
    expect(snapshot(session)).not.toBeNull();
    socket.failSend = true; session.readActivityEvidence(); expect(snapshot(session)).toBeNull();
    session.disconnect();
  });

  it.each(['selection', 'close', 'page', 'leave', 'lock', 'disconnect'] as const)('invalidates successful data on %s, including a same-key row', action => {
    const { session, socket, sockets } = loadedLog();
    const old = session.getSnapshot().activity, late = socket.onmessage!;
    if (action === 'selection') session.selectActivity(old.page!.records[1]!); // Same EventKey, different original row.
    else if (action === 'close') session.closeActivitySelection();
    else if (action === 'page') session.nextActivity();
    else if (action === 'leave') session.clearActivity();
    else if (action === 'lock') session.disconnect();
    else socket.onclose!();
    expect(snapshot(session)).toBeNull();
    expect(session.getActivityEvidenceSnapshot(old.selection!, old.evidence)).toBeNull();
    session.disconnect(); session.connect(testToken); const next = sockets[1]!; next.open();
    next.receive(JSON.stringify({ version: 2, authenticated: { access: 'manage_service', chat_content: false } }));
    const current = session.getSnapshot(); late({ data: businessWire(id, contextResult()) });
    expect(session.getSnapshot()).toBe(current); expect(snapshot(session)).toBeNull();
    session.disconnect();
  });

  it.each(['ja-JP', 'zh-CN', 'en-US'] as const)('renders the translated action and plain description only for current successful evidence: %s', locale => {
    vi.stubGlobal('localStorage', { getItem: () => locale });
    const { session } = loadedLog();
    const render = () => renderToStaticMarkup(createElement(I18nProvider, { children: createElement(ActivityView, {
      activity: session.getSnapshot().activity, access: 'manage_service', chatContent: true,
      session, askVisible: false,
    }) }));
    try {
      expect(render()).toContain('id="activity-evidence-download"');
      expect(render()).toContain('aria-describedby="activity-evidence-download-note"');
      expect(render()).toContain(translate(locale, 'evidence.download'));
      expect(render()).toContain(translate(locale, 'evidence.downloadNote'));
      expect(render().split(translate(locale, 'evidence.reread'))).toHaveLength(2);
      session.readActivityEvidence(); expect(render()).not.toContain('id="activity-evidence-download"');
    } finally { session.disconnect(); vi.unstubAllGlobals(); }
  });

  it.each(['ja-JP', 'zh-CN', 'en-US'] as const)('keeps opposite actual results for one path in separate rows, with one initial authorized read: %s', locale => {
    vi.stubGlobal('localStorage', { getItem: () => locale });
    const { session, socket } = setup('manage_service', true);
    const allowed: RuleLog = { ...ruleLog,
      event: { ...ruleLog.event, header: { ...ruleLog.event.header, sequence: '9007199254740994' } },
      result: { Ok: { driver: { outcome: 'allowed', termination: {
        requested: 'none', actor: { status: 'not_requested' }, parent: { status: 'not_requested' },
      } } } },
    };
    session.enterActivity();
    socket.receive(logReply(logCommand(socket).id, { records: [
      { id: '9223372036854775807', record: allowed }, { id: '9007199254740993', record: ruleLog },
    ], next_before: null }));
    const render = () => renderToStaticMarkup(createElement(I18nProvider, { children: createElement(ActivityView, {
      activity: session.getSnapshot().activity, access: 'manage_service', chatContent: true,
      session, askVisible: false,
    }) }));
    try {
      const rows = render().match(/<button\b[^>]*class="process-row"[^>]*>[\s\S]*?<\/button>/g) ?? [];
      expect(rows).toHaveLength(2);
      expect(rows[0]).toContain(translate(locale, 'checkpoint.allowed'));
      expect(rows[1]).toContain(translate(locale, 'checkpoint.denied'));
      for (const row of rows) {
        expect(row).toContain('/tmp/a');
        expect(row).not.toContain(translate(locale, 'activity.transferred'));
      }
      const sent = socket.sent.length;
      session.selectActivity(session.getSnapshot().activity.page!.records[0]!);
      const detail = render().split('id="activity-detail"')[1]!;
      expect(detail.split(translate(locale, 'evidence.read'))).toHaveLength(2);
      expect(detail.indexOf('activity-detail-target')).toBeLessThan(detail.indexOf('activity-checkpoint'));
      expect(detail.indexOf('activity-checkpoint')).toBeLessThan(detail.indexOf('activity-evidence stack-tight'));
      expect(detail.indexOf('activity-evidence stack-tight')).toBeLessThan(detail.indexOf('activity-originals'));
      expect(detail).not.toContain('id="activity-evidence-download"');
      expect(socket.sent).toHaveLength(sent + 2); // Initial SQL read and one shared stream.
      expect(detail).toContain('class="activity-originals" open=""');
    } finally { session.disconnect(); vi.unstubAllGlobals(); }
  });
});


describe('current-context stream invalidation', () => {
  const commands = (socket: FakeSocket) => socket.sent.map(text => JSON.parse(text)).flatMap(frame => frame.stream ? [frame.stream] : []);
  const ack = (commandId: readonly number[], status = 'subscribed', subscriptionId = commandId) => factsJson({ version: 2,
    stream_reply: { id: commandId, result: { status, subscription_id: subscriptionId } } });
  const update = (subscriptionId: readonly number[], sequence: string, serviceInstance = id) => factsJson({ version: 2, message: { type: 'chat',
    subscription_id: subscriptionId, envelope: { cursor: { service_instance: serviceInstance, sequence },
      event: { type: 'finished', data: historyMessage() } } } });
  const reads = (socket: FakeSocket) => socket.sent.map(text => JSON.parse(text)).filter(frame =>
    frame.business?.query.type === 'ask_context' || frame.business?.query.type === 'activity_context');

  it('strictly decodes the original stream lane, cursor, message and revision delta without replacing source values', () => {
    const result = parseFrame(update(id, '18446744073709551615'));
    expect(result).toMatchObject({ kind: 'chat', subscriptionId: id, cursor: { service_instance: id, sequence: '18446744073709551615' } });
    expect(result.kind === 'chat' && result.event.data).toEqual(historyMessage());
    expect(parseFrame(ack(id))).toMatchObject({ kind: 'stream_reply', reply: { id, result: { status: 'subscribed', subscription_id: id } } });
    const delta = { type: 'chat', subscription_id: id, envelope: { cursor: { service_instance: id, sequence: '0' },
      event: { type: 'delta', data: { id: historyMessage().id, base_revision: '0', revision: '1', changes: [
        { kind: 'put_part', part: { type: 'tool_result', id: 0, native_id: null, content: { kind: 'json', value: ['', 0, false, null, []] } } },
      ] } } } };
    expect(parseFrame(wire(delta))).toMatchObject({ kind: 'chat', event: delta.envelope.event });
    for (const raw of [update(id, '1').replace('"version": 2', '"version": 1'), update(id, '01'),
      update(id.slice(1), '1'), ack(id).replace('subscribed', 'invented'),
      wire({ ...delta, envelope: { ...delta.envelope, event: { type: 'delta', data: { ...delta.envelope.event.data, revision: 1 } } } })])
      expect(() => parseFrame(raw)).toThrow();
  });

  it('shares one tail-only subscription, coalesces pending invalidations, rejects stale identities and keeps answers independent', () => {
    vi.useFakeTimers();
    const { session, socket } = setup('manage_service', true);
    try {
      session.setUiReady(true); socket.receive(wire(ask));
      const row = session.getSnapshot().reviews[0]!; session.selectAskContext(row);
      const askId = queryId(socket), subscription = commands(socket)[0]!;
      expect(subscription.command).toEqual({ type: 'subscribe', filter: {}, after: null });
      session.enterActivity(); socket.receive(logReply(logCommand(socket).id));
      session.selectActivity(session.getSnapshot().activity.page!.records[0]!);
      const activityId = queryId(socket);
      expect(commands(socket)).toHaveLength(1);
      socket.receive(ack(Array(16).fill(99))); socket.receive(ack(subscription.id));
      socket.receive(update(subscription.id, '1')); socket.receive(update(subscription.id, '2'));
      expect(reads(socket)).toHaveLength(2);
      session.submit(row, 0); session.submit(row, 1);
      expect(socket.sent.map(text => JSON.parse(text)).filter(frame => frame.message?.type === 'answer')).toHaveLength(1);
      expect(session.remaining(session.getSnapshot().reviews[0]!)).toBe(5000);
      socket.receive(businessWire(askId, askContextResult())); const nextAsk = queryId(socket);
      socket.receive(businessWire(activityId, contextResult())); const nextActivity = queryId(socket);
      expect(reads(socket)).toHaveLength(4);
      socket.receive(update(subscription.id, '2')); socket.receive(update(subscription.id, '3', Array(16).fill(99)));
      socket.receive(update(Array(16).fill(98), '100'));
      socket.receive(businessWire(nextAsk, askContextResult())); socket.receive(businessWire(nextActivity, contextResult()));
      expect(reads(socket)).toHaveLength(4);
      vi.advanceTimersByTime(20000); expect(vi.getTimerCount()).toBe(0); expect(reads(socket)).toHaveLength(4);
      const confirmedAskView = session.getSnapshot().askEvidence.result;
      const confirmedActivityView = session.getSnapshot().activity.evidence.result;
      socket.receive(update(subscription.id, '3'));
      expect(session.getSnapshot().askEvidence.result).toBe(confirmedAskView);
      expect(session.getSnapshot().activity.evidence.result).toBe(confirmedActivityView);
      const oldAsk = session.getSnapshot().askEvidence.pending!, oldActivity = session.getSnapshot().activity.evidence.pending!;
      session.cancelAskContext(session.getSnapshot().reviews[0]!, session.getSnapshot().askEvidence);
      socket.receive(update(subscription.id, '4'));
      expect(session.getSnapshot().askEvidence.phase).toBe('cancelled');
      session.selectAskContext(undefined); session.closeActivitySelection();
      const unsubscribe = commands(socket).at(-1)!;
      expect(unsubscribe.command).toEqual({ type: 'unsubscribe', subscription_id: subscription.id });
      const count = reads(socket).length;
      socket.receive(update(subscription.id, '4'));
      socket.receive(businessWire(oldAsk, askContextResult())); socket.receive(businessWire(oldActivity, contextResult()));
      socket.receive(ack(unsubscribe.id, 'unsubscribed', subscription.id));
      expect(reads(socket)).toHaveLength(count);
      expect(session.getSnapshot().askEvidence.ask).toBeNull(); expect(session.getSnapshot().activity.selection).toBeNull();
    } finally { session.disconnect(); vi.useRealTimers(); }
  });

  it('waits for a late subscribe acknowledgement before retiring and ignores old sockets after lock', () => {
    const denied = setup('manage_rules', false);
    denied.session.setUiReady(true); denied.socket.receive(wire(ask)); denied.session.selectAskContext(denied.session.getSnapshot().reviews[0]!);
    expect(commands(denied.socket)).toEqual([]); denied.session.disconnect();
    const { session, socket, sockets } = setup('manage_rules', true);
    session.setUiReady(true); socket.receive(wire(ask)); session.selectAskContext(session.getSnapshot().reviews[0]!);
    const subscription = commands(socket)[0]!, late = socket.onmessage!;
    session.selectAskContext(undefined); expect(commands(socket)).toHaveLength(1);
    socket.receive(ack(subscription.id)); expect(commands(socket)).toHaveLength(2);
    expect(reads(socket)).toHaveLength(1);
    session.disconnect(); session.connect(testToken); const next = sockets[1]!;
    next.open(); next.receive(JSON.stringify({ version: 2, authenticated: { access: 'manage_rules', chat_content: true } }));
    const state = session.getSnapshot(); late({ data: update(subscription.id, '1') });
    expect(session.getSnapshot()).toBe(state); expect(commands(next)).toEqual([]);
    session.disconnect();
  });

  it('drops dirty reads on content revocation while a cancelled subscription failure cannot revoke the connection', () => {
    for (const cancelled of [false, true]) {
      const { session, socket } = setup('manage_rules', true);
      session.setUiReady(true); socket.receive(wire(ask));
      const row = session.getSnapshot().reviews[0]!; session.selectAskContext(row);
      const subscription = commands(socket)[0]!, pending = queryId(socket);
      if (cancelled) {
        session.selectAskContext(undefined);
        socket.receive(factsJson({ version: 2, stream_reply: { id: subscription.id, result: { status: 'failed', code: 'forbidden' } } }));
        expect(session.getSnapshot().chatContent).toBe(true);
      } else {
        socket.receive(ack(subscription.id)); socket.receive(update(subscription.id, '1'));
        socket.receive(businessWire(pending, { status: 'failed', code: 'unauthorized', message: '' }));
        expect(session.getSnapshot().chatContent).toBe(false);
        expect(commands(socket).at(-1)!.command.type).toBe('unsubscribe');
      }
      const count = reads(socket).length;
      socket.receive(update(subscription.id, '2')); socket.receive(businessWire(pending, askContextResult()));
      expect(reads(socket)).toHaveLength(count); expect(session.getSnapshot().askEvidence.result?.status).not.toBe('completed');
      session.submit(row, 0); expect(session.getSnapshot().reviews[0]?.phase).toBe('sent');
      session.disconnect();
    }
  });
});

describe('same-connection on-demand Ask context', () => {
  it('does not poll, restart cancelled initial reads, restore deselected replies or read without a mounted Ask', () => {
    vi.useFakeTimers();
    const { session, socket } = setup('manage_rules', true);
    try {
      session.setUiReady(true); socket.receive(wire(ask));
      const row = session.getSnapshot().reviews[0]!;
      session.selectAskContext(row);
      const read = session.getSnapshot().askEvidence, oldId = queryId(socket), count = socket.sent.length;
      session.selectAskContext(row); vi.advanceTimersByTime(20000);
      expect(socket.sent).toHaveLength(count); expect(vi.getTimerCount()).toBe(0);
      session.cancelAskContext(row, read); session.selectAskContext(row);
      socket.receive(businessWire(oldId, askContextResult()));
      expect(session.getSnapshot().askEvidence.phase).toBe('cancelled');
      expect(socket.sent).toHaveLength(count);
      session.selectAskContext(undefined);
      socket.receive(businessWire(oldId, askContextResult()));
      expect(session.getSnapshot().askEvidence.ask).toBeNull();
      session.setUiReady(false); session.selectAskContext(row);
      expect(socket.sent.filter(text => JSON.parse(text).business?.query.type === 'ask_context')).toHaveLength(1);
    } finally { session.disconnect(); vi.useRealTimers(); }
  });

  it('keeps the original Ask deadline, evidence waiter and availability across workspace entry/leave', () => {
    const { session, socket, sockets, advance } = setup('manage_service', true);
    session.setUiReady(true); socket.receive(wire(ask));
    const row = session.getSnapshot().reviews[0]!;
    session.selectAskContext(row); session.readAskContext(row);
    const evidence = session.getSnapshot().askEvidence, evidenceId = queryId(socket);
    session.enterRules(); session.leaveRules();
    session.refreshProcesses(); session.clearProcesses();
    session.enterActivity(); session.clearActivity();
    session.enterSettings(); session.enterEgress(); session.leaveEgress(); session.leaveSettings();
    advance(1000);
    expect(session.getSnapshot().reviews[0]).toBe(row);
    expect(session.remaining(row)).toBe(4000);
    expect(session.getSnapshot().askEvidence).toBe(evidence);
    expect(session.getSnapshot().reviewDeclared).toBe(true);
    expect(sockets).toHaveLength(1);
    expect(socket.sent.filter(text => JSON.parse(text).message?.type === 'review_availability')).toHaveLength(1);
    socket.receive(businessWire(evidenceId, askContextResult()));
    expect(session.getSnapshot().askEvidence.result).toEqual(askContextResult());
    session.submit(row, 0); session.submit(row, 1);
    expect(socket.sent.filter(text => JSON.parse(text).message?.type === 'answer')).toHaveLength(1);
    socket.receive(wire(completed('denied')));
    expect(session.getSnapshot().reviews[0]?.completion).toEqual(completed('denied'));
    session.disconnect();
  });

  it('decodes only the frozen AskContext shape and uses the shared original Ask and ContextView', () => {
    const data = askContextResult().data;
    expect(decodeAskContext(data)).toEqual(data);
    expect(parseFrame(businessWire(id, askContextResult()))).toEqual({ kind: 'business_reply', reply: { id, result: askContextResult() } });
    for (const key of ['event', 'agent', 'trigger_call', 'window', 'view', 'command_checks', 'attribution', 'candidate_windows']) {
      expect(() => decodeAskContext({ ...data, context: withoutKey(data.context, key) })).toThrow();
    }
    expect(() => decodeAskContext({ ...data, context: { ...data.context, decision_id: id.slice(1) } })).toThrow();
    expect(() => decodeAskContext({ ...data, context: { ...data.context, event: { ...eventAnchor, sequence: 1 } } })).toThrow();
    expect(() => decodeAskContext({ ...data, context: { ...data.context, view: {
      ...data.context.view, source_records: [{ kind: 'question', value: completed() }],
    } } })).toThrow();
    expect(() => askContextCommand(id, id.slice(1))).toThrow();
    expect(() => parseFrame(JSON.stringify({ version: 1, business_reply: { id, result: askContextResult() } }))).toThrow();
  });

  it('reads once on authorized selection and shares the global command allocator', () => {
    for (const content of [false, true]) {
      const { session, socket } = setup('manage_rules', content);
      const overviewId = queryId(socket);
      session.setUiReady(true); socket.receive(wire(ask));
      const row = session.getSnapshot().reviews[0]!;
      const count = socket.sent.length;
      session.selectAskContext(row);
      expect(socket.sent).toHaveLength(count + 2 * Number(content));
      session.readAskContext({ ...row }); // A caller-made row is not a received Ask.
      expect(socket.sent).toHaveLength(count + 2 * Number(content));
      session.readAskContext(row); session.readAskContext(row);
      expect(socket.sent).toHaveLength(count + 2 * Number(content));
      if (content) {
        const commandId = queryId(socket);
        expect(commandId).not.toEqual(overviewId);
        expect(JSON.parse(socket.sent.at(-1)!)).toEqual({ version: 2, business: {
          id: commandId, query: { type: 'ask_context', decision_id: id, attribution: null },
        } });
        expect(JSON.parse(askContextCommand(commandId, id))).toEqual(JSON.parse(socket.sent.at(-1)!));
        socket.receive(businessWire(overviewId));
        expect(session.getSnapshot().askEvidence.phase).toBe('loading');
        socket.receive(businessWire(commandId, askContextResult()));
        expect(session.getSnapshot().askEvidence.result).toEqual(askContextResult());
        expect(session.getSnapshot().activity.selection).toBeNull();
      }
      expect(session.getSnapshot().reviews[0]).toBe(row);
      session.disconnect();
    }
  });
});

describe('Ask context identity and deadline isolation', () => {
  it('cancels the Ask read wait without answering, renewing its sample or changing selection, and permits explicit rereads', () => {
    const { session, socket, advance } = setup('manage_rules', true);
    session.setUiReady(true); socket.receive(wire(ask));
    const row = session.getSnapshot().reviews[0]!;
    session.readAskContext(row);
    const read = session.getSnapshot().askEvidence, oldId = queryId(socket), count = socket.sent.length;
    advance(1500); session.cancelAskContext(row, read);
    const cancelled = session.getSnapshot();
    expect(cancelled.askEvidence).toMatchObject({ ask: row.ask, phase: 'cancelled', pending: null, result: null });
    expect(cancelled.reviews[0]).toBe(row); expect(session.remaining(row)).toBe(3500);
    expect(cancelled.access).toBe('manage_rules'); expect(cancelled.chatContent).toBe(true);
    socket.receive(businessWire(oldId, askContextResult()));
    expect(session.getSnapshot()).toBe(cancelled); expect(socket.sent).toHaveLength(count);
    session.readAskContext(row);
    const pending = session.getSnapshot(), newId = queryId(socket);
    expect(newId).not.toEqual(oldId); expect(socket.sent).toHaveLength(count + 1);
    session.cancelAskContext(row, read);
    socket.receive(businessWire(oldId, askContextResult()));
    expect(session.getSnapshot()).toBe(pending);
    socket.receive(businessWire(newId, askContextResult()));
    expect(session.getSnapshot().askEvidence.result).toEqual(askContextResult());
    session.readAskContext(row); // Successful reads can also be explicitly refreshed.
    expect(session.getSnapshot().askEvidence).toMatchObject({ phase: 'loading', result: null });
    expect(queryId(socket)).not.toEqual(newId);
    session.submit(row, 0); session.submit(row, 0);
    expect(socket.sent.filter(text => JSON.parse(text).message?.type === 'answer')).toHaveLength(1);
    expect(session.getSnapshot().reviews[0]?.observedAt).toBe(row.observedAt);
    session.disconnect();
  });

  it('allows an explicit retry for an authorized read failure without consuming the answer or changing the deadline', () => {
    for (const code of ['unauthorized', 'not_found', 'unavailable'] as const) {
      const { session, socket, advance } = setup('manage_rules', true);
      session.setUiReady(true); socket.receive(wire(ask)); const row = session.getSnapshot().reviews[0]!;
      session.readAskContext(row); const commandId = queryId(socket);
      advance(2000); socket.receive(businessWire(commandId, { status: 'failed', code, message: '' }));
      expect(session.getSnapshot().askEvidence).toMatchObject({ phase: 'returned', pending: null,
        result: { status: 'failed', code, message: '' } });
      const count = socket.sent.length; session.readAskContext(row);
      expect(socket.sent).toHaveLength(count + Number(code !== 'unauthorized'));
      expect(session.remaining(row)).toBe(3000);
      session.submit(row, 0); expect(session.getSnapshot().reviews[0]?.phase).toBe('sent');
      session.disconnect();
    }
    const { session, socket } = setup('manage_rules', true);
    session.setUiReady(true); socket.receive(wire(ask)); const row = session.getSnapshot().reviews[0]!;
    socket.failSend = true; session.readAskContext(row);
    expect(session.getSnapshot().askEvidence).toMatchObject({ phase: 'failed', pending: null,
      problem: { key: 'evidence.sendFailed' } });
    socket.failSend = false; const count = socket.sent.length; session.readAskContext(row);
    expect(socket.sent).toHaveLength(count + 1); session.submit(row, 0);
    expect(session.getSnapshot().reviews[0]?.phase).toBe('sent'); session.disconnect();
  });

  it.each([
    { decision_id: Array(16).fill(99) },
    { event: { ...eventAnchor, source_instance: Array(16).fill(9) } },
    { event: { ...eventAnchor, sequence_domain: '18446744073709551614' } },
    { event: { ...eventAnchor, sequence: '9007199254740992' } },
    { view: { ...askContext().view, anchor: { kind: 'question', value: Array(16).fill(9) } } },
    { view: { ...askContext().view, anchor: { kind: 'event', value: eventAnchor } } },
  ])('rejects mismatched full Ask identity without affecting the answer: %j', changed => {
    const { session, socket, advance } = setup('manage_rules', true);
    session.setUiReady(true); socket.receive(wire(ask));
    const row = session.getSnapshot().reviews[0]!;
    session.readAskContext(row); const commandId = queryId(socket);
    const before = session.getSnapshot();
    socket.receive(businessWire(Array(16).fill(77), askContextResult()));
    expect(session.getSnapshot()).toBe(before);
    socket.receive(businessWire(commandId, askContextResult({ ...askContext(), ...changed } as AskAuditContext)));
    expect(session.getSnapshot().askEvidence).toMatchObject({ phase: 'failed', pending: null, result: null,
      problem: { key: 'ask.wrongReply' } });
    expect(session.getSnapshot().reviews[0]).toBe(row);
    advance(1000); expect(session.remaining(row)).toBe(4000);
    session.submit(row, 0);
    expect(JSON.parse(socket.sent.at(-1)!)).toEqual({ version: 2, message: { type: 'answer', decision_id: id, decision: allow } });
    expect(session.getSnapshot().phase).toBe('authenticated');
    session.disconnect();
  });

  it('keeps wrong business/ordinary lanes out of the slot and never renews a repeated Ask', () => {
    for (const result of [overviewResult(), contextResult()]) {
      const { session, socket, advance } = setup('manage_service', true);
      session.setUiReady(true); socket.receive(wire(ask)); const row = session.getSnapshot().reviews[0]!;
      session.readAskContext(row); const commandId = queryId(socket);
      socket.receive(wire({ type: 'reply', id: commandId, result: driverRead }));
      expect(session.getSnapshot().askEvidence.phase).toBe('loading');
      socket.receive(businessWire(commandId, result));
      expect(session.getSnapshot().askEvidence).toMatchObject({ phase: 'failed', result: null, pending: null });
      advance(4000); socket.receive(wire({ ...ask, remaining_ms: 90000 }));
      expect(session.getSnapshot().reviews[0]).toBe(row);
      expect(session.remaining(row)).toBe(1000);
      advance(1001); const count = socket.sent.length;
      session.submit(row, 0); expect(socket.sent).toHaveLength(count);
      session.disconnect();
    }
  });

  it('allows exactly one answer during a pending database read, retaining the same Ask after completion', () => {
    const { session, socket, advance } = setup('manage_rules', true);
    session.setUiReady(true); socket.receive(wire(ask)); const row = session.getSnapshot().reviews[0]!;
    session.readAskContext(row); const commandId = queryId(socket);
    advance(1500); session.submit(row, 1); session.submit(row, 0);
    const sent = session.getSnapshot().reviews[0]!;
    expect(sent.ask).toBe(row.ask); expect(sent.observedAt).toBe(row.observedAt);
    socket.receive(wire(completed('denied')));
    const receipt = session.getSnapshot().reviews[0]!;
    expect(receipt.ask).toBe(row.ask); expect(receipt.observedAt).toBe(row.observedAt);
    socket.receive(businessWire(commandId, askContextResult()));
    expect(session.getSnapshot().askEvidence).toMatchObject({ phase: 'initial', result: null });
    session.readAskContext(receipt);
    const refreshId = queryId(socket);
    expect(refreshId).not.toEqual(commandId);
    socket.receive(businessWire(refreshId, askContextResult()));
    expect(session.getSnapshot().askEvidence.result).toEqual(askContextResult());
    expect(session.remaining(receipt)).toBe(3500);
    expect(socket.sent.filter(text => JSON.parse(text).message?.type === 'answer')).toHaveLength(1);
    session.disconnect();
  });

  it('clears the one-slot cache on Ask changes and ignores late replies after switching, disconnect and lock', () => {
    const { session, socket, sockets } = setup('manage_rules', true);
    session.setUiReady(true); socket.receive(wire(ask)); const first = session.getSnapshot().reviews[0]!;
    session.readAskContext(first); const firstId = queryId(socket);
    socket.receive(wire({ ...ask, decision_id: Array(16).fill(22) }));
    const second = session.getSnapshot().reviews[1]!;
    expect(session.getSnapshot().askEvidence).toMatchObject({ ask: first.ask, phase: 'loading', pending: firstId });
    session.selectAskContext(second);
    expect(session.getSnapshot().askEvidence).toMatchObject({ ask: second.ask, phase: 'loading', result: null });
    session.readAskContext(second); const secondId = queryId(socket);
    socket.receive(businessWire(firstId, askContextResult()));
    expect(session.getSnapshot().askEvidence.pending).toEqual(secondId);
    session.selectAskContext(first);
    socket.receive(businessWire(secondId, askContextResult()));
    expect(session.getSnapshot().askEvidence).toMatchObject({ ask: first.ask, phase: 'loading', result: null });
    session.readAskContext(first); const currentId = queryId(socket);
    socket.receive(businessWire(firstId, askContextResult()));
    expect(session.getSnapshot().askEvidence.pending).toEqual(currentId);
    socket.receive(businessWire(currentId, askContextResult()));
    const returned = session.getSnapshot();
    socket.receive(businessWire(currentId, { status: 'failed', code: 'unavailable', message: 'late' }));
    expect(session.getSnapshot()).toBe(returned);
    const late = socket.onmessage!; socket.onclose?.();
    expect(session.getSnapshot().askEvidence).toMatchObject({ ask: null, phase: 'initial', result: null });
    session.connect(testToken); const next = sockets[1]!; next.open();
    next.receive(JSON.stringify({ version: 2, authenticated: { access: 'manage_rules', chat_content: false } }));
    const lockedOut = session.getSnapshot(); late({ data: businessWire(currentId, askContextResult()) });
    session.readAskContext(first); expect(session.getSnapshot()).toBe(lockedOut);
    session.disconnect(); expect(session.getSnapshot().askEvidence.ask).toBeNull();
  });
});

describe('Ask confirmed-call presentation boundary', () => {
  it('projects the same confirmed call and operation from the two authorized views', () => {
    const context = linkedContext();
    const askChain = projectCausalChain({ kind: 'ask', ask, completion: null }, { kind: 'ask', context });
    const activityChain = projectCausalChain({ kind: 'completion', completion: completed() },
      { kind: 'activity', view: { ...context.view, anchor: { kind: 'event', value: eventAnchor } } });
    expect(activityChain.steps[0]).toEqual(askChain.steps[0]);
    expect(activityChain.steps[1]?.incoming).toEqual(askChain.steps[1]?.incoming);
    expect(askChain.steps[0]?.stage).toBe('upstream');
    expect(askChain.steps[1]?.stage === 'operation' && askChain.steps[1].event).toBe(ask.event);
  });

  it('decodes cancellation as an unfinished feedback outcome', () => {
    const context = linkedContext();
    const record = { kind: 'rule_feedback', value: {
      denial: eventAnchor, call: context.trigger_call!,
      state: { type: 'failed', original: null, code: 'cancelled' },
    } };
    const data = { type: 'activity_context', attribution: null,
      view: { ...context.view, source_records: [...context.view.source_records, record] } };
    expect(decodeActivityContext(data).view.source_records.at(-1)).toEqual(record);
  });

  it('displays only the feedback fact for the selected event and confirmed call', () => {
    const { session, socket } = setup('manage_service', true);
    session.enterActivity();
    socket.receive(logReply(logCommand(socket).id));
    session.selectActivity(session.getSnapshot().activity.page!.records[0]!);
    const context = linkedContext();
    const original = { kind: 'event' as const, value: eventAnchor };
    const record = { kind: 'rule_feedback' as const, value: {
      denial: eventAnchor, call: context.trigger_call!, state: {
        type: 'submitted' as const, original: { exchange: context.trigger_call!.message.exchange, sequence: '2' },
      },
    } };
    const render = (source?: typeof record, relations = context.view.relations) => {
      session.readActivityEvidence();
      socket.receive(businessWire(queryId(socket), contextResult({
        ...context.view, anchor: original, relations,
        source_records: source ? [...context.view.source_records, source] : context.view.source_records,
      })));
      return renderToStaticMarkup(createElement(I18nProvider, { children: createElement(ActivityView, {
        activity: session.getSnapshot().activity, access: 'manage_service', chatContent: true, session, askVisible: false,
      }) }));
    };
    const submitted = translate('ja-JP', 'activity.feedbackSubmitted');
    try {
      expect(render(record)).toContain(submitted);
      expect(render()).not.toContain(submitted);
      expect(render(record, [])).not.toContain(submitted);
      expect(render({ ...record, value: { ...record.value, denial: { ...eventAnchor, sequence: '999' } } })).not.toContain(submitted);
      expect(render({ ...record, value: { ...record.value, call: { ...record.value.call, part_id: 99 } } })).not.toContain(submitted);
    } finally { session.disconnect(); }
  });

  it('groups repeated trusted sources once per stage, leaving unrelated originals in the raw receipt', () => {
    const context = linkedContext();
    const candidate = { kind: 'message' as const, value: { id: {
      exchange: { flow_instance: Array(16).fill(9), sequence: '2' }, sequence: '99',
    }, observed_parts: [{ type: 'tool_call', name: 'candidate-only' }] } };
    const view: ContextView = { ...context.view, anchor: { kind: 'event', value: eventAnchor },
      source_records: [...context.view.source_records, ...context.view.source_records, candidate],
      relations: [...context.view.relations, ...context.view.relations],
    };
    const before = factsJson(view);
    const model = projectCausalChain({ kind: 'completion', completion: completed() }, { kind: 'activity', view });
    expect(model.originals.upstream.map(source => source.kind).sort()).toEqual(['message', 'protocol', 'question', 'relation']);
    for (const originals of Object.values(model.originals)) {
      expect(originals.filter(source => source.kind === 'message').length).toBeLessThanOrEqual(1);
      expect(originals.filter(source => source.kind === 'protocol').length).toBeLessThanOrEqual(1);
    }
    expect(factsJson(model.originals)).not.toContain('candidate-only');
    expect(model.receipt).toBe(view);
    expect(factsJson(model.receipt)).toContain('candidate-only');
    const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(CausalChain, { model }) }));
    const stages = markup.match(/<li\b[^>]*data-stage="[^"]+"[^>]*>[\s\S]*?<\/li>/g) ?? [];
    for (const stage of stages) {
      expect((stage.match(/class="chain-originals"/g) ?? []).length).toBeLessThanOrEqual(1);
      expect((stage.match(/class="chain-message-details"/g) ?? []).length).toBeLessThanOrEqual(1);
    }
    expect(markup).not.toContain('<pre>');
    expect(factsJson(view)).toBe(before);

    const protocol = view.source_records.find(source => source.kind === 'protocol');
    if (protocol?.kind !== 'protocol') throw new Error('missing original protocol fixture');
    const gap = { at: { kind: 'protocol' as const, value: protocol.value.id as Record<string, unknown> }, reason: 'not_retained' as const };
    const missing = projectCausalChain({ kind: 'completion', completion: completed() }, { kind: 'activity', view: { ...view, gaps: [gap] } });
    expect(missing.steps.some(step => step.stage === 'upstream')).toBe(false);
    expect(missing.originals.upstream).toEqual([{ kind: 'gap', references: [gap.at], value: gap }]);
    expect(factsJson(missing.originals)).not.toContain('candidate-only');
  });

  it('keeps the actual checkpoint, return and related gaps at their own stages', () => {
    const context = linkedContext();
    const completion = completed('allowed');
    const returned = { event: eventAnchor, operation: 'file_open', decision_id: id, wait_id: '7',
      returned_boottime_ns: '9007199254740994', return_value: -1 } as const;
    const view: ContextView = { ...context.view, anchor: { kind: 'event', value: eventAnchor },
      source_records: [...context.view.source_records,
        { kind: 'operation_result', value: completion }, { kind: 'request_return', value: returned }],
      gaps: [{ at: { kind: 'event', value: { ...eventAnchor, sequence: '9007199254740992' } }, reason: 'not_retained' }],
    };
    const chain = projectCausalChain({ kind: 'completion', completion }, { kind: 'activity', view });
    expect(chain.steps.map(step => step.stage)).toEqual(['upstream', 'operation', 'checkpoint', 'native_return']);
    expect(chain.steps[3]).toMatchObject({ stage: 'native_return', record: { return_value: -1 } });
    expect(chain.steps[3]?.stage === 'native_return' && chain.steps[3].record).toBe(returned);
    expect(chain.issues).toMatchObject([{ stage: 'tool_result', reason: 'no_explicit_relation', original: null }]);
    const missing = projectCausalChain({ kind: 'completion', completion }, { kind: 'activity',
      view: { ...view, source_records: view.source_records.map(source => source.kind === 'request_return'
        ? { ...source, value: { ...source.value, decision_id: Array(16).fill(99) } } : source) } });
    expect(missing.steps.some(step => step.stage === 'native_return')).toBe(false);
    expect(missing.issues).toMatchObject([
      { stage: 'native_return', reason: 'not_provided' },
      { stage: 'tool_result', reason: 'no_explicit_relation', original: null },
    ]);
    const direct = projectCausalChain({ kind: 'completion', completion: completed('allowed', null) }, { kind: 'activity', view });
    expect(direct.steps.some(step => step.stage === 'native_return')).toBe(false);
  });

  it('exposes a disconnected missing result link only after the confirmed operation receipt', () => {
    const context = linkedContext();
    const completion = completed();
    const retained: AskAuditContext = { ...context, view: { ...context.view,
      source_records: [...context.view.source_records, { kind: 'operation_result', value: completion }] } };
    const view: ContextView = { ...retained.view, anchor: { kind: 'event', value: eventAnchor } };
    const originals = factsJson(view);
    for (const model of [
      projectCausalChain({ kind: 'ask', ask, completion }, { kind: 'ask', context: retained }),
      projectCausalChain({ kind: 'log', log: { ...ruleLog, decision_id: id } }, { kind: 'activity', view }),
    ]) {
      expect(model.steps.some(step => step.stage === 'upstream')).toBe(true);
      expect(model.steps.some(step => step.stage === 'checkpoint')).toBe(true);
      expect(model.steps.some(step => step.stage === 'tool_result')).toBe(false);
      expect(model.issues.filter(issue => issue.stage === 'tool_result')).toEqual([
        { stage: 'tool_result', at: { kind: 'part', value: context.trigger_call! }, reason: 'no_explicit_relation', original: null },
      ]);
      expect(model.view?.relations).toEqual(context.view.relations);
    }
    expect(factsJson(view)).toBe(originals);
    const waiting = projectCausalChain({ kind: 'ask', ask, completion: null }, { kind: 'ask', context: retained });
    expect(waiting.issues.filter(issue => issue.stage === 'tool_result')).toEqual([]);
    for (const model of [
      projectCausalChain({ kind: 'ask', ask, completion: null }, { kind: 'ask', context }),
      projectCausalChain({ kind: 'completion', completion }),
      projectCausalChain({ kind: 'completion', completion }, { kind: 'activity', view: { ...view, relations: [] } }),
      projectCausalChain({ kind: 'completion', completion }, { kind: 'activity',
        view: { ...view, anchor: { kind: 'event', value: { ...eventAnchor, sequence: '1' } } } }),
      projectCausalChain({ kind: 'log', log: { ...ruleLog, decision_id: id } }, { kind: 'activity',
        view: { ...context.view, anchor: { kind: 'event', value: eventAnchor } } }),
    ]) expect(model.issues.filter(issue => issue.stage === 'tool_result')).toEqual([]);
  });

  it('keeps an explicit result edge with missing originals in its existing gap categories', () => {
    const context = linkedContext();
    const result = { kind: 'part' as const, value: { message: context.trigger_call!.message, part_id: 1 } };
    const edge = { kind: 'native_tool_reference' as const,
      from: { kind: 'part' as const, value: { ...context.trigger_call! } }, to: result, evidence: [result] };
    for (const reason of [null, 'not_retained', 'conflicting_evidence'] as const) {
      const gap = reason === null ? null : { at: result, reason };
      const view: ContextView = { ...context.view, anchor: { kind: 'event', value: eventAnchor },
        relations: [...context.view.relations, edge], gaps: gap ? [gap] : [] };
      const model = projectCausalChain({ kind: 'completion', completion: completed() }, { kind: 'activity', view });
      const issues = model.issues.filter(issue => issue.stage === 'tool_result');
      expect(issues).toContainEqual({ stage: 'tool_result', at: edge.from, reason: 'not_provided', original: null });
      expect(issues.some(issue => issue.reason === 'no_explicit_relation')).toBe(false);
      if (gap) expect(issues).toContainEqual({ stage: 'tool_result', at: gap.at, reason, original: gap });
      expect(model.steps.some(step => step.stage === 'tool_result')).toBe(false);
    }
  });

  it('requires the original call, directed backend link, event and retained evidence together', () => {
    const context = linkedContext();
    const decoded = decodeAskContext({ type: 'ask_context', context }).context;
    expect(confirmedExecutionCall(ask.event, decoded.view, { ask, context: decoded })?.part)
      .toMatchObject({ name: '', arguments: { kind: 'text', value: '' } });
    expect(projectCausalChain({ kind: 'ask', ask, completion: null }, { kind: 'ask', context: decoded }).steps.map(step => step.stage))
      .toEqual(['upstream', 'operation']);
    const relation = context.view.relations[0]!;
    for (const view of [
      { ...context.view, relations: [] },
      { ...context.view, relations: [{ ...relation, from: relation.to, to: relation.from }] },
      { ...context.view, relations: [{ ...relation, from: { kind: 'part' as const, value: {
        ...context.trigger_call!, part_id: 1,
      } } }] },
      { ...context.view, relations: [{ ...relation, to: { kind: 'event' as const, value: { ...eventAnchor, sequence: '9007199254740992' } } }] },
      { ...context.view, source_records: context.view.source_records.filter(record => record.kind !== 'protocol') },
      { ...context.view, source_records: context.view.source_records.filter(record => record.kind !== 'question') },
      { ...context.view, gaps: [{ at: relation.from, reason: 'conflicting_evidence' as const }] },
    ]) expect(confirmedExecutionCall(ask.event, view, { ask, context: { ...context, view } })).toBeNull();
    expect(confirmedExecutionCall(ask.event, context.view, { ask, context: { ...context, trigger_call: null } })).toBeNull();
  });
  it('shows the backend same-container candidate in both Ask and activity without confirming execution', () => {
    const original = linkedContext();
    const context = decodeAskContext({ type: 'ask_context', context: { ...original, trigger_call: null, window: null,
      view: { ...original.view, relations: original.view.relations.map(link => ({ ...link, kind: 'tool_container_candidate' })) } } }).context;
    expect(confirmedExecutionCall(ask.event, context.view, { ask, context })).toBeNull();
    for (const model of [
      projectCausalChain({ kind: 'ask', ask, completion: null }, { kind: 'ask', context }),
      projectCausalChain({ kind: 'completion', completion: completed() }, { kind: 'activity', view: {
        ...context.view, anchor: { kind: 'event', value: eventAnchor },
      } }),
    ]) {
      expect(model.steps.find(step => step.stage === 'upstream')).toMatchObject({ candidate: true });
      expect(model.steps.find(step => step.stage === 'operation')?.incoming?.kind).toBe('candidate');
      const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(CausalChain, { model }) }));
      expect(markup).toContain(translate('ja-JP', 'chain.containerCandidate'));
    }
  });
  it('uses backend-confirmed relationships with explicit deferred originals and reads one body page through the same slot', () => {
    const context = linkedContext();
    const protocol = context.view.source_records.find(source => source.kind === 'protocol');
    if (!protocol || protocol.kind !== 'protocol') throw new Error('protocol fixture');
    const reference = { kind: 'protocol' as const, value: protocol.value.id as Record<string, unknown> };
    const view: ContextView = { ...context.view, anchor: { kind: 'event', value: eventAnchor },
      source_records: context.view.source_records.map(source => source.kind === 'protocol'
        ? { kind: 'deferred' as const, value: reference } : source) };
    expect(confirmedExecutionCall(ask.event, view)).not.toBeNull();
    const { session, socket } = setup('manage_service', true);
    try {
      session.enterActivity(); socket.receive(logReply(logCommand(socket).id));
      session.selectActivity(session.getSnapshot().activity.page!.records[0]!);
      socket.receive(businessWire(queryId(socket), contextResult(view)));
      expect(session.openOriginalRecord(session.getSnapshot().activity.evidence, reference)).toBe(true);
      expect(JSON.parse(socket.sent.at(-1)!).business.query).toMatchObject({ type: 'original_record', reference, body_offset: '0', body_limit: 65536 });
      socket.receive(businessWire(queryId(socket), contextResult({ anchor: reference, relations: [], gaps: [],
        source_records: [{ kind: 'protocol_page', value: { observation: protocol.value, next_offset: '65536' } }] })));
      expect(session.getSnapshot().activity.evidence.result?.status).toBe('completed');
      expect(session.openOriginalRecord(session.getSnapshot().activity.evidence, reference, '65536')).toBe(true);
      expect(JSON.parse(socket.sent.at(-1)!).business.query.body_offset).toBe('65536');
    } finally { session.disconnect(); }
  });
  it('does not promote native context, message proximity or equal command bytes into a call link', () => {
    const context = linkedContext();
    context.view = { ...context.view, relations: [], source_records: [...context.view.source_records,
      { kind: 'tool_execution', value: { event: eventAnchor, context: { tool_name: 'actual-native-name', arguments: null } } },
    ] };
    context.command_checks = [{ call: context.trigger_call!, operation: eventAnchor,
      argument_pointer: '', exec_argument_index: 0, outcome: 'equal_bytes', evidence: [] }];
    expect(confirmedExecutionCall(ask.event, context.view, { ask, context })).toBeNull();
  });
});

describe('frozen activity codecs', () => {
  it('keeps i64 row IDs and exclusive before strings exact, including the maximum', () => {
    for (const value of ['1', '9007199254740993', '9223372036854775807']) expect(logRowId(value)).toBe(value);
    for (const value of ['0', '-1', '01', '9223372036854775808', '18446744073709551615', 1]) expect(() => logRowId(value)).toThrow();
    const data = { type: 'security_log', page: logPage() };
    expect(decodeSecurityLog(data)).toEqual(data);
    expect(JSON.parse(securityLogCommand(id, { before: '9223372036854775807', limit: 8 }))).toEqual({
      version: 2, message: { type: 'security_log', id, command: { before: '9223372036854775807', limit: 8 } },
    });
    expect(matchesLogPage(logPage(['9007199254740992'], false), { before: '9007199254740993', limit: 8 })).toBe(true);
    expect(matchesLogPage(logPage(['9007199254740993'], false), { before: '9007199254740993', limit: 8 })).toBe(false);
    for (const page of [logPage(['1', '2']), logPage(['2', '2']), { ...logPage(), next_before: '1' }]) {
      expect(() => decodeSecurityLog({ type: 'security_log', page })).toThrow();
    }
  });
  it('separates untagged proposals, actual outcomes and module errors without requiring an Ask', () => {
    for (const decision of [{ state: 'pending' }, { state: 'transferred' }, { state: 'resolved', decision: deny, reason: 'direct' }]) {
      for (const result of [{ Ok: 'observed' }, ruleLog.result]) {
        expect(decodeLogEntry({ ...ruleLog, decision, result })).toEqual({ ...ruleLog, decision, result });
      }
    }
    const termination = { requested: 'none', actor: { status: 'not_requested' }, parent: { status: 'not_requested' } };
    const execution = { Ok: { execution: { decision_id: id, outcome: { failed: { native_error: -5 } }, termination } } };
    expect(decodeLogEntry({ ...ruleLog, result: execution })).toMatchObject({ result: execution, decision: { state: 'transferred' } });
    expect(decodeLogEntry({ ...ruleLog, result: { Err: 'invalid_input' } })).toMatchObject({ result: { Err: 'invalid_input' } });
    expect(() => decodeLogEntry({ ...ruleLog, kind: 'rule' })).toThrow();
    expect(() => decodeLogEntry({ ...ruleLog, result: { Err: 'invalid' } })).toThrow();
  });
  it('preserves signed reporter TID, nullable/empty path and u64 inode without manufacturing an anchor', () => {
    for (const path of [null, [], [255, 0]]) {
      const record = decodeLogEntry({ kind: 'file_write_close', reporter_tid: -12, path, path_error: path === null ? -2 : null,
        device_major: 0, device_minor: 0, inode: '18446744073709551615' });
      expect(record).toMatchObject({ reporter_tid: -12, path, path_error: path === null ? -2 : null, inode: '18446744073709551615' });
      expect(selectionAnchor({ kind: 'log', row: { id: '1', record } })).toBeNull();
    }
    for (const record of [{ kind: 'file_close_queue_overflow' }, { kind: 'file_close_source_failed', errno: null }]) {
      expect(() => decodeLogEntry(record)).toThrow();
    }
  });
  it('retains negative returns, distinct futex wait pairing, gap reasons and evidence-backed relations', () => {
    const view = contextView();
    for (const wait of [null, { id: '0', observed_boottime_ns: '9007199254740993' }]) {
      view.source_records = [...contextView().source_records, { kind: 'futex_selection', value: { event: eventAnchor,
        waker: { tgid: 1, tid: 2, birth_boottime_ns: '0' }, waiter: { tgid: 3, tid: 4, birth_boottime_ns: '0' },
        selected_boottime_ns: '9007199254740993', wait } }];
      const data = { type: 'activity_context', attribution: null, view };
      expect(decodeActivityContext(data)).toEqual(data);
      expect(factsJson(decodeActivityContext(data))).toContain('"return_value": -13');
    }
    view.relations = [{ kind: 'question_completion', from: { kind: 'question', value: id },
      to: { kind: 'completion', value: eventAnchor }, evidence: [{ kind: 'event', value: eventAnchor }] }];
    expect(decodeActivityContext({ type: 'activity_context', attribution: null, view }).view.relations).toEqual(view.relations);
    expect(() => decodeActivityContext({ type: 'activity_context', attribution: null,
      view: { ...view, relations: [{ ...view.relations[0], evidence: [] }] } })).toThrow();
    expect(() => decodeActivityContext({ type: 'activity_context', attribution: null,
      view: { ...view, source_records: [{ kind: 'question', value: completed() }] } })).toThrow();
    expect(JSON.parse(activityContextCommand(id, { kind: 'event', value: eventAnchor }))).toEqual({ version: 2,
      business: { id, query: { type: 'activity_context', anchor: { kind: 'event', value: eventAnchor }, attribution: null } } });
  });
  it('distinguishes the untagged Context operation result from an Up message and retains unconsumed source facts', () => {
    const originalResult = withoutKey(completed(), 'type');
    const protocol = { request_session: null, original_numeric: 18446744073709551615n, empty: '', items: [], absent: null, flag: false };
    const data = { type: 'activity_context', attribution: null, view: { ...contextView(), source_records: [
      { kind: 'question', value: ask }, { kind: 'operation_result', value: originalResult },
      { kind: 'protocol', value: protocol }, { kind: 'message', value: { original_text: '' } },
      { kind: 'tool_execution', value: { event: eventAnchor, context: { original_value: 0 } } },
    ] } };
    const decoded = parseFrame(factsJson({ version: 2, business_reply: { id, result: { status: 'completed', data } } }));
    expect(decoded).toEqual({ kind: 'business_reply', reply: { id, result: { status: 'completed', data } } });
    expect(factsJson(decoded)).toContain('18446744073709551615');
    expect(() => decodeActivityContext({ ...data, view: { ...data.view,
      source_records: [{ kind: 'operation_result', value: completed() }] } })).toThrow();
    expect(() => decodeActivityContext({ ...data, view: { ...data.view,
      source_records: [{ kind: 'native_return', value: {} }] } })).toThrow();
  });
});

describe('activity production session consumption', () => {
  it('ends a same-ID wrong-type overview locally without completing another read or the Ask', () => {
    const { session, socket } = setup('manage_service', true);
    const overviewId = queryId(socket);
    session.setUiReady(true); socket.receive(wire(ask)); session.enterActivity();
    const before = session.getSnapshot();
    socket.receive(businessWire(overviewId, contextResult()));
    const failed = session.getSnapshot();
    expect(failed.overview).toEqual({ phase: 'failed', id: overviewId, message: { key: 'overview.wrongReply' } });
    expect(overviewView(failed.overview).label).toBe('overview.failed');
    expect(driverFacts(failed.overview)).toBeUndefined();
    expect(failed.activity).toBe(before.activity);
    expect(failed.reviews).toBe(before.reviews);
    expect(failed.phase).toBe('authenticated');
    socket.receive(businessWire(overviewId));
    expect(session.getSnapshot()).toBe(failed);
    session.refreshOverview(); socket.receive(businessWire(queryId(socket)));
    expect(session.getSnapshot().overview.phase).toBe('returned');
    session.disconnect();
  });

  it('enforces the two independent grants at send time and opens only actually received completions', () => {
    const service = setup('manage_service', false);
    expect(service.session.getSnapshot().chatContent).toBe(false);
    expect(service.socket.sent).toHaveLength(2); // Authentication + existing overview; no log prefetch.
    service.session.enterActivity();
    service.socket.receive(logReply(logCommand(service.socket).id));
    service.session.selectActivity(service.session.getSnapshot().activity.page!.records[0]!);
    const count = service.socket.sent.length;
    service.session.readActivityEvidence();
    expect(service.socket.sent).toHaveLength(count);
    expect(service.session.getSnapshot().activity.evidence.phase).toBe('initial');
    service.session.disconnect();

    const chat = setup('manage_rules', true);
    chat.session.enterActivity(); chat.session.refreshActivity();
    expect(chat.socket.sent).toHaveLength(2);
    chat.session.openCompletionEvidence(completed()); // Fabricated object cannot become a source.
    expect(chat.session.getSnapshot().activity.selection).toBeNull();
    chat.session.setUiReady(true); chat.socket.receive(wire(ask)); chat.socket.receive(wire(completed()));
    const row = chat.session.getSnapshot().reviews[0]!;
    const sent = chat.socket.sent.length;
    chat.session.openCompletionEvidence(row.completion!);
    expect(chat.socket.sent).toHaveLength(sent + 2); // Initial SQL read and shared stream.
    chat.session.readActivityEvidence(); chat.session.readActivityEvidence();
    expect(chat.socket.sent).toHaveLength(sent + 2);
    expect(JSON.parse(chat.socket.sent.at(-1)!)).toMatchObject({ business: { query: {
      type: 'activity_context', anchor: { kind: 'event', value: eventAnchor }, attribution: null,
    } } });
    expect(chat.session.getSnapshot().reviews[0]).toBe(row);
    expect(chat.socket.sent.some(text => JSON.parse(text).message?.type === 'answer')).toBe(false);
    chat.session.disconnect(); expect(chat.session.getSnapshot().chatContent).toBe(false);

    const none = setup('read_only', false);
    none.session.enterActivity(); none.session.refreshActivity(); none.session.readActivityEvidence();
    expect(none.socket.sent).toHaveLength(2); none.session.disconnect();
  });

  it('pages with the exact original i64 next_before, clears old selection and distinguishes empty from failed', () => {
    const { session, socket } = setup('manage_service', true);
    session.enterActivity(); const first = logCommand(socket);
    expect(first.command).toEqual({ before: null, limit: 8 });
    socket.receive(wire({ type: 'reply', id: first.id, result: { status: 'accepted', data: { type: 'security_log', page: logPage([], false) } } }));
    expect(session.getSnapshot().activity).toMatchObject({ phase: 'loading', page: null, pending: first.id });
    const count = socket.sent.length; session.refreshActivity(16); expect(socket.sent).toHaveLength(count);
    socket.receive(logReply(first.id));
    session.selectActivity(session.getSnapshot().activity.page!.records[0]!); session.readActivityEvidence();
    const oldEvidence = queryId(socket);
    const evidenceCount = socket.sent.length;
    session.selectActivity(session.getSnapshot().activity.page!.records[0]!); session.readActivityEvidence();
    expect(socket.sent).toHaveLength(evidenceCount); // A repeated row action cannot re-arm the same pending read.
    session.nextActivity(); const next = logCommand(socket);
    expect(next.command).toEqual({ before: '9007199254740993', limit: 8 });
    expect(session.getSnapshot().activity).toMatchObject({ pageNumber: 2, page: null, selection: null,
      evidence: { phase: 'initial', pending: null } });
    socket.receive(businessWire(oldEvidence, contextResult()));
    socket.receive(logReply(first.id));
    expect(session.getSnapshot().activity.phase).toBe('loading');
    socket.receive(logReply(next.id, logPage(['9007199254740993'], false))); // before is exclusive.
    expect(session.getSnapshot().activity).toMatchObject({ phase: 'failed', pending: null,
      problem: { key: 'activity.wrongReply' } });
    expect(session.getSnapshot().phase).toBe('authenticated');
    session.refreshActivity(16); expect(logCommand(socket).command).toEqual({ before: null, limit: 16 });
    socket.receive(logReply(logCommand(socket).id, logPage([], false)));
    expect(session.getSnapshot().activity).toMatchObject({ phase: 'ready', page: { records: [], next_before: null } });
    const end = socket.sent.length; session.nextActivity(); expect(socket.sent).toHaveLength(end);
    session.disconnect();
  });

  it('keeps ordinary/business/overview/process/rule lanes isolated and ends wrong-type evidence locally', () => {
    const { session, socket } = setup('manage_service', true); const overviewId = queryId(socket);
    session.setUiReady(true); socket.receive(wire(ask)); socket.receive(wire(completed()));
    session.refreshRules(); session.refreshProcesses(); session.enterActivity();
    const logId = logCommand(socket).id;
    const row = session.getSnapshot().reviews[0]!;
    session.openCompletionEvidence(row.completion!); session.readActivityEvidence(); const evidenceId = queryId(socket);
    const others = session.getSnapshot();
    socket.receive(logReply(evidenceId)); socket.receive(businessWire(logId, contextResult()));
    expect(session.getSnapshot()).toBe(others);
    socket.receive(businessWire(evidenceId, overviewResult()));
    expect(session.getSnapshot().activity.evidence).toMatchObject({ phase: 'failed', pending: null,
      problem: { key: 'evidence.wrongReply' } });
    expect(session.getSnapshot().activity.pending).toEqual(logId);
    session.readActivityEvidence(); const currentId = queryId(socket);
    socket.receive(businessWire(evidenceId, contextResult()));
    expect(session.getSnapshot().activity.evidence.phase).toBe('loading');
    socket.receive(businessWire(currentId, contextResult()));
    socket.receive(logReply(logId, { records: [{ id: '1', record: { kind: 'file_write_close', reporter_tid: 0, path: null, path_error: -5, device_major: 0, device_minor: 0, inode: '0' } }], next_before: null }));
    expect(session.getSnapshot().activity.evidence.phase).toBe('returned');
    expect(session.getSnapshot().activity.phase).toBe('ready');
    expect(session.getSnapshot().reviews[0]).toBe(row);
    expect(session.getSnapshot().rules).toBe(others.rules);
    expect(session.getSnapshot().processes).toBe(others.processes);
    expect(session.getSnapshot().overview).toBe(others.overview);
    socket.receive(businessWire(overviewId)); expect(session.getSnapshot().overview.phase).toBe('returned');
    session.disconnect();
  });

  it.each([
    { source_instance: Array(16).fill(9) }, { sequence_domain: '1' }, { sequence: '1' },
  ])('fails a same-ID wrong original anchor locally: %j', changed => {
    const { session, socket } = setup('manage_rules', true);
    session.setUiReady(true); socket.receive(wire(ask)); socket.receive(wire(completed()));
    const row = session.getSnapshot().reviews[0]!;
    session.openCompletionEvidence(row.completion!); session.readActivityEvidence();
    socket.receive(businessWire(queryId(socket), contextResult({ ...contextView(),
      anchor: { kind: 'event', value: { ...eventAnchor, ...changed } } })));
    expect(session.getSnapshot().activity.evidence).toMatchObject({ phase: 'failed', pending: null, result: null,
      problem: { key: 'evidence.wrongReply' } });
    expect(session.getSnapshot().reviews[0]).toBe(row);
    expect(session.getSnapshot().phase).toBe('authenticated'); session.disconnect();
  });

  it.each(['output_too_large', 'output_encoding_failed'])('consumes only the exact ordinary output diagnostic: %s', code => {
    const { session, socket } = setup('manage_service', true);
    session.setUiReady(true); socket.receive(wire(ask)); const review = session.getSnapshot().reviews[0]!;
    session.refreshProcesses(); session.enterActivity(); const logId = logCommand(socket).id;
    const processes = session.getSnapshot().processes;
    socket.receive(JSON.stringify({ version: 2, diagnostic: { type: 'reply_output_unavailable', id: Array(16).fill(99), code } }));
    socket.receive(JSON.stringify({ version: 2, diagnostic: { type: 'reply_output_unavailable', id: logId, code: 'other' } }));
    expect(session.getSnapshot().activity.phase).toBe('loading');
    socket.receive(JSON.stringify({ version: 2, diagnostic: { type: 'reply_output_unavailable', id: logId, code } }));
    expect(session.getSnapshot().activity).toMatchObject({ phase: 'failed', pending: null, page: null, diagnostic: { id: logId, code } });
    socket.receive(logReply(logId)); expect(session.getSnapshot().activity.phase).toBe('failed');
    expect(session.getSnapshot().reviews[0]).toBe(review);
    expect(session.getSnapshot().processes).toBe(processes);
    session.disconnect();
  });

  it('does not derive write-close anchors, and keeps read/send/budget failures local and explicit', () => {
    const { session, socket } = setup('manage_service', true);
    session.setUiReady(true); socket.receive(wire(ask)); socket.receive(wire(completed()));
    const review = session.getSnapshot().reviews[0]!;
    session.enterActivity(); socket.receive(logReply(logCommand(socket).id, { records: [{ id: '1', record: {
      kind: 'file_write_close', reporter_tid: -1, path: null, path_error: -2, device_major: 0, device_minor: 0, inode: '0',
    } }], next_before: null }));
    session.selectActivity(session.getSnapshot().activity.page!.records[0]!);
    const sent = socket.sent.length; session.readActivityEvidence(); expect(socket.sent).toHaveLength(sent);
    session.openCompletionEvidence(review.completion!); session.readActivityEvidence();
    const evidenceId = queryId(socket);
    socket.receive(JSON.stringify({ version: 2, diagnostic: { type: 'reply_output_unavailable', id: evidenceId, code: 'output_too_large' } }));
    expect(session.getSnapshot().activity.evidence.phase).toBe('loading'); // Business failures use business_reply.
    socket.receive(businessWire(evidenceId, { status: 'failed', code: 'unavailable', message: 'output_too_large' }));
    expect(session.getSnapshot().activity.evidence).toMatchObject({ phase: 'returned', pending: null,
      result: { status: 'failed', code: 'unavailable', message: 'output_too_large' } });
    socket.failSend = true; session.readActivityEvidence();
    expect(session.getSnapshot().activity.evidence).toMatchObject({ phase: 'failed', pending: null,
      problem: { key: 'evidence.sendFailed' } });
    session.refreshActivity();
    expect(session.getSnapshot().activity).toMatchObject({ phase: 'failed', pending: null, page: null,
      problem: { key: 'activity.sendFailed' } });
    socket.failSend = false; session.refreshActivity();
    const failed = { status: 'failed', code: 'unauthorized', message: 'simulated', facts: { type: 'security_log', page: logPage([], false) } };
    socket.receive(wire({ type: 'reply', id: logCommand(socket).id, result: failed }));
    expect(session.getSnapshot().activity).toMatchObject({ phase: 'failed', page: null, receipt: { result: failed } });
    expect(session.getSnapshot().phase).toBe('authenticated'); expect(session.getSnapshot().reviews[0]).toBe(review);
    session.disconnect();
  });

  it('invalidates old evidence IDs on selection, departure and lock, even for another row with the same EventKey', () => {
    const { session, socket, sockets } = setup('manage_service', true);
    session.enterActivity(); socket.receive(logReply(logCommand(socket).id));
    const rows = session.getSnapshot().activity.page!.records;
    session.selectActivity(rows[0]!); session.readActivityEvidence(); const oldId = queryId(socket);
    session.selectActivity(rows[1]!); socket.receive(businessWire(oldId, contextResult()));
    expect(session.getSnapshot().activity.evidence.phase).toBe('loading');
    session.readActivityEvidence(); const currentId = queryId(socket);
    socket.receive(businessWire(oldId, contextResult()));
    expect(session.getSnapshot().activity.evidence.phase).toBe('loading');
    socket.receive(businessWire(currentId, contextResult())); const returned = session.getSnapshot();
    socket.receive(businessWire(currentId, { status: 'failed', code: 'not_found', message: 'simulated' }));
    expect(session.getSnapshot()).toBe(returned);
    session.readActivityEvidence(); const leavingId = queryId(socket); session.clearActivity();
    socket.receive(businessWire(leavingId, contextResult()));
    expect(session.getSnapshot().activity).toMatchObject({ active: false, page: null, selection: null, evidence: { phase: 'initial' } });
    const late = socket.onmessage!; session.disconnect();
    session.connect(testToken); const next = sockets[1]!; next.open();
    next.receive(JSON.stringify({ version: 2, authenticated: { access: 'manage_rules', chat_content: false } }));
    const lockedOut = session.getSnapshot(); late({ data: businessWire(leavingId, contextResult()) });
    expect(session.getSnapshot()).toBe(lockedOut); expect(lockedOut.chatContent).toBe(false);
    session.disconnect();
  });
});

describe('current process observations and page-local container grouping', () => {
  it('keeps full process/container identities distinct and decodes names strictly without losing empty values', () => {
    const a = trackedRecord('9007199254740992'); const b = trackedRecord('9007199254740993');
    expect(processIdentity(a.key)).not.toBe(processIdentity(b.key));
    expect(processIdentity(a.key)).not.toBe(processIdentity({ ...a.key, boot_id: Array(16).fill(5) }));
    const page = trackedPage([a, b]);
    expect(groupProcesses(page).containers[0]?.records).toHaveLength(2);
    const c = trackedRecord('3');
    c.latest.process.workload = { kind: 'container', instance: { ...containerInstance, namespace: '' } };
    // This is the fact domain, not RuleScope author validation: an acquired empty value stays empty.
    expect(decodeContainerInstance({ ...containerInstance, namespace: '' })).toEqual({ ...containerInstance, namespace: '' });
    expect(decodeContainerInstance(containerInstance)).toEqual(containerInstance);
    const d = trackedRecord('4');
    d.latest.process.workload = { kind: 'container', instance: { ...containerInstance, runtime: 'another-runtime' } };
    expect(groupProcesses(trackedPage([a, c, d])).containers).toHaveLength(3);
    expect(containerIdentity(containerInstance)).not.toBe(containerIdentity({ ...containerInstance, id: 'b'.repeat(63) + 'c' }));
    expect(byteText([])).toEqual({ kind: 'empty', value: '' });
    expect(byteText([0xe6, 0x97, 0xa5])).toEqual({ kind: 'text', value: '日' });
    expect(byteText([0xff, 0])).toEqual({ kind: 'bytes', value: '\\xff\\x00' });
    expect(byteText([0xef, 0xbb, 0xbf, 65]).value).toBe('\uFEFFA');
  });

  it('uses only current Known evidence, never falls back to initial, and retains original old values', () => {
    const record = trackedRecord();
    record.latest.process.name = [];
    expect(observedValue(record, 'name', '0')).toEqual([]);
    for (const state of ['missing', 'inaccessible', 'truncated', 'stale'] as const) {
      const changed = { ...record, latest: { ...record.latest, stamps: { ...record.latest.stamps,
        workload: { ...processStamp, state } } } };
      expect(observedValue(changed, 'workload', '0')).toBeNull();
      expect(observationState(changed, 'workload', '0')).toBe(state);
      expect(groupProcesses(trackedPage([changed])).unknown).toHaveLength(1);
      expect(changed.latest.process.workload).toEqual(record.latest.process.workload);
    }
    expect(observationState(record, 'workload', '1')).toBe('old_epoch');
    expect(observedValue(record, 'workload', '1')).toBeNull();
    const missing = { ...record, latest: { ...record.latest, stamps: {} } };
    expect(observationState(missing, 'name', '0')).toBe('no_stamp');
    const unknown = trackedRecord('2'); unknown.latest.process.workload = null;
    expect(observationState(unknown, 'workload', '0')).toBe('no_value');
    const host = trackedRecord('3'); host.latest.process.workload = { kind: 'host' };
    const grouped = groupProcesses(trackedPage([missing, unknown, host]));
    expect(grouped.host).toEqual([host]); expect(grouped.unknown).toEqual([missing, unknown]);
    expect(grouped.containers).toEqual([]);
  });
});

describe('session retention and uncertain rule replies', () => {
  it('bounds finished Ask payloads while keeping live choices and refusing retired duplicate IDs', () => {
    const { session, socket } = setup();
    session.setUiReady(true);
    socket.receive(wire(ask));
    let firstFinished: Ask | undefined;
    for (let index = 1; index <= 18; index++) {
      const decisionId = Array(16).fill(index);
      const event = { ...ask.event, header: { ...ask.event.header, sequence: String(index) } };
      const row = { ...ask, decision_id: decisionId, event };
      firstFinished ??= row;
      socket.receive(wire(row));
      socket.receive(wire({ ...completed('allowed', decisionId), event }));
    }
    const snapshot = session.getSnapshot();
    expect(snapshot.reviews).toHaveLength(17);
    expect(snapshot.reviews[0]?.ask.decision_id).toEqual(ask.decision_id);
    expect(snapshot.reviews.slice(1).every(review => review.completion !== null)).toBe(true);
    socket.receive(wire(firstFinished));
    expect(session.getSnapshot()).toBe(snapshot);
    session.submit(snapshot.reviews[0]!, 0);
    expect(session.getSnapshot().reviews[0]?.phase).toBe('sent');
  });

  it('keeps a locally expired Ask for a late terminal reply and retains that newly arrived result', () => {
    const { session, socket, advance } = setup();
    session.setUiReady(true);
    socket.receive(wire(ask));
    advance(ask.remaining_ms + 1);
    for (let index = 1; index <= 18; index++) {
      const decisionId = Array(16).fill(index);
      const event = { ...ask.event, header: { ...ask.event.header, sequence: String(index) } };
      socket.receive(wire({ ...ask, decision_id: decisionId, event }));
      socket.receive(wire({ ...completed('allowed', decisionId), event }));
    }
    const expired = session.getSnapshot().reviews[0]!;
    expect(expired.ask.decision_id).toEqual(ask.decision_id);
    expect(session.remaining(expired)).toBe(0);
    expect(expired.completion).toBeNull();
    socket.receive(wire(completed('cancelled')));
    expect(session.getSnapshot().reviews).toHaveLength(16);
    expect(session.getSnapshot().reviews[0]?.completion).toEqual(completed('cancelled'));
  });

  it('releases a rule edit after a matching output diagnostic without claiming a save or resending', () => {
    const { session, socket } = setup();
    const snapshot = readPolicy(session, socket);
    session.editRules(snapshot, editCases[0]!);
    const request = ruleCommand(socket);
    const before = socket.sent.length;
    socket.receive(JSON.stringify({ version: 2, diagnostic: {
      type: 'reply_output_unavailable', id: request.id, code: 'output_too_large',
    } }));
    expect(session.getSnapshot().rules).toMatchObject({
      phase: 'failed', pending: null, snapshot: null, receipt: null,
      readFailure: { type: 'reply_output_unavailable', code: 'output_too_large' },
    });
    expect(socket.sent).toHaveLength(before);
    session.refreshRules();
    expect(ruleCommand(socket).command).toEqual({ type: 'get' });
  });
});

// Current R7 config (task contract/r7-config.rs), original ConfigSnapshot and receipts. Synthetic supply only.
function settingsRequest(socket: FakeSocket, type: 'configuration' | 'control' = 'configuration') {
  return parseUnique(socket.sent.filter(text => (parseUnique(text) as { message?: { type: string } }).message?.type === type).at(-1)!) as {
    version: number; message: { type: string; id: readonly number[]; command: Record<string, unknown> };
  };
}
function readConfiguration(session: ReviewSession, socket: FakeSocket, snapshot = configSnapshot) {
  session.enterSettings();
  socket.receive(ruleReply(settingsRequest(socket).message.id, { status: 'completed', data: { type: 'configuration', snapshot } }));
  return session.getSnapshot().settings.snapshot!;
}
const changedDefaults = (snapshot: ConfigSnapshot): ConfigDocument => ({ ...snapshot.document,
  driver: { ...snapshot.document.driver, startup_protection: { ...snapshot.document.driver.startup_protection, file: true } } });

describe('service settings on the existing formal WS', () => {
  it.each(['read_only', 'manage_rules'])('allows Get for %s but exposes and sends no writes', access => {
    const { session, socket } = setup(access);
    expect(session.getSnapshot().settings.active).toBe(false);
    const snapshot = readConfiguration(session, socket);
    expect(settingsRequest(socket)).toEqual({ version: 2, message: { type: 'configuration',
      id: settingsRequest(socket).message.id, command: { type: 'get' } } });
    expect(snapshot).toEqual(configSnapshot);
    const sent = socket.sent.length;
    session.saveConfiguration(snapshot, changedDefaults(snapshot)); session.reloadConfiguration(snapshot);
    session.controlRuntime({ type: 'set_runtime_feature', feature: 'file', enabled: true });
    session.controlRuntime({ type: 'resume_runtime_feature', feature: 'file' });
    expect(socket.sent).toHaveLength(sent);
    socket.receive(businessWire(queryId(socket)));
    const state = session.getSnapshot();
    const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(SettingsView, {
      settings: state.settings, overview: state.overview, access: state.access, session,
    }) }));
    for (const key of ['settings.edit', 'settings.reload', 'settings.enableTemporarily', 'settings.disableTemporarily', 'settings.resume'] as const) {
      expect(markup).not.toContain(translate('ja-JP', key));
    }
    session.disconnect();
  });

  it.each([null, { a2a_port: 8001, otlp_http_port: 4318, gateway_ws_port: null }])(
    'sends the original version and complete document preserving untouched Group settings %j', group_observation => {
    const { session, socket } = setup('manage_service');
    const snapshot = readConfiguration(session, socket, { ...configSnapshot,
      document: { ...configDocument, group_observation } }), document = changedDefaults(snapshot);
    expect(decodeConfigDocument(document)).toBe(document);
    expect(document.process_tree).toBe(snapshot.document.process_tree);
    expect(document.middleware).toBe(snapshot.document.middleware);
    expect(document.credentials).toBe(snapshot.document.credentials);
    expect(document.group_observation).toBe(snapshot.document.group_observation);
    expect(changedConfigSections(snapshot.document, document)).toEqual(['driver']);
    session.saveConfiguration(snapshot, document);
    const request = settingsRequest(socket);
    expect(request.message.command).toEqual({ type: 'update', expected_version: snapshot.version, document });
    expect(record(request.message.command.document).group_observation).toEqual(group_observation);
    expect(request.message.command).not.toHaveProperty('paths');
    expect(socket.sent.at(-1)).toBe(configurationCommand(request.message.id,
      { type: 'update', expected_version: snapshot.version, document }));
    expect(socket.sent.at(-1)).toContain('18446744073709551615');
    const sent = socket.sent.length;
    session.saveConfiguration(snapshot, document); session.reloadConfiguration(snapshot); session.refreshSettings();
    expect(socket.sent).toHaveLength(sent);
    socket.receive(ruleReply(request.message.id, { status: 'accepted', data: savedConfiguration }));
    expect(session.getSnapshot().settings.pending?.id).toEqual(request.message.id);
    socket.receive(ruleReply(request.message.id, { status: 'completed', data: savedConfiguration }));
    expect(session.getSnapshot().settings.receipt?.result).toEqual({ status: 'completed', data: savedConfiguration });
    expect(settingsRequest(socket).message.command).toEqual({ type: 'get' });
    session.disconnect();
  });

  it('uses the same egress wire shapes for configuration drafts and saved settings', () => {
    const document = egressSnapshot.document;
    expect(decodeConfigDocument(document)).toBe(document);
    const flow = document.flow!;
    const egress = record(flow.egress);
    const routes = flow.provider_routes as readonly Record<string, unknown>[];
    for (const changed of [
      { ...document, flow: { ...flow, egress: { ...egress, ca: 'aa'.repeat(16) } } },
      { ...document, flow: { ...flow, provider_routes: [{ ...routes[0], id: 'bb'.repeat(16) }] } },
      { ...document, credentials: { bindings: [{ ...egressBinding, secret: 'cc'.repeat(16) }] } },
    ]) expect(() => decodeConfigDocument(changed)).toThrow();
    expect(readEgressConfig(decodeConfigSnapshot(egressSnapshot))).toEqual(readEgressConfig(egressSnapshot));
  });

  it('strictly checks complete documents, consumed fields, native paths and duplicate advanced JSON keys', () => {
    for (const document of [withoutKey(configDocument, 'flow'), withoutKey(configDocument, 'log_storage'),
      { ...configDocument, log_storage: { total_bytes: 0 } }, { ...configDocument, log_storage: { total_bytes: -1 } },
      withoutKey(configDocument, 'group_observation'),
      withoutKey(configDocument, 'file_close_audit'), { ...configDocument, format_version: 1 },
      { ...configDocument, configuration_file: '/tmp/elsewhere' },
      { ...configDocument, driver: { ...configDocument.driver, self_protection: { log: false, enabled: false } } },
      { ...configDocument, queues: { ...configDocument.queues, callbacks: 0 } },
      { ...configDocument, process_tree: { ...configDocument.process_tree, reconcile_interval_ms: '1' } },
      { ...configDocument, middleware: { ...configDocument.middleware, websocket: { listen: '', origin: null, token_sha256: Array(31).fill(0) } } },
    ]) expect(() => decodeConfigDocument(document)).toThrow();
    const text = factsJson(configDocument);
    expect(decodeConfigDocument(parseUnique(text))).toEqual(configDocument);
    for (const token of ['9007199254740991.1', '9.007199254740993e15', '1e309']) {
      expect(() => decodeConfigDocument(parseUnique(text.replace('"flow": null', `"flow": {"event_queue_bytes": ${token}}`), { integerOnly: true }))).toThrow();
    }
    expect(() => decodeConfigDocument({ ...configDocument, flow: { event_queue_bytes: Infinity } })).toThrow();
    expect(() => decodeConfigDocument(parseUnique(text.replace('"log": false', '"log": false, "log": true')))).toThrow();
    const paths = { ...configSnapshot.paths, configuration_file: { encoding: 'utf16', value: [0xd800, 0] } };
    expect(decodeConfigSnapshot({ ...configSnapshot, paths }).paths.configuration_file).toEqual(paths.configuration_file);
    expect(() => decodeConfigSnapshot({ ...configSnapshot, version: Array(31).fill(9) })).toThrow();
    expect(() => decodeConfigSnapshot({ ...configSnapshot, paths: { ...paths, rules_file: { encoding: 'unix_bytes', value: [256] } } })).toThrow();
  });

  it('requires explicit independently optional Group sources with distinct nonzero u16 ports in advanced JSON', () => {
    const ports = { a2a_port: 1, otlp_http_port: 65535, gateway_ws_port: null };
    for (const group_observation of [null, ports, { a2a_port: 65535, otlp_http_port: 1, gateway_ws_port: null },
      { a2a_port: 1, otlp_http_port: null, gateway_ws_port: null },
      { a2a_port: null, otlp_http_port: 4318, gateway_ws_port: null },
      { a2a_port: null, otlp_http_port: null, gateway_ws_port: 18789 }]) {
      const document = { ...configDocument, group_observation };
      expect(decodeConfigDocument(document)).toBe(document);
      expect(decodeConfigDocument(parseUnique(factsJson(document), { integerOnly: true }))).toEqual(document);
    }
    for (const group_observation of [undefined, false, [], {}, { a2a_port: 1 }, { otlp_http_port: 65535 },
      { ...ports, extra: null }, { a2a_port: 1, otlp_http_port: 1 },
      { a2a_port: 1, otlp_http_port: 65535 },
      { a2a_port: null, otlp_http_port: null, gateway_ws_port: null },
      { a2a_port: 1, otlp_http_port: null, gateway_ws_port: 1 },
      { a2a_port: null, otlp_http_port: 65535, gateway_ws_port: 65535 }]) {
      expect(() => decodeConfigDocument({ ...configDocument, group_observation })).toThrow();
    }
    for (const field of ['a2a_port', 'otlp_http_port', 'gateway_ws_port'] as const) {
      for (const value of [-1, 0, 65536, 1.5, '1', true, 1n]) {
        expect(() => decodeConfigDocument({ ...configDocument, group_observation: { ...ports, [field]: value } })).toThrow();
      }
    }
    const text = factsJson(configDocument).replace('"group_observation": null',
      '"group_observation": {"a2a_port": 1, "a2a_port": 2, "otlp_http_port": 65535, "gateway_ws_port": null}');
    expect(() => decodeConfigDocument(parseUnique(text, { integerOnly: true }))).toThrow();
  });

  it.each([null, { a2a_port: 1, otlp_http_port: 65535, gateway_ws_port: null }])(
    'previews and saves observation changes %j with the original Update and translated receipt', group_observation => {
    const { session, socket } = setup('manage_service');
    try {
      const snapshot = readConfiguration(session, socket, { ...configSnapshot,
        document: { ...configDocument, group_observation: { a2a_port: 8001, otlp_http_port: 4318, gateway_ws_port: null } } });
      const document = decodeConfigDocument(parseUnique(factsJson({ ...snapshot.document, group_observation }), { integerOnly: true }));
      expect(changedConfigSections(snapshot.document, document)).toEqual(['group_observation']);
      const sent = socket.sent.length;
      session.saveConfiguration(snapshot, withoutKey(document, 'group_observation') as unknown as ConfigDocument);
      expect(socket.sent).toHaveLength(sent);
      session.saveConfiguration(snapshot, document);
      const request = settingsRequest(socket);
      expect(request.message.command).toEqual({ type: 'update', expected_version: snapshot.version, document });
      const data = { ...savedConfiguration, receipt: { ...savedConfiguration.receipt,
        changed_sections: ['group_observation'], required_effects: ['reinitialize_main'] } };
      socket.receive(ruleReply(request.message.id, { status: 'completed', data }));
      const state = session.getSnapshot();
      expect(state.phase).toBe('authenticated');
      expect(state.settings.receipt?.result).toEqual({ status: 'completed', data });
      for (const locale of ['ja-JP', 'zh-CN', 'en-US'] as const) {
        vi.stubGlobal('localStorage', { getItem: () => locale });
        const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(SettingsView, {
          settings: state.settings, overview: state.overview, access: state.access, session,
        }) }));
        expect(markup).toContain(translate(locale, 'settings.groupObservation'));
        expect(markup).toContain(translate(locale, 'settings.effectReinitialize'));
      }
    } finally { session.disconnect(); vi.unstubAllGlobals(); }
  });

  it.each(['settings', 'egress'])('rejects a missing observation field in the original %s Get reply', page => {
    const { session, socket } = setup('manage_service');
    session.enterSettings(); // Settings and Egress share one configuration group.
    if (page === 'egress') session.enterEgress();
    const reply = ruleReply(settingsRequest(socket).message.id, { status: 'completed', data: { type: 'configuration',
      snapshot: { ...configSnapshot, document: withoutKey(configDocument, 'group_observation') } } });
    expect(() => parseFrame(reply)).toThrow();
    socket.receive(reply);
    expect(session.getSnapshot()).toMatchObject({ phase: 'locked', settings: { snapshot: null, pending: null } });
  });

  it('discards the conflicted draft, reads a fresh snapshot and never replays the old version', () => {
    const { session, socket } = setup('manage_service');
    const original = readConfiguration(session, socket);
    session.saveConfiguration(original, changedDefaults(original));
    socket.receive(ruleReply(settingsRequest(socket).message.id, { status: 'failed', code: 'conflict', message: 'source changed', facts: null }));
    expect(session.getSnapshot().settings).toMatchObject({ phase: 'loading', snapshot: null, conflict: true });
    const request = settingsRequest(socket);
    expect(request.message.command).toEqual({ type: 'get' });
    socket.receive(ruleReply(request.message.id, { status: 'completed', data: { type: 'configuration',
      snapshot: { ...configSnapshot, version: Array(32).fill(11) } } }));
    const sent = socket.sent.length;
    session.saveConfiguration(original, changedDefaults(original)); session.reloadConfiguration(original);
    expect(socket.sent).toHaveLength(sent);
    const fresh = session.getSnapshot().settings.snapshot!;
    session.saveConfiguration(fresh, changedDefaults(fresh));
    expect(settingsRequest(socket).message.command.expected_version).toEqual(fresh.version);
    session.disconnect();
  });

  it.each(['configuration_saved', 'configuration'])('retains Failed %s facts and original code/message through a failed reread', type => {
    const { session, socket } = setup('manage_service');
    const original = readConfiguration(session, socket);
    session.saveConfiguration(original, changedDefaults(original));
    const facts = type === 'configuration_saved' ? { ...savedConfiguration, receipt: { ...savedConfiguration.receipt,
      required_effects: ['apply_credentials', 'apply_driver', 'reconnect_middleware', 'reinitialize_main'] } }
      : { type, snapshot: { ...configSnapshot, version: Array(32).fill(10) } };
    const failure = { status: 'failed', code: 'unavailable', message: 'saved; required adoption failed', facts };
    socket.receive(ruleReply(settingsRequest(socket).message.id, failure));
    expect(session.getSnapshot().settings.receipt?.result).toEqual(failure);
    socket.receive(ruleReply(settingsRequest(socket).message.id, { status: 'failed', code: 'unavailable', message: '', facts: null }));
    expect(session.getSnapshot().settings.receipt?.result).toEqual(failure);
    expect(session.getSnapshot().settings.phase).toBe('failed');
    const state = session.getSnapshot();
    const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(SettingsView, {
      settings: state.settings, overview: state.overview, access: state.access, session,
    }) }));
    expect(markup).toContain(failure.message);
    expect(markup).toContain(translate('ja-JP', type === 'configuration_saved' ? 'settings.savedPending' : 'settings.sourceRead'));
    expect(markup).not.toContain(translate('ja-JP', 'settings.driverReceipt'));
    session.disconnect();
  });

  it.each(['configuration', 'configuration_reloaded'])('accepts the actual Reload completion %s and retains it separately from the next Get', type => {
    const { session, socket } = setup('manage_service');
    const snapshot = readConfiguration(session, socket);
    session.reloadConfiguration(snapshot);
    const request = settingsRequest(socket);
    expect(request.message.command).toEqual({ type: 'reload', expected_version: snapshot.version });
    const data = { type, snapshot: configSnapshot, ...(type === 'configuration_reloaded' ? { applied: driverStatus.snapshot } : {}) };
    socket.receive(ruleReply(request.message.id, { status: 'completed', data }));
    expect(session.getSnapshot().settings.receipt?.result).toEqual({ status: 'completed', data });
    expect(settingsRequest(socket).message.command).toEqual({ type: 'get' });
    session.disconnect();
  });

  it('matches only the ordinary reply lane and exact ID; late reads are ignored but in-flight writes finish after leaving', () => {
    const { session, socket } = setup('manage_service');
    session.enterSettings();
    const readId = settingsRequest(socket).message.id, before = session.getSnapshot();
    socket.receive(businessWire(readId));
    socket.receive(ruleReply(Array(16).fill(99), { status: 'completed', data: { type: 'configuration', snapshot: configSnapshot } }));
    expect(session.getSnapshot()).toBe(before);
    session.leaveSettings();
    const left = session.getSnapshot();
    socket.receive(ruleReply(readId, { status: 'completed', data: { type: 'configuration', snapshot: configSnapshot } }));
    expect(session.getSnapshot()).toBe(left);
    const snapshot = readConfiguration(session, socket);
    session.saveConfiguration(snapshot, changedDefaults(snapshot));
    const writeId = settingsRequest(socket).message.id;
    session.leaveSettings();
    socket.receive(ruleReply(writeId, { status: 'completed', data: savedConfiguration }));
    expect(session.getSnapshot().settings).toMatchObject({ active: false, pending: null, snapshot: null,
      receipt: { id: writeId, result: { status: 'completed', data: savedConfiguration } } });
    const sent = socket.sent.length;
    socket.receive(ruleReply(writeId, { status: 'failed', code: 'failed', message: 'duplicate', facts: null }));
    expect(socket.sent).toHaveLength(sent);
    session.enterSettings();
    expect(settingsRequest(socket).message.command).toEqual({ type: 'get' });
    session.disconnect();
  });

  it('rereads configuration after returning while a real runtime control is still pending', () => {
    const { session, socket } = setup('manage_service');
    readConfiguration(session, socket);
    session.controlRuntime({ type: 'resume_runtime_feature', feature: 'file' });
    const pending = session.getSnapshot().settings.pending!, before = socket.sent.length;
    session.leaveSettings(); session.enterSettings();
    expect(session.getSnapshot().settings.pending).toBe(pending);
    expect(socket.sent).toHaveLength(before);
    socket.receive(ruleReply(pending.id, { status: 'completed', data: driverStatus }));
    expect(session.getSnapshot().settings.receipt?.id).toEqual(pending.id);
    expect(settingsRequest(socket).message.command).toEqual({ type: 'get' });
    expect(socket.sent.filter(text => JSON.parse(text).message?.type === 'control')).toHaveLength(1);
    session.disconnect();
  });

  it.each(['get', 'update', 'control'])('terminates only the same-ID output diagnostic for %s without replay', operation => {
    const { session, socket } = setup('manage_service');
    if (operation === 'get') session.enterSettings();
    else {
      const snapshot = readConfiguration(session, socket);
      if (operation === 'update') session.saveConfiguration(snapshot, changedDefaults(snapshot));
      else session.controlRuntime({ type: 'resume_runtime_feature', feature: 'file' });
    }
    const pending = session.getSnapshot().settings.pending!, before = session.getSnapshot().settings;
    const diagnostic = { type: 'reply_output_unavailable', id: pending.id, code: 'output_encoding_failed' };
    socket.receive(factsJson({ version: 2, diagnostic: { ...diagnostic, id: Array(16).fill(99) } }));
    expect(session.getSnapshot().settings).toBe(before);
    const sent = socket.sent.length;
    socket.receive(factsJson({ version: 2, diagnostic }));
    expect(session.getSnapshot().settings).toMatchObject({ phase: 'failed', pending: null, diagnostic });
    expect(socket.sent).toHaveLength(sent);
    socket.receive(ruleReply(pending.id, { status: 'completed', data: savedConfiguration }));
    expect(session.getSnapshot().settings.pending).toBeNull();
    session.disconnect();
  });

  it.each(['process', 'file', 'network'] as const)('sends temporary %s commands and preserves the actual restored disabled default', feature => {
    const { session, socket } = setup('manage_service');
    readConfiguration(session, socket);
    const commands = [{ type: 'set_runtime_feature', feature, enabled: false },
      { type: 'set_runtime_feature', feature, enabled: true }, { type: 'resume_runtime_feature', feature }] as const;
    for (const command of commands) {
      const overviewBefore = session.getSnapshot().overview;
      session.controlRuntime(command);
      const request = settingsRequest(socket, 'control');
      expect(request.message.command).toEqual(command);
      expect(socket.sent.at(-1)).toBe(runtimeCommand(request.message.id, command));
      expect(session.getSnapshot().overview).toBe(overviewBefore);
      const actual = { ...driverStatus, snapshot: { ...driverStatus.snapshot,
        [feature]: { startup_default: false, runtime_override: 'default', effective_state: 'disabled' } } };
      socket.receive(ruleReply(request.message.id, { status: 'completed', data: actual }));
      expect(session.getSnapshot().settings.receipt?.result).toEqual({ status: 'completed', data: actual });
      expect(session.getSnapshot().overview.phase).toBe('loading');
      socket.receive(businessWire(queryId(socket), overviewResult({ status: 'completed', data: actual })));
      expect(driverFacts(session.getSnapshot().overview)?.[feature]).toEqual(actual.snapshot[feature]);
    }
    session.disconnect();
  });

  it('retains any preceding accepted facts when final output is unavailable', () => {
    const { session, socket } = setup('manage_service');
    const snapshot = readConfiguration(session, socket);
    session.saveConfiguration(snapshot, changedDefaults(snapshot));
    const id = settingsRequest(socket).message.id;
    const accepted = { status: 'accepted', data: savedConfiguration };
    socket.receive(ruleReply(id, accepted));
    socket.receive(factsJson({ version: 2, diagnostic: { type: 'reply_output_unavailable', id, code: 'output_too_large' } }));
    expect(session.getSnapshot().settings).toMatchObject({ pending: null, phase: 'failed',
      receipt: { result: accepted, problem: { key: 'settings.unconfirmed' } } });
    session.disconnect();
  });

  it('locks away old drafts and callbacks on disconnect and never replays a write after authentication', () => {
    const { session, socket, sockets } = setup('manage_service');
    const snapshot = readConfiguration(session, socket);
    session.saveConfiguration(snapshot, changedDefaults(snapshot));
    const oldId = settingsRequest(socket).message.id, late = socket.onmessage!;
    socket.onclose?.();
    expect(session.getSnapshot().settings).toMatchObject({ active: false, snapshot: null, receipt: null, pending: null });
    session.connect(testToken);
    const next = sockets[1]!; next.open();
    next.receive(JSON.stringify({ version: 2, authenticated: { access: 'manage_service', chat_content: false } }));
    session.enterSettings();
    const before = session.getSnapshot();
    session.saveConfiguration(snapshot, changedDefaults(snapshot));
    late({ data: ruleReply(oldId, { status: 'completed', data: savedConfiguration }) });
    next.receive(ruleReply(oldId, { status: 'completed', data: savedConfiguration }));
    expect(session.getSnapshot()).toBe(before);
    expect(next.sent.filter(text => (parseUnique(text) as { message?: { type: string } }).message?.type === 'configuration')).toHaveLength(1);
    expect(settingsRequest(next).message.command).toEqual({ type: 'get' });
    session.disconnect();
  });
});

describe('lossless JSON boundary', () => {
  it.each(['-0', '-0.0', '1.0', '0.10000000000000001', '9.007199254740993e15', '1E+04', '1e400', '1e-400'])(
    'preserves opaque numeric lexeme %s while rejecting it as a structured integer or DTO', token => {
      const value = parseUnique(token);
      expect(wireJson(value)).toBe(token);
      expect(factsJson({ value })).toBe('{\n  "value": ' + token + '\n}');
      expect(sameJsonValue(value, parseUnique(token))).toBe(true);
      expect(sameJsonValue(value, {})).toBe(false);
      expect(sameJsonValue(value, Number(token))).toBe(false);
      expect(() => record(value)).toThrow();
      expect(() => integer(value, 0, Number.MAX_SAFE_INTEGER)).toThrow();
      expect(() => numericU64(value)).toThrow();
      expect(() => parseFrame(wire(ask).replace('"remaining_ms":5000', '"remaining_ms":' + token))).toThrow();
    });
  it('distinguishes numeric lexemes and rejects non-JSON programmatic numbers', () => {
    expect(sameJsonValue(parseUnique('1.0'), parseUnique('1e0'))).toBe(false);
    expect(sameJsonValue(parseUnique('-0'), parseUnique('0'))).toBe(false);
    for (const value of [Infinity, -Infinity, NaN]) expect(() => wireJson(value)).toThrow();
    expect(wireJson(-0)).toBe('-0');
    expect(parseUnique('9007199254740991')).toBe(9007199254740991);
    expect(wireJson(parseUnique('[0,42,-42,9007199254740991,18446744073709551615]')))
      .toBe('[0,42,-42,9007199254740991,18446744073709551615]');
  });
  it.each(['[1,]', '{"k":1,}', '{"k" 1}', '[1 2]', 'true false', '{"k":01}', '[1e]', '\u00a0null',
    '{"k":1,"\\u006b":2}', '"\\q"', '{"k":"unterminated}'])('rejects invalid or ambiguous syntax %j', text => {
    expect(() => parseUnique(text)).toThrow();
  });

  it('preserves unsafe integer lexemes and own property names in one parse', () => {
    const decoded = parseUnique('{"__proto__":{"v":1},"pos":18446744073709551615,"neg":-9007199254740993,"empty":[]}') as Record<string, unknown>;
    expect(Object.getPrototypeOf(decoded)).toBe(Object.prototype);
    expect(Object.hasOwn(decoded, '__proto__')).toBe(true);
    expect(decoded.pos).toBe(18446744073709551615n);
    expect(decoded.neg).toBe(-9007199254740993n);
    expect(decoded.empty).toEqual([]);
  });
});

it('retains an unexpected same-ID setting result only as unconfirmed evidence', () => {
  const { session, socket } = setup('manage_service');
  const snapshot = readConfiguration(session, socket);
  session.saveConfiguration(snapshot, changedDefaults(snapshot));
  const request = settingsRequest(socket);
  socket.receive(ruleReply(request.message.id, { status: 'completed', data: driverStatus }));
  const state = session.getSnapshot().settings;
  expect(state.receipt).toMatchObject({ operation: 'update', problem: { key: 'settings.unconfirmed' } });
  expect(state.snapshot).toBeNull();
  expect(settingsRequest(socket).message.command).toEqual({ type: 'get' });
  session.disconnect();
});

// Private-management supply is entirely synthetic; no service, CA creation or real key.
const privateReply = (commandId: readonly number[], result: unknown) => JSON.stringify({ version: 2, private_reply: { id: commandId, result } });
function privateRequest(socket: FakeSocket) {
  return JSON.parse(socket.sent.filter(text => Object.hasOwn(JSON.parse(text), 'private')).at(-1)!) as {
    version: number; private: { id: readonly number[]; command: Record<string, unknown> };
  };
}
function privateSetup() {
  const context = setup('manage_service');
  const snapshot = readConfiguration(context.session, context.socket, egressSnapshot);
  context.session.enterEgress();
  return { ...context, snapshot, binding: readEgressConfig(snapshot).bindings[0]! };
}

describe('credential private lane on the original authenticated connection', () => {
  it('borrows only explicitly configured process or container subjects for the setup form', () => {
    const withScope = (scope: unknown) => readEgressConfig({ document: { ...egressSnapshot.document,
      flow: { ...egressSnapshot.document.flow!, scope } } });
    const process = trackedRecord().key;
    const processSubjects = configuredSubjects(withScope({ kind: 'processes', values: [process] }));
    expect(processSubjects).toEqual([{ kind: 'process', value: process }]);
    expect(processSubjects[0]!.value).toBe(process);
    const containerSubjects = configuredSubjects(withScope({ kind: 'workloads', values: [{ kind: 'container', instance: containerInstance }] }));
    expect(containerSubjects).toEqual([{ kind: 'container', value: containerInstance }]);
    expect(containerSubjects[0]!.value).toBe(containerInstance);
    expect(configuredSubjects(withScope({ kind: 'all_protected' }))).toEqual([]);
  });
  function readCa() {
    const supply = privateSetup();
    supply.session.credentialCommand({ type: 'public_ca', ca: syntheticCa.id });
    supply.socket.receive(privateReply(privateRequest(supply.socket).private.id,
      { status: 'completed', data: { type: 'ca', info: syntheticCa } }));
    return { ...supply, ca: supply.session.getCredentials().ca! };
  }
  it('binds DER to the exact stable configuration read and requires a fresh CA read even when its ID is unchanged', () => {
    const { session, socket, snapshot, ca } = readCa();
    expect(session.getConfiguredCa(snapshot, ca)).toBe(ca);
    expect(session.getConfiguredCa({ ...snapshot }, ca)).toBeNull();
    expect(session.getConfiguredCa(snapshot, { ...ca })).toBeNull();
    session.refreshSettings();
    expect(session.getConfiguredCa(snapshot, ca)).toBeNull();
    socket.receive(ruleReply(settingsRequest(socket).message.id,
      { status: 'completed', data: { type: 'configuration', snapshot: egressSnapshot } }));
    const current = session.getSnapshot().settings.snapshot!;
    expect(session.getConfiguredCa(current, ca)).toBeNull();
    session.credentialCommand({ type: 'public_ca', ca: syntheticCa.id });
    socket.receive(privateReply(privateRequest(socket).private.id,
      { status: 'completed', data: { type: 'ca', info: syntheticCa } }));
    expect(session.getConfiguredCa(current, session.getCredentials().ca)).not.toBeNull();
    expect(session.getConfiguredCa(snapshot, ca)).toBeNull();
    session.disconnect();
  });
  it.each(['failure', 'changed-id', 'revoked', 'leave-return', 'disconnect'] as const)(
    'revokes old CA download on %s without discarding its historical identity', action => {
      const { session, socket, snapshot, ca } = readCa();
      if (action === 'failure' || action === 'changed-id') {
        session.refreshSettings();
        const changed = decodeConfigSnapshot({ ...egressSnapshot, document: { ...egressSnapshot.document,
          flow: { ...egressSnapshot.document.flow, egress: { ...record(egressSnapshot.document.flow!.egress), ca: Array(16).fill(41) } } } });
        socket.receive(ruleReply(settingsRequest(socket).message.id, action === 'failure'
          ? { status: 'failed', code: 'unavailable', message: 'fixture', facts: null }
          : { status: 'completed', data: { type: 'configuration', snapshot: changed } }));
      } else if (action === 'revoked') {
        // No private operation is pending: idle revocation still invalidates the certificate.
        socket.receive(JSON.stringify({ version: 2, diagnostic: { type: 'private_not_authorized', code: 'unauthorized' } }));
        expect(session.getCredentials().revoked).toBe(true);
      } else if (action === 'leave-return') { session.leaveEgress(); session.enterEgress(); }
      else session.disconnect();
      const sent = socket.sent.length;
      expect(session.getConfiguredCa(snapshot, ca)).toBeNull();
      expect(session.getConfiguredCa(session.getSnapshot().settings.snapshot, session.getCredentials().ca)).toBeNull();
      if (action !== 'disconnect') expect(session.getCredentials().ca).toBe(ca);
      expect(socket.sent).toHaveLength(sent);
      session.disconnect();
    });
  it('does not bind a late CA across a configuration reread or leave, and Ensure CA does not configure a new ID', () => {
    for (const action of ['reread', 'leave', 'new-id', 'same-id'] as const) {
      const { session, socket, snapshot } = privateSetup();
      session.credentialCommand({ type: 'ensure_ca' });
      const request = privateRequest(socket).private;
      if (action === 'reread') {
        session.refreshSettings();
        socket.receive(ruleReply(settingsRequest(socket).message.id,
          { status: 'completed', data: { type: 'configuration', snapshot: egressSnapshot } }));
      } else if (action === 'leave') { session.leaveEgress(); session.enterEgress(); }
      const sent = socket.sent.length;
      socket.receive(privateReply(request.id, { status: 'completed', data: { type: 'ca',
        info: action === 'new-id' ? { ...syntheticCa, id: Array(16).fill(99) } : syntheticCa } }));
      const ca = session.getCredentials().ca;
      expect(session.getConfiguredCa(session.getSnapshot().settings.snapshot, ca) !== null).toBe(action === 'same-id');
      if (action !== 'reread') expect(session.getSnapshot().settings.snapshot).toBe(snapshot);
      expect(socket.sent).toHaveLength(sent);
      session.disconnect();
    }
  });
  it.each(['leave', 'leave-return', 'close', 'lock'] as const)('discards late issuance after %s without a resend', action => {
    const { session, socket, snapshot, binding } = privateSetup();
    session.issuePhantom(snapshot, binding);
    const pending = privateRequest(socket).private.id, late = socket.onmessage!;
    const before = socket.sent.length;
    if (action === 'leave' || action === 'leave-return') session.leaveEgress();
    if (action === 'leave-return') session.enterEgress();
    if (action === 'close') session.discardPhantom();
    if (action === 'lock') session.disconnect();
    late({ data: privateReply(pending, { status: 'completed', data: phantomData }) });
    expect(session.readPhantomValue()).toBeNull();
    expect(session.getCredentials().hasValue).toBe(false);
    expect(factsJson(session.getCredentials())).not.toContain(syntheticPhantom);
    expect(socket.sent).toHaveLength(before);
    if (action === 'lock') expect(session.getSnapshot().notice?.key).toBe('egress.unconfirmed');
    else expect(session.getCredentials().issued?.token_id).toEqual(phantomData.token_id);
    session.disconnect();
  });

  it('clears a received value on a new operation or close, and copies only on an explicit request', async () => {
    const { session, socket, snapshot, binding } = privateSetup();
    const clipboard = vi.fn(async (_value: string) => {});
    session.issuePhantom(snapshot, binding);
    socket.receive(privateReply(privateRequest(socket).private.id, { status: 'completed', data: phantomData }));
    expect(clipboard).not.toHaveBeenCalled();
    expect(await session.copyPhantomValue(clipboard)).toBe(true);
    expect(clipboard).toHaveBeenCalledWith(syntheticPhantom);
    session.credentialCommand({ type: 'ensure_ca' });
    expect(session.readPhantomValue()).toBeNull();
    socket.receive(privateReply(privateRequest(socket).private.id, { status: 'failed', code: 'busy' }));
    session.issuePhantom(snapshot, binding);
    socket.receive(privateReply(privateRequest(socket).private.id, { status: 'completed', data: phantomData }));
    session.discardPhantom();
    expect(await session.copyPhantomValue(clipboard)).toBe(false);
    expect(clipboard).toHaveBeenCalledTimes(1);
    session.disconnect();
  });

  it('matches no-ID permission revocation to the unique private slot while leaving the config Get intact', () => {
    const { session, socket, snapshot, binding } = privateSetup();
    session.issuePhantom(snapshot, binding);
    const pendingId = privateRequest(socket).private.id;
    session.refreshSettings();
    const settingsPending = session.getSnapshot().settings.pending;
    socket.receive(JSON.stringify({ version: 2, diagnostic: { type: 'private_not_authorized', code: 'unauthorized' } }));
    expect(session.getCredentials()).toMatchObject({ pending: null, revoked: true, problem: { key: 'egress.permissionRevoked' } });
    expect(session.getSnapshot().settings.pending).toBe(settingsPending);
    const before = socket.sent.length;
    session.credentialCommand({ type: 'ensure_ca' });
    socket.receive(privateReply(pendingId, { status: 'completed', data: phantomData }));
    expect(session.readPhantomValue()).toBeNull();
    expect(socket.sent).toHaveLength(before);
    session.disconnect();
  });

  it('treats matching output loss and transport failure as uncertain without erasing earlier receipts', () => {
    const { session, socket } = privateSetup();
    session.credentialCommand({ type: 'import_secret' }, syntheticSecret);
    socket.receive(privateReply(privateRequest(socket).private.id, { status: 'completed', data: { type: 'secret_saved', receipt: { secret: id, revision: '9' } } }));
    session.credentialCommand({ type: 'replace_secret', secret: id, expected_revision: '9' }, syntheticSecret);
    const pendingId = privateRequest(socket).private.id;
    socket.receive(JSON.stringify({ version: 2, diagnostic: { type: 'reply_output_unavailable', id: Array(16).fill(99), code: 'output_too_large' } }));
    expect(session.getCredentials().pending?.id).toEqual(pendingId);
    const before = socket.sent.length;
    socket.receive(JSON.stringify({ version: 2, diagnostic: { type: 'reply_output_unavailable', id: pendingId, code: 'output_too_large' } }));
    expect(session.getCredentials()).toMatchObject({ pending: null, saved: { secret: id, revision: '9' }, problem: { key: 'egress.unconfirmed' } });
    expect(socket.sent).toHaveLength(before);
    socket.failSend = true;
    session.credentialCommand({ type: 'replace_secret', secret: id, expected_revision: '9' }, syntheticSecret);
    expect(session.getCredentials()).toMatchObject({ pending: null, saved: { secret: id, revision: '9' }, problem: { key: 'egress.unconfirmed' } });
    session.disconnect();
  });

  it.each([null, { a2a_port: 8001, otlp_http_port: 4318, gateway_ws_port: null }])(
    'uses the same settings owner and in-flight Get across settings/egress switches with Group settings %j', group_observation => {
    const { session, socket } = setup('manage_service');
    session.enterSettings();
    const pending = session.getSnapshot().settings.pending;
    const before = socket.sent.length;
    session.enterEgress(); session.enterSettings(); session.leaveEgress(); session.enterSettings(); session.enterEgress();
    expect(socket.sent).toHaveLength(before);
    expect(session.getSnapshot().settings.pending).toBe(pending);
    socket.receive(ruleReply(pending!.id, { status: 'completed', data: { type: 'configuration',
      snapshot: { ...egressSnapshot, document: { ...egressSnapshot.document, group_observation } } } }));
    const snapshot = session.getSnapshot().settings.snapshot;
    expect(session.getSnapshot().phase).toBe('authenticated');
    expect(session.getSnapshot().settings.phase).toBe('ready');
    expect(snapshot?.document.group_observation).toEqual(group_observation);
    session.leaveEgress(); session.enterSettings(); session.enterEgress();
    expect(session.getSnapshot().settings.snapshot).toBe(snapshot);
    expect(socket.sent).toHaveLength(before);
    session.disconnect();
  });

  it('clears an already received value on egress leave while retaining the group configuration', () => {
    const { session, socket, snapshot, binding } = privateSetup();
    session.issuePhantom(snapshot, binding);
    socket.receive(privateReply(privateRequest(socket).private.id, { status: 'completed', data: phantomData }));
    expect(session.readPhantomValue()).toBe(syntheticPhantom);
    const before = socket.sent.length;
    session.leaveEgress(); session.enterSettings();
    expect(session.readPhantomValue()).toBeNull();
    expect(session.getSnapshot().settings.snapshot).toBe(snapshot);
    session.enterEgress();
    expect(session.getCredentials().hasValue).toBe(false);
    expect(socket.sent).toHaveLength(before);
    session.leaveEgress(); session.leaveSettings();
    expect(session.getSnapshot().settings).toMatchObject({ active: false, snapshot: null, pending: null });
    session.disconnect();
  });

  it('keeps provider input out of every published snapshot and preserves original rotation reference/version', () => {
    const { session, socket } = privateSetup();
    const published: string[] = [];
    const capture = () => { published.push(factsJson({ global: session.getSnapshot(), credentials: session.getCredentials() })); };
    const off = session.subscribe(capture), offPrivate = session.subscribeCredentials(capture);
    session.credentialCommand({ type: 'import_secret' }, syntheticSecret);
    const request = privateRequest(socket).private;
    expect(request.command).toEqual({ type: 'import_secret', value: syntheticSecret });
    expect(session.getCredentials().pending?.operation).toEqual({ type: 'import_secret' });
    const saved = { type: 'secret_saved', receipt: { secret: id, revision: '9007199254740993' } };
    socket.receive(privateReply(request.id, { status: 'completed', data: saved }));
    session.credentialCommand({ type: 'replace_secret', secret: id, expected_revision: '9007199254740993' }, syntheticSecret);
    expect(privateRequest(socket).private.command).toEqual({ type: 'replace_secret', secret: id,
      expected_revision: '9007199254740993', value: syntheticSecret });
    socket.receive(privateReply(privateRequest(socket).private.id, { status: 'failed', code: 'persistence_uncertain' }));
    expect(session.getCredentials().saved).toEqual(saved.receipt);
    expect(session.getCredentials().receipt).toMatchObject({ code: 'persistence_uncertain', problem: { key: 'egress.unconfirmed' } });
    expect(published.join('')).not.toContain(syntheticSecret);
    expect(session.getCredentials().receipt?.operation).not.toHaveProperty('value');
    expect(session.getSnapshot()).not.toHaveProperty('credentials');
    off(); offPrivate(); session.disconnect();
  });

  it('requires the exact private lane, current ID, response type and original object reference', () => {
    const { session, socket } = privateSetup();
    session.credentialCommand({ type: 'replace_secret', secret: id, expected_revision: '9' }, syntheticSecret);
    const current = privateRequest(socket).private.id;
    socket.receive(ruleReply(current, { status: 'failed', code: 'unavailable', message: 'fixture', facts: null }));
    socket.receive(businessWire(current, { status: 'failed', code: 'unavailable', message: 'fixture' }));
    socket.receive(privateReply(Array(16).fill(99), { status: 'failed', code: 'busy' }));
    expect(session.getCredentials().pending?.id).toEqual(current);
    const before = socket.sent.length;
    socket.receive(privateReply(current, { status: 'completed', data: { type: 'secret_saved', receipt: { secret: Array(16).fill(99), revision: '10' } } }));
    expect(session.getCredentials()).toMatchObject({ pending: null, saved: null, problem: { key: 'egress.unconfirmed' } });
    expect(socket.sent).toHaveLength(before);
    session.credentialCommand({ type: 'public_ca', ca: Array(16).fill(25) });
    socket.receive(privateReply(privateRequest(socket).private.id, { status: 'completed', data: {
      type: 'ca', info: { id: Array(16).fill(99), certificate_der: [], fingerprint_sha256: Array(32).fill(1) },
    } }));
    expect(session.getCredentials().ca).toBeNull();
    session.credentialCommand({ type: 'ensure_ca' });
    socket.receive(privateReply(privateRequest(socket).private.id, { status: 'completed', data: { type: 'phantom_revoked', revision: '10' } }));
    expect(session.getCredentials().receipt?.data).toBeNull();
    session.disconnect();
  });

  it('issues only for a current enabled binding and records its original non-secret scope/version', () => {
    const { session, socket, snapshot, binding } = privateSetup();
    const before = socket.sent.length;
    session.issuePhantom({ ...snapshot }, binding);
    session.issuePhantom(snapshot, { ...binding });
    session.issuePhantom(snapshot, readEgressConfig(snapshot).bindings[1]!);
    session.credentialCommand({ type: 'issue_phantom', binding: binding.id, expected_config: snapshot.version });
    expect(socket.sent).toHaveLength(before);
    session.issuePhantom(snapshot, binding);
    const request = privateRequest(socket).private;
    expect(request.command).toEqual({ type: 'issue_phantom', binding: binding.id, expected_config: snapshot.version });
    expect(session.getCredentials().pending?.scope).toEqual({ binding, routes: [egressRouteFact] });
    socket.receive(privateReply(request.id, { status: 'completed', data: phantomData }));
    expect(session.getCredentials().receipt?.scope?.binding).toBe(binding);
    expect(session.getCredentials().receipt?.scope?.routes[0]).toEqual(egressRouteFact);
    expect(session.readPhantomValue()).toBe(syntheticPhantom);
    expect(session.getCredentials().issued).not.toHaveProperty('value');
    expect(factsJson(session.getCredentials())).not.toContain(syntheticPhantom);
    expect(factsJson(session.getSnapshot())).not.toContain(syntheticPhantom);
    const s = session.getSnapshot();
    const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(EgressView, {
      settings: s.settings, overview: s.overview, access: s.access, session, askVisible: false,
    }) }));
    expect(markup).toContain('type="password"');
    expect(markup).not.toContain(syntheticPhantom);
    expect(markup).toContain('fixture-instance');
    expect(markup).toContain('/v1/%72esponses');
    session.disconnect();
  });

  it('strictly decodes all four current data types and finite failure codes without message', () => {
    const data = [
      { type: 'ca', info: { id, certificate_der: [], fingerprint_sha256: Array(32).fill(1) } },
      { type: 'secret_saved', receipt: { secret: id, revision: '9007199254740993' } },
      phantomData, { type: 'phantom_revoked', revision: '18446744073709551615' },
    ];
    for (const d of data) expect(parseFrame(privateReply(id, { status: 'completed', data: d }))).toEqual({
      kind: 'private_reply', reply: { id, result: { status: 'completed', data: d } },
    });
    for (const code of credentialErrors) expect(parseFrame(privateReply(id, { status: 'failed', code }))).toMatchObject({
      kind: 'private_reply', reply: { result: { status: 'failed', code } },
    });
    for (const result of [{ status: 'failed', code: 'unknown' }, { status: 'failed', code: 'busy', message: syntheticSecret },
      { status: 'accepted', data: data[0] }, { status: 'completed', data: { type: 'secret_saved', receipt: { secret: id, revision: 1 } } },
      { status: 'completed', data: { ...phantomData, binding: Array(15).fill(1) } },
      { status: 'completed', data: { ...phantomData, extra: syntheticSecret } },
      { status: 'completed', data: { type: 'ca', info: { id, certificate_der: [256], fingerprint_sha256: Array(32).fill(1) } } },
    ]) expect(() => parseFrame(privateReply(id, result))).toThrow();
    expect(() => parseFrame(JSON.stringify({ version: 2, private_reply: { id, result: { status: 'failed', code: 'busy' } }, diagnostic: {} }))).toThrow();
    expect(() => parseFrame(privateReply(Array(15).fill(0), { status: 'failed', code: 'busy' }))).toThrow();
    expect(() => parseFrame(`{"version":2,"private_reply":{"id":${JSON.stringify(id)},"result":{"status":"failed","code":"busy","code":"busy"}}}`)).toThrow();
    expect(() => parseFrame(JSON.stringify({ version: 2, diagnostic: { type: 'private_not_authorized', id, code: 'unauthorized' } }))).toThrow();
  });

  it('preserves input bytes and original decimal revisions in only the exact private command', () => {
    expect(MAX_SECRET_BYTES).toBe(16384);
    expect(validSecret(' '.repeat(MAX_SECRET_BYTES))).toBe(true);
    for (const value of ['', 'a\rb', 'a\nb', 'a\0b', 'x'.repeat(MAX_SECRET_BYTES + 1), '界'.repeat(5462)]) {
      expect(validSecret(value)).toBe(false);
      expect(() => privateCommand(id, { type: 'import_secret' }, value)).toThrow();
    }
    const operation: PrivateOperation = { type: 'replace_secret', secret: id, expected_revision: '18446744073709551615' };
    expect(JSON.parse(privateCommand(id, operation, syntheticSecret))).toEqual({ version: 2,
      private: { id, command: { ...operation, value: syntheticSecret } } });
    expect(() => privateCommand(id, { ...operation, expected_revision: '01' }, syntheticSecret)).toThrow();
    expect(() => privateCommand(id, { type: 'ensure_ca' }, syntheticSecret)).toThrow();
  });

  it.each(['read_only', 'manage_rules'])('does not send any private operation for %s', access => {
    const { session, socket } = setup(access), snapshot = readConfiguration(session, socket, egressSnapshot);
    session.enterEgress();
    const before = socket.sent.length;
    const operations: PrivateOperation[] = [{ type: 'ensure_ca' }, { type: 'public_ca', ca: Array(16).fill(25) },
      { type: 'import_secret' }, { type: 'replace_secret', secret: id, expected_revision: '9' },
      { type: 'issue_phantom', binding: egressBinding.id, expected_config: snapshot.version }, { type: 'revoke_phantom', token: id }];
    for (const op of operations) session.credentialCommand(op, op.type === 'import_secret' || op.type === 'replace_secret' ? syntheticSecret : undefined);
    session.issuePhantom(snapshot, readEgressConfig(snapshot).bindings[0]!);
    expect(socket.sent).toHaveLength(before);
    const s = session.getSnapshot();
    const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(EgressView, {
      settings: s.settings, overview: s.overview, access: s.access, session, askVisible: false,
    }) }));
    expect(markup).toContain(egressRouteFact.origin);
    expect(markup).not.toContain('id="egress-provider-secret"');
    session.disconnect();
  });
});

// Exact Process Get: same owner and one read slot as the List fixtures above.
const detailsReply = (commandId: readonly number[], details = instanceDetails()) => ruleReply(commandId, {
  status: 'completed', data: { type: 'processes', result: { type: 'details', data: details } },
});

describe('Group exact ProcessKey navigation through the single Processes read slot', () => {
  it('keeps the original Group endpoint as a connection-local target and sends only Get on entry', () => {
    const { session, socket, sockets } = setup('read_only', true);
    session.enterGroupDelegations(); socket.receive(businessWire(queryId(socket), groupResult()));
    const row = groupPage(session.getSnapshot().group)!.records[0]!;
    if (row.kind !== 'a2a') throw Error('fixture');
    const sent = socket.sent.length;
    session.openProcess(row.child);
    expect(socket.sent).toHaveLength(sent);
    expect(session.getSnapshot().processes.query).toEqual({ type: 'get', key: row.child });
    if (session.getSnapshot().processes.query.type === 'get')
      expect((session.getSnapshot().processes.query as { key: unknown }).key).toBe(row.child);
    session.leaveGroupDelegations(); session.enterProcesses();
    const command = processCommand(socket);
    expect(JSON.parse(socket.sent.at(-1)!)).toEqual({ version: 2, message: { type: 'processes', id: command.id,
      command: { type: 'query', query: { type: 'get', key: row.child } } } });
    expect(JSON.parse(processesCommand(command.id, { type: 'get', key: row.child }))).toEqual(JSON.parse(socket.sent.at(-1)!));
    session.enterProcesses(); expect(socket.sent).toHaveLength(sent + 1);
    const record = { ...trackedRecord(row.child.birth_boottime_ns), key: row.child };
    const details = instanceDetails(record, '9');
    socket.receive(detailsReply(command.id, details));
    const read = session.getSnapshot().processes, returned = processDetails(read)!;
    expect(returned.record).toEqual(record); expect(returned.evidence_epoch).toBe('9');
    expect(observedValue(returned.record, 'name', returned.evidence_epoch)).toBeNull(); // No borrowed List epoch.
    expect(returned.record.latest.process.argv).toEqual([[], [255]]);
    const result = read.receipt!.result;
    if (result.status !== 'completed' || result.data.type !== 'processes' || result.data.result.type !== 'details') throw Error('fixture');
    expect(returned).toBe(result.data.result.data); expect(read.page).toBeNull();
    expect(sockets).toHaveLength(1); expect(session.getSnapshot().reviewDeclared).toBe(false);
    for (const locale of ['ja-JP', 'zh-CN', 'en-US'] as const) {
      vi.stubGlobal('localStorage', { getItem: () => locale });
      const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(ProcessesView, { read, session }) }));
      expect(markup).toContain(translate(locale, 'processes.getDetailNote'));
      expect(markup).toContain(translate(locale, 'processes.oldEpoch'));
      expect(markup).toContain(row.child.birth_boottime_ns);
      expect(markup).not.toContain(translate(locale, 'processes.listDetailNote'));
      vi.unstubAllGlobals();
    }
    session.disconnect();
  });

  it.each(['boot', 'birth', 'tgid'] as const)('rejects a valid Details record with the wrong full key component: %s', component => {
    const { session, socket } = setup('read_only', false);
    const original = trackedRecord(); session.openProcess(original.key); session.enterProcesses();
    const key = { ...original.key, ...(component === 'boot' ? { boot_id: Array(16).fill(8) }
      : component === 'birth' ? { birth_boottime_ns: '18446744073709551614' } : { host_tgid: original.key.host_tgid + 1 }) };
    const sent = socket.sent.length;
    socket.receive(detailsReply(processCommand(socket).id, instanceDetails({ ...original, key })));
    const read = session.getSnapshot().processes;
    expect(read).toMatchObject({ phase: 'failed', pending: null, page: null, query: { type: 'get', key: original.key },
      problem: { key: 'processes.wrongInstance' } });
    expect(processDetails(read)).toBeNull(); expect(socket.sent).toHaveLength(sent);
    expect(read.receipt?.result.status).toBe('completed'); // Original wrong-key fact remains inspectable, never selected.
    session.reloadProcess(); expect(socket.sent).toHaveLength(sent + 1);
    expect(JSON.parse(socket.sent.at(-1)!).message.command.query).toEqual({ type: 'get', key: original.key });
    socket.receive(detailsReply(processCommand(socket).id, instanceDetails(original)));
    expect(processDetails(session.getSnapshot().processes)?.record.key).toEqual(original.key);
    session.disconnect();
  });

  it('discriminates List versus Get, replaces old IDs and ignores business-lane data with the same ID', () => {
    const { session, socket } = setup('read_only', false);
    session.enterProcesses(); const list = processCommand(socket).id;
    expect(JSON.parse(socket.sent.at(-1)!).message.command.query.type).toBe('list');
    session.openProcess(trackedRecord().key); const get = processCommand(socket).id, pending = session.getSnapshot().processes;
    socket.receive(processReply(list)); socket.receive(businessWire(get));
    expect(session.getSnapshot().processes).toBe(pending);
    socket.receive(processReply(get)); expect(session.getSnapshot().processes.problem?.key).toBe('processes.wrongReply');
    session.refreshProcesses(); const nextList = processCommand(socket).id;
    socket.receive(detailsReply(nextList)); expect(session.getSnapshot().processes.problem?.key).toBe('processes.wrongReply');
    session.openProcess(trackedRecord().key); const old = processCommand(socket).id;
    session.openProcess(trackedRecord('2').key); const current = processCommand(socket).id;
    const state = session.getSnapshot().processes;
    socket.receive(detailsReply(old)); expect(session.getSnapshot().processes).toBe(state);
    socket.receive(detailsReply(current, instanceDetails(trackedRecord('2'))));
    expect(processDetails(session.getSnapshot().processes)?.record.key.birth_boottime_ns).toBe('2');
    session.disconnect();
  });

  it('clears the target and pending on leaving, re-enters with List, and does not revive old-socket Get', () => {
    const { session, socket, sockets } = setup('read_only', false);
    session.openProcess(trackedRecord().key); session.enterProcesses(); const old = processCommand(socket).id;
    session.clearProcesses(); const left = session.getSnapshot();
    socket.receive(detailsReply(old)); expect(session.getSnapshot()).toBe(left);
    expect(session.getSnapshot().processes.query.type).toBe('list');
    session.enterProcesses(); const list = processCommand(socket).id, pending = session.getSnapshot();
    expect(JSON.parse(socket.sent.at(-1)!).message.command.query.type).toBe('list');
    socket.receive(detailsReply(old)); expect(session.getSnapshot()).toBe(pending);
    socket.receive(processReply(list)); expect(session.getSnapshot().processes.page).not.toBeNull();
    session.openProcess(trackedRecord().key); const oldGet = processCommand(socket).id, callback = socket.onmessage!;
    session.disconnect(); session.connect(testToken); const next = sockets[1]!; next.open();
    next.receive(JSON.stringify({ version: 2, authenticated: { access: 'read_only', chat_content: false } }));
    session.enterProcesses(); const current = session.getSnapshot();
    callback({ data: detailsReply(oldGet) }); next.receive(detailsReply(oldGet));
    expect(session.getSnapshot()).toBe(current); expect(session.getSnapshot().processes.query.type).toBe('list');
    session.disconnect();
  });

  it('preserves not_found, output and send failures without falling back to List or guessing a successor', () => {
    const { session, socket } = setup('read_only', false);
    session.openProcess(trackedRecord().key); session.enterProcesses();
    const command = processCommand(socket).id, sent = socket.sent.length;
    socket.receive(ruleReply(command, { status: 'failed', code: 'not_found', message: 'original instance not retained', facts: null }));
    expect(session.getSnapshot().processes).toMatchObject({ phase: 'failed', pending: null, query: { type: 'get' } });
    expect(processDetails(session.getSnapshot().processes)).toBeNull(); expect(socket.sent).toHaveLength(sent);
    const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(ProcessesView, {
      read: session.getSnapshot().processes, session,
    }) }));
    expect(markup).toContain(translate('ja-JP', 'processes.instanceNotFound'));
    session.reloadProcess(); const next = processCommand(socket).id;
    socket.receive(JSON.stringify({ version: 2, diagnostic: { type: 'reply_output_unavailable', id: next, code: 'output_too_large' } }));
    expect(session.getSnapshot().processes).toMatchObject({ phase: 'failed', pending: null, problem: { key: 'processes.outputFailed' } });
    socket.failSend = true; session.reloadProcess();
    expect(session.getSnapshot().processes).toMatchObject({ phase: 'failed', pending: null, query: { type: 'get' }, problem: { key: 'processes.sendFailed' } });
    session.disconnect();
  });

  it('uses the existing deep ProcessRecord parser and rejects malformed exact Details rather than retaining an opaque record', () => {
    const details = instanceDetails();
    for (const data of [withoutKey(details, 'record'), { ...details, evidence_epoch: 0 },
      { ...details, record: { ...details.record, key: { ...details.record.key, birth_boottime_ns: 1 } } },
      { ...details, record: { ...details.record, latest: { ...details.record.latest, process: { ...details.record.latest.process, name: 'not bytes' } } } },
      { ...details, children: -1 }])
      expect(() => parseFrame(ruleReply(id, { status: 'completed', data: { type: 'processes', result: { type: 'details', data } } }))).toThrow();
    const { session, socket } = setup('read_only'); const before = session.getSnapshot(), sent = socket.sent.length;
    session.openProcess({ ...trackedRecord().key, boot_id: [1] });
    expect(session.getSnapshot()).toBe(before); expect(socket.sent).toHaveLength(sent);
    session.disconnect();
  });
});

// Original contract-shaped chat supply; no service, Agent or external content is used.
const historyReply = (commandId: unknown, result: unknown) => wireJson({ version: 2, business_reply: { id: commandId, result } });

describe('chat history original exchange and message lifecycle', () => {
  it('opens a current Group receipt by its complete exchange key, then uses the one existing message page and cursor', () => {
    const { session, socket, sockets } = setup('read_only', true);
    session.enterGroupDelegations();
    socket.receive(businessWire(queryId(socket), groupResult({ records: [delegationRow('10'), historyReceipt()], next_before: null })));
    const row = groupPage(session.getSnapshot().group)!.records[1]!, sent = socket.sent.length;
    expect(session.openGroupReceipt({ ...row })).toBe(false);
    for (const locale of ['ja-JP', 'zh-CN', 'en-US'] as const) {
      vi.stubGlobal('localStorage', { getItem: () => locale });
      try {
        const render = (chatContent: boolean) => renderToStaticMarkup(createElement(I18nProvider, {
          children: createElement(GroupDelegationsView, { read: session.getSnapshot().group, chatContent, session }),
        }));
        expect(render(true)).toContain(translate(locale, 'group.openReceiptExchange'));
        expect(render(true)).toContain(translate(locale, 'group.receiptExchangeScope'));
        expect(render(false)).not.toContain(translate(locale, 'group.openReceiptExchange'));
      } finally { vi.unstubAllGlobals(); }
    }
    expect(session.openGroupReceipt(row)).toBe(true);
    expect(socket.sent).toHaveLength(sent);
    expect(session.getSnapshot().chat).toMatchObject({ active: false, selection: {kind:'model_http',exchange:historyExchange} });
    session.leaveGroupDelegations(); session.chatHistory.enter();
    const first = queryId(socket);
    expect(JSON.parse(socket.sent.at(-1)!).business.query).toEqual({ type: 'chat_messages', scope:{kind:'model_http',exchange: historyExchange}, after: null, limit: 8 });
    expect(socket.sent).toHaveLength(sent + 1);
    expect(session.getSnapshot().chat.exchanges).toMatchObject({ phase: 'initial', pending: null, result: null });
    const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(ChatHistoryView, {
      read: session.getSnapshot().chat, chatContent: true, session,
    }) }));
    expect(markup).toContain('18446744073709551615');
    expect(markup).toContain('0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b');
    expect(markup).toContain('id="chat-messages"'); expect(markup).toContain('has-detail');
    const msg = historyMessage('19');
    socket.receive(historyReply(first, messagesResult([msg], msg.id)));
    session.chatHistory.nextMessages();
    expect(JSON.parse(socket.sent.at(-1)!).business.query).toEqual({ type: 'chat_messages', scope:{kind:'model_http',exchange: historyExchange}, after: {kind:'model_http',message:msg.id}, limit: 8 });
    socket.receive(historyReply(queryId(socket), messagesResult([historyMessage('20')])));
    expect(chatData(session.getSnapshot().chat.messages)).toMatchObject({ messages: [{kind:'model_http',message:{ id: { sequence: '20' } }}] });
    expect(sockets).toHaveLength(1); expect(session.getSnapshot().reviewDeclared).toBe(false);
    session.disconnect();
  });

  it('rejects stale Group selections and clears the direct target on close, leave, authorization failure and lock', () => {
    const { session, socket, sockets } = setup('read_only', true);
    session.enterGroupDelegations();
    socket.receive(businessWire(queryId(socket), groupResult({ records: [delegationRow('10'), historyReceipt()], next_before: null })));
    const page = groupPage(session.getSnapshot().group)!;
    expect(session.openGroupReceipt(page.records[0]!)).toBe(false);
    const row = page.records[1]!;
    session.chatHistory.enter(); const oldList = queryId(socket);
    expect(session.openGroupReceipt(row)).toBe(true); const oldMessages = queryId(socket);
    const pending = session.getSnapshot();
    socket.receive(historyReply(oldList, exchangesResult())); expect(session.getSnapshot()).toBe(pending);
    session.chatHistory.close(); const closed = session.getSnapshot();
    socket.receive(historyReply(oldMessages, messagesResult())); expect(session.getSnapshot()).toBe(closed);
    expect(closed.chat).toMatchObject({ selection: null, messages: { pending: null, result: null } });
    session.openGroupReceipt(row); const beforeList = queryId(socket);
    session.chatHistory.refreshExchanges(); const readingList = session.getSnapshot();
    socket.receive(historyReply(beforeList, messagesResult())); expect(session.getSnapshot()).toBe(readingList);
    session.openGroupReceipt(row); const deniedId = queryId(socket);
    socket.receive(historyReply(deniedId, { status: 'failed', code: 'unauthorized', message: 'content revoked' }));
    const denied = session.getSnapshot(), sent = socket.sent.length;
    expect(denied.chat).toMatchObject({ selection: null, messages: { pending: null, result: { code: 'unauthorized' } } });
    expect(denied.chatContent).toBe(false); expect(chatData(denied.chat.messages)).toBeNull();
    expect(session.openGroupReceipt(row)).toBe(false); session.chatHistory.refreshMessages();
    socket.receive(historyReply(deniedId, messagesResult())); expect(session.getSnapshot()).toBe(denied);
    expect(socket.sent).toHaveLength(sent);
    session.chatHistory.leave(); session.openGroupReceipt(row); session.chatHistory.leave();
    expect(session.getSnapshot().chat.selection).toBeNull();
    session.refreshGroupDelegations(); expect(session.openGroupReceipt(row)).toBe(false);
    expect(socket.sent).toHaveLength(sent);
    const lateId = deniedId, late = socket.onmessage!;
    session.disconnect();
    expect(session.getSnapshot().chat).toMatchObject({ active: false, selection: null, messages: { pending: null, result: null } });
    expect(session.openGroupReceipt(row)).toBe(false);
    session.connect(testToken); const next = sockets[1]!; next.open();
    next.receive(JSON.stringify({ version: 2, authenticated: { access: 'read_only', chat_content: true } }));
    const reconnected = session.getSnapshot();
    late({ data: historyReply(lateId, messagesResult()) }); next.receive(historyReply(lateId, messagesResult()));
    expect(session.getSnapshot()).toBe(reconnected); session.disconnect();
  });

  it('replaces the direct message read and validates both parts of ExchangeKey without list fallback', () => {
    const { session, socket } = setup('read_only', true);
    session.enterGroupDelegations();
    const sameSequence = { ...historyExchange, flow_instance: Array(16).fill(12) };
    socket.receive(businessWire(queryId(socket), groupResult({ records: [historyReceipt(), { ...historyReceipt(sameSequence), id: '8' }], next_before: null })));
    const rows = groupPage(session.getSnapshot().group)!.records;
    session.openGroupReceipt(rows[0]!); session.chatHistory.enter(); const old = queryId(socket);
    session.openGroupReceipt(rows[1]!); const current = queryId(socket), pending = session.getSnapshot();
    socket.receive(historyReply(old, messagesResult())); expect(session.getSnapshot()).toBe(pending);
    expect(JSON.parse(socket.sent.at(-1)!).business.query.scope.exchange).toEqual(sameSequence);
    socket.receive(historyReply(current, messagesResult()));
    expect(session.getSnapshot().chat.messages.problem?.key).toBe('chat.wrongReply');
    session.chatHistory.refreshMessages();
    socket.receive(historyReply(queryId(socket), messagesResult([historyMessage('9', { ...sameSequence, sequence: '1' })])));
    expect(session.getSnapshot().chat.messages.problem?.key).toBe('chat.wrongReply');
    session.chatHistory.refreshMessages();
    const sent = socket.sent.length;
    socket.receive(historyReply(queryId(socket), { status: 'failed', code: 'not_found', message: 'original exchange not retained' }));
    expect(session.getSnapshot().chat.messages.result).toMatchObject({ status: 'failed', code: 'not_found' });
    expect(session.getSnapshot().chat.selection).toEqual({kind:'model_http',exchange:sameSequence});
    expect(socket.sent).toHaveLength(sent);
    socket.failSend = true; session.chatHistory.refreshMessages();
    expect(session.getSnapshot().chat.messages.problem?.key).toBe('chat.sendFailed');
    expect(session.getSnapshot().chat.exchanges).toMatchObject({ phase: 'initial', pending: null, result: null });
    session.leaveGroupDelegations(); expect(session.openGroupReceipt(rows[0]!)).toBe(false);
    session.disconnect();
  });

  it('paginates both levels explicitly, replaces pages and invalidates a pending message read on exchange change', () => {
    const { session, socket, sockets } = setup('read_only', true);
    expect(session.getSnapshot().chat.active).toBe(false);
    session.chatHistory.enter(); const first = queryId(socket);
    expect(JSON.parse(socket.sent.at(-1)!)).toEqual(JSON.parse(chatCommand(first, { type: 'chat_exchanges', session: null, after: null, limit: 8 })));
    const sent = socket.sent.length; session.chatHistory.enter(); session.chatHistory.refreshExchanges();
    expect(socket.sent).toHaveLength(sent);
    socket.receive(historyReply(first, exchangesResult()));
    const page = chatData(session.getSnapshot().chat.exchanges)!;
    if (page.type !== 'chat_exchanges') throw Error('fixture');
    expect(socket.sent).toHaveLength(sent); // Keys do not cause N+1 message reads.
    session.chatHistory.select(page.exchanges[0]!); const old = queryId(socket);
    session.chatHistory.select(page.exchanges[1]!); const current = queryId(socket);
    expect(current).not.toEqual(old);
    const pending = session.getSnapshot();
    socket.receive(historyReply(old, messagesResult())); expect(session.getSnapshot()).toBe(pending);
    const msg = historyMessage('9', anotherExchange);
    socket.receive(historyReply(current, messagesResult([msg], msg.id)));
    expect(chatData(session.getSnapshot().chat.messages)).toEqual(messagesResult([msg], msg.id).data);
    session.chatHistory.nextMessages(); const next = queryId(socket);
    expect(JSON.parse(socket.sent.at(-1)!).business.query).toEqual({ type: 'chat_messages', scope:{kind:'model_http',exchange: anotherExchange}, after: {kind:'model_http',message:msg.id}, limit: 8 });
    expect(chatData(session.getSnapshot().chat.messages)).toBeNull();
    socket.receive(historyReply(next, messagesResult([historyMessage('1', anotherExchange)])));
    expect(chatData(session.getSnapshot().chat.messages)).toMatchObject({ messages: [{kind:'model_http',message:{ id: { sequence: '1' } }}] });
    session.chatHistory.nextExchanges(); const nextExchanges = queryId(socket);
    expect(JSON.parse(socket.sent.at(-1)!).business.query).toEqual({ type: 'chat_exchanges', session: null, after: anotherExchange, limit: 8 });
    expect(session.getSnapshot().chat.selection).toBeNull(); expect(chatData(session.getSnapshot().chat.messages)).toBeNull();
    socket.receive(historyReply(nextExchanges, exchangesResult([], null)));
    expect(chatData(session.getSnapshot().chat.exchanges)).toMatchObject({ exchanges: [] });
    expect(session.getSnapshot().chat.exchanges.pageNumber).toBe(2);
    session.chatHistory.refreshExchanges(16);
    expect(JSON.parse(socket.sent.at(-1)!).business.query).toEqual({ type: 'chat_exchanges', session: null, after: null, limit: 16 });
    expect(sockets).toHaveLength(1); expect(session.getSnapshot().reviewDeclared).toBe(false);
    session.disconnect();
  });

  it.each(['read_only', 'manage_rules', 'manage_service'])('requires independent content authorization: %s', access => {
    const { session, socket } = setup(access, false), before = socket.sent.length;
    session.chatHistory.enter(); session.chatHistory.refreshExchanges(); session.chatHistory.select(historyExchange);
    session.chatHistory.refreshMessages(); session.chatHistory.nextExchanges(); session.chatHistory.nextMessages();
    expect(session.chatHistory.open(historyExchange)).toBe(false); expect(session.openGroupReceipt(historyReceipt())).toBe(false);
    expect(socket.sent).toHaveLength(before); expect(session.getSnapshot().chat.exchanges.phase).toBe('initial');
    session.disconnect();
  });

  it('rejects wrong lane, IDs, business types, cross-exchange messages and invalid cursor boundaries', () => {
    const { session, socket } = setup('manage_rules', true);
    session.setUiReady(true); socket.receive(wire(ask)); const review = session.getSnapshot().reviews[0]!;
    session.chatHistory.enter(); const first = queryId(socket), pending = session.getSnapshot().chat;
    socket.receive(wire({ type: 'reply', id: first, result: driverRead }));
    socket.receive(historyReply(Array(16).fill(91), exchangesResult()));
    expect(session.getSnapshot().chat).toBe(pending);
    socket.receive(businessWire(first)); expect(session.getSnapshot().chat.exchanges.problem?.key).toBe('chat.wrongReply');
    session.chatHistory.refreshExchanges(); socket.receive(historyReply(queryId(socket), exchangesResult()));
    const page = chatData(session.getSnapshot().chat.exchanges)!;
    if (page.type !== 'chat_exchanges') throw Error('fixture');
    session.chatHistory.select(page.exchanges[0]!);
    socket.receive(historyReply(queryId(socket), messagesResult([historyMessage('1', anotherExchange)])));
    expect(session.getSnapshot().chat.messages.problem?.key).toBe('chat.wrongReply');
    session.chatHistory.refreshMessages();
    socket.receive(historyReply(queryId(socket), messagesResult([historyMessage()], { exchange: historyExchange, sequence: '1' })));
    expect(session.getSnapshot().chat.messages.phase).toBe('failed');
    session.chatHistory.refreshMessages();
    socket.receive(historyReply(queryId(socket), messagesResult([historyMessage(), historyMessage()])));
    expect(session.getSnapshot().chat.messages.phase).toBe('failed');
    session.chatHistory.refreshExchanges();
    socket.receive(historyReply(queryId(socket), exchangesResult([historyExchange], anotherExchange)));
    expect(session.getSnapshot().chat.exchanges.phase).toBe('failed');
    expect(session.getSnapshot().reviews[0]).toBe(review); expect(session.remaining(review)).toBe(5000);
    session.disconnect();
  });

  it('releases body and late consumption on leave, parent refresh, lock and old socket callbacks', () => {
    const { session, socket, sockets } = setup('read_only', true);
    session.chatHistory.enter(); socket.receive(historyReply(queryId(socket), exchangesResult()));
    const page = chatData(session.getSnapshot().chat.exchanges)!;
    if (page.type !== 'chat_exchanges') throw Error('fixture');
    session.chatHistory.select(page.exchanges[0]!); const old = queryId(socket);
    session.chatHistory.refreshExchanges(); const pending = session.getSnapshot();
    socket.receive(historyReply(old, messagesResult())); expect(session.getSnapshot()).toBe(pending);
    const late = queryId(socket); session.chatHistory.leave(); const left = session.getSnapshot();
    socket.receive(historyReply(late, exchangesResult())); expect(session.getSnapshot()).toBe(left);
    session.chatHistory.enter(); const oldId = queryId(socket), oldCallback = socket.onmessage!;
    session.disconnect(); expect(session.getSnapshot().chat.active).toBe(false);
    session.connect(testToken); const next = sockets[1]!; next.open();
    next.receive(JSON.stringify({ version: 2, authenticated: { access: 'read_only', chat_content: true } }));
    session.chatHistory.enter(); const newId = queryId(next), current = session.getSnapshot();
    oldCallback({ data: historyReply(oldId, exchangesResult()) }); next.receive(historyReply(oldId, exchangesResult()));
    expect(session.getSnapshot()).toBe(current);
    next.receive(historyReply(newId, exchangesResult())); const returned = session.getSnapshot();
    next.receive(historyReply(newId, { status: 'failed', code: 'busy', message: 'late' }));
    expect(session.getSnapshot()).toBe(returned); session.disconnect();
  });

  it('preserves original failures and clears all content on content-authorization failure without retry', () => {
    const { session, socket } = setup('read_only', true);
    session.chatHistory.enter();
    for (const code of ['busy', 'not_found', 'unavailable'] as const) {
      socket.receive(historyReply(queryId(socket), { status: 'failed', code, message: 'output_too_large' }));
      expect(session.getSnapshot().chat.exchanges.result).toMatchObject({ status: 'failed', code });
      expect(chatData(session.getSnapshot().chat.exchanges)).toBeNull();
      session.chatHistory.refreshExchanges();
    }
    socket.receive(historyReply(queryId(socket), exchangesResult()));
    const page = chatData(session.getSnapshot().chat.exchanges)!;
    if (page.type !== 'chat_exchanges') throw Error('fixture');
    session.chatHistory.select(page.exchanges[0]!);
    socket.receive(historyReply(queryId(socket), messagesResult()));
    expect(chatData(session.getSnapshot().chat.messages)).not.toBeNull();
    socket.failSend = true; session.chatHistory.refreshMessages();
    expect(session.getSnapshot().chat.messages).toMatchObject({ phase: 'failed', pending: null, result: null, problem: { key: 'chat.sendFailed' } });
    socket.failSend = false; session.chatHistory.refreshMessages();
    socket.receive(historyReply(queryId(socket), { status: 'failed', code: 'unauthorized', message: 'grant withdrawn' }));
    expect(session.getSnapshot().chat.selection).toBeNull();
    expect(chatData(session.getSnapshot().chat.exchanges)).toBeNull(); expect(chatData(session.getSnapshot().chat.messages)).toBeNull();
    const sent = socket.sent.length; session.chatHistory.refreshMessages(); session.chatHistory.refreshExchanges();
    expect(socket.sent).toHaveLength(sent); session.disconnect();
  });

  it('retains full original parts, roles, source and numeric lexemes through the shared decoder', () => {
    const original = historyMessage();
    original.observed_parts = [
      { type: 'text', id: 0, kind: 'refusal', text: '' },
      { type: 'text', id: 1, kind: 'visible_reasoning', text: 'public reasoning only' },
      { type: 'tool_call', id: 2, native_id: { namespace: 'tool_call', field_path: '/input/0/call_id', value: { kind: 'json_number', value: '1e30' } },
        name: 'tool-original', arguments: { kind: 'json', value: parseUnique('{"n":9007199254740993,"e":1e400,"z":-0,"empty":[null,"",false]}') } },
      { type: 'tool_call', id: 3, native_id: null, name: '', arguments: null },
      { type: 'tool_result', id: 4, native_id: null, content: { kind: 'json', value: null } },
      { type: 'opaque', id: 5, provider_type: '', reason: 'original unsupported code' },
      { type: 'media_ref', id: 6, kind: 'image', safe_label: 'safe original label' },
    ];
    const data = messagesResult([original]).data;
    expect(decodeChatData(data)).toBe(data);
    const text = historyReply(id, messagesResult([original]));
    const parsed = parseFrame(text);
    expect(wireJson(parsed)).toContain('"n":9007199254740993,"e":1e400,"z":-0');
    for (const changed of [withoutKey(original, 'revision'), { ...original, revision: 1 }, { ...original, role: { kind: 'new' } },
      { ...original, observed_parts: [{ type: 'tool_call', id: 0, name: 'x', native_id: null, arguments: {} }] },
      { ...original, source: { ...original.source, config_version: [] } }])
      expect(() => parseFrame(historyReply(id, messagesResult([changed as ChatMessage])))).toThrow();
    expect(() => chatCommand(id, { type: 'chat_messages', scope: {kind:'model_http',exchange: historyExchange}, after: {kind:'model_http',message:{ exchange: anotherExchange, sequence: '0' }}, limit: 8 })).toThrow();
    for (const locale of ['ja-JP', 'zh-CN', 'en-US'] as const) {
      vi.stubGlobal('localStorage', { getItem: () => locale });
      for (const kind of ['system', 'developer', 'user', 'assistant', 'tool', 'other'] as const) {
        const message = { ...original, role: kind === 'other' ? { kind, value: 'original custom role' } : { kind } };
        const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(OriginalChatMessage, { message }) }));
        expect(markup).toContain('tool-original'); expect(markup).toContain('9007199254740993');
        expect(markup).toContain('1e400'); expect(markup).toContain('safe original label'); expect(markup).not.toContain('<img');
        expect(markup).toContain(translate(locale, 'chat.argumentsMissing'));
      }
      const { session } = setup('read_only', false);
      const denied = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(ChatHistoryView, {
        read: session.getSnapshot().chat, chatContent: false, session,
      }) }));
      expect(denied).toContain(translate(locale, 'chat.noGrant')); expect(denied).not.toContain('chat-message stack-tight');
      session.disconnect(); vi.unstubAllGlobals();
    }
  });
});


function currentSessions(session: ReviewSession) {
  const data = chatData(session.getSnapshot().chat.sessions);
  if (data?.type !== 'chat_sessions') throw Error('fixture');
  return data.sessions;
}

describe('original application-session filtering', () => {
  it('reads a directory only explicitly and drops downstream late replies when switching sessions or directory pages', () => {
    const { session, socket, sockets } = setup('read_only', true);
    session.chatHistory.enter(); const all = queryId(socket);
    expect(JSON.parse(socket.sent.at(-1)!).business.query.type).toBe('chat_exchanges');
    session.chatHistory.refreshSessions();
    expect(JSON.parse(socket.sent.at(-1)!).business.query).toEqual({ type: 'chat_sessions', before: null, limit: 8 });
    socket.receive(historyReply(queryId(socket), { status: 'completed', data: originalSessions() }));
    const rows = currentSessions(session), beforeStaleSelection = socket.sent.length;
    session.chatHistory.selectSession({ ...rows[0]! }); expect(socket.sent).toHaveLength(beforeStaleSelection);
    session.chatHistory.selectSession(rows[0]!); const oldExchanges = queryId(socket);
    expect(JSON.parse(socket.sent.at(-1)!).business.query).toEqual({ type: 'chat_exchanges', session: rows[0]!.reference, after: null, limit: 8 });
    socket.receive(historyReply(oldExchanges, exchangesResult()));
    const page = chatData(session.getSnapshot().chat.exchanges)!;
    if (page.type !== 'chat_exchanges') throw Error('fixture');
    session.chatHistory.select(page.exchanges[0]!); const oldMessages = queryId(socket);
    session.chatHistory.selectSession(rows[1]!); const pendingB = queryId(socket), snapshotB = session.getSnapshot();
    socket.receive(historyReply(oldMessages, messagesResult()));
    socket.receive(historyReply(all, exchangesResult())); expect(session.getSnapshot()).toBe(snapshotB);
    expect(snapshotB.chat).toMatchObject({ sessionSelection: rows[1]!.reference, selection: null, messages: { result: null, pending: null } });
    session.chatHistory.nextSessions(); const nextDirectory = queryId(socket), nextState = session.getSnapshot();
    expect(JSON.parse(socket.sent.at(-1)!).business.query).toEqual({ type: 'chat_sessions', before: rows[1]!.reference, limit: 8 });
    socket.receive(historyReply(pendingB, exchangesResult())); expect(session.getSnapshot()).toBe(nextState);
    socket.receive(historyReply(nextDirectory, { status: 'completed', data: { type: 'chat_sessions', sessions: [], next_before: null } }));
    session.chatHistory.selectSession(rows[0]!); expect(queryId(socket)).toEqual(nextDirectory);
    session.chatHistory.selectSession(null);
    expect(JSON.parse(socket.sent.at(-1)!).business.query.session).toBeNull();
    expect(sockets).toHaveLength(1); session.disconnect();
  });

  it('keeps Group direct opening and content revocation on the same existing read lifecycle', () => {
    const { session, socket } = setup('read_only', true);
    session.chatHistory.enter(); session.chatHistory.refreshSessions();
    socket.receive(historyReply(queryId(socket), { status: 'completed', data: originalSessions() }));
    session.chatHistory.selectSession(currentSessions(session)[0]!); const oldFiltered = queryId(socket);
    expect(session.chatHistory.open(anotherExchange)).toBe(true); const direct = queryId(socket), state = session.getSnapshot();
    expect(state.chat.sessionSelection).toBeNull();
    socket.receive(historyReply(oldFiltered, exchangesResult())); expect(session.getSnapshot()).toBe(state);
    expect(JSON.parse(socket.sent.at(-1)!).business.query).toEqual({ type: 'chat_messages', scope:{kind:'model_http',exchange: anotherExchange}, after: null, limit: 8 });
    socket.receive(historyReply(direct, messagesResult([historyMessage('1', anotherExchange)])));
    session.chatHistory.refreshSessions();
    socket.receive(historyReply(queryId(socket), { status: 'failed', code: 'unauthorized', message: 'original revocation' }));
    expect(session.getSnapshot().chat).toMatchObject({ selection: null, sessionSelection: null,
      sessions: { result: { code: 'unauthorized', message: 'original revocation' } }, messages: { result: null }, exchanges: { result: null } });
    const sent = socket.sent.length; session.chatHistory.refreshSessions(); session.chatHistory.selectSession(null);
    session.chatHistory.leave(); session.chatHistory.enter(); session.chatHistory.refreshSessions();
    expect(session.chatHistory.open(anotherExchange)).toBe(false);
    expect(socket.sent).toHaveLength(sent); session.disconnect();
  });

  it('preserves original session types, bytes and number lexemes and strictly migrates all current consumers', () => {
    const data = originalSessions(); expect(decodeChatData(data)).toBe(data);
    expect(() => decodeChatData({ ...data, sessions: [{ ...data.sessions[0], exchanges: '1\n' }] })).toThrow();
    const header = { field: { kind: 'header', name: 'x-session' }, values: [{ kind: 'header', value: [32, 65, 32] }], issue: null };
    expect(decodeSessionObservation(header)).toBe(header);
    for (const value of ['1\n', '1 ', '01', 'NaN', '+1']) {
      const invalidObservation = { ...data.sessions[1]!.session, values: [{ kind: 'json', value: { kind: 'json_number', value } }] };
      expect(() => decodeSessionObservation(invalidObservation)).toThrow();
    }
    const observation = { ...data.sessions[1]!.session, values: [{ kind: 'json', value: { kind: 'json_number', value: '1.2300e+400' } }] };
    expect(decodeSessionObservation(observation)).toBe(observation);
    const snapshot = { ...egressSnapshot, document: { ...egressSnapshot.document } };
    snapshot.document.flow = { ...record(snapshot.document.flow), provider_routes: [{ ...egressRouteFact, session: header.field }] };
    expect(readEgressConfig(snapshot).flow?.routes[0]?.session).toEqual(header.field);
    snapshot.document.flow = { ...record(snapshot.document.flow), provider_routes: [withoutKey(egressRouteFact, 'session')] };
    expect(() => readEgressConfig(snapshot)).toThrow();
    expect(() => decodeActivityContext({ type: 'activity_context', attribution: null,
      view: { ...contextView(), source_records: [{ kind: 'protocol', value: {} }] } })).toThrow();
    const context = { type: 'activity_context', attribution: null,
      view: { ...contextView(), source_records: [{ kind: 'protocol', value: { request_session: header } }] } };
    expect(decodeActivityContext(context).view.source_records[0]!.value).toEqual({ request_session: header });
  });

  it('distinguishes same-text string and number sessions in Japanese, Chinese and English', () => {
    const { session, socket } = setup('read_only', true);
    session.chatHistory.enter(); session.chatHistory.refreshSessions();
    socket.receive(historyReply(queryId(socket), { status: 'completed', data: originalSessions() }));
    for (const locale of ['ja-JP', 'zh-CN', 'en-US'] as const) {
      vi.stubGlobal('localStorage', { getItem: () => locale });
      try {
        const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(ChatHistoryView, {
          read: session.getSnapshot().chat, chatContent: true, session,
        }) }));
        expect(markup).toContain('&quot;7&quot;'); expect(markup).toContain('<bdi>7</bdi>');
        expect(markup).toContain('18446744073709551615'); expect(markup).toContain(translate(locale, 'chat.sessions'));
      } finally { vi.unstubAllGlobals(); }
    }
    session.disconnect();
  });
});


describe('connection-owned content authorization across every consumer', () => {
  it.each(['group', 'chat', 'ask', 'activity'] as const)('revokes only the current live content grant from %s and retains its original denial', target => {
    const { session, socket, sockets } = setup('manage_service', true);
    session.setUiReady(true); socket.receive(wire(ask)); const review = session.getSnapshot().reviews[0]!;
    session.enterGroupDelegations(); socket.receive(businessWire(queryId(socket), groupResult()));
    session.chatHistory.enter(); socket.receive(historyReply(queryId(socket), exchangesResult()));
    const exchanges = chatData(session.getSnapshot().chat.exchanges)!;
    if (exchanges.type !== 'chat_exchanges') throw Error('fixture');
    session.chatHistory.select(exchanges.exchanges[0]!); socket.receive(historyReply(queryId(socket), messagesResult()));
    session.chatHistory.refreshSessions(); socket.receive(historyReply(queryId(socket), { status: 'completed', data: originalSessions() }));
    session.readAskContext(review); socket.receive(businessWire(queryId(socket), askContextResult()));
    session.enterActivity(); socket.receive(logReply(logCommand(socket).id));
    session.selectActivity(session.getSnapshot().activity.page!.records[0]!);
    session.readActivityEvidence(); socket.receive(businessWire(queryId(socket), contextResult()));
    expect(groupPage(session.getSnapshot().group)).not.toBeNull();
    expect(chatData(session.getSnapshot().chat.messages)).not.toBeNull();
    expect(session.getSnapshot().askEvidence.result?.status).toBe('completed');
    expect(session.getSnapshot().activity.evidence.result?.status).toBe('completed');
    if (target === 'group') session.refreshGroupDelegations();
    if (target === 'chat') session.chatHistory.refreshMessages();
    if (target === 'ask') session.readAskContext(review);
    if (target === 'activity') session.readActivityEvidence();
    const deniedId = queryId(socket), failure = { status: 'failed', code: 'unauthorized', message: 'original content denial' };
    const grants: boolean[] = []; const unsubscribe = session.subscribe(() => grants.push(session.getSnapshot().chatContent));
    socket.receive(businessWire(deniedId, failure)); const denied = session.getSnapshot(); unsubscribe();
    expect(grants.length).toBeGreaterThan(0); expect(grants.every(grant => grant === false)).toBe(true);
    expect(denied).toMatchObject({ phase: 'authenticated', access: 'manage_service', chatContent: false, reviewDeclared: true });
    expect(denied.reviews[0]).toBe(review);
    expect(groupPage(denied.group)).toBeNull();
    for (const read of [denied.chat.sessions, denied.chat.exchanges, denied.chat.messages]) expect(chatData(read)).toBeNull();
    expect(denied.askEvidence.result?.status).not.toBe('completed');
    expect(denied.activity.evidence.result?.status).not.toBe('completed');
    const original = target === 'group' ? denied.group.reply?.result : target === 'chat' ? denied.chat.messages.result
      : target === 'ask' ? denied.askEvidence.result : denied.activity.evidence.result;
    expect(original).toEqual(failure);
    const sent = socket.sent.length;
    session.leaveGroupDelegations(); session.enterGroupDelegations(); session.refreshGroupDelegations();
    session.chatHistory.leave(); session.chatHistory.enter(); session.chatHistory.refreshSessions(); session.chatHistory.refreshExchanges();
    session.readAskContext(review); session.readActivityEvidence();
    expect(socket.sent).toHaveLength(sent);
    session.submit(review, 0);
    expect(JSON.parse(socket.sent.at(-1)!).message).toEqual({ type: 'answer', decision_id: ask.decision_id, decision: allow });
    const oldCallback = socket.onmessage!;
    session.disconnect(); session.connect(testToken); const next = sockets[1]!; next.open();
    next.receive(JSON.stringify({ version: 2, authenticated: { access: 'manage_service', chat_content: true } }));
    session.chatHistory.enter(); const newId = queryId(next), current = session.getSnapshot();
    oldCallback({ data: businessWire(deniedId, failure) }); next.receive(businessWire(deniedId, failure));
    expect(session.getSnapshot()).toBe(current); expect(current.chatContent).toBe(true);
    next.receive(historyReply(newId, exchangesResult())); expect(chatData(session.getSnapshot().chat.exchanges)).not.toBeNull();
    session.disconnect();
  });

  it('keeps management denials and recoverable content failures independent of the content grant', () => {
    const { session, socket } = setup('manage_service', true);
    session.enterRules(); socket.receive(ruleReply(ruleCommand(socket).id,
      { status: 'failed', code: 'unauthorized', message: 'rule management denied', facts: null }));
    expect(session.getSnapshot().chatContent).toBe(true);
    socket.receive(JSON.stringify({ version: 2, diagnostic: { type: 'private_not_authorized', code: 'unauthorized' } }));
    expect(session.getSnapshot().chatContent).toBe(true);
    session.refreshOverview(); socket.receive(businessWire(queryId(socket), { status: 'failed', code: 'unauthorized', message: 'overview denied' }));
    expect(session.getSnapshot().chatContent).toBe(true);
    session.enterGroupDelegations();
    for (const code of ['not_found', 'conflict', 'invalid', 'unavailable'] as const) {
      socket.receive(businessWire(queryId(socket), { status: 'failed', code, message: 'original recoverable failure' }));
      expect(session.getSnapshot().chatContent).toBe(true); session.refreshGroupDelegations();
    }
    session.disconnect();
  });
});


describe('original attribution request and read ownership', () => {
  const request = (): AttributionRequest => ({pattern:{actor:'same_process',lookback_ns:'1000000000',target:{type:'file_path',pointer:'/path'}},after:null,limit:8});
  const result = (query: AttributionRequest): ToolAttribution => ({event:eventAnchor,pattern:query.pattern,
    required_connection_process:{boot_id:id,host_tgid:1,birth_boottime_ns:'0'},required_connection_thread:null,
    event_boottime_ns:'1000000000',event_target:{type:'bytes',value:[]},
    candidates:[{call:{message:{exchange:{flow_instance:id,sequence:'2'},sequence:'0'},part_id:0},
      message_revision:'1',response_observed_ns:'0',target_comparison:'equal'}],next_after:null});

  it('rejects wrong Ask comparison identity only in its read slot and keeps the original answer usable', () => {
    const {session,socket}=setup('manage_rules',true);
    session.setUiReady(true);socket.receive(wire(ask));
    const row=session.getSnapshot().reviews[0]!;session.selectAskContext(row);
    socket.receive(businessWire(queryId(socket),askContextResult()));
    const query=request();session.readAskContext(row,query);const first=queryId(socket);
    expect(JSON.parse(socket.sent.at(-1)!).business.query.attribution).toEqual(query);
    const wrong=askContext();wrong.attribution={...result(query),event:{...eventAnchor,sequence:'998'}};
    socket.receive(businessWire(first,askContextResult(wrong)));
    expect(session.getSnapshot()).toMatchObject({phase:'authenticated',askEvidence:{phase:'failed'}});
    expect(session.getSnapshot().reviews[0]).toBe(row);
    session.readAskContext(row,query);const second=queryId(socket),context=askContext();context.attribution=result(query);
    socket.receive(businessWire(first,askContextResult(context)));
    expect(session.getSnapshot().askEvidence.phase).toBe('loading');
    socket.receive(businessWire(second,askContextResult(context)));
    const read=session.getSnapshot().askEvidence;
    expect(read.phase).toBe('returned');expect(read.attributionRequest).toBe(query);
    expect(session.openAuditExchange(read,context.attribution.candidates[0]!.call.message.exchange)).toBe(true);
    session.selectAskContext(undefined);
    expect(session.openAuditExchange(read,context.attribution.candidates[0]!.call.message.exchange)).toBe(false);
    session.submit(row,0);
    expect(JSON.parse(socket.sent.at(-1)!).message).toMatchObject({type:'answer',decision_id:row.ask.decision_id});
    session.disconnect();
  });

  it('pages Activity candidates with original conditions and never revives an obsolete read or exchange link', () => {
    const {session,socket}=setup('manage_service',true);session.enterActivity();
    socket.receive(logReply(logCommand(socket).id));session.selectActivity(session.getSnapshot().activity.page!.records[0]!);
    socket.receive(businessWire(queryId(socket),contextResult()));
    const query=request(),attribution=result(query);attribution.next_after=attribution.candidates[0]!.call.message;
    session.readActivityEvidence(query);socket.receive(businessWire(queryId(socket),{status:'completed',data:{type:'activity_context',view:contextView(),attribution}}));
    const read=session.getSnapshot().activity.evidence,exchange=attribution.candidates[0]!.call.message.exchange;
    expect(read.phase).toBe('returned');expect(session.openAuditExchange(read,exchange)).toBe(true);
    const next={...query,after:attribution.next_after};session.readActivityEvidence(next);const pending=queryId(socket);
    expect(session.openAuditExchange(read,exchange)).toBe(false);
    expect(JSON.parse(socket.sent.at(-1)!).business.query.attribution).toEqual(next);
    const mismatched={...attribution,pattern:{...query.pattern,lookback_ns:'0'}};
    socket.receive(businessWire(pending,{status:'completed',data:{type:'activity_context',view:contextView(),attribution:mismatched}}));
    expect(session.getSnapshot().activity.evidence.phase).toBe('failed');expect(session.getSnapshot().phase).toBe('authenticated');
    session.disconnect();
  });
});


describe('original native request observations', () => {
  const fact = (phase: unknown) => ({ observation: eventAnchor, request: null, operation: null,
    actor: { tgid: 7, tid: 8, birth_boottime_ns: '0' }, observed_boottime_ns: '0', phase });
  const open = { type: 'submission', site: {
    process: { boot_id: id, host_tgid: 7, birth_boottime_ns: '0' }, exec_cookie: '0',
    instruction_pointer: '0', stack_pointer: '0', link_register: null, frame_pointer: '0' },
    call: { type: 'file_open', request_address: '0', loop_address: '0', path_address: '0', path: [],
      path_status: { status: 'complete' }, flags: 0, mode: 0, callback_address: '0' },
    projection: null, analysis: null, projection_error: 'allocation' };
  function data(value: unknown) {
    return { type: 'activity_context', attribution: null,
      view: { ...contextView(), source_records: [{ kind: 'request_handoff', value }] } };
  }
  it('keeps empty, zero and partially read raw observations without tool or request identities', () => {
    for (const phase of [open, { ...open, call: { ...open.call, path: [0, 255], path_status: { status: 'read_fault', native_error: -14 } } },
      { ...open, call: { ...open.call, path: [47], path_status: { status: 'capacity' } } },
      { type: 'work_enter', work_address: '18446744073709551615' },
      { type: 'done', work_address: '0', result: null, result_read_error: -14 },
      { type: 'done', work_address: '0', result: 0, result_read_error: null },
      { type: 'event_bound', event: eventAnchor, binding: { type: 'controller_wait', wait_id: '0' }, enclosing_call: null },
      { type: 'event_bound', event: eventAnchor, binding: { type: 'direct_check' }, enclosing_call: eventAnchor },
      { type: 'api_returned', result: 0 },
      { type: 'child_created', child: open.site.process }]) {
      const value = fact(phase), input = data(value);
      expect(decodeActivityContext(input)).toEqual(input);
      expect(decodeActivityContext(input).view.source_records[0]!.value).toBe(value);
    }
  });
  it('retains the native signed result exactly and rejects it as a relation anchor', () => {
    const raw = JSON.stringify(data(fact({ type: 'done', work_address: '0', result: null, result_read_error: null })))
      .replace('"result":null', '"result":-9223372036854775808');
    expect(wireJson(decodeActivityContext(parseUnique(raw)))).toBe(raw);
    const input = data(fact(open));
    expect(() => decodeActivityContext({ ...input, view: { ...input.view,
      anchor: input.view.source_records[0] } })).toThrow();
  });
  it('decodes process and network submissions without inventing file fields or queue ownership', () => {
    const calls = [
      { type: 'spawn', loop_address: '0', process_address: '0', options_address: '0' },
      { type: 'tcp_connect', request_address: '0', stream_address: '0', address: '0', callback_address: '0' },
      { type: 'stream_write', entry: 'write', request_address: '0', stream_address: '0', buffers_address: '0',
        buffer_count: 0, send_stream_address: null, callback_address: '0' },
      { type: 'stream_write', entry: 'write2', request_address: '0', stream_address: '0', buffers_address: '0',
        buffer_count: 0, send_stream_address: '0', callback_address: '0' },
      { type: 'udp_send', request_address: '0', stream_address: '0', buffers_address: '0', buffer_count: 0,
        address: '0', callback_address: '0' },
      { type: 'stream_try_write', entry: 'try_write', stream_address: '0', buffers_address: '0', buffer_count: 0,
        send_stream_address: null },
      { type: 'stream_try_write', entry: 'try_write2', stream_address: '0', buffers_address: '0', buffer_count: 0,
        send_stream_address: '0' },
    ];
    for (const call of calls) {
      const input = data(fact({ ...open, call }));
      expect(decodeActivityContext(input)).toEqual(input);
    }
    for (const call of [{ ...calls[2], send_stream_address: '0' }, { ...calls[3], send_stream_address: null },
      { ...calls[5], send_stream_address: '0' }, { ...calls[6], send_stream_address: null }]) {
      expect(() => decodeActivityContext(data(fact({ ...open, call })))).toThrow();
    }
    expect(() => decodeActivityContext(data(fact({ ...open, type: 'open' })))).toThrow();
    expect(() => decodeActivityContext(data(fact({ type: 'event_bound', event: eventAnchor, wait_id: '0' })))).toThrow();
  });
  const opening = { ...eventAnchor, sequence_domain: '2', sequence: '1000' };
  const leaving = { ...opening, sequence: '1001' }, done = { ...opening, sequence: '1002' };
  const nativeRef = (value = opening) => ({ kind: 'native_observation' as const, value });
  function partialNativeView(): ContextView {
    return { ...contextView(), source_records: [...contextView().source_records,
      { kind: 'request_handoff', value: { ...fact({ type: 'work_leave', work_address: '2048' }), observation: leaving,
        request: opening, operation: 'file_open' } },
      { kind: 'request_handoff', value: { ...fact({ type: 'done', work_address: '2048', result: -13, result_read_error: null }),
        observation: done, request: opening, operation: 'file_open' } }],
      relations: [
        { kind: 'native_request_reference', from: nativeRef(leaving), to: nativeRef(), evidence: [nativeRef(leaving)] },
        { kind: 'same_thread_return_sequence', from: { kind: 'event', value: eventAnchor }, to: nativeRef(leaving),
          evidence: [{ kind: 'event', value: eventAnchor }, nativeRef(leaving), nativeRef(done)] },
      ], gaps: [{ at: nativeRef(), reason: 'not_provided' }] };
  }
  it('shows backend partial references and temporal patterns with missing Opening, without granting a tool execution bridge', () => {
    const view = partialNativeView(), before = factsJson(view);
    const input = { type: 'activity_context', attribution: null, view };
    expect(decodeActivityContext(input).view).toBe(view);
    const model = projectCausalChain({ kind: 'completion', completion: completed() }, { kind: 'activity', view });
    expect(confirmedExecutionCall(completed().event, view)).toBeNull();
    expect(model.steps.some(step => step.stage === 'upstream')).toBe(false);
    const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(CausalChain,
      { model, onReadNative: () => undefined }) }));
    expect(markup).toContain(translate('ja-JP', 'evidence.originalReference'));
    expect(markup).toContain(translate('ja-JP', 'evidence.temporalPattern'));
    expect(markup).toContain(translate('ja-JP', 'evidence.notProvided'));
    expect(markup).toContain(translate('ja-JP', 'evidence.readNativeObservation'));
    const noReturn = { ...view, source_records: view.source_records.filter(source => source.kind !== 'request_return'),
      relations: view.relations.filter(link => link.kind === 'native_request_reference') };
    const partial = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(NativeEvidence, { view: noReturn }) }));
    expect(partial).toContain(translate('ja-JP', 'evidence.originalReference'));
    expect(partial).not.toContain('<button');
    expect(factsJson(view)).toBe(before);
    for (const changed of [
      { ...view.relations[0], from: { kind: 'event', value: leaving } },
      { ...view.relations[1], to: { kind: 'completion', value: leaving } },
    ]) expect(() => decodeActivityContext({ ...input, view: { ...view, relations: [changed] } })).toThrow();
    expect(() => activityContextCommand(id, nativeRef(), { pattern: { actor: 'same_process', lookback_ns: '0', target: { type: 'file_path', pointer: '' } }, after: null, limit: 1 })).toThrow();
  });
  it('reads an explicit missing native reference in the existing Activity slot and verifies its complete non-LSM anchor', () => {
    const { session, socket } = setup('manage_service', true);
    session.enterActivity(); socket.receive(logReply(logCommand(socket).id));
    session.selectActivity(session.getSnapshot().activity.page!.records[0]!);
    session.readActivityEvidence(); socket.receive(businessWire(queryId(socket), { status: 'completed', data: {
      type: 'activity_context', attribution: null, view: partialNativeView() } }));
    const read = session.getSnapshot().activity.evidence, sent = socket.sent.length;
    expect(session.openNativeObservation(read, { ...opening, sequence: '9999' })).toBe(false);
    expect(socket.sent).toHaveLength(sent);
    expect(session.openNativeObservation(read, opening)).toBe(true);
    const current = session.getSnapshot().activity;
    expect(selectionAnchor(current.selection)).toBeNull();
    expect(JSON.parse(socket.sent.at(-1)!).business.query).toEqual({ type: 'activity_context', anchor: nativeRef(), attribution: null });
    const missing = { anchor: nativeRef(), source_records: [], relations: [], gaps: [{ at: nativeRef(), reason: 'not_provided' }] };
    socket.receive(businessWire(queryId(socket), { status: 'completed', data: { type: 'activity_context', attribution: null, view: missing } }));
    expect(session.getSnapshot().activity.evidence).toMatchObject({ phase: 'returned', problem: null });
    const returned = session.getSnapshot().activity;
    expect(session.getActivityEvidenceSnapshot(returned.selection!, returned.evidence)).toBeNull();
    session.readActivityEvidence();
    socket.receive(businessWire(queryId(socket), { status: 'completed', data: { type: 'activity_context', attribution: null,
      view: { ...missing, anchor: { kind: 'event', value: opening } } } }));
    expect(session.getSnapshot().activity.evidence).toMatchObject({ phase: 'failed', problem: { key: 'evidence.wrongReply' } });
    expect(session.getSnapshot().phase).toBe('authenticated');
    session.disconnect();
  });
});
