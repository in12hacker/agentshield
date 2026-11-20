// Single synthetic fact owner for Vitest and browser contract consumption. No service/driver effect.
import type { Ask, OperationResult, Outcome } from '../src/event-wire';
import type { PolicySnapshot, RulesSaved } from '../src/rules-wire';
import type { FieldStamp, ProcessCursor, ProcessDetails, ProcessPage, ProcessRecord, ProcessSnapshot } from '../src/processes-wire';
import type { GroupDelegation, GroupPage } from '../src/group-wire';
import type { LogPage, RuleLog } from '../src/activity-wire';
import { gapReasons, type AskAuditContext, type ContextView, type ExchangeKey } from '../src/context-wire';
import { configPathFields, decodeConfigSnapshot, type ConfigDocument } from '../src/settings-wire';
import type { ChatMessage } from '../src/chat-wire';

export const id = Array.from({ length: 16 }, (_, i) => i + 1);
export const allow = { type: 'allow' } as const;
export const deny = { type: 'deny', terminate: 'none' } as const;
export const testToken = 'a'.repeat(64);
export const process = Object.fromEntries([
  'kind', 'exec_cookie', 'pid_namespace_inode', 'net_namespace_inode', 'user_namespace_inode',
  'mount_namespace_inode', 'namespace_tgids', 'workload', 'name', 'executable', 'argv', 'cwd',
  'uid', 'euid', 'suid', 'fsuid', 'gid', 'egid', 'sgid', 'fsgid', 'parent_pid',
  'parent_birth_boottime_ns', 'parent_name', 'parent_executable', 'parent_argv',
].map(key => [key, null]));
export const ask: Ask = {
  type: 'ask', decision_id: id,
  hit: { group_id: 'file-review', group_number: 1, rule_id: 'file-open', rule_number: 1,
    source: { kind: 'rules_document', version: Array(32).fill(2) }, log: false, deny_message: '' },
  event: {
    header: { kind: 'file', stage: 'controlled_request', operation: 1,
      source_instance: Array(16).fill(3), sequence_domain: '18446744073709551615',
      sequence: '9007199254740993', source_boottime_ns: '1000000',
      actor_tgid: 42, actor_tid: 42, actor_birth_boottime_ns: '100',
      actor_thread_birth_boottime_ns: '100', actor_cgroup_id: null, actor_cgroup_lineage: null },
    host_boot_id: Array(16).fill(4), process: { ...process, name: [], argv: [[], [65]], uid: 0 },
    payload: { path: [47, 116, 109, 112, 47, 97], new_path: null, device: null, inode: '9',
      open_flags: 0, access_mask: 2, rename_flags: null, creation_mode: null,
      map_requested_prot: null, map_effective_prot: null, map_flags: null,
      truncate_size: null, truncate_attributes: null },
    syscall: null, kernel_stack: null,
    user_stack: { result: 16, snapshot: { instruction_pointers: ['0', '9007199254740993'], buffer_full: false } },
  },
  choices: [allow, deny], timeout: deny, remaining_ms: 5000,
};
export const wire = (message: unknown) => JSON.stringify({ version: 2, message });
export const feature = { startup_default: true, runtime_override: 'default', effective_state: 'enabled' };
export const driverStatus = {
  type: 'driver_status', snapshot: {
    instance: Array(16).fill(8), revision: 2, source_state: 'running', persistent_generation: 1,
    state_revision: 1, lifecycle_epoch: 1, driver_config_version: Array(32).fill(1),
    rule_version: Array(32).fill(2), startup_protection: { process: true, file: true, network: true },
    process: feature, file: feature, network: feature, self_protection: 'enabled',
    pending: 0, pending_receipts: 0, dropped: 0, error: null,
  },
};
export const completed = (outcome: Outcome = 'allowed', decisionId: readonly number[] | null = id): OperationResult => ({
  type: 'operation_result', decision_id: decisionId, event: ask.event, hit: ask.hit,
  result: { type: 'receipt', proposal: { decision: allow, reason: 'user' }, outcome,
    termination: { requested: 'none', actor: { status: 'not_requested' }, parent: { status: 'not_requested' } } },
});
export const flowStatus = { type: 'flow_status', ingress: null, status: {
  intake_state: 'running', mode: 'mediate_tls', decoder_profiles: [],
  active_exchanges: '9007199254740993', lost_deliveries: '0', parse_failures: '18446744073709551615',
} };
export const driverRead = { status: 'completed', data: driverStatus };
export const flowRead = { status: 'completed', data: flowStatus };
export const overviewResult = (driver: unknown = driverRead, flow: unknown = flowRead) => ({
  status: 'completed', data: { type: 'protection_overview', driver, flow },
});
export const rulePolicy: PolicySnapshot = {
  source: { encoding: 'unix_bytes', value: [47, 101, 116, 99, 47, 114, 117, 108, 101, 115] },
  version: Array(32).fill(2), document: { groups: [{ kind: 'file', id: 'saved-decisions', number: 1, enabled: true,
    execution: { location: 'backend', stage: 'remembered' }, rules: [{ id: 'saved-open', number: 1, enabled: true,
      scope: { kind: 'containers', instances: [{ runtime: 'docker', namespace: null, id: 'full-container-object-id' }] },
      operations: ['open'], source: { pid: null, ppid: null, name: null, executable: null, arguments: [], cwd: null,
        uid: 0, gid: null, euid: null, egid: null, parent_name: null, parent_executable: null },
      target: { path: { operator: 'exact', value: '' }, new_path: null, device: [0, 1], inode: 18446744073709551615n, access_mask: 2 },
      action: { type: 'deny', terminate: 'none' }, log: false, deny_message: '',
    }] }] },
};
export const savedRule: RulesSaved = { type: 'rules_saved', receipt: { saved_version: Array(32).fill(3), required_effects: ['apply_driver'] }, applied: null };
export const containerInstance = { runtime: 'docker', namespace: null, id: 'b'.repeat(64) };
export const processWindow = { begin_boottime_ns: '9007199254740993', end_boottime_ns: '9007199254740994' };
export const processStamp: FieldStamp = { evidence_epoch: '0', origin: { kind: 'startup_scan' }, window: processWindow, state: 'known' };
export function trackedSnapshot(): ProcessSnapshot {
  return { process: { ...process, name: [112, 114, 111, 99], executable: [47, 98, 105, 110, 47, 112],
    workload: { kind: 'container', instance: containerInstance }, argv: [[], [255]], uid: 0 },
  cgroup_id: '0', leader_state: 83, stamps: Object.fromEntries([
    'pid_namespace', 'net_namespace', 'user_namespace', 'mount_namespace', 'namespace_tgids',
    'suid', 'fsuid', 'sgid', 'fsgid', 'name', 'executable', 'argv', 'cwd', 'uid', 'euid', 'gid', 'egid',
    'parent_pid', 'parent_birth', 'parent_name', 'parent_executable', 'parent_argv', 'workload',
    'cgroup', 'exec_cookie', 'kind', 'leader_state',
  ].map(field => [field, { ...processStamp }])) };
}
export function trackedRecord(birth = '18446744073709551615'): ProcessRecord {
  return { key: { boot_id: Array(16).fill(4), host_tgid: 42, birth_boottime_ns: birth },
    initial: trackedSnapshot(), latest: trackedSnapshot(), lifecycle: 'present', first_observed: processWindow,
    exit_observed: null, exit_code: null, creator: null, revision: '9007199254740993' };
}
export function trackedPage(records: readonly ProcessRecord[] = [trackedRecord()], next_cursor: ProcessCursor | null = null): ProcessPage {
  return { records, next_cursor, tree_revision: '9007199254740993', status: {
    evidence_epoch: '0', phase: 'ready', instance: Array(16).fill(9), generation: '1', revision: '9007199254740993',
    nodes: 12, retired: 2, memory_used: 18446744073709551615n, tracking_epoch: '1', marker: '0', loss_epoch: '0',
    backend_rejected: '0', historical_gap: false,
  } };
}
export const instanceDetails = (record = trackedRecord(), evidence_epoch = '0'): ProcessDetails => ({
  evidence_epoch, record, initial_parent: { kind: 'root' }, latest_parent: { kind: 'unresolved', observed_pid: null }, children: 0,
});
export const parentKey = trackedRecord('9007199254740993').key;
export const childKey = { ...parentKey, birth_boottime_ns: '9007199254740994' };
export const otherBootKey = { ...parentKey, boot_id: Array(16).fill(5) };
export const delegationRow = (id: string, parent = parentKey, child = childKey): Extract<GroupDelegation, { kind: 'a2a' }> => ({
  id, kind: 'a2a', capture_id: Array(32).fill(6), parent, child, delegation: {
    request_id: null, message_id: '', task_id: null, context_id: null, terminal_state: null,
    parent_tool_id: null, parent_tool_span: null, client_span: null, server_span: null, trace_id: null,
    tool_call: [], tool_result: [], child_responses: [], gap: null,
  },
});
export const delegationPage = (): GroupPage => ({ records: [delegationRow('9223372036854775807'),
  delegationRow('9007199254740993', parentKey, otherBootKey)], next_before: '9007199254740993' });
