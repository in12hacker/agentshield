import { sameJsonValue } from './global/json-value';
import { sameEventKey, sameId, type Ask, type DecisionId, type EventKey, type OperationResultFacts,
  type OriginalEvent, type RuleHit } from './event-wire';
import type { RuleLog } from './activity-wire';
import { isMessageId, isPartReference, isToolCandidateKind, type AskAuditContext, type EvidenceRelation, type ContextGap,
  type ContextView, type FactReference, type RequestReturn, type SourceRecord } from './context-wire';
import { decodeChatContent, type Facts } from './wire-json';
import type { ChatContent } from './chat-wire';

export function factObject(value: unknown): Facts {
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? value as Facts : {};
}
function unique<T>(values: readonly T[]): T | null {
  return values.length && values.every(value => sameJsonValue(value, values[0])) ? values[0]! : null;
}
function blocksEvidence(reason: ContextGap['reason']) {
  return !['agent_binding_unavailable', 'command_mapping_unavailable', 'command_input_unavailable'].includes(reason);
}
function original(view: ContextView, reference: FactReference): unknown | null {
  if (view.gaps.some(gap => sameJsonValue(gap.at, reference) && blocksEvidence(gap.reason))) return null;
  if (reference.kind === 'part') {
    if (!isPartReference(reference.value)) return null;
    const ref = reference.value;
    const message = factObject(original(view, { kind: 'message', value: factObject(ref.message) }));
    return Array.isArray(message.observed_parts)
      ? unique(message.observed_parts.filter(part => factObject(part).id === ref.part_id)) : null;
  }
  return unique(view.source_records.flatMap<unknown>(source => {
    if (reference.kind === 'native_observation' && source.kind === 'request_handoff') {
      return sameJsonValue(source.value.observation, reference.value) ? [source.value] : [];
    }
    if (reference.kind === 'event' && (source.kind === 'question' || source.kind === 'operation_result')) {
      return sameEventKey(source.value.event.header, reference.value) ? [source.value.event] : [];
    }
    if (reference.kind === 'question' && source.kind === 'question') {
      return sameId(source.value.decision_id, reference.value) ? [source.value.event] : [];
    }
    if (reference.kind === 'completion' && source.kind === 'operation_result') {
      return sameEventKey(source.value.event.header, reference.value) ? [source.value] : [];
    }
    return (reference.kind === 'message' || reference.kind === 'protocol') && source.kind === reference.kind
      && sameJsonValue(source.value.id, reference.value) ? [source.value] : [];
  }));
}
interface OriginalPart { reference: FactReference & { kind: 'part' }; part: Facts; message: Facts }
// One presentation identity for collection, confirmed-result suppression and React keys.
// The tuple borrows original references; it never copies a message or result body.
export function resultObservationIdentity(reference: FactReference, relation: EvidenceRelation) {
  return [reference, relation.kind, relation.from] as const;
}
function retainedRelation(view: ContextView, link: EvidenceRelation): boolean {
  return link.evidence.length > 0 && link.evidence.every(ref => retainedReference(view, ref));
}
function retainedReference(view: ContextView, reference: FactReference): boolean {
  if (view.gaps.some(gap => sameJsonValue(gap.at, reference) && blocksEvidence(gap.reason))) return false;
  return original(view, reference) !== null || view.source_records.some(source => source.kind === 'deferred' && sameJsonValue(source.value, reference));
}
function retainedMessageReferences(view: ContextView, message: Facts): boolean {
  return Array.isArray(message.record_refs) && message.record_refs.length > 0
    && message.record_refs.every(ref => isMessageId(ref) && retainedReference(view, { kind: 'protocol', value: factObject(ref) }));
}
// Resolve only the backend's explicit candidate endpoints. No exchange/source-domain matcher runs here.
function candidatePart(view: ContextView, reference: FactReference, type: 'tool_call' | 'tool_result'): OriginalPart | null {
  if (reference.kind !== 'part' || !isPartReference(reference.value)) return null;
  const part = factObject(original(view, reference));
  if (part.type !== type || (type === 'tool_call' && typeof part.name !== 'string')) return null;
  return { reference: { kind: 'part', value: reference.value }, part,
    message: factObject(original(view, { kind: 'message', value: factObject(reference.value.message) })) };
}
export interface ConfirmedCall extends OriginalPart { relation: EvidenceRelation }
// The same strict execution bridge is used for both owners. Local facts cannot satisfy WS evidence.
export function confirmedExecutionCall(event: OriginalEvent, view: ContextView,
  question?: { ask: Ask; context: AskAuditContext }): ConfirmedCall | null {
  const target: FactReference = { kind: 'event', value: eventKeyOf(event) };
  const links = view.relations.filter(link => link.kind === 'tool_execution' && sameJsonValue(link.to, target));
  const first = links[0];
  if (!first || first.from.kind !== 'part' || !isPartReference(first.from.value)
    || links.some(link => !sameJsonValue(link.from, first.from) || !retainedRelation(view, link))
    || !sameJsonValue(original(view, target), event)) return null;
  if (question) {
    const { ask, context } = question;
    if (!context.trigger_call || !sameId(ask.decision_id, context.decision_id)
      || !sameEventKey(ask.event.header, context.event) || view.anchor.kind !== 'question'
      || !sameId(view.anchor.value, ask.decision_id) || !sameJsonValue(first.from.value, context.trigger_call)
      || !sameJsonValue(original(view, { kind: 'question', value: ask.decision_id }), ask.event)) return null;
  }
  const call = candidatePart(view, first.from, 'tool_call');
  if (!call || typeof call.part.name !== 'string' || call.message.origin !== 'response_output'
    || call.message.state !== 'complete' || !retainedMessageReferences(view, call.message)) return null;
  return { ...call, relation: first };
}
function toolResult(view: ContextView, call: ConfirmedCall): (OriginalPart & { relation: EvidenceRelation }) | null {
  const links = view.relations.filter(link => link.kind === 'native_tool_reference' && sameJsonValue(link.from, call.reference));
  const first = links[0];
  if (!first || links.some(link => !sameJsonValue(link.to, first.to) || !retainedRelation(view, link))) return null;
  const result = candidatePart(view, first.to, 'tool_result');
  return result && retainedMessageReferences(view, result.message) ? { ...result, relation: first } : null;
}

