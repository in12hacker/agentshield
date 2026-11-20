import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { askAuditWindows } from './audit-window-model';
import { AskAuditWindows } from './audit-window';
import type { ChatMessage } from './chat-wire';
import { decodeAskContext, type AskAuditContext, type AskMessageWindow, type ExchangeKey, type ToolAttribution } from './context-wire';
import { confirmedExecutionCall, projectCausalChain } from './causal-chain-model';
import { CausalChain } from './causal-chain';
import type { Ask } from './event-wire';
import { I18nProvider } from './i18n';
import { translate } from './messages';
import { ask as receivedAsk } from '../test-support/contract-supply';

// Original contract-shaped replies; these tests do not assert collection or driver protection.
const id = Array<number>(16).fill(1);
const event = { source_instance: id, sequence_domain: '1', sequence: '2' };
const exchange: ExchangeKey = { flow_instance: id, sequence: '9007199254740993' };
const otherExchange = { ...exchange, sequence: '9007199254740994' };
const ask: Ask = { type: 'ask', decision_id: id, event: { header: event },
  hit: { group_id: 'file', group_number: 1, rule_id: 'open', rule_number: 1,
    source: { kind: 'rules_document', version: Array<number>(32).fill(2) }, log: false, deny_message: null },
  choices: [{ type: 'allow' }], timeout: { type: 'allow' }, remaining_ms: 5000 };
function message(sequence: string, selectedExchange = exchange): ChatMessage {
  return { id: { exchange: selectedExchange, sequence }, revision: '1',
    source: { actor: { process: null, info: {}, tid: null, thread_birth_boottime_ns: null,
      cgroup_id: null, observed_boottime_ns: '0' }, channel: null, side: 'client_request', request_traceparent: null,
      profile: 'open_ai_responses', origin: '', route_label: '', config_version: Array<number>(32).fill(0),
      observed_at_utc_ms: null, actual_auth_scope: null, credential_use: null },
    record_refs: [{ exchange: selectedExchange, sequence: '99' }], origin: 'request_context', role: { kind: 'user' },
    candidate_index: null, model: '', observed_parts: [{ type: 'text', id: 0, kind: 'plain', text: `original ${sequence}` }],
    state: 'complete', quality: [], provider_status: '', finish_reason: null, usage: [],
    first_seen_boottime_ns: '0', last_seen_boottime_ns: '0' };
}
function window(messages: readonly ChatMessage[], selectedExchange = exchange): AskMessageWindow {
  return { exchange: selectedExchange, message_refs: messages.map(item => item.id),
    earlier_observed_messages: 3, source_gaps: [{ kind: 'original gap', value: '' }] };
}
function context(messages: readonly ChatMessage[]): AskAuditContext {
  return { decision_id: id, event, agent: null, trigger_call: null, window: window(messages),
    view: { anchor: { kind: 'question', value: id }, gaps: [], relations: [],
      source_records: messages.map(value => ({ kind: 'message', value })) },
    command_checks: [], attribution: null, candidate_windows: [] };
}
function render(context: AskAuditContext) {
  return renderToStaticMarkup(createElement(I18nProvider, {
    children: createElement(AskAuditWindows, { context, ask }),
  }));
}

