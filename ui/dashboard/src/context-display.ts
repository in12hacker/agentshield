import type { ContextGap, SourceRecord } from './context-wire';
import type { MessageKey } from './messages';

export const sourceLabels = { request_return: 'evidence.requestReturn', futex_selection: 'evidence.futexSelection', request_handoff: 'evidence.requestHandoff',
  deferred: 'evidence.originalReference', protocol_page: 'evidence.protocol',
  tool_execution: 'evidence.toolExecution', question: 'evidence.question', operation_result: 'evidence.operationResult',
  rule_feedback: 'activity.feedbackTitle', protocol: 'evidence.protocol', message: 'evidence.message' } as const satisfies Record<SourceRecord['kind'], MessageKey>;
export const gapLabels = { agent_binding_unavailable: 'evidence.agentBindingUnavailable', command_mapping_unavailable: 'evidence.commandMappingUnavailable',
  command_input_unavailable: 'evidence.commandInputUnavailable', not_provided: 'evidence.notProvided',
  not_retained: 'evidence.notRetained', source_unavailable: 'evidence.sourceUnavailable',
  no_explicit_relation: 'evidence.noExplicitRelation', conflicting_evidence: 'evidence.conflictingEvidence' } satisfies Record<ContextGap['reason'], MessageKey>;