export type ChainSubject = { kind: 'ask'; ask: Ask; completion: OperationResultFacts | null }
  | { kind: 'log'; log: RuleLog } | { kind: 'completion'; completion: OperationResultFacts };
export type ChainEvidence = { kind: 'ask'; context: AskAuditContext } | { kind: 'activity'; view: ContextView };
export type ChainStage = 'upstream' | 'operation' | 'checkpoint' | 'native_return' | 'tool_result';
export const chainStages: readonly ChainStage[] = ['upstream', 'operation', 'checkpoint', 'native_return', 'tool_result'];
export type ChainConnection = { kind: 'confirmed'; relation: EvidenceRelation }
  | { kind: 'candidate'; relations: readonly EvidenceRelation[] }
  | { kind: 'same_operation'; event: EventKey; decisionId: DecisionId | null };
interface Step { incoming: ChainConnection | null }
export type ChainStep = (Step & { stage: 'upstream'; call: OriginalPart & { relation: EvidenceRelation }; candidate?: true })
  | (Step & { stage: 'operation'; event: OriginalEvent; hit: RuleHit | null; subject: ChainSubject;
    native: readonly SourceRecord[]; futex: readonly SourceRecord[] })
  | (Step & { stage: 'checkpoint'; completion: OperationResultFacts | null; log: RuleLog | null })
  | (Step & { stage: 'native_return'; record: RequestReturn })
  | (Step & { stage: 'tool_result'; result: OriginalPart & { relation: EvidenceRelation }; content: ChatContent | null;
    observations: readonly (OriginalPart & { relation: EvidenceRelation })[]; candidate?: true });
