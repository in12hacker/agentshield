import { describe, expect, it } from 'vitest';
import {
  activityContextCommand, askContextCommand, decodeActivityContext, decodeAskContext,
  decodeAttributionPattern, decodeAttributionRequest, decodeToolAttribution, decodeToolAttributionData,
  matchesAttribution,
  type AttributionPattern, type AttributionRequest, type ToolAttribution,
} from './context-wire';
import { parseUnique } from './wire-json';

const id = Array<number>(16).fill(7);
const event = { source_instance: id, sequence_domain: '1', sequence: '18446744073709551615' };
const exchange = { flow_instance: id, sequence: '9007199254740993' };
const message = { exchange, sequence: '9223372036854775808' };
const pattern: AttributionPattern = { actor: 'same_process', lookback_ns: '0', target: { type: 'file_path', pointer: '' } };
const request: AttributionRequest = { pattern, after: message, limit: 64 };
const attribution = (): ToolAttribution => ({
  event, pattern, required_connection_process: { boot_id: id, host_tgid: 0, birth_boottime_ns: '0' },
  required_connection_thread: null, event_boottime_ns: '18446744073709551615', event_target: { type: 'bytes', value: [] },
  candidates: [
    { call: { message, part_id: 0 }, message_revision: '18446744073709551615', response_observed_ns: '0', target_comparison: 'equal' },
    { call: { message, part_id: 1 }, message_revision: '18446744073709551615', response_observed_ns: '0', target_comparison: 'different' },
  ], next_after: message,
});

