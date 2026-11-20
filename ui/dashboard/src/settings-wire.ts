import { sameJsonValue } from './global/json-value';
// Current R7 configuration: task contract/r7-config.rs; original snapshot/command protocol.
import { arrayOf, boolean, bytes, exact, integer, invalid, numericU64, oneOf, record, string,
  type Facts, type JsonU64 } from './wire-json';
import { managementCommand, type CommandId } from './request-wire';
import { decodeDriverStatus, decodeProtectionDefaults, protectionKinds, type DriverStatus,
  type ProtectionDefaults, type ProtectionKind } from './driver-wire';
import { decodeEgressSettings, type CredentialBinding } from './egress-wire';
export type ConfigVersion = readonly number[];
export const configSections = ['queues', 'log_storage', 'process_tree', 'driver', 'middleware', 'flow', 'credentials',
  'file_close_audit', 'group_observation'] as const;
export type ConfigSection = typeof configSections[number];
export const configEffects = ['apply_credentials', 'apply_driver', 'reconnect_middleware', 'reinitialize_main'] as const;
export type ConfigEffect = typeof configEffects[number];
export interface ConfigDocument {
  readonly format_version: 2;
  readonly log_storage: { readonly total_bytes: JsonU64 };
  readonly queues: Readonly<Record<'callbacks' | 'review' | 'messages' | 'driver_urgent', JsonU64>>;
  readonly process_tree: Readonly<Record<'memory_budget_bytes' | 'retired_retention_ms' | 'reconcile_interval_ms' | 'query_page_size', JsonU64>>;
  readonly driver: {
    readonly startup_protection: ProtectionDefaults;
    readonly limits: { readonly max_pending: number; readonly max_hold_ms: number };
    readonly self_protection: { readonly log: boolean };
  };
  readonly middleware: {
    readonly peer: { readonly kind: 'unix_uid'; readonly uid: number } | { readonly kind: 'windows_sid'; readonly sid: string };
    readonly access: 'read_only' | 'manage_rules' | 'manage_service';
    readonly chat_content: boolean;
    readonly websocket: { readonly listen: string; readonly origin: string | null; readonly token_sha256: readonly number[] } | null;
  };
  // Original objects are retained; shape validation reuses the egress wire owner.
  readonly flow: Facts | null;
  readonly credentials: { readonly bindings: readonly CredentialBinding[] };
  readonly file_close_audit: readonly string[];
  readonly group_observation: { readonly a2a_port: number | null; readonly otlp_http_port: number | null;
    readonly gateway_ws_port: number | null } | null;
}
export const configPathFields = ['backend_executable', 'self_protection_root', 'configuration_file', 'rules_file',
  'driver_assets_directory', 'middleware_endpoint', 'data_directory', 'credential_store_file', 'ca_public_file',
  'security_log_file', 'middleware_database_file'] as const;
export interface NativePath { readonly encoding: 'unix_bytes' | 'utf16'; readonly value: readonly number[] }
export interface ConfigSnapshot {
  readonly document: ConfigDocument;
  readonly version: ConfigVersion;
  readonly paths: Readonly<Record<typeof configPathFields[number], NativePath>>;
}
export type ConfigCommand = { type: 'get' } | { type: 'update'; expected_version: ConfigVersion; document: ConfigDocument }
  | { type: 'reload'; expected_version: ConfigVersion };
export type RuntimeCommand = { type: 'set_runtime_feature'; feature: ProtectionKind; enabled: boolean }
  | { type: 'resume_runtime_feature'; feature: ProtectionKind };
export interface ConfigWriteReceipt {
  saved_version: ConfigVersion;
  changed_sections: readonly ConfigSection[];
  required_effects: readonly ConfigEffect[];
}
export type ConfigurationData = { type: 'configuration'; snapshot: ConfigSnapshot }
  | { type: 'configuration_saved'; receipt: ConfigWriteReceipt; applied: DriverStatus | null }
  | { type: 'configuration_reloaded'; snapshot: ConfigSnapshot; applied: DriverStatus };