export interface ChainIssue { stage: ChainStage; at: FactReference; reason: ContextGap['reason']; original: ContextGap | null }
type ChainOriginalKind = SourceRecord['kind'] | 'security_log' | 'relation' | 'gap';
export interface ChainOriginal { kind: ChainOriginalKind; references: FactReference[]; value: unknown }
export interface CausalChainModel {
  steps: readonly ChainStep[]; issues: readonly ChainIssue[]; view: ContextView | null;
  originals: Readonly<Record<ChainStage, readonly ChainOriginal[]>>;
  receipt: ContextView | AskAuditContext | null;
  verified: boolean;
}
function eventKeyOf(event: OriginalEvent): EventKey {
  const { source_instance, sequence_domain, sequence } = event.header;
  return { source_instance, sequence_domain, sequence };
}
function sameCompletion(a: OperationResultFacts, b: OperationResultFacts): boolean {
  return sameJsonValue(a.decision_id, b.decision_id) && sameJsonValue(a.event, b.event)
    && sameJsonValue(a.hit, b.hit) && sameJsonValue(a.result, b.result);
}
function nativeOperation(event: OriginalEvent): RequestReturn['operation'] | null {
  const { kind, operation, stage } = event.header;
  if (stage !== 'controlled_request') return null;
  if (kind === 'file' && operation === 1) return 'file_open';
  if (kind === 'process' && operation === 1) return 'execute';
  if (kind === 'network' && operation === 1) return 'connect';
  if (kind === 'network' && operation === 2) return 'send';
  return null;
}

