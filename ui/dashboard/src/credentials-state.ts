import { sameId } from './event-wire';
import { message, type Message } from './messages';
import type { CommandId } from './request-wire';
import type { ConfigSnapshot } from './settings-wire';
import type { SettingsState } from './settings-state';
import { replyOutputUnavailable, type Access, type Diagnostic } from './wire';
import { decodePrivateOperation, privateCommand, validSecret, type CaInfo, type CredentialError,
  type PrivateData, type PrivateOperation, type PrivateReply, type SecretReceipt } from './credentials-wire';
import { issuableBinding, readEgressConfig, type CredentialBinding, type ProviderRoute } from './egress-wire';

type PublicData = Exclude<PrivateData, { type: 'phantom_issued' }>
  | Omit<Extract<PrivateData, { type: 'phantom_issued' }>, 'value'>;
export interface CredentialReceipt {
  id: CommandId; operation: PrivateOperation; data: PublicData | null; code: CredentialError | null; problem: Message | null;
  scope?: { binding: CredentialBinding; routes: readonly ProviderRoute[] };
}
export interface CredentialState {
  active: boolean; revoked: boolean; hasValue: boolean;
  pending: { id: CommandId; operation: PrivateOperation; epoch: number; scope?: CredentialReceipt['scope'] } | null;
  receipt: CredentialReceipt | null; ca: CaInfo | null; saved: SecretReceipt | null;
  issued: Extract<PublicData, { type: 'phantom_issued' }> | null;
  problem: Message | null;
}
const empty = (): CredentialState => ({ active: false, revoked: false, hasValue: false, pending: null,
  receipt: null, ca: null, saved: null, issued: null, problem: null });