describe('complete original Ask message windows', () => {
  it('decodes backend candidates across exchanges and shows original late results without confirming execution or changing the frozen window', () => {
    const ask: Ask = { ...receivedAsk, decision_id: id,
      event: { ...receivedAsk.event, header: { ...receivedAsk.event.header, ...event } } };
    const call = message('1');
    call.origin = 'response_output'; call.role = { kind: 'assistant' };
    call.source = { ...call.source, side: 'upstream_response' };
    call.observed_parts = [{ type: 'tool_call', id: 0, native_id: null, name: '', arguments: { kind: 'json', value: { path: '', count: 0 } } }];
    const result = message('2', otherExchange);
    result.source = { ...result.source, origin: 'https://other-observed-source.example' };
    result.role = { kind: 'tool' };
    const content = { kind: 'json' as const, value: ['', 0, false, null, [], ['']] };
    result.observed_parts = [{ type: 'tool_result', id: 0, native_id: null, content }];
    const from = { kind: 'part' as const, value: { message: call.id, part_id: 0 } };
    const to = { kind: 'part' as const, value: { message: result.id, part_id: 0 } };
    const target = { kind: 'event' as const, value: event };
    const missingNative = { kind: 'native_observation' as const, value: { ...event, sequence: '3' } };
    const reply = context([call, result]);
    reply.window = null; reply.candidate_windows = [window([call])];
    reply.view = { ...reply.view, source_records: [...reply.view.source_records, { kind: 'question', value: ask },
      ...[call, result].map(item => ({ kind: 'protocol' as const,
        value: { id: item.record_refs[0], source: item.source, native_ids: [], request_session: null } }))],
    relations: [
      { kind: 'tool_reference_candidate', from, to: target, evidence: [from, target, missingNative] },
      { kind: 'tool_reference_candidate', from, to, evidence: [from, to,
        { kind: 'protocol', value: { ...call.record_refs[0]! } }, { kind: 'protocol', value: { ...result.record_refs[0]! } }] },
    ] };
    expect(decodeAskContext({ type: 'ask_context', context: reply }).context).toBe(reply);
    expect(reply.trigger_call).toBeNull();
    expect(confirmedExecutionCall(ask.event, reply.view, { ask, context: reply })).toBeNull();
    const frozen = reply.candidate_windows[0]!;
    const projected = askAuditWindows(ask, reply)[0]!;
    expect(projected.kind).toBe('candidate'); expect(projected.original).toBe(frozen);
    expect(projected.messages.map(entry => entry.message)).toEqual([call]);
    for (const view of [reply.view, { ...reply.view, relations: [reply.view.relations[1]!] },
      { ...reply.view, source_records: reply.view.source_records.filter(source => source.kind !== 'message' || source.value !== call) }]) {
      const model = projectCausalChain({ kind: 'ask', ask, completion: null }, { kind: 'ask', context: { ...reply, view } });
      const reported = model.steps.find(step => step.stage === 'tool_result');
      expect(reported?.stage === 'tool_result' && reported.content).toBe(content);
      expect(model.steps.find(step => step.stage === 'operation')?.incoming?.kind)
        .toBe(view === reply.view ? 'candidate' : undefined);
      expect(model.originals.tool_result.some(item => item.kind === 'message' && item.value === result)).toBe(true);
      const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(CausalChain, { model }) }));
      expect(markup).toContain(translate('ja-JP', 'chain.candidate'));
      expect(markup).toContain(translate('ja-JP', 'chain.toolResult'));
      expect(markup).toContain('<pre class="chat-body">[\n  &quot;&quot;,\n  0,\n  false,\n  null,\n  [],\n  [\n    &quot;&quot;\n  ]\n]</pre>');
      expect(markup).not.toContain(translate('ja-JP', 'chain.confirmedLink'));
    }
    const wrongEndpoint = { ...reply.view.relations[0]!, from: target };
    expect(() => decodeAskContext({ type: 'ask_context', context: { ...reply, view: { ...reply.view, relations: [wrongEndpoint] } } })).toThrow();
    const secondCall = { ...call, id: { ...call.id, sequence: '9' } };
    const secondRef = { kind: 'part' as const, value: { message: secondCall.id, part_id: 0 } };
    const multiple = projectCausalChain({ kind: 'ask', ask, completion: null }, { kind: 'ask', context: { ...reply,
      view: { ...reply.view, source_records: [...reply.view.source_records, { kind: 'message', value: secondCall }],
        relations: [...reply.view.relations, { kind: 'tool_reference_candidate', from: secondRef, to: target, evidence: [secondRef, target] }] } } });
    expect(multiple.steps.filter(step => step.stage === 'upstream')).toHaveLength(2);
    const multipleMarkup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(CausalChain, { model: multiple }) }));
    expect(multipleMarkup.split(translate('ja-JP', 'chain.candidate'))).toHaveLength(5); // Two calls, the independent result, and the explicitly unconfirmed connection.
    expect(multipleMarkup).toContain('data-candidate="true"');
    expect(multiple.issues.some(item => item.at === missingNative && item.reason === 'not_provided')).toBe(true);
    expect(reply.candidate_windows[0]).toBe(frozen);
    expect(frozen.message_refs).toEqual([call.id]);
  });

  it('keeps all twenty ordered references, roles, empty values and tool results as their original objects', () => {
    const roles: ChatMessage['role'][] = [{ kind: 'system' }, { kind: 'developer' }, { kind: 'user' },
      { kind: 'assistant' }, { kind: 'tool' }, { kind: 'other', value: '' }];
    const messages = Array.from({ length: 20 }, (_, index) => ({ ...message(String(100 - index)),
      role: roles[index % roles.length]! }));
    messages[0]!.observed_parts = [
      { type: 'text', id: 0, kind: 'plain', text: '' },
      { type: 'tool_call', id: 1, native_id: null, name: '', arguments: null },
      { type: 'tool_result', id: 2, native_id: null, content: { kind: 'json', value: [null, '', false, 0, []] } },
      { type: 'opaque', id: 3, provider_type: '', reason: '' },
      { type: 'media_ref', id: 4, kind: 'image', safe_label: 'original media reference' },
    ];
    const reply = context(messages);
    reply.view = { ...reply.view, source_records: [...reply.view.source_records].reverse() };
    const projected = askAuditWindows(ask, reply)[0]!;
    expect(projected.original).toBe(reply.window);
    expect(projected.kind).toBe('unverified');
    expect(projected.messages).toHaveLength(20);
    for (const [index, entry] of projected.messages.entries()) {
      expect(entry.reference).toBe(reply.window!.message_refs[index]);
      expect(entry.message).toBe(messages[index]);
      expect(entry.problem).toBeNull();
    }
    const markup = render(reply);
    expect(markup.match(/class="chat-message stack-tight"/g)).toHaveLength(20);
    expect(markup).toContain('<pre class="chat-body"></pre>');
    expect(markup).toContain(translate('ja-JP', 'chat.toolResult'));
    expect(markup).toContain('false'); expect(markup).toContain('original media reference');
    expect(markup).toContain(translate('ja-JP', 'auditWindow.sourceGaps'));
    expect(markup).toContain(translate('ja-JP', 'auditWindow.earlier', { count: 3 }));
    expect(reply.window!.source_gaps[0]).toEqual({ kind: 'original gap', value: '' });
    expect(askAuditWindows(ask, context([]))[0]!.messages).toEqual([]);
    expect(render(context([]))).toContain(translate('ja-JP', 'auditWindow.empty'));
  });

  it('keeps missing, invalid and conflicting IDs in place without selecting the newest revision or another exchange', () => {
    const original = message('1'), newer = { ...original, revision: '2' };
    const invalid = { ...message('2'), observed_parts: [{ type: 'text', id: 0, kind: 'plain' }] };
    const missing = message('3'), available = message('4');
    const reply = context([original, message('2'), missing, available]);
    const gap = { at: { kind: 'message' as const, value: { ...original.id } }, reason: 'conflicting_evidence' as const };
    reply.view = { ...reply.view, gaps: [gap], source_records: [
      { kind: 'message', value: original }, { kind: 'message', value: newer },
      { kind: 'message', value: invalid }, { kind: 'message', value: message('3', otherExchange) },
      { kind: 'message', value: available }, { kind: 'message', value: available },
    ] };
    const entries = askAuditWindows(ask, reply)[0]!.messages;
    expect(entries.map(entry => entry.problem)).toEqual(['conflicting', 'invalid', 'missing', null]);
    expect(entries.map(entry => entry.reference)).toEqual(reply.window!.message_refs);
    expect(entries[0]!.message).toBeNull(); expect(entries[0]!.originals[0]).toBe(original);
    expect(entries[0]!.originals[1]).toBe(newer); expect(entries[0]!.gaps[0]).toBe(gap);
    expect(entries[1]!.originals[0]).toBe(invalid); expect(entries[2]!.originals).toEqual([]);
    expect(entries[3]!.message).toBe(available);
    const markup = render(reply);
    for (const key of ['auditWindow.messageConflict', 'auditWindow.messageInvalid', 'auditWindow.messageMissing'] as const)
      expect(markup).toContain(translate('ja-JP', key));
    expect(markup.match(/class="chat-message stack-tight"/g)).toHaveLength(1);
  });

  it('labels only the proven Ask exchange as confirmed and same-page equal exchanges as candidates', () => {
    const original = { ...message('1'), origin: 'response_output' as const, role: { kind: 'assistant' as const } };
    original.source = { ...original.source, side: 'upstream_response' };
    const native = { namespace: 'tool_call', field_path: '/call_id', value: { kind: 'json_string', value: '' } };
    original.observed_parts = [{ type: 'tool_call', id: 0, native_id: native, name: '', arguments: null }];
    const call = { message: original.id, part_id: 0 };
    const candidateMessage = message('2', otherExchange);
    candidateMessage.origin = 'response_output'; candidateMessage.role = { kind: 'assistant' };
    candidateMessage.source = { ...candidateMessage.source, side: 'upstream_response' };
    candidateMessage.observed_parts = [{ type: 'tool_call', id: 0, native_id: native, name: '', arguments: null }];
    const candidateWindow = window([candidateMessage], otherExchange);
    const reply = context([original]);
    reply.trigger_call = call;
    reply.view = { ...reply.view, source_records: [...reply.view.source_records,
      { kind: 'question', value: ask }, { kind: 'message', value: candidateMessage },
      { kind: 'protocol', value: { id: original.record_refs[0], source: original.source,
        native_ids: [native], request_session: null } }],
    relations: [{ kind: 'tool_execution', from: { kind: 'part', value: call }, to: { kind: 'event', value: event },
      evidence: [{ kind: 'part', value: call }, { kind: 'event', value: event }] }] };
    const attribution: ToolAttribution = { event, pattern: { actor: 'same_process', lookback_ns: '0',
      target: { type: 'file_path', pointer: '' } }, required_connection_process: { boot_id: id, host_tgid: 1, birth_boottime_ns: '0' },
    required_connection_thread: null, event_boottime_ns: '0', event_target: null, candidates: [
      { call: { message: candidateMessage.id, part_id: 0 }, message_revision: '1', response_observed_ns: '0', target_comparison: 'equal' },
    ], next_after: null };
    reply.attribution = attribution; reply.candidate_windows = [candidateWindow];
    expect(askAuditWindows(ask, reply).map(item => item.kind)).toEqual(['confirmed', 'candidate']);
    expect(askAuditWindows(ask, { ...reply, trigger_call: null }).map(item => item.kind)).toEqual(['unverified', 'candidate']);
    expect(askAuditWindows(ask, { ...reply, view: { ...reply.view, relations: [] } }).map(item => item.kind))
      .toEqual(['unverified', 'candidate']);
    expect(askAuditWindows({ ...ask, decision_id: Array<number>(16).fill(2) }, reply).map(item => item.kind))
      .toEqual(['unverified', 'unverified']);
    for (const changed of [null, { ...attribution, candidates: [{ ...attribution.candidates[0]!, target_comparison: 'different' as const }] },
      { ...attribution, candidates: [{ ...attribution.candidates[0]!, call }] },
      { ...attribution, event: { ...event, sequence: '3' } }]) {
      const projected = askAuditWindows(ask, { ...reply, attribution: changed });
      expect(projected.map(item => item.kind)).toEqual(['confirmed', 'unverified']);
      expect(projected[1]!.original).toBe(candidateWindow);
      expect(projected[1]!.messages[0]!.message).toBe(candidateMessage);
    }
    const markup = render(reply);
    expect(markup).toContain(translate('ja-JP', 'auditWindow.confirmed'));
    expect(markup).toContain(translate('ja-JP', 'auditWindow.candidateNotice'));

    const wrongScope = { ...reply, window: { ...reply.window!, message_refs: [original.id, candidateMessage.id] },
      candidate_windows: [{ ...candidateWindow, message_refs: [candidateMessage.id, original.id] }] };
    const scoped = askAuditWindows(ask, wrongScope);
    expect(scoped.map(item => item.kind)).toEqual(['unverified', 'unverified']);
    for (const [index, foreign] of [candidateMessage, original].entries()) {
      const entry = scoped[index]!.messages[1]!;
      expect(entry.scopeMismatch).toBe(true); expect(entry.problem).toBeNull();
      expect(entry.reference).toBe(foreign.id); expect(entry.message).toBe(foreign);
      expect(entry.originals[0]).toBe(foreign);
    }
    const scopedMarkup = render(wrongScope);
    expect(scopedMarkup).toContain(translate('ja-JP', 'auditWindow.messageScopeMismatch'));
    expect(scopedMarkup).not.toContain(translate('ja-JP', 'auditWindow.messageMissing'));
    expect(scopedMarkup.match(/class="chat-message stack-tight"/g)).toHaveLength(2);
  });
});