// Resolve admitted confirmed/candidate references without changing their strength. A part is
// retained in its actual message, not rendered again as a second source file.
function stageOriginals(steps: readonly ChainStep[], issues: readonly ChainIssue[], view: ContextView | null) {
  const originals: Record<ChainStage, ChainOriginal[]> = {
    upstream: [], operation: [], checkpoint: [], native_return: [], tool_result: [],
  };
  const add = (stage: ChainStage, kind: ChainOriginalKind, value: unknown, references: readonly FactReference[] = []) => {
    const existing = originals[stage].find(item => item.kind === kind && (sameJsonValue(item.value, value)
      || (kind === 'operation_result' && sameCompletion(item.value as OperationResultFacts, value as OperationResultFacts))));
    const item: ChainOriginal = existing ?? { kind, value, references: [] };
    if (!existing) originals[stage].push(item);
    for (const ref of references) if (!item.references.some(value => sameJsonValue(value, ref))) item.references.push(ref);
  };
  const resolve = (stage: ChainStage, ref: FactReference) => {
    if (!view || original(view, ref) === null) return;
    const messageRef = ref.kind === 'part' && isPartReference(ref.value)
      ? { kind: 'message' as const, value: factObject(ref.value.message) } : null;
    const at = messageRef ?? ref;
    for (const source of view.source_records) {
      const matches = (at.kind === 'message' || at.kind === 'protocol') && (source.kind === 'message' || source.kind === 'protocol') && source.kind === at.kind
        ? sameJsonValue(source.value.id, at.value)
        : at.kind === 'event' && (source.kind === 'question' || source.kind === 'operation_result')
          ? sameEventKey(source.value.event.header, at.value)
          : at.kind === 'question' && source.kind === 'question' ? sameId(source.value.decision_id, at.value)
            : at.kind === 'completion' && source.kind === 'operation_result' ? sameEventKey(source.value.event.header, at.value)
              : at.kind === 'native_observation' && source.kind === 'request_handoff' ? sameJsonValue(source.value.observation, at.value) : false;
      if (!matches) continue;
      add(stage, source.kind, source.value, messageRef ? [ref, messageRef] : [ref]);
      if (source.kind === 'message' && Array.isArray(source.value.record_refs)) {
        for (const id of source.value.record_refs) resolve(stage, { kind: 'protocol', value: factObject(id) });
      }
    }
  };
  const relation = (stage: ChainStage, link: EvidenceRelation) => {
    const links = view?.relations.filter(value => value.kind === link.kind && sameJsonValue(value.from, link.from)
      && sameJsonValue(value.to, link.to) && (isToolCandidateKind(link.kind) || retainedRelation(view, value))) ?? [];
    for (const value of links) {
      add(stage, 'relation', value);
      for (const ref of [value.from, value.to, ...value.evidence]) resolve(stage, ref);
    }
  };
  for (const step of steps) {
    const stage = step.stage;
    if (step.incoming?.kind === 'confirmed') relation(stage, step.incoming.relation);
    if (step.stage === 'operation') {
      const { subject, event } = step;
      const ref: FactReference = { kind: 'event', value: eventKeyOf(event) };
      if (subject.kind === 'ask') add(stage, 'question', subject.ask, [ref, { kind: 'question', value: subject.ask.decision_id }]);
      else if (subject.kind === 'log') add(stage, 'security_log', subject.log, [ref]);
      else add(stage, 'operation_result', subject.completion, [ref, { kind: 'completion', value: ref.value }]);
      for (const source of [...step.native, ...step.futex]) add(stage, source.kind, source.value, [ref]);
    } else if (step.stage === 'upstream') {
      resolve(stage, step.call.reference);
      relation(stage, step.call.relation);
    } else if (step.stage === 'checkpoint') {
      if (step.log) add(stage, 'security_log', step.log, [{ kind: 'event', value: eventKeyOf(step.log.event) }]);
      if (step.completion) add(stage, 'operation_result', step.completion,
        [{ kind: 'completion', value: eventKeyOf(step.completion.event) }]);
    } else if (step.stage === 'native_return') {
      add(stage, 'request_return', step.record, [{ kind: 'event', value: step.record.event }]);
    } else {
      for (const observation of step.observations) {
        resolve(stage, observation.reference);
        relation(stage, observation.relation);
      }
    }
  }
  const operation = steps.find(step => step.stage === 'operation');
  if (view && operation) for (const link of view.relations) {
    if (!isToolCandidateKind(link.kind)) continue;
    if (link.to.kind === 'event' && sameEventKey(link.to.value, operation.event.header)) relation('upstream', link);
    else if (link.to.kind === 'part') relation('tool_result', link);
  }
  for (const issue of issues) if (issue.original) add(issue.stage, 'gap', issue.original, [issue.at]);
  return originals;
}

