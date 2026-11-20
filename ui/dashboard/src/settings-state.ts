import { sameId } from './event-wire';
import { message, type Message } from './messages';
import { commandFacts, type CommandReply, type CommandResult } from './reply-wire';
import type { CommandId } from './request-wire';
import { replyOutputUnavailable, type Access, type Diagnostic } from './wire';
import { configurationCommand, decodeConfigDocument, decodeRuntimeCommand, runtimeCommand,
  type ConfigDocument, type ConfigSnapshot, type RuntimeCommand } from './settings-wire';

export type SettingsOperation = 'get' | 'update' | 'reload' | RuntimeCommand;
export interface SettingsReceipt {
  id: CommandId;
  operation: Exclude<SettingsOperation, 'get'>;
  result: CommandResult | null;
  diagnostic: Diagnostic | null;
  problem: Message | null;
}
export interface SettingsState {
  active: boolean;
  phase: 'initial' | 'loading' | 'ready' | 'failed';
  snapshot: ConfigSnapshot | null;
  pending: { id: CommandId; operation: SettingsOperation } | null;
  receipt: SettingsReceipt | null;
  readReply: CommandReply | null;
  diagnostic: Diagnostic | null;
  problem: Message | null;
  conflict: boolean;
}
export const emptySettings = (): SettingsState => ({ active: false, phase: 'initial', snapshot: null,
  pending: null, receipt: null, readReply: null, diagnostic: null, problem: null, conflict: false });

// This module owns configuration/control lifecycle. The session supplies only its existing
// command allocator, one snapshot slot, current grant and the existing overview refresh.
interface SettingsPort {
  read(): SettingsState;
  publish(state: SettingsState): void;
  access(): Access | null;
  command(encode: (id: CommandId) => string, begin: (id: CommandId) => void,
    failed: (id: CommandId | null, problem: Message) => void, sendProblem: Message): void;
  refreshOverview(): void;
}
export class ServiceSettings {
  constructor(private readonly port: SettingsPort) {}
  enter() {
    const state = this.port.read();
    if (this.port.access() === null || state.active) return;
    this.port.publish({ ...state, active: true });
    if (!state.pending) this.refresh();
  }
  leave() {
    const state = this.port.read();
    if (!state.active) return;
    // Late reads have no consumer. Actual writes retain their waiter and real receipt.
    this.port.publish({ ...state, active: false, snapshot: null, phase: 'initial',
      pending: state.pending?.operation === 'get' ? null : state.pending,
      readReply: null, diagnostic: null, problem: null });
  }
  refresh = () => {
    const state = this.port.read();
    if (!state.active || state.pending || this.port.access() === null) return;
    this.send('get', id => configurationCommand(id, { type: 'get' }));
  };
  save = (snapshot: ConfigSnapshot, proposed: ConfigDocument) => {
    if (!this.canWrite(snapshot)) return;
    let document: ConfigDocument;
    try { document = decodeConfigDocument(proposed); } catch { return; }
    this.send('update', id => configurationCommand(id, { type: 'update', expected_version: snapshot.version, document }));
  };
  reload = (snapshot: ConfigSnapshot) => {
    if (!this.canWrite(snapshot)) return;
    this.send('reload', id => configurationCommand(id, { type: 'reload', expected_version: snapshot.version }));
  };
  control = (proposed: RuntimeCommand) => {
    const state = this.port.read();
    if (this.port.access() !== 'manage_service' || !state.active || state.pending) return;
    let command: RuntimeCommand;
    try { command = decodeRuntimeCommand(proposed); } catch { return; }
    this.send(command, id => runtimeCommand(id, command));
  };
  private canWrite(snapshot: ConfigSnapshot) {
    const state = this.port.read();
    return this.port.access() === 'manage_service' && state.active && state.phase === 'ready'
      && state.snapshot === snapshot && !state.pending;
  }
  private send(operation: SettingsOperation, encode: (id: CommandId) => string) {
    this.port.command(encode,
      id => {
        const state = this.port.read();
        this.port.publish({ ...state, pending: { id, operation }, problem: null, diagnostic: null,
          ...(operation === 'get' ? { phase: 'loading', snapshot: null, readReply: null }
            : { receipt: { id, operation, result: null, diagnostic: null, problem: null }, conflict: false }) });
      },
      (id, problem) => {
        const state = this.port.read();
        if (id && (!state.pending || !sameId(id, state.pending.id))) return;
        this.port.publish({ ...state, pending: null, phase: 'failed', problem,
          receipt: operation !== 'get' && id ? { id, operation, result: null, diagnostic: null, problem } : state.receipt });
      }, message(operation === 'get' ? 'settings.readSendFailed' : 'settings.unconfirmed'));
  }
  receive(reply: CommandReply): boolean {
    const state = this.port.read(), pending = state.pending;
    if (!pending || !sameId(pending.id, reply.id)) return false;
    const { result } = reply;
    const facts = commandFacts(result);
    if (pending.operation === 'get') {
      if (result.status === 'accepted') {
        this.port.publish({ ...state, readReply: reply });
      } else {
        const snapshot = result.status === 'completed' && facts?.type === 'configuration' ? facts.snapshot : null;
        const ready = snapshot !== null;
        this.port.publish({ ...state, pending: null, phase: ready ? 'ready' : 'failed',
          snapshot, readReply: reply,
          problem: ready ? null : message('settings.readFailed') });
      }
      return true;
    }
    const isControl = typeof pending.operation === 'object';
    const matches = isControl ? facts?.type === 'driver_status'
      : pending.operation === 'update' ? facts?.type === 'configuration_saved'
        : facts?.type === 'configuration' || facts?.type === 'configuration_reloaded';
    const validFailureFacts = facts === null || (!isControl && (facts.type === 'configuration'
      || (pending.operation === 'update' && facts.type === 'configuration_saved')));
    const problem = (result.status === 'completed' && !matches) || (result.status === 'failed' && !validFailureFacts)
      ? message('settings.unconfirmed') : null;
    const receipt: SettingsReceipt = { id: reply.id, operation: pending.operation, result, diagnostic: null, problem };
    if (result.status === 'accepted') {
      this.port.publish({ ...state, receipt });
      return true;
    }
    this.port.publish({ ...state, pending: null, receipt,
      snapshot: state.active && isControl ? state.snapshot : null,
      phase: isControl ? state.phase : 'failed',
      conflict: result.status === 'failed' && result.code === 'conflict' });
    this.port.refreshOverview();
    // No write retry. A fresh Get is required before another configuration edit.
    if (this.port.read().active && (!isControl || this.port.read().snapshot === null)) this.refresh();
    return true;
  }
  diagnostic(diagnostic: Diagnostic): boolean {
    const state = this.port.read(), pending = state.pending;
    if (!pending || !replyOutputUnavailable(diagnostic, pending.id)) return false;
    const problem = message(pending.operation === 'get' ? 'settings.readFailed' : 'settings.unconfirmed');
    this.port.publish({ ...state, pending: null, phase: 'failed', diagnostic, problem,
      receipt: pending.operation === 'get' ? state.receipt
        : { id: pending.id, operation: pending.operation,
          result: state.receipt && sameId(state.receipt.id, pending.id) ? state.receipt.result : null,
          diagnostic, problem } });
    return true;
  }
}
