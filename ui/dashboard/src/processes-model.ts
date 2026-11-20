import type { FieldState, ProcessKey, ProcessPage, ProcessRecord } from './processes-wire';
import type { ContainerInstance } from './workload-wire';

export const processIdentity = (key: ProcessKey) => JSON.stringify([key.boot_id, key.host_tgid, key.birth_boottime_ns]);
export const containerIdentity = (instance: ContainerInstance) => JSON.stringify([instance.runtime, instance.namespace, instance.id]);
export type ObservedField = 'name' | 'executable' | 'workload';
export type ObservationState = FieldState | 'old_epoch' | 'no_stamp' | 'no_value';
export function observationState(record: ProcessRecord, field: ObservedField, epoch: string): ObservationState {
  const stamp = record.latest.stamps[field];
  if (!stamp) return 'no_stamp';
  if (stamp.evidence_epoch !== epoch) return 'old_epoch';
  if (stamp.state !== 'known') return stamp.state;
  return record.latest.process[field] === null ? 'no_value' : 'known';
}
export function observedValue<K extends ObservedField>(record: ProcessRecord, field: K, epoch: string): ProcessRecord['latest']['process'][K] | null {
  return observationState(record, field, epoch) === 'known' ? record.latest.process[field] : null;
}
export function groupProcesses(page: ProcessPage) {
  const host: ProcessRecord[] = [];
  const unknown: ProcessRecord[] = [];
  const containers = new Map<string, { instance: ContainerInstance; records: ProcessRecord[] }>();
  for (const record of page.records) {
    const workload = observedValue(record, 'workload', page.status.evidence_epoch);
    if (workload === null) unknown.push(record);
    else if (workload.kind === 'host') host.push(record);
    else {
      const key = containerIdentity(workload.instance);
      let group = containers.get(key);
      if (!group) { group = { instance: workload.instance, records: [] }; containers.set(key, group); }
      group.records.push(record);
    }
  }
  return { host, unknown, containers: [...containers.values()] };
}
