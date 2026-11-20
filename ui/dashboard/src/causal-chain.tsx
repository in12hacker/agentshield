import { useEffect, useState } from 'react';
import { eventKey, type EventKey, type Decision, type OriginalEvent, type Outcome, type OperationResultFacts, type Signal, type TerminationReport } from './event-wire';
import type { ContextView, ExchangeKey, FactReference } from './context-wire';
import type { RuleLog } from './activity-wire';
import { chainStages, factObject, resultObservationIdentity, type CausalChainModel, type ChainOriginal, type ChainStage, type ChainStep } from './causal-chain-model';
import { gapLabels } from './context-display';
import { byteText, isByteString } from './global/byte-text';
import { RawFacts } from './global/raw-facts';
import { useI18n } from './i18n';
import type { MessageKey } from './messages';
import { factsJson } from './wire-json';
import { idLabel } from './wire';
import { ChatContentValue, OriginalChatMessage } from './chat-message';
import { decodeChatMessage, type ChatMessage } from './chat-wire';

const fileOperations = ['ask.open', 'ask.readFile', 'ask.writeFile', 'ask.createFile', 'ask.createDirectory',
  'ask.unlink', 'ask.removeDirectory', 'ask.link', 'ask.rename', 'ask.truncate', 'ask.changeMode',
  'ask.changeOwner', 'ask.writableMapping', 'ask.mount', 'ask.unmount', 'ask.moveMount',
  'ask.setMetadata', 'ask.truncate', 'ask.mapSharedWritable'] as const satisfies readonly MessageKey[];
function dnsQuestion(event: OriginalEvent): unknown | null {
  const payload = factObject(event.payload);
  return event.header.kind === 'network' && event.header.operation === 2
    && factObject(factObject(payload.dns_snapshot).analysis).status === 'question'
    && isByteString(payload.target_domain) ? payload.target_domain : null;
}
export function eventOperationLabel(event: OriginalEvent): { key: MessageKey; params?: Record<string, string> } {
  const { kind, operation } = event.header;
  const domain = dnsQuestion(event);
  if (isByteString(domain)) return { key: 'ask.dnsQuery', params: { domain: byteText(domain).value } };
  const accessMask = factObject(event.payload).access_mask;
  if (kind === 'file' && operation === 1 && typeof accessMask === 'number' && (accessMask & 2) !== 0) {
    return { key: 'ask.writeFile' };
  }
  const key = typeof operation !== 'number' ? null : kind === 'file' ? fileOperations[operation - 1]
    : kind === 'process' ? ({ 1: 'ask.execute', 2: 'ask.createProcess', 3: 'ask.exitProcess' } as const)[operation as 1 | 2 | 3]
      : kind === 'network' ? operation === 1 ? 'ask.connect' : operation === 2 ? 'ask.send' : null : null;
  return key ? { key } : { key: 'ask.operationCode', params: { kind: String(kind), code: String(operation) } };
}
const eventCategories = { file: 'category.file', process: 'category.process', network: 'category.network' } as const;

export function EventCategory({ event }: { event: OriginalEvent }) {
  const { t } = useI18n();
  return <>{t(eventCategories[event.header.kind as keyof typeof eventCategories])}</>;
}

