// Shared original workload identity: as-types/workload.rs, also used by rule scope.
import { exact, invalid, record, string } from './wire-json';

export interface ContainerInstance { runtime: string; namespace: string | null; id: string }
export type Workload = { kind: 'host' } | { kind: 'container'; instance: ContainerInstance };

export function decodeContainerInstance(value: unknown): ContainerInstance {
  const o = exact(value, ['runtime', 'namespace', 'id']);
  return { runtime: string(o.runtime), namespace: o.namespace === null ? null : string(o.namespace), id: string(o.id) };
}
export function decodeWorkload(value: unknown): Workload {
  const o = record(value);
  if (o.kind === 'host') { exact(o, ['kind']); return { kind: 'host' }; }
  exact(o, ['kind', 'instance']);
  if (o.kind !== 'container') return invalid();
  return { kind: 'container', instance: decodeContainerInstance(o.instance) };
}

export function sameWorkload(observed: Workload | null, expected: Workload): boolean {
  if (!observed) return false;
  if (expected.kind === 'host') return observed.kind === 'host';
  if (observed.kind !== 'container') return false;
  return observed.instance.runtime === expected.instance.runtime
    && observed.instance.namespace === expected.instance.namespace
    && observed.instance.id === expected.instance.id;
}