export function projectCausalChain(subject: ChainSubject, evidence: ChainEvidence | null = null): CausalChainModel {
  const base = subject.kind === 'ask' ? subject.ask : subject.kind === 'log' ? subject.log : subject.completion;
  const event = base.event; const key = eventKeyOf(event); const decisionId = base.decision_id;
  const target: FactReference = { kind: 'event', value: key };
  const context = evidence?.kind === 'ask' ? evidence.context : null;
  const supplied = evidence?.kind === 'activity' ? evidence.view : context?.view ?? null;
  const verified = supplied === null || (evidence?.kind === 'ask'
    ? subject.kind === 'ask' && context !== null && sameId(context.decision_id, subject.ask.decision_id)
      && sameEventKey(context.event, key) && supplied.anchor.kind === 'question' && sameId(supplied.anchor.value, subject.ask.decision_id)
    : supplied.anchor.kind === 'event' && sameEventKey(supplied.anchor.value, key));
  const view = verified ? supplied : null;
  const issues: ChainIssue[] = [];
  const issue = (stage: ChainStage, at: FactReference, reason: ContextGap['reason'], original: ContextGap | null = null) => {
    if (!issues.some(value => value.stage === stage && sameJsonValue(value.at, at) && value.reason === reason)) issues.push({ stage, at, reason, original });
  };
  if (!verified) issue('operation', target, 'conflicting_evidence');
  const call = view ? confirmedExecutionCall(event, view,
    subject.kind === 'ask' && context ? { ask: subject.ask, context } : undefined) : null;
  const result = view && call ? toolResult(view, call) : null;
  const candidateCalls: (OriginalPart & { relation: EvidenceRelation })[] = [];
  const reportedResults: (OriginalPart & { relation: EvidenceRelation })[] = [];
  if (view) {
    const retainCandidate = (stage: 'upstream' | 'tool_result', link: EvidenceRelation, ref: FactReference,
      type: 'tool_call' | 'tool_result') => {
      for (const at of [link.from, link.to, ...link.evidence]) {
        const gap = view.gaps.find(gap => sameJsonValue(gap.at, at));
        if (!retainedReference(view, at)) issue(stage, at, gap?.reason ?? 'not_provided', gap ?? null);
      }
      const part = candidatePart(view, ref, type);
      if (!part) {
        if (!issues.some(item => item.stage === stage && sameJsonValue(item.at, ref))) issue(stage, ref, 'not_provided');
        return;
      }
      const parts = stage === 'upstream' ? candidateCalls : reportedResults;
      const identity = resultObservationIdentity(ref, link);
      if (!parts.some(item => stage === 'upstream' ? sameJsonValue(item.reference, ref)
        : sameJsonValue(resultObservationIdentity(item.reference, item.relation), identity))) {
        parts.push({ ...part, relation: link });
      }
    };
    for (const link of view.relations) {
      if (isToolCandidateKind(link.kind) && (sameJsonValue(link.to, target) || link.to.kind === 'part')
        && !sameJsonValue(link.from, call?.reference)) retainCandidate('upstream', link, link.from, 'tool_call');
    }
    for (const link of view.relations) {
      if (isToolCandidateKind(link.kind) && link.to.kind === 'part'
        && !(result && sameJsonValue(resultObservationIdentity(link.to, link), resultObservationIdentity(result.reference, result.relation)))) {
        retainCandidate('tool_result', link, link.to, 'tool_result');
      }
    }
  }
  const upstreamRefs: FactReference[] = [];
  const resultRefs: FactReference[] = [];
  const includePart = (references: FactReference[], ref: FactReference) => {
    references.push(ref);
    if (ref.kind !== 'part' || !isPartReference(ref.value)) return;
    const message: FactReference = { kind: 'message', value: factObject(ref.value.message) };
    references.push(message);
    const originals = view?.source_records.filter(source => source.kind === 'message' && sameJsonValue(source.value.id, message.value)) ?? [];
    for (const source of originals) if (source.kind === 'message' && Array.isArray(source.value.record_refs)) {
      for (const id of source.value.record_refs) references.push({ kind: 'protocol', value: factObject(id) });
    }
  };
  if (view) {
    for (const link of view.relations) if ((link.kind === 'tool_execution' || isToolCandidateKind(link.kind))
      && sameJsonValue(link.to, target)) includePart(upstreamRefs, link.from);
    if (context?.trigger_call) includePart(upstreamRefs, { kind: 'part', value: factObject(context.trigger_call) });
    for (const link of view.relations) if ((link.kind === 'native_tool_reference' || isToolCandidateKind(link.kind))
      && link.to.kind === 'part' && upstreamRefs.some(ref => sameJsonValue(ref, link.from))) includePart(resultRefs, link.to);
    for (const gap of view.gaps) {
      const at = gap.at;
      if (sameJsonValue(at, target)) issue(['agent_binding_unavailable', 'no_explicit_relation'].includes(gap.reason)
        ? 'upstream' : 'operation', at, gap.reason, gap);
      else if (at.kind === 'completion' && sameEventKey(at.value, key)) issue('checkpoint', at, gap.reason, gap);
      else if (at.kind === 'question' && decisionId !== null && sameId(at.value, decisionId)) issue('operation', at, gap.reason, gap);
      else if (upstreamRefs.some(ref => sameJsonValue(ref, at))) issue('upstream', at, gap.reason, gap);
      else if (resultRefs.some(ref => sameJsonValue(ref, at))) issue('tool_result', at, gap.reason, gap);
    }
    if (!call && !issues.some(value => (value.stage === 'upstream' && blocksEvidence(value.reason))
      || (sameJsonValue(value.at, target) && value.reason === 'conflicting_evidence'))) {
      const bridges = view.relations.filter(link => link.kind === 'tool_execution' && sameJsonValue(link.to, target));
      issue('upstream', target, bridges.some(link => !sameJsonValue(link.from, bridges[0]?.from)) ? 'conflicting_evidence'
        : bridges.length ? 'not_provided' : 'no_explicit_relation');
    }
    if (call && !result && view.relations.some(link => link.kind === 'native_tool_reference' && sameJsonValue(link.from, call.reference))) {
      issue('tool_result', call.reference, 'not_provided');
    }
  }
  const decisionMatches = (id: DecisionId | null) => decisionId === null ? id === null : id !== null && sameId(id, decisionId);
  const candidates = view?.source_records.flatMap(source => source.kind === 'operation_result'
    && sameEventKey(source.value.event.header, key) && decisionMatches(source.value.decision_id) ? [source.value] : []) ?? [];
  const local = subject.kind === 'completion' ? subject.completion : subject.kind === 'ask' ? subject.completion : null;
  const validLocal = local && sameJsonValue(local.event, event) && decisionMatches(local.decision_id) ? local : null;
  const completions = [...(validLocal ? [validLocal] : []), ...candidates];
  const conflictingCompletion = completions.some(value => !sameJsonValue(value.event, event))
    || completions.some(value => !sameCompletion(value, completions[0]!));
  if (conflictingCompletion || (local && !validLocal)) issue('checkpoint', { kind: 'completion', value: key }, 'conflicting_evidence');
  const completion = validLocal ?? (!conflictingCompletion ? completions[0] ?? null : null);
  const expectedReturn = nativeOperation(event);
  const returns = view?.source_records.flatMap(source => source.kind === 'request_return'
    && sameEventKey(source.value.event, key) && decisionMatches(source.value.decision_id)
    && source.value.operation === expectedReturn ? [source.value] : []) ?? [];
  const returned = unique(returns);
  if (returns.length && !returned) issue('native_return', target, 'conflicting_evidence');
  const native = view?.source_records.filter(source => source.kind === 'tool_execution' && sameJsonValue(source.value.event, key)) ?? [];
  if (native.length && !unique(native)) issue('operation', target, 'conflicting_evidence');
  const futex = view?.source_records.filter(source => source.kind === 'futex_selection' && sameEventKey(source.value.event, key)) ?? [];
  const blocks = (...stages: ChainStage[]) => issues.some(value => stages.includes(value.stage) && blocksEvidence(value.reason));
  const sameOperation: ChainConnection = { kind: 'same_operation', event: key, decisionId };
  const steps: ChainStep[] = [];
  const candidateConnections = candidateCalls.map(call => call.relation).filter(link => sameJsonValue(link.to, target));
  if (call) steps.push({ stage: 'upstream', call, incoming: null });
  for (const call of candidateCalls) steps.push({ stage: 'upstream', call, candidate: true, incoming: null });
  steps.push({ stage: 'operation', event, hit: base.hit, subject, native: unique(native) ? native.slice(0, 1) : [],
    futex: futex.filter((source, index) => !futex.slice(0, index).some(value => sameJsonValue(value, source))),
    incoming: call && !blocks('upstream', 'operation') ? { kind: 'confirmed', relation: call.relation }
      : candidateConnections.length ? { kind: 'candidate', relations: candidateConnections } : null });
  const log = subject.kind === 'log' ? subject.log : null;
  const logExecution = log && 'Ok' in log.result && typeof log.result.Ok === 'object' && 'execution' in log.result.Ok
    ? log.result.Ok.execution : null;
  const validLog = logExecution && !decisionMatches(logExecution.decision_id) ? null : log;
  if (log && !validLog) issue('checkpoint', { kind: 'completion', value: key }, 'conflicting_evidence');
  const outcome = completion?.result.type === 'receipt' ? completion.result.outcome
    : validLog ? logExecution?.outcome : undefined;
  if (view && expectedReturn && decisionId && !returns.length && (outcome === 'allowed' || outcome === 'denied')) {
    issue('native_return', target, 'not_provided');
  }
  // A completed call exposes the missing result link, never an inferred result or a pending Ask gap.
  if (view && call && !result && completion?.result.type === 'receipt' && !conflictingCompletion
    && (subject.kind !== 'ask' || validLocal !== null)
    && !view.relations.some(link => link.kind === 'native_tool_reference' && sameJsonValue(link.from, call.reference))
    && !issues.some(value => value.stage === 'tool_result')) {
    issue('tool_result', call.reference, 'no_explicit_relation');
  }
  const questionLink = completion && decisionId !== null ? view?.relations.find(link => link.kind === 'question_completion'
    && sameJsonValue(link.from, { kind: 'question', value: decisionId })
    && sameJsonValue(link.to, { kind: 'completion', value: key }) && retainedRelation(view, link)
    && sameJsonValue(original(view, link.from), event)
    && sameCompletion(factObject(original(view, link.to)) as unknown as OperationResultFacts, completion)) : undefined;
  if (completion || validLog) steps.push({ stage: 'checkpoint', completion, log: validLog,
    incoming: !blocks('operation', 'checkpoint') ? questionLink ? { kind: 'confirmed', relation: questionLink } : sameOperation : null });
  if (returned) steps.push({ stage: 'native_return', record: returned,
    incoming: !blocks('operation', 'checkpoint', 'native_return') ? sameOperation : null });
  // Presentation only: keep each observation, but show repeated historical result content once
  // under the same original call reference, native ID and fully known acquisition domain.
  const results: (Extract<ChainStep, { stage: 'tool_result' }> & {
    observations: (OriginalPart & { relation: EvidenceRelation })[];
  })[] = [];
  for (const item of [...(result ? [result] : []), ...reportedResults]) {
    let content: ChatContent | null = null;
    try { decodeChatContent(item.part.content); content = item.part.content as ChatContent; }
    catch { issue('tool_result', item.reference, 'not_provided'); }
    const repeated = content && results.find(step => step.content && sameResultPresentation(step.result, item));
    if (repeated) { repeated.observations.push(item); continue; }
    results.push({ stage: 'tool_result', result: item, content, observations: [item],
      ...(isToolCandidateKind(item.relation.kind) ? { candidate: true as const } : {}), incoming: null });
  }
  steps.push(...results);
  return { steps, issues, view, verified, originals: stageOriginals(steps, issues, view), receipt: context ?? supplied };
}