export function EventOperation({ event }: { event: OriginalEvent }) {
  const { t } = useI18n();
  const { key, params } = eventOperationLabel(event);
  return <>{t(key, params)}</>;
}
export function FactValue({ value }: { value: unknown }) {
  const { t } = useI18n();
  if (value === null || value === undefined) return <span className="muted">{t(value === null ? 'common.null' : 'common.missing')}</span>;
  if (isByteString(value)) {
    const text = byteText(value);
    return <bdi>{text.kind === 'empty' ? JSON.stringify(text.value) : text.value}</bdi>;
  }
  return <bdi>{typeof value === 'string' ? value === '' ? JSON.stringify(value) : value : factsJson(value)}</bdi>;
}
function pathTarget(event: OriginalEvent) {
  const payload = factObject(event.payload);
  return event.header.kind === 'file' ? payload.path : payload.target_executable;
}
function compactPath(value: unknown): string | null {
  const text = isByteString(value) ? byteText(value).value : typeof value === 'string' ? value : null;
  if (text === null || text.length <= 76) return text;
  const leaf = text.slice(text.lastIndexOf('/') + 1);
  return leaf.length && leaf.length < 60 ? `…/${leaf}` : `${text.slice(0, 24)}…${text.slice(-48)}`;
}
function networkTarget(event: OriginalEvent) {
  const payload = factObject(event.payload);
  return { ip: payload.remote_ip, port: payload.remote_port };
}
export function eventTargetLabel(event: OriginalEvent): string | null {
  if (event.header.kind !== 'network') return compactPath(pathTarget(event));
  const domain = dnsQuestion(event);
  if (isByteString(domain)) return byteText(domain).value;
  const { ip, port } = networkTarget(event);
  if (typeof ip !== 'string') return typeof port === 'number' ? String(port) : null;
  if (typeof port !== 'number') return ip;
  return ip.includes(':') ? `[${ip}]:${port}` : `${ip}:${port}`;
}
export function EventTarget({ event, compact = false }: { event: OriginalEvent; compact?: boolean }) {
  if (event.header.kind === 'network') {
    const domain = dnsQuestion(event);
    if (domain !== null) return <FactValue value={domain} />;
    const { ip, port } = networkTarget(event);
    return <><FactValue value={ip} />{' : '}<FactValue value={port} /></>;
  }
  const value = pathTarget(event);
  return <FactValue value={compact ? compactPath(value) ?? value : value} />;
}
export function DecisionValue({ value }: { value: Decision }) {
  const { t } = useI18n();
  return <>{t(value.type === 'allow' ? 'choice.allow' : value.terminate === 'none' ? 'choice.deny'
    : value.terminate === 'actor' ? 'choice.terminateActor' : 'choice.terminateParent')}</>;
}
export function OutcomeValue({ value, compact = false }: { value: Outcome; compact?: boolean }) {
  const { t } = useI18n();
  const keys = { allowed: 'result.allowed', denied: 'result.denied', cancelled: 'result.cancelled', unknown: 'result.unknown' } as const;
  const shortKeys = { allowed: 'checkpoint.allowed', denied: 'checkpoint.denied', cancelled: 'checkpoint.cancelled', unknown: 'checkpoint.unknown' } as const;
  return typeof value === 'string' ? <>{t((compact ? shortKeys : keys)[value])}</>
    : <>{t(compact ? 'checkpoint.failed' : 'result.nativeFailed')}{!compact && <>{' · '}<code>{t('result.nativeError', { error: value.failed.native_error })}</code></>}</>;
}
function SignalValue({ value }: { value: Signal }) {
  const { t } = useI18n();
  const keys = { not_requested: 'ask.notRequested', signal_sent: 'ask.signalSent', not_running: 'ask.notRunning', failed: 'ask.signalFailed' } as const;
  return <>{t(keys[value.status])}{value.status === 'failed' && <code>{' / '}{value.native_error}</code>}</>;
}
function TerminationValue({ report }: { report: TerminationReport }) {
  const { t } = useI18n();
  return report.requested !== 'none' && <dl className="ask-facts">
    <div><dt>{t('ask.actorTermination')}</dt><dd><SignalValue value={report.actor} /></dd></div>
    {report.requested === 'actor_and_parent' && <div><dt>{t('ask.parentTermination')}</dt><dd><SignalValue value={report.parent} /></dd></div>}
  </dl>;
}
export function CheckpointValue({ completion, log, compact = false }: {
  completion?: OperationResultFacts | null; log?: RuleLog | null; compact?: boolean;
}) {
  const { t } = useI18n();
  if (completion) return completion.result.type === 'failed' ? <>{t(compact ? 'checkpoint.failed' : 'result.reportFailed')}</>
    : <OutcomeValue value={completion.result.outcome} compact={compact} />;
  if (!log) return <>{t('checkpoint.unknown')}</>;
  return 'Err' in log.result ? <>{t('checkpoint.failed')}</> : log.result.Ok === 'observed' ? <>{t('activity.observed')}</>
    : <OutcomeValue value={'driver' in log.result.Ok ? log.result.Ok.driver.outcome : log.result.Ok.execution.outcome} compact={compact} />;
}
function Completion({ completion }: { completion: OperationResultFacts }) {
  const { t } = useI18n();
  const report = completion.result;
  return <div className="stack-tight" role="status" aria-label={t('result.actual')}>
    {report.type === 'failed' ? <div className="receipt-failure stack-tight"><strong>{t('result.reportFailed')}</strong>
      <p>{t('common.failureCode')}<code>{report.code}</code></p>
      <p>{t('common.originalMessage')}<code>{JSON.stringify(report.message)}</code></p></div>
      : <strong><OutcomeValue value={report.outcome} /></strong>}
    {report.type === 'receipt' && <>
      {report.outcome === 'allowed' && <p className="muted">{t('result.allowedDetail')}</p>}
      <TerminationValue report={report.termination} />
    </>}
  </div>;
}
function LogCheckpoint({ log }: { log: RuleLog }) {
  // pending/transferred are displayed as original decision state on the operation, never as an outcome.
  const { t } = useI18n();
  const receipt = 'Ok' in log.result && log.result.Ok !== 'observed'
    ? 'driver' in log.result.Ok ? log.result.Ok.driver : log.result.Ok.execution : null;
  return <div className="stack-tight"><strong><CheckpointValue log={log} /></strong>
    {'Err' in log.result && <p className="receipt-failure">{t('common.failureCode')}<code>{log.result.Err}</code></p>}
    {receipt && <>
      {receipt.outcome === 'allowed' && <p className="muted">{t('result.allowedDetail')}</p>}
      <TerminationValue report={receipt.termination} />
    </>}
  </div>;
}
function Operation({ step, operationId, expanded, summarized }: {
  step: Extract<ChainStep, { stage: 'operation' }>; operationId?: string; expanded: boolean; summarized: boolean;
}) {
  const { t } = useI18n();
  const { event, subject } = step;
  const process = factObject(event.process); const payload = factObject(event.payload);
  const proposal = subject.kind === 'log' ? subject.log.decision.state === 'resolved' ? subject.log.decision : null
    : subject.completion?.result.proposal;
  return <div className="stack-tight">
    {!summarized && <>
      {!operationId && <strong className="chain-operation-name"><EventOperation event={event} /></strong>}
      <p id={operationId} className="ask-target"><EventTarget event={event} /></p>
    </>}
    <dl className="ask-facts">
      <div><dt>{t('facts.process')}</dt><dd><FactValue value={process.name} />
        {process.executable !== null && process.executable !== undefined && <span className="ask-secondary"><FactValue value={process.executable} /></span>}
        {typeof event.header.actor_tgid === 'number' && <span className="ask-secondary">
          {t('processes.pidValue', { pid: event.header.actor_tgid })}</span>}</dd></div>
      {event.header.kind === 'network' && <div><dt>{t('ask.domain')}</dt><dd><FactValue value={payload.target_domain} /></dd></div>}
      {dnsQuestion(event) !== null && <div><dt>{t('ask.dnsResolver')}</dt><dd>
        <FactValue value={payload.remote_ip} />{' : '}<FactValue value={payload.remote_port} />
      </dd></div>}
      {payload.new_path !== null && payload.new_path !== undefined && <div><dt>{t('ask.newPath')}</dt><dd><FactValue value={payload.new_path} /></dd></div>}
      <div className="ask-rule"><dt>{t('facts.hit')}</dt><dd>{step.hit ? <bdi>{step.hit.group_id} / {step.hit.rule_id}</bdi> : t('common.null')}</dd></div>
      {proposal && <>
        <div><dt>{t('activity.proposal')}</dt><dd><DecisionValue value={proposal.decision} /></dd></div>
        <div><dt>{t('activity.proposalReason')}</dt><dd><code>{proposal.reason}</code></dd></div>
      </>}
      {subject.kind === 'ask' && !subject.completion && <div><dt>{t('chain.controlled')}</dt><dd>{t('chain.originalQuestion')}</dd></div>}
      {subject.kind === 'log' && subject.log.decision.state !== 'resolved' && <div><dt>{t('chain.controlled')}</dt><dd>
        {t(subject.log.decision.state === 'pending' ? 'activity.pending' : 'activity.transferred')}
      </dd></div>}
    </dl>
    {expanded && step.native.map((source, index) => {
      const tool = factObject(factObject(source.value).context);
      return <p className="chain-native" key={index}>{t('evidence.toolExecution')}{' · '}<FactValue value={tool.tool_name} /></p>;
    })}
    {expanded && step.futex.map((source, index) => source.kind === 'futex_selection' && <p key={index} className="chain-native muted">
      {t(source.value.wait === null ? 'evidence.waitUnpaired' : 'evidence.waitPaired')}
    </p>)}
  </div>;
}
function OriginalMessageDetails({ original, reference, onOpenExchange }: {
  original: unknown; reference: FactReference; onOpenExchange?: (exchange: ExchangeKey) => boolean;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [original]);
  let message: ChatMessage | null = null;
  if (open) { try { message = decodeChatMessage(original); } catch { /* The original remains inspectable below. */ } }
  return <details className="chain-message-details" onToggle={event => setOpen(event.currentTarget.open)}>
    <summary>{t('chat.originalMessage')}</summary>
    {open && <div className="stack-tight">
      <RawFacts summaryKey="auditWindow.references" value={reference} />
      {message ? <>
        {onOpenExchange && <button type="button" onClick={() => setFailed(onOpenExchange(message.id.exchange) === false)}>{t('auditWindow.openExchange')}</button>}
        {failed && <p className="error" role="status">{t('auditWindow.exchangeUnavailable')}</p>}
        <OriginalChatMessage message={message} />
      </> : <><p className="amber">{t('auditWindow.messageInvalid')}</p><RawFacts summaryKey="chat.originalMessage" value={original} /></>}
    </div>}
  </details>;
}