describe('current read-only attribution wire', () => {
  it('uses only explicit request conditions and keeps general context reads unfiltered', () => {
    expect(decodeAttributionRequest(request)).toBe(request);
    expect(parseUnique(activityContextCommand(id, { kind: 'event', value: event }, request))).toMatchObject({ business: {
      query: { type: 'activity_context', anchor: { kind: 'event', value: event }, attribution: request },
    } });
    expect(parseUnique(askContextCommand(id, id, request))).toMatchObject({ business: {
      query: { type: 'ask_context', decision_id: id, attribution: request },
    } });
    for (const command of [activityContextCommand(id, { kind: 'event', value: event }), askContextCommand(id, id)])
      expect(parseUnique(command)).toMatchObject({ business: { query: { attribution: null } } });
    for (const target of [
      { type: 'exec_argument', pointer: '/command/~0~1', index: 0 },
      { type: 'executable', pointer: '' },
      { type: 'network_peer', address_pointer: '/address', port_pointer: '/port' },
    ]) expect(decodeAttributionPattern({ ...pattern, target })).toMatchObject({ target });
    for (const bad of [
      { ...request, pattern: { ...pattern, lookback_ns: 0 } },
      { ...request, pattern: { ...pattern, lookback_ns: '00' } },
      { ...request, pattern: { ...pattern, target: { type: 'file_path' } } },
      { ...request, pattern: { ...pattern, target: { type: 'file_path', pointer: '/~2' } } },
      { ...request, pattern: { ...pattern, actor: 'agent_name' } },
      { ...request, pattern: { ...pattern, target: { type: 'network_peer', address_pointer: '/ip' } } },
      { ...request, limit: 0 }, { ...request, limit: 65 }, { ...request, after: undefined },
    ]) {
      expect(() => decodeAttributionRequest(bad)).toThrow();
      expect(() => activityContextCommand(id, { kind: 'event', value: event }, bad as AttributionRequest)).toThrow();
    }
  });

  it('validates complete conditions and keeps every original candidate, scalar and reference', () => {
    const original = attribution();
    expect(decodeToolAttribution(original)).toBe(original);
    expect(decodeToolAttribution(original).candidates).toBe(original.candidates);
    expect(decodeToolAttribution(original).candidates[0]!.call.message).toBe(message);
    expect(decodeToolAttribution(original).event_target).toBe(original.event_target);
    expect(decodeToolAttribution(original).event_boottime_ns).toBe('18446744073709551615');
    const wrapper = { type: 'tool_attribution', view: original };
    expect(decodeToolAttributionData(wrapper)).toBe(wrapper);
    expect(decodeToolAttribution({ ...original, candidates: [], next_after: null, event_target: null }).candidates).toEqual([]);
    for (const target_comparison of ['event_target_unavailable', 'argument_unavailable', 'source_incomplete'])
      expect(decodeToolAttribution({ ...original, candidates: [{ ...original.candidates[0], target_comparison }] }).candidates[0]!.target_comparison)
        .toBe(target_comparison);
    const thread = { tid: 0, birth_boottime_ns: '0' };
    expect(decodeToolAttribution({ ...original, pattern: { ...pattern, actor: 'same_thread' }, required_connection_thread: thread })
      .required_connection_thread).toBe(thread);
    for (const bad of [
      { ...original, event_boottime_ns: 9007199254740993 },
      { ...original, required_connection_thread: thread },
      { ...original, pattern: { ...pattern, actor: 'same_thread' } },
      { ...original, candidates: [{ ...original.candidates[0], message_revision: '18446744073709551616' }] },
      { ...original, candidates: [{ ...original.candidates[0], target_comparison: 'confirmed' }] },
      { ...original, candidates: [{ ...original.candidates[0], call: { message, part_id: -1 } }] },
      { ...original, required_connection_process: null },
      { ...original, inferred_relation: true },
    ]) expect(() => decodeToolAttribution(bad)).toThrow();
  });

  it('keeps network originals and rejects values outside the current IpAddr/u16 shape', () => {
    const original = attribution();
    for (const address of ['0.0.0.0', '255.255.255.255', '::1', '::ffff:192.0.2.1']) {
      const target = { type: 'network_peer', address, port: 0 };
      expect(decodeToolAttribution({ ...original, event_target: target }).event_target).toBe(target);
    }
    for (const target of [
      { type: 'network_peer', address: '256.0.0.1', port: 1 },
      { type: 'network_peer', address: 'localhost', port: 1 },
      { type: 'network_peer', address: '::gg', port: 1 },
      { type: 'network_peer', address: '::1]/?wrong[', port: 1 },
      { type: 'network_peer', address: '127.0.0.1', port: 65536 },
      { type: 'bytes', value: [256] },
    ]) expect(() => decodeToolAttribution({ ...original, event_target: target })).toThrow();
  });

  it('validates context shapes and leaves request identity checks to the original read slot', () => {
    const view = { anchor: { kind: 'event', value: event }, source_records: [], relations: [], gaps: [] };
    const original = attribution();
    const activity = { type: 'activity_context', view, attribution: original };
    expect(decodeActivityContext(activity)).toBe(activity);
    expect(decodeActivityContext(activity).view.relations).toBe(view.relations);
    const mismatch = { ...original, event: { ...event, sequence: '1' } };
    expect(decodeActivityContext({ ...activity, attribution: mismatch }).attribution).toBe(mismatch);
    expect(matchesAttribution(original, request, event)).toBe(true);
    expect(matchesAttribution(mismatch, request, event)).toBe(false);
    expect(matchesAttribution(original, null, event)).toBe(false);
    expect(matchesAttribution(null, request, event)).toBe(true);
    expect(matchesAttribution(null, null, event)).toBe(true);
    expect(matchesAttribution({ ...original, pattern: { ...pattern, lookback_ns: '1' } }, request, event)).toBe(false);
    const window = { exchange, message_refs: Array.from({ length: 20 }, (_, i) => ({ exchange, sequence: String(i) })),
      earlier_observed_messages: 0, source_gaps: [] };
    const context = { decision_id: id, event, agent: null, trigger_call: null, window: null,
      view: { ...view, anchor: { kind: 'question', value: id } }, command_checks: [], attribution: original, candidate_windows: [window] };
    const ask = { type: 'ask_context', context };
    expect(decodeAskContext(ask)).toBe(ask);
    expect(decodeAskContext(ask).context.attribution).toBe(original);
    expect(decodeAskContext(ask).context.candidate_windows[0]).toBe(window);
    expect(decodeAskContext(ask).context.trigger_call).toBeNull();
    expect(decodeAskContext(ask).context.window).toBeNull();
    expect(decodeAskContext({ ...ask, context: { ...context, attribution: mismatch } }).context.attribution).toBe(mismatch);
    expect(decodeAskContext({ ...ask, context: { ...context, decision_id: Array<number>(16).fill(1) } }).context.decision_id).toEqual(Array<number>(16).fill(1));
    expect(() => decodeAskContext({ ...ask, context: { ...context, candidate_windows: [{ ...window, message_refs: [...window.message_refs, message] }] } })).toThrow();
    const wrongWindow = { ...window, message_refs: [{ ...message, exchange: { ...exchange, sequence: '1' } }] };
    expect(decodeAskContext({ ...ask, context: { ...context, candidate_windows: [wrongWindow] } }).context.candidate_windows[0]).toBe(wrongWindow);
  });
});