function sameResultPresentation(left: OriginalPart & { relation: EvidenceRelation }, right: OriginalPart & { relation: EvidenceRelation }): boolean {
  if (left.relation.kind !== right.relation.kind || !sameJsonValue(left.relation.from, right.relation.from)) return false;
  const aId = factObject(left.part.native_id), bId = factObject(right.part.native_id);
  const nativeValue = factObject(aId.value);
  if (aId.namespace !== 'tool_result_reference' || bId.namespace !== aId.namespace
    || !['json_string', 'json_number'].includes(String(nativeValue.kind)) || typeof nativeValue.value !== 'string'
    || !sameJsonValue(aId.value, bId.value)) return false;
  const a = factObject(left.message.source), b = factObject(right.message.source);
  const process = factObject(a.actor).process;
  if (process === null || process === undefined || !sameJsonValue(process, factObject(b.actor).process)) return false;
  for (const name of ['actual_auth_scope', 'profile', 'origin', 'route_label', 'config_version', 'side']) {
    if (a[name] === null || a[name] === undefined || !sameJsonValue(a[name], b[name])) return false;
  }
  return ['origin', 'role', 'state', 'quality'].every(field => sameJsonValue(left.message[field], right.message[field]))
    && sameJsonValue(left.part.content, right.part.content);
}