export const groupResult = (page = delegationPage()) => ({ status: 'completed', data: { type: 'group_delegations', ...page } });
export const eventAnchor = { source_instance: ask.event.header.source_instance,
  sequence_domain: ask.event.header.sequence_domain, sequence: ask.event.header.sequence };
export const ruleLog: RuleLog = { decision_id: null, event: ask.event, hit: ask.hit,
  decision: { state: 'transferred' }, result: { Ok: { driver: {
    outcome: 'denied', termination: { requested: 'none', actor: { status: 'not_requested' }, parent: { status: 'not_requested' } },
  } } } };
export const logPage = (ids = ['9223372036854775807', '9007199254740993'], more = true): LogPage => ({
  records: ids.map(id => ({ id, record: ruleLog })), next_before: more ? ids.at(-1) ?? null : null,
});
export const contextView = (): ContextView => ({ anchor: { kind: 'event', value: eventAnchor }, source_records: [{
  kind: 'request_return', value: { event: eventAnchor, operation: 'file_open', decision_id: id,
    wait_id: '9007199254740993', returned_boottime_ns: '18446744073709551615', return_value: -13 },
}], relations: [], gaps: gapReasons.map(reason => ({ at: { kind: 'event', value: eventAnchor }, reason })) });
export const contextResult = (view = contextView()) => ({ status: 'completed', data: { type: 'activity_context', view, attribution: null } });
export const askContext = (): AskAuditContext => ({ decision_id: id, event: eventAnchor, agent: null, trigger_call: null,
  window: null, command_checks: [], attribution: null, candidate_windows: [],
  view: { anchor: { kind: 'question', value: id }, source_records: [{ kind: 'question', value: ask }],
    relations: [], gaps: [{ at: { kind: 'event', value: eventAnchor }, reason: 'no_explicit_relation' }] } });