function positive(value: unknown) { if (numericU64(value) === 0 || value === 0n) invalid(); }
function preservedJson(value: unknown): void {
  if (value === null || typeof value === 'string' || typeof value === 'boolean' || typeof value === 'bigint') return;
  if (typeof value === 'number') {
    // Current configuration numbers are integers. Reject rounded exponent/fraction
    // inputs and infinities rather than serializing a different value or null.
    if (!Number.isSafeInteger(value)) invalid();
    return;
  }
  if (Array.isArray(value)) value.forEach(preservedJson);
  else Object.values(record(value)).forEach(preservedJson);
}
export function decodeConfigDocument(value: unknown): ConfigDocument {
  const doc = exact(value, ['format_version', ...configSections]);
  if (doc.format_version !== 2) invalid();
  positive(exact(doc.log_storage, ['total_bytes']).total_bytes);
  const queues = exact(doc.queues, ['callbacks', 'review', 'messages', 'driver_urgent']);
  Object.values(queues).forEach(positive);
  const tree = exact(doc.process_tree, ['memory_budget_bytes', 'retired_retention_ms', 'reconcile_interval_ms', 'query_page_size']);
  Object.values(tree).forEach(positive);
  const driver = exact(doc.driver, ['startup_protection', 'limits', 'self_protection']);
  decodeProtectionDefaults(driver.startup_protection);
  const limits = exact(driver.limits, ['max_pending', 'max_hold_ms']);
  Object.values(limits).forEach(item => integer(item, 1, 0xffffffff));
  boolean(exact(driver.self_protection, ['log']).log);
  const middleware = exact(doc.middleware, ['peer', 'access', 'chat_content', 'websocket']);
  const peer = record(middleware.peer);
  if (peer.kind === 'unix_uid') integer(exact(peer, ['kind', 'uid']).uid, 0, 0xffffffff);
  else if (peer.kind === 'windows_sid') string(exact(peer, ['kind', 'sid']).sid);
  else invalid();
  oneOf(middleware.access, ['read_only', 'manage_rules', 'manage_service'] as const);
  boolean(middleware.chat_content);
  if (middleware.websocket !== null) {
    const ws = exact(middleware.websocket, ['listen', 'origin', 'token_sha256']);
    string(ws.listen);
    if (ws.origin !== null) string(ws.origin);
    bytes(ws.token_sha256, 32);
  }
  decodeEgressSettings(doc.flow, doc.credentials);
  preservedJson(doc.flow);
  preservedJson(doc.credentials);
  arrayOf(doc.file_close_audit, string);
  if (doc.group_observation !== null) {
    const group = exact(doc.group_observation, ['a2a_port', 'otlp_http_port', 'gateway_ws_port']);
    const ports = [group.a2a_port, group.otlp_http_port, group.gateway_ws_port]
      .filter(port => port !== null).map(port => integer(port, 1, 65535));
    if (ports.length === 0 || new Set(ports).size !== ports.length) invalid();
  }
  // Validation never rebuilds an unmodified subtree or supplies defaults.
  return doc as unknown as ConfigDocument;
}
function nativePath(value: unknown): NativePath {
  const path = exact(value, ['encoding', 'value']);
  const encoding = oneOf(path.encoding, ['unix_bytes', 'utf16'] as const);
  return { encoding, value: arrayOf(path.value, item => integer(item, 0, encoding === 'unix_bytes' ? 255 : 65535)) };
}
export function decodeConfigSnapshot(value: unknown): ConfigSnapshot {
  const snapshot = exact(value, ['document', 'version', 'paths']);
  const paths = exact(snapshot.paths, configPathFields);
  return { document: decodeConfigDocument(snapshot.document), version: bytes(snapshot.version, 32),
    paths: Object.fromEntries(configPathFields.map(field => [field, nativePath(paths[field])])) as ConfigSnapshot['paths'] };
}
export function decodeConfigurationData(value: unknown): ConfigurationData {
  const data = record(value);
  if (data.type === 'configuration_saved') {
    exact(data, ['type', 'receipt', 'applied']);
    const receipt = exact(data.receipt, ['saved_version', 'changed_sections', 'required_effects']);
    return { type: 'configuration_saved', receipt: { saved_version: bytes(receipt.saved_version, 32),
      changed_sections: arrayOf(receipt.changed_sections, item => oneOf(item, configSections)),
      required_effects: arrayOf(receipt.required_effects, item => oneOf(item, configEffects)) },
    applied: data.applied === null ? null : decodeDriverStatus(data.applied) };
  }
  if (data.type === 'configuration_reloaded') {
    exact(data, ['type', 'snapshot', 'applied']);
    return { type: 'configuration_reloaded', snapshot: decodeConfigSnapshot(data.snapshot), applied: decodeDriverStatus(data.applied) };
  }
  exact(data, ['type', 'snapshot']);
  if (data.type !== 'configuration') return invalid();
  return { type: 'configuration', snapshot: decodeConfigSnapshot(data.snapshot) };
}
export function configurationCommand(id: CommandId, command: ConfigCommand) {
  if (command.type === 'get') exact(command, ['type']);
  else {
    exact(command, command.type === 'update' ? ['type', 'expected_version', 'document'] : ['type', 'expected_version']);
    bytes(command.expected_version, 32);
    if (command.type !== 'update' && command.type !== 'reload') invalid();
  }
  return managementCommand(id, 'configuration', command);
}
export function decodeRuntimeCommand(value: unknown): RuntimeCommand {
  const command = record(value);
  const feature = oneOf(command.feature, protectionKinds);
  if (command.type === 'set_runtime_feature') {
    exact(command, ['type', 'feature', 'enabled']);
    return { type: 'set_runtime_feature', feature, enabled: boolean(command.enabled) };
  }
  exact(command, ['type', 'feature']);
  if (command.type !== 'resume_runtime_feature') return invalid();
  return { type: 'resume_runtime_feature', feature };
}
export const runtimeCommand = (id: CommandId, command: RuntimeCommand) => managementCommand(id, 'control', command);

export const changedConfigSections = (before: ConfigDocument, after: ConfigDocument) =>
  configSections.filter(section => !sameJsonValue(before[section], after[section]));
