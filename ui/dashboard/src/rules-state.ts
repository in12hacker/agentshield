import type { CommandId } from './request-wire';
import { commandFacts, type CommandReply, type CommandResult } from './reply-wire';
import { message, type Message } from './messages';
import { sameId } from './event-wire';
import { replyOutputUnavailable, type Diagnostic } from './wire';
import type { PolicySnapshot, RuleCommand, RulesSaved } from './rules-wire';

export interface RulesState {
  phase: 'initial' | 'loading' | 'ready' | 'failed';
  snapshot: PolicySnapshot | null;
  pending: { id: CommandId; command: RuleCommand['type'] } | null;
  receipt: { id: CommandId; result: CommandResult } | null;
  readFailure: CommandReply | Diagnostic | null;
  problem: Message | null;
  conflict: boolean;
}
export const emptyRules = (): RulesState => ({ phase: 'initial', snapshot: null,
  pending: null, receipt: null, readFailure: null, problem: null, conflict: false });
export function savedFacts(result: CommandResult): RulesSaved | null {
  const facts = commandFacts(result);
  return facts?.type === 'rules_saved' ? facts : null;
}

export function receiveRulesDiagnostic(read: RulesState, diagnostic: Diagnostic): RulesState | null {
  if (!replyOutputUnavailable(diagnostic, read.pending?.id ?? null)) return null;
  return { ...read, phase: 'failed', pending: null, snapshot: null, readFailure: diagnostic,
    problem: message(read.pending?.command === 'edit' ? 'rules.sendFailed' : 'rules.readFailed') };
}
export function adoptionConfirmed(saved: RulesSaved): boolean {
  const applied = saved.applied;
  return applied !== null && applied.source_state === 'running' && applied.rule_version !== null
    && sameId(applied.rule_version, saved.receipt.saved_version);
}

export function receiveRulesReply(read: RulesState, reply: CommandReply): RulesState | null {
  const pending = read.pending;
  if (!pending || !sameId(pending.id, reply.id)) return null;
  const { result } = reply;
  if (pending.command === 'edit') {
    if (result.status === 'accepted') return { ...read, receipt: reply };
    return { ...read, pending: null, phase: 'loading', snapshot: null,
      receipt: reply, conflict: result.status === 'failed' && result.code === 'conflict' };
  }
  if (result.status === 'accepted') return read;
  if (result.status === 'completed' && result.data.type === 'rules') {
    return { ...read, phase: 'ready', snapshot: result.data.snapshot, pending: null, problem: null };
  }
  return { ...read, phase: 'failed', pending: null, problem: message('rules.readFailed'), readFailure: reply };
}