interface CredentialPort {
  access(): Access | null;
  settings(): SettingsState;
  command(encode: (id: CommandId) => string, begin: (id: CommandId) => void,
    failed: (id: CommandId | null, problem: Message) => void, sendProblem: Message): void;
}
// This owner is intentionally outside the global Snapshot. No retained wire frame or request.
export class PrivateCredentials {
  private state = empty();
  private value: string | null = null;
  private epoch = 0;
  private pendingCaSnapshot: ConfigSnapshot | null = null;
  private caBinding: { snapshot: ConfigSnapshot; ca: CaInfo } | null = null;
  private listeners = new Set<() => void>();
  constructor(private readonly port: CredentialPort) {}
  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  private publish(state: CredentialState) { this.state = state; this.listeners.forEach(listener => listener()); }
  enter = () => { if (!this.state.active && this.port.access() !== null) this.publish({ ...this.state, active: true }); };
  leave = () => {
    if (!this.state.active) return;
    this.value = null; this.epoch++;
    this.caBinding = null; this.pendingCaSnapshot = null;
    this.publish({ ...this.state, active: false, hasValue: false });
  };
  discard = () => {
    this.value = null; this.epoch++;
    if (this.state.hasValue) this.publish({ ...this.state, hasValue: false });
  };
  reset(): boolean {
    const uncertain = this.state.pending !== null;
    this.value = null; this.epoch++;
    this.caBinding = null; this.pendingCaSnapshot = null;
    this.publish(empty());
    return uncertain;
  }
  private validSnapshot(snapshot: ConfigSnapshot): boolean {
    const settings = this.port.settings();
    return settings.active && settings.phase === 'ready' && !settings.pending && settings.snapshot === snapshot;
  }
  private matchesConfiguredCa(snapshot: ConfigSnapshot, ca: readonly number[]): boolean {
    if (!this.validSnapshot(snapshot)) return false;
    try {
      const configured = readEgressConfig(snapshot).flow?.ca;
      return !!configured && sameId(configured, ca);
    } catch { return false; }
  }
  configurationChanged() {
    if (this.pendingCaSnapshot && !this.validSnapshot(this.pendingCaSnapshot)) this.pendingCaSnapshot = null;
    if (this.caBinding && !this.matchesConfiguredCa(this.caBinding.snapshot, this.caBinding.ca.id)) {
      this.caBinding = null;
      this.publish({ ...this.state });
    }
  }
  configuredCa = (snapshot: ConfigSnapshot | null, ca: CaInfo | null): CaInfo | null => {
    const binding = this.caBinding;
    if (!snapshot || !ca || !this.state.active || this.state.revoked || this.port.access() !== 'manage_service'
      || !binding || this.state.ca !== ca || binding.snapshot !== snapshot || binding.ca !== ca
      || !this.matchesConfiguredCa(snapshot, ca.id)) return null;
    return ca;
  };
  readValue = () => this.state.active && !this.state.revoked && this.port.access() === 'manage_service' ? this.value : null;
  copyValue = (write: (value: string) => Promise<void>): Promise<boolean> => {
    const value = this.readValue();
    if (value === null) return Promise.resolve(false);
    try { return write(value).then(() => true, () => false); }
    catch { return Promise.resolve(false); }
  };
  send = (proposed: PrivateOperation, value?: string) => {
    if (!this.state.active || this.state.revoked || this.state.pending || this.port.access() !== 'manage_service') return;
    let operation: PrivateOperation;
    try { operation = decodePrivateOperation(proposed); } catch { return; }
    const secret = operation.type === 'import_secret' || operation.type === 'replace_secret';
    if (secret ? value === undefined || !validSecret(value) : value !== undefined) {
      this.publish({ ...this.state, problem: message('egress.invalidSecret') }); return;
    }
    if (operation.type === 'issue_phantom') return; // Requires the original snapshot and selected binding below.
    if (operation.type === 'public_ca') {
      const snapshot = this.port.settings().snapshot;
      if (!snapshot || !this.matchesConfiguredCa(snapshot, operation.ca)) return;
    }
    this.execute(operation, value);
  };
  issue = (snapshot: ConfigSnapshot, binding: CredentialBinding) => {
    const settings = this.port.settings();
    if (!this.state.active || this.state.revoked || this.state.pending || this.port.access() !== 'manage_service'
      || settings.snapshot !== snapshot || settings.phase !== 'ready' || settings.pending) return;
    try {
      const config = readEgressConfig(snapshot);
      if (!issuableBinding(config, binding)) return;
      this.execute({ type: 'issue_phantom', binding: binding.id, expected_config: snapshot.version }, undefined,
        { binding, routes: config.flow!.routes.filter(r => binding.routes.some(id => sameId(id, r.id))) });
    } catch { return; }
  };
  private execute(operation: PrivateOperation, value?: string, scope?: CredentialReceipt['scope']) {
    this.discard();
    if (operation.type === 'public_ca' || operation.type === 'ensure_ca') {
      this.caBinding = null;
      const snapshot = this.port.settings().snapshot;
      this.pendingCaSnapshot = snapshot && this.validSnapshot(snapshot) ? snapshot : null;
    }
    this.port.command(id => privateCommand(id, operation, value),
      id => this.publish({ ...this.state, pending: { id, operation, epoch: this.epoch, ...(scope ? { scope } : {}) }, receipt: null, problem: null }),
      (id, problem) => {
        if (id && (!this.state.pending || !sameId(id, this.state.pending.id))) return;
        this.pendingCaSnapshot = null;
        this.publish({ ...this.state, pending: null, problem,
          receipt: id ? { id, operation, data: null, code: null, problem, ...(scope ? { scope } : {}) } : null });
      }, message('egress.unconfirmed'));
    value = undefined;
  }
  receive(reply: PrivateReply) {
    const pending = this.state.pending;
    if (!pending || !sameId(reply.id, pending.id)) return;
    const operation = pending.operation, result = reply.result;
    const caSnapshot = this.pendingCaSnapshot;
    this.pendingCaSnapshot = null;
    if (result.status === 'failed') {
      this.publish({ ...this.state, pending: null, receipt: { id: pending.id, operation,
        ...(pending.scope ? { scope: pending.scope } : {}), data: null, code: result.code,
        problem: result.code === 'persistence_uncertain'
          || (operation.type === 'issue_phantom' && result.code === 'scope_denied') ? message('egress.unconfirmed') : null } });
      return;
    }
    const d = result.data;
    const matches = d.type === 'ca' ? operation.type === 'ensure_ca' || (operation.type === 'public_ca' && sameId(operation.ca, d.info.id))
      : d.type === 'secret_saved' ? operation.type === 'import_secret' || (operation.type === 'replace_secret' && sameId(operation.secret, d.receipt.secret))
      : d.type === 'phantom_issued' ? operation.type === 'issue_phantom' && sameId(operation.binding, d.binding)
      : operation.type === 'revoke_phantom';
    if (!matches) {
      this.publish({ ...this.state, pending: null, problem: message('egress.unconfirmed'),
        receipt: { id: pending.id, operation, data: null, code: null, problem: message('egress.unconfirmed') } });
      return;
    }
    let data: PublicData;
    if (d.type === 'phantom_issued') {
      data = { type: d.type, token_id: d.token_id, binding: d.binding, revision: d.revision };
      if (this.state.active && !this.state.revoked && pending.epoch === this.epoch && this.port.access() === 'manage_service') this.value = d.value;
    } else data = d;
    if (d.type === 'ca' && caSnapshot && this.state.active && !this.state.revoked
      && this.port.access() === 'manage_service'
      && this.matchesConfiguredCa(caSnapshot, d.info.id)) this.caBinding = { snapshot: caSnapshot, ca: d.info };
    this.publish({ ...this.state, pending: null, hasValue: this.value !== null,
      ca: d.type === 'ca' ? d.info : this.state.ca, saved: d.type === 'secret_saved' ? d.receipt : this.state.saved,
      issued: data.type === 'phantom_issued' ? data : this.state.issued,
      receipt: { id: pending.id, operation, ...(pending.scope ? { scope: pending.scope } : {}), data, code: null, problem: null } });
  }
  diagnostic(diagnostic: Diagnostic): boolean {
    const pending = this.state.pending;
    const denied = diagnostic.type === 'private_not_authorized' && diagnostic.id === undefined;
    if (!denied && (!pending || !replyOutputUnavailable(diagnostic, pending.id))) return false;
    this.discard();
    if (denied) this.caBinding = null;
    this.pendingCaSnapshot = null;
    const problem = message(denied ? 'egress.permissionRevoked' : 'egress.unconfirmed');
    this.publish({ ...this.state, pending: null, revoked: denied || this.state.revoked, problem,
      receipt: pending ? { id: pending.id, operation: pending.operation, data: null, code: null, problem } : this.state.receipt });
    return true;
  }
}