function ResultObservations({ step, onOpenExchange }: {
  step: Extract<ChainStep, { stage: 'tool_result' }>; onOpenExchange?: (exchange: ExchangeKey) => boolean;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  if (step.observations.length === 1) return <OriginalMessageDetails original={step.result.message}
    reference={step.result.reference} onOpenExchange={onOpenExchange} />;
  return <details className="chain-result-observations" onToggle={event => setOpen(event.currentTarget.open)}>
    <summary>{t('chain.resultObservations', { count: step.observations.length })}</summary>
    {open && <div className="stack-tight">
      <p className="overview-note">{t('chain.resultObservationsNote')}</p>
      {step.observations.map(observation => <div className="stack-tight" key={factsJson(observation.reference)}>
        <code>{factsJson(observation.reference)}</code>
        <OriginalMessageDetails original={observation.message} reference={observation.reference} onOpenExchange={onOpenExchange} />
      </div>)}
    </div>}
  </details>;
}

function StepContent({ step, operationId, expanded, operationInHeader, onOpenExchange }: {
  step: ChainStep; operationId?: string; expanded: boolean; operationInHeader: boolean;
  onOpenExchange?: (exchange: ExchangeKey) => boolean;
}) {
  const { t } = useI18n();
  if (step.stage === 'operation') return <Operation step={step} operationId={operationId} expanded={expanded} summarized={operationInHeader} />;
  if (step.stage === 'upstream') return <div className="stack-tight">
    {step.candidate && <p className="amber">{t(step.call.relation.kind === 'tool_container_candidate' ? 'chain.containerCandidate' : 'chain.candidate')}</p>}
    <p><FactValue value={step.call.part.name} /></p>
    <OriginalMessageDetails original={step.call.message} reference={step.call.reference} onOpenExchange={onOpenExchange} />
  </div>;
  if (step.stage === 'checkpoint') return step.completion ? <Completion completion={step.completion} />
    : step.log && <LogCheckpoint log={step.log} />;
  if (step.stage === 'tool_result') return <div className="stack-tight">
    {step.candidate && <p className="amber">{t('chain.candidate')}</p>}
    <p className="muted">{t('chain.toolResultNote')}</p>
    {step.content && <ChatContentValue value={step.content} />}
    <ResultObservations step={step} onOpenExchange={onOpenExchange} />
  </div>;
  return <div className="stack-tight">
    <p className="muted">{t('evidence.returnNote')}</p>
    <dl className="ask-facts">{([
      ['evidence.returnValue', step.record.return_value], ['evidence.operation', step.record.operation],
      ['evidence.decisionId', idLabel(step.record.decision_id)], ['evidence.waitId', step.record.wait_id],
      ['evidence.returnedAt', step.record.returned_boottime_ns],
     ] as const).map(([label, value]) => <div key={label}><dt>{t(label)}</dt><dd><FactValue value={value} /></dd></div>)}</dl>
  </div>;
}
const stageLabels = { upstream: 'chain.upstream', operation: 'chain.operation', checkpoint: 'activity.checkpoint',
  native_return: 'evidence.requestReturn', tool_result: 'chain.toolResult' } as const satisfies Record<ChainStage, MessageKey>;

const originalLabels = {
  deferred: 'evidence.originalReference', protocol_page: 'evidence.protocol',
  question: 'evidence.question', operation_result: 'evidence.operationResult',
  request_return: 'evidence.requestReturn', futex_selection: 'evidence.futexSelection', request_handoff: 'evidence.requestHandoff',
  tool_execution: 'evidence.toolExecution', rule_feedback: 'activity.feedbackTitle', protocol: 'evidence.protocol', message: 'evidence.message',
  security_log: 'chain.originalLog', relation: 'chain.originalRelation', gap: 'chain.originalGap',
} as const satisfies Record<ChainOriginal['kind'], MessageKey>;

function StageOriginals({ originals }: { originals: readonly ChainOriginal[] }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  return <details className="chain-originals" onToggle={event => setOpen(event.currentTarget.open)}>
    <summary>{t('chain.originals', { count: originals.length })}</summary>
    {open && <ul>{originals.map(({kind, value, references}, index) => <li key={index}>
      <strong>{t(originalLabels[kind])}</strong>
      <pre>{factsJson(references.length ? { references, value } : value)}</pre>
    </li>)}</ul>}
  </details>;
}

// The backend owns these partial relations. No retained-chain or timing matcher runs here.
export function NativeEvidence({ view, onReadNative }: { view: ContextView; onReadNative?: (key: EventKey) => void }) {
  const { t } = useI18n();
  const sources = view.source_records.filter(source => source.kind === 'request_handoff');
  const links = view.relations.filter(link => link.kind === 'native_request_reference' || link.kind === 'same_thread_return_sequence');
  if (!sources.length && !links.length && view.anchor.kind !== 'native_observation') return null;
  const reference = (ref: FactReference) => <span className="stack-tight">
    <code>{factsJson(ref)}</code>
    {ref.kind === 'native_observation' && onReadNative && <button type="button" onClick={() => onReadNative(ref.value)}>
      {t('evidence.readNativeObservation')}</button>}
  </span>;
  return <section className="chain-evidence stack-tight" aria-label={t('evidence.nativeLinks')}>
    <h4>{t('evidence.nativeLinks')}</h4>
    <p className="muted">{t('evidence.nativeSourcesNote')}</p>
    {links.length > 0 && <ol className="stack-tight">{links.map((link, index) => <li key={index} className="stack-tight">
      <strong>{t(link.kind === 'native_request_reference' ? 'evidence.originalReference' : 'evidence.temporalPattern')}
        {' · '}{t(link.kind === 'native_request_reference' ? 'evidence.nativeRequestReference' : 'evidence.sameThreadReturnSequence')}</strong>
      <p className="muted">{t(link.kind === 'native_request_reference' ? 'evidence.nativeReferenceNote' : 'evidence.returnSequenceNote')}</p>
      {reference(link.from)}<span aria-hidden="true">→</span>{reference(link.to)}
      <RawFacts summaryKey="evidence.relationEvidence" value={link} />
    </li>)}</ol>}
    {sources.map((source, index) => <div key={index} className="stack-tight">
      <strong>{t('evidence.requestHandoff')}{' · '}<FactValue value={factObject(source.value.phase).type} /></strong>
      {reference({ kind: 'native_observation', value: eventKey(source.value.observation) })}
      <RawFacts summaryKey="activity.original" value={source.value} />
    </div>)}
    {view.gaps.map((gap, index) => <div className="chain-gap" key={index}>
      <p>{t(gapLabels[gap.reason])}</p>{reference(gap.at)}
      <RawFacts summaryKey="evidence.gapReference" value={gap} />
    </div>)}
  </section>;
}

export function OriginalReferences({ view, onRead }: { view: ContextView; onRead: (reference: FactReference) => void }) {
  const { t } = useI18n();
  const records = view.source_records.filter(source => source.kind === 'deferred');
  return records.length > 0 && <details><summary>{t('evidence.originalReference')}</summary>
    {records.map((source, index) => <div className="stack-tight" key={index}>
      <code>{factsJson(source.value)}</code><button type="button" onClick={() => onRead(source.value)}>{t('evidence.read')}</button>
    </div>)}
  </details>;
}

export function CausalChain({ model, operationId, expanded = true, operationInHeader = false, onReadNative, onReadOriginal, onOpenExchange }: {
  model: CausalChainModel; operationId?: string; expanded?: boolean; operationInHeader?: boolean; onReadNative?: (key: EventKey) => void;
  onReadOriginal?: (reference: FactReference) => void;
  onOpenExchange?: (exchange: ExchangeKey) => boolean;
}) {
  const { t } = useI18n();
  const stages = expanded ? chainStages : chainStages.filter(stage => stage === 'operation' || stage === 'checkpoint');
  return <div className="causal-chain stack-tight">
    {!model.verified && <p className="error" role="status">{t('chain.evidenceMismatch')}</p>}
    <ol className="chain-rail" aria-label={t('ask.chain')}>
      {stages.map((stage, position) => {
        const steps = model.steps.filter(value => value.stage === stage);
        const step = steps[0];
        const issues = model.issues.filter(value => value.stage === stage);
        if (!step && !issues.length) return null;
        const nextStage = stages.slice(position + 1).find(next => model.steps.some(value => value.stage === next)
          || model.issues.some(value => value.stage === next));
        const outgoing = model.steps.find(value => value.stage === nextStage)?.incoming;
        const incoming = step?.incoming?.kind === 'confirmed' && !expanded && stage === 'operation' ? null : step?.incoming;
        return <li key={stage} className={`chain-row${stage === 'operation' ? ' chain-anchor' : ''}${!step ? ' chain-gap-row' : ''}`}
          data-stage={stage} data-connected={incoming ? true : undefined} data-outgoing={outgoing ? true : undefined}
          data-candidate={incoming?.kind === 'candidate' || outgoing?.kind === 'candidate' ? true : undefined}>
          <h4>{t(stageLabels[stage])}</h4>
          {incoming && <div className="chain-connection">
            {t(incoming.kind === 'confirmed' ? 'chain.confirmedLink' : incoming.kind === 'candidate' ? 'chain.candidate' : 'chain.sameOperation')}
          </div>}
          {steps.map((step, index) => <StepContent key={step.stage === 'tool_result' ? factsJson(resultObservationIdentity(step.result.reference, step.result.relation))
            : step.stage === 'upstream' ? factsJson(step.call.reference) : index} step={step} operationId={operationId}
            expanded={expanded} operationInHeader={operationInHeader} onOpenExchange={onOpenExchange} />)}
          {issues.map((issue, index) => <div className="chain-gap" key={index}>
            <p>{issue.at.kind === 'completion' && <span className="muted">{t('evidence.operationResult')}{' · '}</span>}
              <span>{t(gapLabels[issue.reason])}</span></p>
            {!issue.original && <p className="muted">{t('chain.projectionLimit')}</p>}
          </div>)}
          {expanded && model.originals[stage].length > 0 && <StageOriginals originals={model.originals[stage]} />}
        </li>;
      })}
    </ol>
    {expanded && model.view && <NativeEvidence view={model.view} onReadNative={onReadNative} />}
    {expanded && model.view && onReadOriginal && <OriginalReferences view={model.view} onRead={onReadOriginal} />}
    {expanded && model.receipt && <div className="chain-evidence">
      <RawFacts summaryKey="receipt.details" value={model.receipt} />
    </div>}
  </div>;
}