export const askContextResult = (context = askContext()) => ({ status: 'completed', data: { type: 'ask_context', context } });
export const configDocument: ConfigDocument = {
  format_version: 2, log_storage: { total_bytes: 1073741824 }, queues: { callbacks: 64, review: 64, messages: 64, driver_urgent: 64 },
  process_tree: { memory_budget_bytes: 1048576, retired_retention_ms: 18446744073709551615n,
    reconcile_interval_ms: 9007199254740993n, query_page_size: 8 },
  driver: { startup_protection: { process: true, file: false, network: true },
    limits: { max_pending: 64, max_hold_ms: 5000 }, self_protection: { log: false } },
  middleware: { peer: { kind: 'unix_uid', uid: 0 }, access: 'manage_service', chat_content: false,
    websocket: { listen: '127.0.0.1:21854', origin: null, token_sha256: Array(32).fill(6) } },
  flow: null, credentials: { bindings: [] }, file_close_audit: [], group_observation: null,
};
export const configSnapshot = decodeConfigSnapshot({ document: configDocument, version: Array(32).fill(9),
  paths: Object.fromEntries(configPathFields.map(field => [field, { encoding: 'unix_bytes', value: [47, 255, 0] }])) });
export const savedConfiguration = { type: 'configuration_saved', receipt: { saved_version: Array(32).fill(10),
  changed_sections: ['driver'], required_effects: ['apply_driver'] }, applied: null };
