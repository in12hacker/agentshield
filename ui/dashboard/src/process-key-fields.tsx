import type { ProcessKey } from './processes-wire';
import { idLabel } from './wire';
import { DetailField } from './global/detail-field';

export function ProcessKeyFields({ identity }: { identity: ProcessKey }) {
  return <dl className="process-fields">
    <DetailField labelKey="processes.bootId"><code>{idLabel(identity.boot_id)}</code></DetailField>
    <DetailField labelKey="processes.hostPid">{identity.host_tgid}</DetailField>
    <DetailField labelKey="processes.birth"><code>{identity.birth_boottime_ns}</code></DetailField>
  </dl>;
}
