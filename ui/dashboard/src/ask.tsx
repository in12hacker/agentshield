import { AttributionPanel } from './attribution-panel';
import { AskAuditWindows } from './audit-window';
import { useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { ReviewSession } from './session';
import { emptyAskEvidence, reviewEligible, type AskEvidence, type Review } from './ask-state';
import { factObject, projectCausalChain } from './causal-chain-model';
import { CausalChain, CheckpointValue, DecisionValue, EventCategory, EventOperation, EventTarget, FactValue } from './causal-chain';
import { idLabel } from './wire';
import { sameJsonValue } from './global/json-value';
import { useI18n } from './i18n';

function AskChain({ row, read, chatContent, session, onNavigate }: {
  row: Review; read: AskEvidence; chatContent: boolean; session: ReviewSession; onNavigate: () => void;
}) {
  const { t } = useI18n();
  const ask = row.ask;
  const context = chatContent && read.result?.status === 'completed' ? read.result.data.context : null;
  const model = useMemo(() => projectCausalChain({ kind: 'ask', ask, completion: row.completion },
    context ? { kind: 'ask', context } : null), [ask, context, row.completion]);
  const failure = read.result?.status === 'failed' ? read.result : null;
  const allowed = chatContent && failure?.code !== 'unauthorized';
  return <div className="ask-information stack-tight">
    <details className="ask-intent">
      <summary>{t('ask.intentDetails')}</summary>
    <section aria-labelledby="ask-chat-heading" className="ask-chat">
      <div className="panel-heading"><h3 id="ask-chat-heading">{t('ask.recentChat')}</h3>
        {allowed && read.phase !== 'loading' && <button type="button" onClick={() => session.readAskContext(row)}>
          {t(read.phase === 'initial' ? 'ask.readContext' : 'evidence.reread')}
        </button>}
      </div>
      <div role="status" aria-live="polite" className="ask-context-status">
        {!allowed ? <p>{t('ask.noGrant')}</p>
          : read.phase === 'loading' && !context ? <p>{t('ask.loading')}</p>
          : read.phase === 'cancelled' ? <p>{t('evidence.cancelled')}</p>
          : read.problem ? <p className="error">{t(read.problem.key, read.problem.params)}</p>
          : failure ? <p className="error">{t(failure.code === 'not_found' ? 'evidence.notFound' : 'evidence.readFailed')}</p>
          : context && !context.window ? <p>{t('ask.noConfirmedChat')}</p> : null}
      </div>
      {allowed && read.phase === 'loading' && <button type="button" onClick={() => session.cancelAskContext(row, read)}>
        {t('evidence.cancelWait')}
      </button>}
      <AttributionPanel key={JSON.stringify(ask.decision_id)} event={ask.event} query={read.attributionRequest}
        result={context?.attribution ?? null} loading={read.phase === 'loading'} completed={read.result?.status === 'completed'} allowed={allowed}
        onRead={query => session.readAskContext(row, query)} onOpenExchange={exchange => {
          if (!session.openAuditExchange(read, exchange)) return false;
          onNavigate(); return true;
        }} />
      {context && <AskAuditWindows context={context} ask={ask} onOpenExchange={exchange => {
        if (!session.openAuditExchange(read, exchange)) return false;
        onNavigate(); window.location.hash = '#chat-history'; return true;
      }} />}
    </section>
    </details>
    <section aria-labelledby="ask-chain-heading" className="ask-chain stack-tight">
      <h3 id="ask-chain-heading">{t('ask.chain')}</h3>
      {!context && <p className="ask-context-status">{t('ask.unconfirmedUpstream')}</p>}
      <CausalChain model={model} onReadNative={key => {
        if (session.openNativeObservation(read, key)) { onNavigate(); window.location.hash = '#activity'; }
      }} onReadOriginal={reference => {
        if (session.openOriginalRecord(read, reference)) { onNavigate(); window.location.hash = '#activity'; }
      }} onOpenExchange={exchange => {
        if (!session.openAuditExchange(read, exchange)) return false;
        onNavigate(); window.location.hash = '#chat-history'; return true;
      }} />
      {failure && <p className="error"><code>{failure.code} / {JSON.stringify(failure.message)}</code></p>}
    </section>
  </div>;
}

function AskIntent(props: Parameters<typeof AskChain>[0]) {
  return <AskChain {...props} />;
}

// 原样本耗尽时重新选择；不续期、不生成服务端结果。
export function useReviewClock(reviews: readonly Review[], session: ReviewSession) {
  const [now, setNow] = useState(() => performance.now());
  useLayoutEffect(() => {
    let timer: ReturnType<typeof setTimeout>;
    function sample() {
      const sampled = performance.now();
      setNow(sampled);
      let next = Infinity;
      for (const review of reviews) {
        if (review.completion) continue;
        const remaining = session.remaining(review, sampled);
        if (remaining > 0) next = Math.min(next, remaining % 1000 || 1000);
      }
      if (Number.isFinite(next)) timer = setTimeout(sample, Math.max(1, Math.ceil(next)));
    }
    sample();
    return () => clearTimeout(timer);
  }, [reviews, session]);
  return now;
}

export interface AskDialogProps {
  row: Review | undefined; onClose: () => void; onInspect: () => void; onEvidence: () => void; session: ReviewSession;
  reviews: readonly Review[];
  now: number;
  evidence: AskEvidence; chatContent: boolean;
}
export function AskDialog({ row, onClose, onInspect, onEvidence, reviews, now, session, evidence, chatContent }: AskDialogProps) {
  const { t } = useI18n();
  const dialog = useRef<HTMLDialogElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);
  const visible = !!row;
  useLayoutEffect(() => { session.selectAskContext(visible ? row : undefined); }, [session, visible, row?.ask, row?.completion, chatContent]);
  const otherPending = visible ? reviews.filter(review => review !== row && reviewEligible(review, now)).length : 0;
  useLayoutEffect(() => {
    const element = dialog.current;
    if (!element) return;
    if (visible && row) {
      if (!element.open) {
        returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
        element.showModal();
      }
      heading.current?.focus();
    } else if (element.open) {
      element.close();
      if (returnFocus.current?.isConnected) returnFocus.current.focus();
      else document.querySelector<HTMLElement>('.page-title>h1')?.focus();
      returnFocus.current = null;
    }
  }, [visible, row?.ask]);
  const remaining = row ? session.remaining(row, now) : 0;
  const read: AskEvidence = evidence.ask === row?.ask ? evidence
    : emptyAskEvidence(row?.ask ?? null);
  const actor = row ? factObject(row.ask.event.process).name : null;
  const pid = row?.ask.event.header.actor_tgid;
  const choices = row?.ask.choices.map((choice, index) => ({choice, index})) ?? [];
  const first = row ? choices.findIndex(({choice}) => sameJsonValue(choice, row.ask.timeout)) : -1;
  const ordered = first > 0 ? [...choices.slice(first, first + 1), ...choices.slice(0, first), ...choices.slice(first + 1)] : choices;
  const countdown = remaining > 0 && !row?.attempted
    ? <span className="ask-default-time" aria-hidden="true">{t('review.remaining', {seconds: Math.ceil(remaining / 1000)})}</span>
    : null;
  return <dialog ref={dialog} className="review-dialog" aria-labelledby="review-title" aria-describedby="ask-target"
    onCancel={event => { event.preventDefault(); event.stopPropagation(); if (row?.completion) onClose(); }}>
    {row && <div className="review-content">
      <header className="ask-header">
        <div><p className="ask-category"><EventCategory event={row.ask.event} /></p><h2 id="review-title" ref={heading} tabIndex={-1}><EventOperation event={row.ask.event} /></h2>
          {row.completion ? <p className="review-countdown">
            <span>{t('review.result')}</span>{' · '}<CheckpointValue completion={row.completion} compact /></p>
            : remaining <= 0 && !row.attempted && <p className="review-countdown error" role="status">{t('review.expired')}</p>}
          {!row.completion && row.attempted && <p className="review-countdown">
            {t('review.remaining', { seconds: Math.ceil(remaining / 1000) })}</p>}
          {otherPending > 0 && <p className="review-countdown">{t('ask.otherPending', { count: otherPending })}</p>}
        </div>
        <div className="actions">
          {row.completion && <>
            <button type="button" onClick={onClose}>{t('ask.closeCompleted')}</button>
            <button type="button" disabled={!chatContent} onClick={onEvidence}>{t('evidence.view')}</button>
            {!chatContent && <p className="muted">{t('evidence.noGrant')}</p>}
          </>}
        </div>
        <div id="ask-target" className="ask-subject">
          <p className="ask-actor"><span>{t('facts.process')}</span>{' '}
            {actor ? <FactValue value={actor} />
              : typeof pid === 'number' ? t('processes.pidValue', { pid }) : <FactValue value={actor} />}</p>
          <p className="ask-target"><EventTarget event={row.ask.event} compact /></p>
        </div>
      </header>
      <div className="ask-body stack-tight" key={idLabel(row.ask.decision_id)}>
        <AskIntent onNavigate={onInspect} row={row} read={read} chatContent={chatContent} session={session} />
      </div>
      {!row.completion && <footer className="ask-actions stack-tight">
        {row.attempted && <p className={row.phase === 'send_failed' ? 'error' : 'muted'} role="status">
          {t(row.phase === 'sending' ? 'review.sending' : row.phase === 'sent' ? 'review.sent'
            : 'review.sendFailed')}
        </p>}
        {first < 0 && !row.attempted && <p className="ask-automatic">
          <span>{t('ask.timeoutDefault')}{' · '}<DecisionValue value={row.ask.timeout} /></span>{countdown}
        </p>}
        <div className="choice-buttons">
          {ordered.map(({choice, index}) => {
            const isDefault = index === first;
            return <button type="button" key={index} data-default={isDefault || undefined}
              className={isDefault ? 'primary' : ''}
              aria-describedby={isDefault ? 'ask-timeout-default' : undefined}
              disabled={row.attempted || remaining <= 0} onClick={() => session.submit(row, index)}
              onKeyDown={event => { if (event.repeat) event.preventDefault(); }}>
              <DecisionValue value={choice} />
              {isDefault && <span className="ask-default-meta">
                <span id="ask-timeout-default" aria-hidden="true">{t('ask.timeoutDefault')}</span>{countdown}
              </span>}
            </button>;
          })}
        </div>
        {row.ask.choices.length === 0 && <p role="status">{t('review.noChoices')}</p>}
      </footer>}
    </div>}
  </dialog>;
}