export const egressBinding = { id: Array(16).fill(21), secret: Array(16).fill(22), enabled: true,
  subjects: [{ kind: 'process', value: { boot_id: Array(16).fill(23), host_tgid: 42, birth_boottime_ns: '9007199254740993' } },
    { kind: 'container', value: { runtime: 'fixture-runtime', namespace: '', id: 'fixture-instance' } }],
  routes: [Array(16).fill(24)], slot: 'authorization_bearer' };
export const egressRouteFact = { id: Array(16).fill(24), origin: 'https://fixture.invalid:443', method: 'POST',
  path: '/v1/%72esponses', profile: 'open_ai_responses', session: null };
export const egressSnapshot = decodeConfigSnapshot({ ...configSnapshot, document: { ...configDocument,
  credentials: { bindings: [egressBinding, { ...egressBinding, id: Array(16).fill(26), enabled: false }] },
  flow: { enabled: false, scope: { kind: 'processes', values: [egressBinding.subjects[0]!.value] }, mode: 'mediate_tls',
    provider_routes: [egressRouteFact], capture_content: false, decode_limits: { max_connection_buffer_bytes: 1024,
      max_message_bytes: 1024, max_active_exchanges: 2, max_json_depth: 8 }, event_queue_bytes: 1024,
    egress: { ca: Array(16).fill(25),
      max_connections: 2, max_streams_per_connection: 2 } } } });
export const syntheticSecret = ' synthetic-provider-input ';
export const syntheticPhantom = 'as_ph_synthetic-not-a-real-token';
export const syntheticCa = { id: Array(16).fill(25), certificate_der: [48, 0], fingerprint_sha256: Array(32).fill(1) };
export const phantomData = { type: 'phantom_issued', token_id: Array(16).fill(31), binding: egressBinding.id,
  revision: '18446744073709551615', value: syntheticPhantom };
export const historyExchange = { flow_instance: Array(16).fill(11), sequence: '18446744073709551615' };
export const anotherExchange = { ...historyExchange, sequence: '1' };
export function historyMessage(sequence = '9', exchange: ExchangeKey = historyExchange): ChatMessage {
  return {
    id: { exchange, sequence }, revision: '18446744073709551615',
    source: { actor: { process: parentKey, info: process, tid: 0, thread_birth_boottime_ns: null,
      cgroup_id: '0', observed_boottime_ns: '0' }, channel: null, side: 'client_request', request_traceparent: null,
      profile: 'open_ai_responses', origin: 'https://provider.example', route_label: '', config_version: Array(32).fill(0),
      observed_at_utc_ms: -9223372036854775808n, actual_auth_scope: null, credential_use: null },
    record_refs: [{ exchange, sequence: '0' }], origin: 'request_context', role: { kind: 'user' },
    candidate_index: 0, model: '', observed_parts: [{ type: 'text', id: 0, kind: 'plain', text: 'original user text' }],
    state: 'complete', quality: ['gap'], provider_status: '', finish_reason: null, usage: [{ name: '', value: '0' }],
    first_seen_boottime_ns: '0', last_seen_boottime_ns: '18446744073709551615',
  };
}
export const exchangesResult = (exchanges = [historyExchange, anotherExchange], next_after: typeof historyExchange | null = anotherExchange) =>
  ({ status: 'completed', data: { type: 'chat_exchanges', exchanges, next_after } });
