// The original DriverStatus is shared by overview, notifications and rule adoption receipts.
import { boolean, bytes, exact, integer, numericU64, oneOf, string, type JsonU64 } from './wire-json';

export type FeatureState = 'enabled' | 'disabled' | 'unavailable';
export const protectionKinds = ['process', 'file', 'network'] as const;
export type ProtectionKind = typeof protectionKinds[number];
export type ProtectionDefaults = Readonly<Record<ProtectionKind, boolean>>;

export function decodeProtectionDefaults(value: unknown): ProtectionDefaults {
  const defaults = exact(value, protectionKinds);
  for (const feature of protectionKinds) boolean(defaults[feature]);
  return defaults as ProtectionDefaults;
}
export interface FeatureStatus {
  startup_default: boolean;
  runtime_override: 'default' | 'enabled' | 'disabled';
  effective_state: FeatureState;
}
export interface DriverStatus {
  instance: readonly number[];
  revision: number;
  source_state: 'dormant' | 'running' | 'quiescing' | 'failed';
  persistent_generation: JsonU64;
  state_revision: JsonU64;
  lifecycle_epoch: number;
  driver_config_version: readonly number[] | null;
  rule_version: readonly number[] | null;
  startup_protection: ProtectionDefaults | null;
  process: FeatureStatus;
  file: FeatureStatus;
  network: FeatureStatus;
  self_protection: FeatureState;
  pending: number;
  pending_receipts: number;
  dropped: JsonU64;
  error: string | null;
}
const featureState = (value: unknown) => oneOf(value, ['enabled', 'disabled', 'unavailable'] as const);
function featureStatus(value: unknown): FeatureStatus {
  const object = exact(value, ['startup_default', 'runtime_override', 'effective_state']);
  return { startup_default: boolean(object.startup_default),
    runtime_override: oneOf(object.runtime_override, ['default', 'enabled', 'disabled'] as const),
    effective_state: featureState(object.effective_state) };
}
export function decodeDriverStatus(value: unknown): DriverStatus {
  const object = exact(value, ['instance', 'revision', 'source_state', 'persistent_generation',
    'state_revision', 'lifecycle_epoch', 'driver_config_version', 'rule_version', 'startup_protection',
    'process', 'file', 'network', 'self_protection', 'pending', 'pending_receipts', 'dropped', 'error']);
  return {
    instance: bytes(object.instance, 16), revision: integer(object.revision, 0, 65535),
    source_state: oneOf(object.source_state, ['dormant', 'running', 'quiescing', 'failed'] as const),
    persistent_generation: numericU64(object.persistent_generation), state_revision: numericU64(object.state_revision),
    lifecycle_epoch: integer(object.lifecycle_epoch, 0, 0xffffffff),
    driver_config_version: object.driver_config_version === null ? null : bytes(object.driver_config_version, 32),
    rule_version: object.rule_version === null ? null : bytes(object.rule_version, 32),
    startup_protection: object.startup_protection === null ? null : decodeProtectionDefaults(object.startup_protection),
    process: featureStatus(object.process), file: featureStatus(object.file), network: featureStatus(object.network),
    self_protection: featureState(object.self_protection), pending: integer(object.pending, 0, 0xffffffff),
    pending_receipts: integer(object.pending_receipts, 0, 0xffffffff), dropped: numericU64(object.dropped),
    error: object.error === null ? null : string(object.error),
  };
}
