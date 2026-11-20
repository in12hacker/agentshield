import { AttributionPanel } from './attribution-panel';
import { sameEventKey } from './event-wire';
import { sameJsonValue } from './global/json-value';
import { ScrollPane } from './global/scroll-pane';
import { memo, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { ReviewSession } from './session';
import { isRuleLog, logPageSizes, type LogEntry, type LogPage } from './activity-wire';
import { selectionAnchor, selectionReference, type ActivitySelection, type ActivityState } from './activity-state';
import type { EvidenceRead } from './evidence-read';
import { factObject, projectCausalChain, type ChainSubject } from './causal-chain-model';
import { CausalChain, NativeEvidence, OriginalReferences, CheckpointValue, EventCategory, EventOperation, EventTarget, FactValue } from './causal-chain';
import { idLabel, type Access } from './wire';
import type { EventKey } from './event-wire';
import { factsJson } from './wire-json';
import { useI18n } from './i18n';
import type { MessageKey } from './messages';
import { RawFacts } from './global/raw-facts';
import { byteText, isByteString } from './global/byte-text';
import { downloadBlob } from './global/download';
import { ConversationExcerpt } from './conversation-excerpt';
import { activityIntent, type ActivityIntent } from './intent-conversation';
import { evidenceBlob, evidenceFilename } from './activity-evidence';
import { focusOutsideDialog } from './global/focus';

function FactFields({ fields }: { fields: readonly (readonly [MessageKey, string | number | null])[] }) {
  const { t } = useI18n();
  return <dl className="process-fields">{fields.map(([label, value]) =>
    <div key={label}><dt>{t(label)}</dt><dd>{value === null ? t('common.null') : <code>{value}</code>}</dd></div>)}</dl>;
}
function KeyFields({ anchor }: { anchor: EventKey }) {
  return <FactFields fields={[
    ['evidence.sourceInstance', idLabel(anchor.source_instance)],
    ['evidence.sequenceDomain', anchor.sequence_domain],
    ['evidence.sequence', anchor.sequence],
  ]} />;
}
function ByteFact({ value }: { value: unknown }) {
  if (!isByteString(value)) return <code>{factsJson(value)}</code>;
  const text = byteText(value);
  return <code>{text.kind === 'bytes' ? text.value : JSON.stringify(text.value)}</code>;
}
function EntryKind({ entry }: { entry: LogEntry }) {
  const { t } = useI18n();
  return <>{isRuleLog(entry) ? <>{t('activity.rule')} · <EventCategory event={entry.event} /></>
    : t('activity.writeClose')}</>;
}
function EntrySummary({ entry }: { entry: LogEntry }) {
  const { t } = useI18n();
  if (isRuleLog(entry)) {
    const process = factObject(entry.event.process);
    return <><span className="activity-row-head">
      <strong className="activity-rule-result" title={t('activity.checkpoint')}><CheckpointValue log={entry} compact /></strong>
      <span className="activity-row-operation"><EventOperation event={entry.event} /></span>
    </span>
      <span className="activity-row-target"><EventTarget event={entry.event} compact /></span>
      <span className="activity-row-process"><FactValue value={process.name ?? process.executable} />
        {typeof entry.event.header.actor_tgid === 'number' && <span>{t('processes.pidValue', { pid: entry.event.header.actor_tgid })}</span>}
      </span></>;
  }
  return <><strong><EntryKind entry={entry} /></strong>
    <span className="activity-row-target"><ByteFact value={entry.path} /></span></>;
}
function EvidenceDownload({ selection, read, session }: {
  selection: ActivitySelection; read: EvidenceRead; session: ReviewSession;
}) {
  const { t } = useI18n();
  const [failed, setFailed] = useState(false);
  const download = () => {
    try {
      // Recheck the live session and exact selection/read, not the render's result.
      const snapshot = session.getActivityEvidenceSnapshot(selection, read);
      if (!snapshot) { setFailed(true); return; }
      downloadBlob(evidenceBlob(snapshot), evidenceFilename(snapshot.event_key));
      setFailed(false);
    } catch { setFailed(true); }
  };
  return <div className="stack-tight">
    <div className="activity-download">
      <button id="activity-evidence-download" type="button" aria-describedby="activity-evidence-download-note"
        onClick={download}>{t('evidence.download')}</button>
      <p id="activity-evidence-download-note" className="overview-note">{t('evidence.downloadNote')}</p>
    </div>
    {failed && <p className="error" role="status">{t('evidence.downloadFailed')}</p>}
  </div>;
}

function IntentSummary({ intent }: { intent: ActivityIntent }) {
  const { t } = useI18n();
  return <div className="activity-intent-summary stack-tight">
    <ul className="activity-conversation">
      {intent.user ? <ConversationExcerpt key={intent.user.id.sequence} message={intent.user} labelKey="activity.intentUser" limit={160} />
        : <li className="conversation-excerpt"><strong>{t('activity.intentUser')}</strong><p className="muted">{t('activity.intentNoUser')}</p></li>}
      {intent.assistant ? <ConversationExcerpt key={intent.assistant.id.sequence} message={intent.assistant} labelKey="activity.intentAssistant" limit={160} />
        : <li className="conversation-excerpt"><strong>{t('activity.intentAssistant')}</strong><p className="muted">{t('activity.intentNoAssistant')}</p></li>}
    </ul>
    <p className="activity-tool"><span>{t('activity.intentTool')}</span> <strong><FactValue value={intent.tool.name} /></strong></p>
  </div>;
}

const feedbackLabels = {
  prepared: 'activity.feedbackPrepared', applied: 'activity.feedbackApplied',
  submitted: 'activity.feedbackSubmitted', failed: 'activity.feedbackFailed',
} as const;

function Detail({ selection, read, chatContent, session, close, askVisible }: {
  selection: ActivitySelection; read: EvidenceRead; chatContent: boolean; session: ReviewSession; close: () => void; askVisible: boolean;
}) {
  const { t } = useI18n();
  const title = useRef<HTMLHeadingElement>(null);
  // Only a new selection moves focus. Dismissing an Ask must retain its own return focus.
  useLayoutEffect(() => {
    if (!askVisible) focusOutsideDialog(title.current);
  }, [selection]);
  const anchor = selectionAnchor(selection);
  const reference = selectionReference(selection);
  const model = useMemo(() => {
    const subject: ChainSubject | null = selection.kind === 'completion' ? { kind: 'completion', completion: selection.completion }
      : selection.kind === 'log' && isRuleLog(selection.row.record) ? { kind: 'log', log: selection.row.record } : null;
    return subject ? projectCausalChain(subject, chatContent && read.result?.status === 'completed'
      ? { kind: 'activity', view: read.result.data.view } : null) : null;
  }, [selection, chatContent, read.result]);
  const operation = model?.steps.find(step => step.stage === 'operation');
  const checkpoint = model?.steps.find(step => step.stage === 'checkpoint');
  const intent = useMemo(() => model ? activityIntent(model) : null, [model]);
  const call = model?.steps.find(step => step.stage === 'upstream');
  const feedback = model?.view?.source_records.filter(source => source.kind === 'rule_feedback') ?? [];
  const fact = feedback.length === 1 ? feedback[0]?.value : undefined;
  const feedbackState = fact && anchor && call && operation?.incoming?.kind === 'confirmed'
    && sameEventKey(fact.denial, anchor) && sameJsonValue(fact.call, call.call.reference.value) ? fact.state : null;
  const [showChain, setShowChain] = useState(model !== null);
  return <ScrollPane as="aside" id="activity-detail" className="process-detail stack-tight" labelledBy="activity-detail-title">
    <div className="panel-heading activity-detail-heading"><h3 id="activity-detail-title" ref={title} tabIndex={-1}>
      {operation ? <EventOperation event={operation.event} /> : selection.kind === 'log' ? <EntryKind entry={selection.row.record} />
        : t(selection.kind === 'native_observation' ? 'evidence.nativeObservation' : selection.kind === 'original' ? 'evidence.originalReference' : 'activity.detail')}</h3>
      <button type="button" onClick={close}>{t('processes.returnToList')}</button></div>
    {operation && <p className="ask-target activity-detail-target">
      <EventTarget event={operation.event} /></p>}
    {model && <div className="activity-checkpoint"><span>{t('activity.checkpoint')}</span>
      <strong><CheckpointValue completion={checkpoint?.completion} log={checkpoint?.log} compact /></strong></div>}
    {selection.kind === 'log' && !isRuleLog(selection.row.record) && <>
      <p className="activity-detail-target"><ByteFact value={selection.row.record.path} /></p>
      <p className="muted">{t('activity.closeNote')}</p>
    </>}
    {anchor && operation && <AttributionPanel key={JSON.stringify(anchor)} event={operation.event}
      query={read.attributionRequest} result={chatContent && read.result?.status === 'completed' ? read.result.data.attribution : null}
      loading={read.phase === 'loading'} completed={read.result?.status === 'completed'} allowed={chatContent}
      onRead={query => session.readActivityEvidence(query)} onOpenExchange={exchange => session.openAuditExchange(read, exchange)} />}
    <section className="activity-evidence stack-tight" aria-labelledby="activity-evidence-title">
      <div className="panel-heading activity-intent-heading">
        <h4 id="activity-evidence-title">{t(anchor ? 'activity.intentTitle' : 'evidence.title')}</h4>
        {reference && chatContent && <button type="button" disabled={read.phase === 'loading'} onClick={() => session.readActivityEvidence()}>
          {t(read.phase === 'initial' || read.phase === 'loading' ? 'evidence.read' : 'evidence.reread')}
        </button>}
      </div>
      {!reference ? <p className="muted" role="status">{t('evidence.noAnchor')}</p>
        : !chatContent ? <p className="muted" role="status">{t('evidence.noGrant')}</p> : <>
           {read.phase === 'loading' && <button type="button" aria-describedby="activity-read-wait-note"
             onClick={() => session.cancelActivityEvidence(selection, read)}>{t('evidence.cancelWait')}</button>}
           <div role="status" aria-live="polite">
             {read.phase === 'initial' && <p className="muted">{t('activity.intentRead')}</p>}
              {read.phase === 'loading' && <p>{t('evidence.loading')}</p>}
              {read.phase === 'loading' && <p id="activity-read-wait-note" className="muted">{t('evidence.cancelNote')}</p>}
              {read.phase === 'cancelled' && <p className="muted">{t('evidence.cancelled')}</p>}
             {read.result?.status === 'completed' && !intent && selection.kind !== 'native_observation' && selection.kind !== 'original' && <p className="muted">{t('activity.intentUnlinked')}</p>}
             {read.problem && <p className="error">{t(read.problem.key, read.problem.params)}</p>}
             {read.result?.status === 'failed' && <p className="error">{t(read.result.code === 'unauthorized'
                ? 'evidence.noGrant' : read.result.code === 'not_found' ? 'evidence.notFound' : 'evidence.readFailed')}</p>}
           </div>
         </>}
      {read.result?.status === 'failed' && <RawFacts summaryKey="activity.reply" value={read.result} />}
      {intent && <IntentSummary intent={intent} />}
      {chatContent && selection.kind === 'native_observation' && read.result?.status === 'completed'
        && <><NativeEvidence view={read.result.data.view} onReadNative={key => session.openNativeObservation(read, key)} />
          <OriginalReferences view={read.result.data.view} onRead={reference => session.openOriginalRecord(read, reference)} /></>}
      {chatContent && selection.kind === 'original' && read.result?.status === 'completed' && <>
        <code>{factsJson(selection.reference)}</code>
        {read.result.data.view.source_records.map((source, index) => <div className="stack-tight" key={index}>
          <RawFacts summaryKey="activity.original" value={source} />
          {source.kind === 'protocol_page' && source.value.next_offset !== null && <button type="button"
            onClick={() => session.openOriginalRecord(read, selection.reference, source.value.next_offset!)}>{t('processes.nextPage')}</button>}
        </div>)}
      </>}
      {feedbackState && <p className="overview-note">
        <strong>{t('activity.feedbackTitle')}</strong>{' · '}{t(feedbackLabels[feedbackState.type])}
        {feedbackState.type === 'failed' && <>{' · '}<code>{feedbackState.code}</code></>}
      </p>}
    </section>
    <details className="activity-originals" open={showChain} onToggle={event => setShowChain(event.currentTarget.open)}>
      <summary>{t(model ? 'activity.inspectChain' : 'activity.original')}</summary>
      {showChain && <div className="stack-tight">
        {model && <CausalChain model={model} operationInHeader onReadNative={key => session.openNativeObservation(read, key)}
          onReadOriginal={reference => session.openOriginalRecord(read, reference)} onOpenExchange={exchange => {
            if (!session.openAuditExchange(read, exchange)) return false;
            window.location.hash = '#chat-history'; return true;
          }} />}
        {!model && selection.kind === 'log' && <RawFacts summaryKey="activity.original" value={selection.row.record} />}
        <p className="overview-note">{selection.kind === 'log' ? t('activity.rowId', { id: selection.row.id })
          : t(selection.kind === 'native_observation' ? 'evidence.nativeObservation' : selection.kind === 'original' ? 'evidence.originalReference' : 'activity.completionSource')}</p>
        {selection.kind === 'native_observation' && <KeyFields anchor={selection.observation} />}
        {anchor && <details><summary>{t('evidence.anchorDetails')}</summary><KeyFields anchor={anchor} />
          {selection.kind === 'completion' && <FactFields fields={[[ 'evidence.decisionId', selection.completion.decision_id === null
            ? null : idLabel(selection.completion.decision_id) ]]} />}</details>}
      </div>}
    </details>
    {session.getActivityEvidenceSnapshot(selection, read) && <EvidenceDownload selection={selection} read={read} session={session} />}
  </ScrollPane>;
}

interface ListPosition {
  page: LogPage; trigger: HTMLButtonElement;
  scroll: { element: Element; top: number; left: number }[];
}
function listPosition(page: LogPage, trigger: HTMLButtonElement): ListPosition {
  const scroll: ListPosition['scroll'] = [];
  // The shell scrolls .dashboard-main; desktop may also scroll the list itself.
  // Document scrolling is separate, including when this view is mounted without the shell.
  for (let element: HTMLElement | null = trigger.parentElement; element; element = element.parentElement) {
    const style = getComputedStyle(element);
    if (/(auto|scroll|overlay)/.test(`${style.overflowY} ${style.overflowX}`)) {
      scroll.push({ element, top: element.scrollTop, left: element.scrollLeft });
    }
  }
  const documentScroll = document.scrollingElement;
  if (documentScroll && !scroll.some(value => value.element === documentScroll)) {
    scroll.push({ element: documentScroll, top: documentScroll.scrollTop, left: documentScroll.scrollLeft });
  }
  return { page, trigger, scroll };
}

export const ActivityView = memo(function ActivityView({ activity: a, access, chatContent, session, askVisible }: {
  activity: ActivityState; access: Access | null; chatContent: boolean; session: ReviewSession;
  askVisible: boolean;
}) {
  const { t } = useI18n();
  const position = useRef<ListPosition | null>(null);
  const restoreFocus = useRef(false);
  useLayoutEffect(() => {
    if (position.current && position.current.page !== a.page) {
      position.current = null;
      restoreFocus.current = false;
    }
    if (a.selection || !restoreFocus.current) return;
    restoreFocus.current = false;
    const saved = position.current;
    position.current = null;
    if (!saved) return;
    if (!askVisible && saved.trigger.isConnected) focusOutsideDialog(saved.trigger, { preventScroll: true });
    for (const { element, top, left } of saved.scroll) if (element.isConnected) {
      element.scrollTop = top;
      element.scrollLeft = left;
    }
  }, [a.selection, a.page]);
  const busy = a.phase === 'loading';
  const allowed = access === 'manage_service';
  const close = () => {
    restoreFocus.current = true;
    session.closeActivitySelection();
  };
  return <section id="activity" className="process-workspace activity-workspace stack-tight"
    aria-labelledby="activity-heading" data-detail-open={a.selection !== null || undefined}>
    <p className="overview-note">{t('activity.subtitle')}</p>
      <div className="activity-list-tools stack-tight">{!allowed ? <p className="muted" role="status">{t('activity.noGrant')}</p> : <>
        <div className="activity-controls">
          <button type="button" disabled={busy} onClick={() => session.refreshActivity()}>{t('common.refresh')}</button>
          <label>{t('activity.pageSize')}<select value={a.query.limit} disabled={busy}
            onChange={event => session.refreshActivity(Number(event.target.value))}>
            {logPageSizes.map(size => <option key={size} value={size}>{size}</option>)}
          </select></label>
          <button type="button" disabled={busy || !a.page?.next_before} onClick={session.nextActivity}>{t('activity.older')}</button>
        </div>
        <div role="status" aria-live="polite">
          {a.phase === 'initial' && <p className="muted">{t('activity.initial')}</p>}
          {busy && <p>{t('activity.loading')}</p>}
          {a.phase === 'ready' && a.page?.records.length === 0 && <p className="process-empty">{t('activity.empty')}</p>}
          {a.problem && <p className="error">{t(a.problem.key, a.problem.params)}</p>}
        </div>
         {(a.diagnostic || a.receipt) && <RawFacts summaryKey="activity.reply" value={{ receipt: a.receipt, diagnostic: a.diagnostic }} />}
      </>}</div>
      <div className={`process-layout activity-layout${a.selection ? ' has-detail' : ''}`}>
        <ScrollPane className="activity-rows" labelledBy="activity-heading" always>
          {allowed && a.page && <>
            <p className="overview-note">{t('activity.page', { page: a.pageNumber })}</p>
            <ul className="process-list">{a.page.records.map(row => <li key={row.id}>
              <button type="button" className="process-row" aria-controls="activity-detail"
                aria-expanded={a.selection?.kind === 'log' && a.selection.row === row}
                onClick={event => { if (a.page) position.current = listPosition(a.page, event.currentTarget); session.selectActivity(row); }}>
                <span className="activity-row-summary"><EntrySummary entry={row.record} /></span>
                <span className="process-row-meta">{t('activity.rowId', { id: row.id })}</span>
              </button>
            </li>)}</ul>
            {a.page.next_before === null && <p className="overview-note">{t('activity.lastPage')}</p>}
          </>}
        </ScrollPane>
        {a.selection && <Detail key={a.selection.kind === 'log' ? a.selection.row.id
          : JSON.stringify(selectionReference(a.selection))}
          selection={a.selection} read={a.evidence} chatContent={chatContent} session={session} close={close} askVisible={askVisible} />}
      </div>
  </section>;
});
