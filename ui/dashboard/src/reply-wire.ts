// Frozen business.rs / protocol.rs / driver_control.rs / flow.rs / driver.rs.
// Every serialized field is required. Only the original Option fields accept null.
import { boolean, bytes, decimalU64, exact, failureCode, invalid, oneOf,
  record, string, type Facts, type FailureCode } from './wire-json';
import type { Message } from './messages';
import { decodeRulesData, type RulesReplyData } from './rules-wire';
import { decodeDriverStatus, type DriverStatus } from './driver-wire';
import { decodeProcessesData, type ProcessesReplyData } from './processes-wire';
import { decodeSecurityLog, type SecurityLogData } from './activity-wire';
import { decodeConfigurationData, type ConfigurationData } from './settings-wire';
import { decodeActivityContext, decodeAskContext, decodeToolAttributionData, type ToolAttributionData, type ActivityContextData, type AskContextData } from './context-wire';
import { decodeGroupData, type GroupData } from './group-wire';
import { decodeChatData, type ChatData } from './chat-wire';

import type { CommandId } from './request-wire';
export interface FlowStatus {
  intake_state: 'running' | 'source_closed' | 'stopped';
  mode: 'observe_tls' | 'mediate_tls';
  decoder_profiles: readonly ('open_ai_responses' | 'open_ai_chat_completions' | 'anthropic_messages')[];
  active_exchanges: string;
  lost_deliveries: string;
  parse_failures: string;
}
export interface TlsIngressStatus {
  generation: string;
  accepting: boolean;
  admitted: string;
  rejected: string;
}
export type ReplyData = { type: 'driver_status'; snapshot: DriverStatus }
  | { type: 'flow_status'; ingress: TlsIngressStatus | null; status: FlowStatus }
  | RulesReplyData
  | ProcessesReplyData
  | SecurityLogData
  | ConfigurationData
  | (Facts & { type: 'main_stop_requested' | 'processes_refreshed' });
export type CommandResult = { status: 'accepted' | 'completed'; data: ReplyData }
  | { status: 'failed'; code: FailureCode; message: string; facts: ReplyData | null };
export interface CommandReply { id: CommandId; result: CommandResult }
export function commandFacts(result: CommandResult | undefined): ReplyData | null {
  return result ? result.status === 'failed' ? result.facts : result.data : null;
}
export type BusinessFailure = { status: 'failed'; code: FailureCode; message: string };
export type OverviewResult = { status: 'completed'; data: {
  type: 'protection_overview'; driver: CommandResult; flow: CommandResult;
} } | BusinessFailure;
export type ContextResult = { status: 'completed'; data: ActivityContextData } | BusinessFailure;
export type AskContextResult = { status: 'completed'; data: AskContextData } | BusinessFailure;
export type BusinessResult = OverviewResult | ContextResult | AskContextResult
  | { status: 'completed'; data: GroupData | ChatData | ToolAttributionData };
export interface BusinessReply { id: CommandId; result: BusinessResult }
export type OverviewRead = { phase: 'initial' }
  | { phase: 'loading'; id: CommandId }
  | { phase: 'returned'; id: CommandId; result: OverviewResult }
  | { phase: 'send_failed'; id: CommandId | null; message: Message }
  | { phase: 'failed'; id: CommandId; message: Message };

function flowStatus(value: unknown): FlowStatus {
  const object = exact(value, ['intake_state', 'mode', 'decoder_profiles', 'active_exchanges', 'lost_deliveries', 'parse_failures']);
  if (!Array.isArray(object.decoder_profiles)) return invalid();
  return {
    intake_state: oneOf(object.intake_state, ['running', 'source_closed', 'stopped'] as const),
    mode: oneOf(object.mode, ['observe_tls', 'mediate_tls'] as const),
    decoder_profiles: object.decoder_profiles.map(profile => oneOf(profile,
      ['open_ai_responses', 'open_ai_chat_completions', 'anthropic_messages'] as const)),
    active_exchanges: decimalU64(object.active_exchanges), lost_deliveries: decimalU64(object.lost_deliveries),
    parse_failures: decimalU64(object.parse_failures),
  };
}
function tlsIngressStatus(value: unknown): TlsIngressStatus {
  const object = exact(value, ['generation', 'accepting', 'admitted', 'rejected']);
  return { generation: decimalU64(object.generation), accepting: boolean(object.accepting),
    admitted: decimalU64(object.admitted), rejected: decimalU64(object.rejected) };
}
function replyData(value: unknown): ReplyData {
  const object = record(value);
  if (['rules', 'rule', 'rules_saved', 'rules_reloaded'].includes(String(object.type))) return decodeRulesData(object);
  if (object.type === 'processes') return decodeProcessesData(object);
  if (object.type === 'security_log') return decodeSecurityLog(object);
  if (['configuration', 'configuration_saved', 'configuration_reloaded'].includes(String(object.type))) return decodeConfigurationData(object);
  if (object.type === 'driver_status') {
    exact(object, ['type', 'snapshot']);
    return { type: 'driver_status', snapshot: decodeDriverStatus(object.snapshot) };
  }
  if (object.type === 'flow_status') {
    exact(object, ['type', 'ingress', 'status']);
    return { type: 'flow_status', ingress: object.ingress === null ? null : tlsIngressStatus(object.ingress), status: flowStatus(object.status) };
  }
  // Other current ReplyData types supply no status for this read; retain their facts.
  const type = oneOf(object.type, ['main_stop_requested', 'processes_refreshed'] as const);
  return { ...object, type };
}
export function decodeCommandResult(value: unknown): CommandResult {
  const object = record(value);
  if (object.status === 'failed') {
    exact(object, ['status', 'code', 'message', 'facts']);
    return { status: 'failed', code: failureCode(object.code), message: string(object.message),
      facts: object.facts === null ? null : replyData(object.facts) };
  }
  exact(object, ['status', 'data']);
  return { status: oneOf(object.status, ['accepted', 'completed'] as const), data: replyData(object.data) };
}
export function decodeBusinessReply(value: unknown): BusinessReply {
  const object = exact(value, ['id', 'result']);
  const id = bytes(object.id, 16);
  const result = record(object.result);
  if (result.status === 'failed') {
    exact(result, ['status', 'code', 'message']);
    return { id, result: { status: 'failed', code: failureCode(result.code), message: string(result.message) } };
  }
  exact(result, ['status', 'data']);
  if (result.status !== 'completed') return invalid();
  const data = record(result.data);
  if (data.type === 'activity_context') return { id, result: { status: 'completed', data: decodeActivityContext(data) } };
  if (data.type === 'tool_attribution') return { id, result: { status: 'completed', data: decodeToolAttributionData(data) } };
  if (data.type === 'ask_context') return { id, result: { status: 'completed', data: decodeAskContext(data) } };
  if (data.type === 'group_delegations') return { id, result: { status: 'completed', data: decodeGroupData(data) } };
  if (data.type === 'chat_sessions' || data.type === 'chat_exchanges' || data.type === 'chat_messages') return { id, result: { status: 'completed', data: decodeChatData(data) } };
  exact(data, ['type', 'driver', 'flow']);
  if (data.type !== 'protection_overview') return invalid();
  return { id, result: { status: 'completed', data: { type: 'protection_overview',
    driver: decodeCommandResult(data.driver), flow: decodeCommandResult(data.flow) } } };
}
