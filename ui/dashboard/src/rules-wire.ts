// Rule schema owner: frozen model.rs and PolicySnapshot / RuleWriteReceipt in lib.rs.
// Shape checks only. Compilation, stage legality and maintenance protection belong to the server.
import { arrayOf, boolean, bytes, exact, integer, invalid, numericU64, oneOf, record, string,
  type JsonU64 } from './wire-json';
import { managementCommand, type CommandId } from './request-wire';
import { decodeDriverStatus, type DriverStatus } from './driver-wire';
import { decodeContainerInstance, type ContainerInstance } from './workload-wire';

export const ruleKinds = ['process', 'file', 'network'] as const;
export type RuleKind = typeof ruleKinds[number];
export const placements = ['driver', 'remembered', 'fast', 'ask'] as const;
export type Placement = typeof placements[number];
export type RuleVersion = readonly number[];
export type RuleExecution = { location: 'driver' } | { location: 'backend'; stage: 'remembered' | 'fast' | 'ask' };
export type Scope = { kind: 'all_protected' } | { kind: 'containers'; instances: readonly ContainerInstance[] };
export type RuleDecision = { type: 'allow' } | { type: 'deny'; terminate: 'none' | 'actor' | 'actor_and_parent' };
export type RuleAction = RuleDecision | { type: 'to_user' } | { type: 'ask'; timeout_ms: JsonU64; timeout: RuleDecision };
export interface TextMatch { operator: 'exact' | 'prefix' | 'glob' | 'regex'; value: string }
export interface ArgumentMatch { index: number; pattern: TextMatch }
export interface ProcessMatch {
  pid?: number | null; ppid?: number | null; name?: TextMatch | null; executable?: TextMatch | null;
  arguments?: readonly ArgumentMatch[]; cwd?: TextMatch | null; uid?: number | null; gid?: number | null;
  euid?: number | null; egid?: number | null; parent_name?: TextMatch | null; parent_executable?: TextMatch | null;
}
export interface ProcessTarget { executable?: TextMatch | null; arguments?: readonly ArgumentMatch[] }
export interface FileTarget {
  path?: TextMatch | null; new_path?: TextMatch | null; device?: readonly number[] | null;
  inode?: JsonU64 | null; access_mask?: number | null; mount_effect?: 'independent_view' | null;
}
export interface NetworkTarget {
  local_cidr?: string | null; remote_cidr?: string | null;
  local_port?: { first: number; last: number } | null; remote_port?: { first: number; last: number } | null;
  protocol?: number | null; domain?: TextMatch | null;
}
export interface Rule<T = ProcessTarget | FileTarget | NetworkTarget> {
  id: string; number: number; enabled: boolean; scope: Scope; operations: readonly string[];
  source: ProcessMatch; target: T; action: RuleAction; log: boolean; deny_message: string;
}
export type RuleDefinition = { kind: 'process'; rule: Rule<ProcessTarget> }
  | { kind: 'file'; rule: Rule<FileTarget> } | { kind: 'network'; rule: Rule<NetworkTarget> };
export interface RuleGroup {
  kind: RuleKind; id: string; number: number; enabled: boolean; execution: RuleExecution; rules: readonly Rule[];
}
export interface PolicySnapshot {
  source: { encoding: 'unix_bytes' | 'utf16'; value: readonly number[] };
  document: { groups: readonly RuleGroup[] }; version: RuleVersion;
}
export interface RuleSnapshot { version: RuleVersion; group_id: string; rule: RuleDefinition }
export type RuleChange = { type: 'create_group'; group: RuleGroup }
  | { type: 'replace_group'; group_id: string; group: RuleGroup }
  | { type: 'delete_group'; group_id: string }
  | { type: 'create_rule'; group_id: string; rule: RuleDefinition }
  | { type: 'replace_rule'; rule_id: string; rule: RuleDefinition }
  | { type: 'delete_rule'; rule_id: string };
export type RuleCommand = { type: 'get' }
  | { type: 'edit'; expected_version: RuleVersion; change: RuleChange };
export interface RuleWriteReceipt { saved_version: RuleVersion; required_effects: readonly 'apply_driver'[] }
export type RulesSaved = { type: 'rules_saved'; receipt: RuleWriteReceipt; applied: DriverStatus | null };
export type RulesReplyData = { type: 'rules'; snapshot: PolicySnapshot }
  | { type: 'rule'; snapshot: RuleSnapshot } | RulesSaved
  | { type: 'rules_reloaded'; snapshot: PolicySnapshot; applied: DriverStatus };


