import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type FormEvent } from 'react';
import { createRoot } from 'react-dom/client';
import { ReviewSession, type Snapshot } from './session';
import { endpoint } from './wire';
import { DashboardShell } from './dashboard';
import { ProtectionOverview } from './overview';
import { I18nProvider, LanguageSelect, useI18n } from './i18n';
import { RulesView } from './rules';
import { ProcessesView } from './processes';
import { ActivityView } from './activity';
import { GroupDelegationsView } from './group';
import { ChatHistoryView } from './chat-history';
import { SettingsView } from './settings';
import { EgressView } from './egress';
import { AskDialog, useReviewClock } from './ask';
import { selectCurrentReview } from './ask-state';
import type { Ask } from './event-wire';
import { browserSocket, useSessionLifecycle } from './global/browser-session';
import { activityRoute, navigateDashboard, useDashboardRoute } from './global/navigation';
import { RawFacts } from './global/raw-facts';
import './style.css';

const deployment = endpoint(import.meta.env.VITE_AS_WS_URL);
const session = new ReviewSession(deployment.url, browserSocket);

function ManagementGate({ state }: { state: Snapshot }) {
  const { t } = useI18n();
  const busy = state.phase !== 'locked';
  const form = useRef<HTMLFormElement>(null);
  useEffect(() => () => { form.current?.reset(); }, []);
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const element = event.currentTarget;
    let token = String(new FormData(element).get('management-token') ?? '');
    element.reset();
    session.connect(token);
    token = '';
  }
  return <main className="gate">
    <section className="unlock-card stack" aria-labelledby="management-title">
      <div className="unlock-tools"><LanguageSelect /></div>
      <h1 id="management-title">{t('gate.title')}</h1>
      <p>{t('gate.intro')}</p>
      <p id="management-help" className="muted">{t('gate.help')}</p>
      <form ref={form} onSubmit={submit} autoComplete="off" className="form-stack">
        <label htmlFor="management-token">{t('gate.token')}</label>
        <input id="management-token" name="management-token" type="password" autoComplete="off"
          autoCapitalize="none" spellCheck={false} autoFocus required minLength={64} maxLength={64}
          pattern="[0-9a-fA-F]{64}" aria-describedby="management-help management-memory"
          disabled={busy || !deployment.url} />
        <p id="management-memory" className="muted">{t('gate.memory')}</p>
        <div className="actions">
          <button className="primary" type="submit" disabled={busy || !deployment.url}>
            {t(busy ? 'gate.connecting' : 'gate.unlock')}
          </button>
          {busy && <button type="button" onClick={() => session.disconnect()}>{t('gate.cancel')}</button>}
        </div>
      </form>
      {deployment.problem && <p className="error" role="alert">{t(deployment.problem.key, deployment.problem.params)}</p>}
      {state.notice && <p className="error" role="alert">{t(state.notice.key, state.notice.params)}</p>}
      <details className="endpoint muted">
        <summary>{t('gate.connectionDetails')}</summary>
        <p>{t('gate.endpoint')}<code>{deployment.url ?? t('gate.noEndpoint')}</code></p>
        <p>{t('gate.origin')}<code>{window.location.origin}</code>{t('gate.originNote')}</p>
      </details>
    </section>
  </main>;
}

function ReviewWorkspace({ state }: { state: Snapshot }) {
  const { t } = useI18n();
  const [selected, setSelected] = useState<{ ask: Ask; inspecting: boolean } | null>(null);
  const route = useDashboardRoute();
  const page = route.page.id;
  const configurationPage = page === 'settings' || page === 'egress';
  const now = useReviewClock(state.reviews, session);
  const current = selectCurrentReview(state.reviews, selected?.ask ?? null, now);
  useLayoutEffect(() => {
    // This runs only after the authenticated review UI has committed to the DOM.
    session.setUiReady(true);
    return () => session.setUiReady(false);
  }, []);
  useLayoutEffect(() => {
    if (page !== 'processes') return;
    session.enterProcesses();
    return session.clearProcesses;
  }, [page]);
  useLayoutEffect(() => {
    if (page !== 'rules') return;
    session.enterRules();
    return session.leaveRules;
  }, [page]);
  useLayoutEffect(() => {
    if (page !== 'activity') return;
    session.enterActivity();
    return session.clearActivity;
  }, [page]);
  useLayoutEffect(() => {
    if (!configurationPage) return;
    session.enterSettings();
    return session.leaveSettings;
  }, [configurationPage]);
  useLayoutEffect(() => {
    if (page !== 'egress') return;
    session.enterEgress();
    return session.leaveEgress;
  }, [page]);
  useLayoutEffect(() => {
    setSelected(current ? { ask: current.ask, inspecting: false } : null);
  }, [current?.ask]);
  const inspecting = !!current && selected?.ask === current.ask && selected.inspecting;
  const askVisible = !!current && !inspecting;
  return <DashboardShell route={route} askVisible={askVisible} read={state.overview} refresh={session.refreshOverview} lock={() => session.disconnect()}
    overlay={<AskDialog row={askVisible ? current : undefined} onInspect={() => {
      if (current) setSelected({ ask: current.ask, inspecting: true });
    }} onClose={() => { if (current) session.closeReview(current); }}
      onEvidence={() => {
        if (!current?.completion) return;
        session.openCompletionEvidence(current.completion);
        session.closeReview(current);
        navigateDashboard(activityRoute);
      }} reviews={state.reviews} now={now}
      session={session} evidence={state.askEvidence} chatContent={state.chatContent} />}
    notice={<>
      {inspecting && current && <aside className="connection-card stack-tight" role="status">
        <p>{t(current.completion ? 'ask.inspectComplete' : 'ask.inspectWaiting')}</p>
        {!current.completion && <p>{t('ask.inspectRemaining', { seconds: Math.ceil(session.remaining(current, now) / 1000) })}</p>}
        <button type="button" onClick={() => setSelected({ ask: current.ask, inspecting: false })}>{t('ask.returnToQuestion')}</button>
      </aside>}
      {state.diagnostic && <aside className="connection-card stack-tight" role="status">
      <h2>{t('review.diagnostic')}</h2><p className="muted">{t('review.diagnosticNote')}</p>
      <RawFacts summaryKey="receipt.details" value={state.diagnostic} />
    </aside>}</>}>
    {page === 'overview' && <ProtectionOverview read={state.overview} refresh={session.refreshOverview} />}
    {page === 'processes' && <ProcessesView read={state.processes} session={session} />}
    {page === 'group' && <GroupDelegationsView read={state.group} chatContent={state.chatContent} session={session} />}
    {page === 'chat' && <ChatHistoryView read={state.chat} chatContent={state.chatContent} session={session} />}
    {page === 'rules' && <RulesView rules={state.rules} access={state.access} session={session} askVisible={askVisible} />}
    {page === 'activity' && <ActivityView activity={state.activity} access={state.access} chatContent={state.chatContent}
      session={session} askVisible={askVisible} />}
    {page === 'settings' && <SettingsView settings={state.settings} overview={state.overview} access={state.access} session={session} />}
    {page === 'egress' && <EgressView settings={state.settings} overview={state.overview} access={state.access} session={session}
      askVisible={askVisible} />}
  </DashboardShell>;
}

function App() {
  const state = useSyncExternalStore(session.subscribe, session.getSnapshot);
  useSessionLifecycle(session.disconnect);
  return state.phase === 'authenticated' ? <ReviewWorkspace state={state} /> : <ManagementGate state={state} />;
}
const root = document.getElementById('root');
if (root) createRoot(root).render(<I18nProvider><App /></I18nProvider>);