export const messagesResult = (messages = [historyMessage()], next_after: ChatMessage['id'] | null = null) =>
  ({ status: 'completed', data: { type: 'chat_messages', messages: messages.map(message => ({ kind: 'model_http', message })), next_after: next_after === null ? null : { kind: 'model_http', message: next_after } } });
export const historyReceipt = (exchange = historyExchange): GroupDelegation => ({
  id: '9', kind: 'openclaw_tool_receipt', actor: parentKey,
  receipt: { kind: 'protocol', value: { exchange, sequence: '0' } },
  call_index: 3, output_index: 7, tool_call_id: 'original-call', child_run_id: 'original-run', child_session_key: 'original-session',
  gateway: { accepted_spawns: [], child_terminals: [], parent_publications: [] },
      gaps: ['gateway_receipt_unobserved', 'child_terminal_unobserved', 'parent_publication_unobserved'],
});
export const sessionReference = (sequence: string) => ({ kind: 'model_http', record: { exchange: historyExchange, sequence } });
export const originalSessions = () => ({ type: 'chat_sessions', sessions: ['json_string', 'json_number'].map((kind, index) => ({
  kind: 'model_http', reference: sessionReference(String(index)), source: historyMessage().source,
  session: { field: { kind: 'json_pointer', pointer: '/metadata/session' }, values: [{ kind: 'json', value: { kind, value: '7' } }], issue: null },
  exchanges: '18446744073709551615',
})), next_before: sessionReference('1') });

export function linkedContext(): AskAuditContext {
  const context = askContext();
  const exchange = { flow_instance: Array(16).fill(7), sequence: '9007199254740993' };
  const messageId = { exchange, sequence: '1' };
  const protocolId = { exchange, sequence: '2' };
  const call = { message: messageId, part_id: 0 };
  const native = { namespace: 'tool_call', field_path: '/tool_calls/0/id', value: { kind: 'json_string', value: '' } };
  const source = { origin: 'https://provider.example', observed_at_utc_ms: 0 };
  context.trigger_call = call;
  context.view = { ...context.view, gaps: [], source_records: [
    { kind: 'question', value: ask },
    { kind: 'message', value: { id: messageId, source, origin: 'response_output', state: 'complete', record_refs: [protocolId], observed_parts: [
      { type: 'tool_call', id: 0, native_id: native, name: '', arguments: { kind: 'text', value: '' } },
    ] } },
    { kind: 'protocol', value: { id: protocolId, source, native_ids: [native], request_session: null } },
  ], relations: [{ kind: 'tool_execution', from: { kind: 'part', value: call }, to: { kind: 'event', value: eventAnchor },
    evidence: [{ kind: 'part', value: call }, { kind: 'event', value: eventAnchor }] }] };
  return context;
}

// Same confirmed facts, augmented only with retained original message text for disclosure controls.
export const longOriginal = '原文 long original 🧪 '.repeat(36);
export function conversationContext(): AskAuditContext {
  const context = linkedContext();
  const call = context.trigger_call!;
  const complete = historyMessage(call.message.sequence, call.message.exchange);
  const responseSource = { ...complete.source, side: 'upstream_response', observed_at_utc_ms: 0 };
  const user = { ...historyMessage('0', call.message.exchange), quality: [],
    observed_parts: [{ type: 'text', id: 0, kind: 'plain', text: longOriginal }] };
  context.view.source_records = context.view.source_records.map(source => source.kind === 'message' ? {
    ...source, value: { ...complete, ...source.value, source: responseSource, role: { kind: 'assistant' }, quality: [], observed_parts: [
      { type: 'text', id: 1, kind: 'plain', text: longOriginal }, ...(source.value.observed_parts as unknown[]),
    ] },
  } : source.kind === 'protocol' ? { ...source, value: { ...source.value, source: responseSource } } : source);
  context.view.source_records = [...context.view.source_records, { kind: 'message', value: user }];
  context.window = { exchange: call.message.exchange, message_refs: [user.id, call.message], earlier_observed_messages: 0, source_gaps: [] };
  return context;
}