const positive = (value: unknown) => integer(value, 1, 0xffffffff);
function textMatch(value: unknown): TextMatch {
  const o = exact(value, ['operator', 'value']);
  return { operator: oneOf(o.operator, ['exact', 'prefix', 'glob', 'regex'] as const), value: string(o.value) };
}
function argumentsMatch(value: unknown) {
  return arrayOf(value, item => {
    const o = exact(item, ['index', 'pattern']);
    return { index: integer(o.index, 0, Number.MAX_SAFE_INTEGER), pattern: textMatch(o.pattern) };
  });
}
function processMatch(value: unknown): ProcessMatch {
  const o = exact(value, [], ['pid', 'ppid', 'name', 'executable', 'arguments', 'cwd', 'uid', 'gid', 'euid', 'egid', 'parent_name', 'parent_executable']);
  for (const [key, val] of Object.entries(o)) {
    if (key === 'arguments') argumentsMatch(val);
    else if (val !== null) {
      if (['pid', 'ppid', 'uid', 'gid', 'euid', 'egid'].includes(key)) integer(val, 0, 0xffffffff);
      else textMatch(val);
    }
  }
  return o as ProcessMatch;
}
function target(value: unknown, kind: RuleKind): ProcessTarget | FileTarget | NetworkTarget {
  const fields = kind === 'process' ? ['executable', 'arguments'] : kind === 'file'
    ? ['path', 'new_path', 'device', 'inode', 'access_mask', 'mount_effect']
    : ['local_cidr', 'remote_cidr', 'local_port', 'remote_port', 'protocol', 'domain'];
  const o = exact(value, [], fields);
  for (const [key, val] of Object.entries(o)) {
    if (key === 'arguments') { argumentsMatch(val); continue; }
    if (val === null) continue;
    if (['executable', 'path', 'new_path', 'domain'].includes(key)) textMatch(val);
    else if (key === 'device') { if (!Array.isArray(val) || val.length !== 2) invalid(); arrayOf(val, n => integer(n, 0, 0xffffffff)); }
    else if (key === 'inode') numericU64(val);
    else if (key === 'access_mask') integer(val, 0, 0xffffffff);
    else if (key === 'mount_effect') oneOf(val, ['independent_view']);
    else if (key === 'protocol') integer(val, 0, 255);
    else if (key.endsWith('_cidr')) string(val);
    else {
      const ports = exact(val, ['first', 'last']);
      integer(ports.first, 0, 65535); integer(ports.last, 0, 65535);
    }
  }
  return o;
}
export function decodeScope(value: unknown): Scope {
  const o = record(value);
  if (o.kind === 'all_protected') { exact(o, ['kind']); return { kind: 'all_protected' }; }
  exact(o, ['kind', 'instances']);
  if (o.kind !== 'containers') return invalid();
  return { kind: 'containers', instances: arrayOf(o.instances, decodeContainerInstance) };
}
function ruleDecision(value: unknown): RuleDecision {
  const o = record(value);
  if (o.type === 'allow') { exact(o, ['type']); return { type: 'allow' }; }
  exact(o, ['type', 'terminate']);
  if (o.type !== 'deny') return invalid();
  return { type: 'deny', terminate: oneOf(o.terminate, ['none', 'actor', 'actor_and_parent'] as const) };
}
export function decodeAction(value: unknown): RuleAction {
  const o = record(value);
  if (o.type === 'to_user') { exact(o, ['type']); return { type: 'to_user' }; }
  if (o.type !== 'ask') return ruleDecision(value);
  exact(o, ['type', 'timeout_ms', 'timeout']);
  const timeout_ms = numericU64(o.timeout_ms);
  if (timeout_ms === 0 || timeout_ms === 0n) return invalid();
  return { type: 'ask', timeout_ms, timeout: ruleDecision(o.timeout) };
}
export const operationNames: Record<RuleKind, readonly string[]> = {
  process: ['execute', 'create', 'exit'],
  file: ['open', 'read', 'write', 'create_file', 'create_directory', 'unlink', 'remove_directory', 'link', 'rename',
    'truncate_open_file', 'change_mode', 'change_owner', 'grant_writable_mapping', 'mount', 'unmount', 'move_mount', 'set_metadata', 'truncate_path', 'map_shared_writable'],
  network: ['connect', 'send'],
};
function decodeRule(value: unknown, kind: RuleKind): Rule {
  const o = exact(value, ['id', 'number', 'enabled', 'scope', 'operations', 'source', 'target', 'action', 'log', 'deny_message']);
  return { id: string(o.id), number: positive(o.number), enabled: boolean(o.enabled), scope: decodeScope(o.scope),
    operations: arrayOf(o.operations, item => oneOf(item, operationNames[kind])), source: processMatch(o.source),
    target: target(o.target, kind), action: decodeAction(o.action), log: boolean(o.log), deny_message: string(o.deny_message) };
}
export function decodeRuleDefinition(value: unknown): RuleDefinition {
  const o = exact(value, ['kind', 'rule']);
  const kind = oneOf(o.kind, ruleKinds);
  return { kind, rule: decodeRule(o.rule, kind) } as RuleDefinition;
}
export function decodeRuleGroup(value: unknown): RuleGroup {
  const o = exact(value, ['kind', 'id', 'number', 'enabled', 'execution', 'rules']);
  const kind = oneOf(o.kind, ruleKinds);
  const e = record(o.execution);
  let execution: RuleExecution;
  if (e.location === 'driver') { exact(e, ['location']); execution = { location: 'driver' }; }
  else {
    exact(e, ['location', 'stage']);
    if (e.location !== 'backend') return invalid();
    execution = { location: 'backend', stage: oneOf(e.stage, ['remembered', 'fast', 'ask'] as const) };
  }
  return { kind, id: string(o.id), number: positive(o.number), enabled: boolean(o.enabled), execution,
    rules: arrayOf(o.rules, item => decodeRule(item, kind)) };
}
function policySnapshot(value: unknown): PolicySnapshot {
  const o = exact(value, ['source', 'document', 'version']);
  const source = exact(o.source, ['encoding', 'value']);
  const encoding = oneOf(source.encoding, ['unix_bytes', 'utf16'] as const);
  return { source: { encoding, value: arrayOf(source.value, n => integer(n, 0, encoding === 'unix_bytes' ? 255 : 65535)) },
    document: { groups: arrayOf(exact(o.document, ['groups']).groups, decodeRuleGroup) }, version: bytes(o.version, 32) };
}
export function decodeRulesData(value: unknown): RulesReplyData {
  const o = record(value);
  if (o.type === 'rules_saved') {
    exact(o, ['type', 'receipt', 'applied']);
    const receipt = exact(o.receipt, ['saved_version', 'required_effects']);
    return { type: 'rules_saved', receipt: { saved_version: bytes(receipt.saved_version, 32),
      required_effects: arrayOf(receipt.required_effects, item => oneOf(item, ['apply_driver'] as const)) },
    applied: o.applied === null ? null : decodeDriverStatus(o.applied) };
  }
  if (o.type === 'rule') {
    exact(o, ['type', 'snapshot']);
    const s = exact(o.snapshot, ['version', 'group_id', 'rule']);
    return { type: 'rule', snapshot: { version: bytes(s.version, 32), group_id: string(s.group_id), rule: decodeRuleDefinition(s.rule) } };
  }
  if (o.type === 'rules') {
    exact(o, ['type', 'snapshot']); return { type: 'rules', snapshot: policySnapshot(o.snapshot) };
  }
  exact(o, ['type', 'snapshot', 'applied']);
  if (o.type !== 'rules_reloaded') return invalid();
  return { type: 'rules_reloaded', snapshot: policySnapshot(o.snapshot), applied: decodeDriverStatus(o.applied) };
}
export function decodeChange(value: unknown): RuleChange {
  const o = record(value);
  switch (o.type) {
    case 'create_group': exact(o, ['type', 'group']); return { type: o.type, group: decodeRuleGroup(o.group) };
    case 'replace_group': exact(o, ['type', 'group_id', 'group']); return { type: o.type, group_id: string(o.group_id), group: decodeRuleGroup(o.group) };
    case 'delete_group': exact(o, ['type', 'group_id']); return { type: o.type, group_id: string(o.group_id) };
    case 'create_rule': exact(o, ['type', 'group_id', 'rule']); return { type: o.type, group_id: string(o.group_id), rule: decodeRuleDefinition(o.rule) };
    case 'replace_rule': exact(o, ['type', 'rule_id', 'rule']); return { type: o.type, rule_id: string(o.rule_id), rule: decodeRuleDefinition(o.rule) };
    case 'delete_rule': exact(o, ['type', 'rule_id']); return { type: o.type, rule_id: string(o.rule_id) };
    default: return invalid();
  }
}
export const placement = (group: RuleGroup): Placement => group.execution.location === 'driver' ? 'driver' : group.execution.stage;
export const definition = (kind: RuleKind, rule: Rule) => ({ kind, rule }) as RuleDefinition;
export function rulesCommand(id: CommandId, command: RuleCommand): string {
  if (command.type === 'edit') bytes(command.expected_version, 32);
  return managementCommand(id, 'rules', command);
}
