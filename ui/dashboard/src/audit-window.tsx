import { useMemo, useState } from 'react';
import { askAuditWindows, type AuditWindow } from './audit-window-model';
import { OriginalChatMessage } from './chat-message';
import type { AskAuditContext, AskMessageWindow, ExchangeKey } from './context-wire';
import type { Ask } from './event-wire';
import { RawFacts } from './global/raw-facts';
import { useI18n } from './i18n';
import type { MessageKey } from './messages';
import { idLabel } from './wire';

const windowLabels = { confirmed: 'auditWindow.confirmed', candidate: 'auditWindow.candidate',
  unverified: 'auditWindow.unverified' } satisfies Record<AuditWindow['kind'], MessageKey>;
const problemLabels = { missing: 'auditWindow.messageMissing', conflicting: 'auditWindow.messageConflict',
  invalid: 'auditWindow.messageInvalid' } satisfies Record<NonNullable<AuditWindow['messages'][number]['problem']>, MessageKey>;

function MessageWindow({ window, onOpenExchange }: {
  window: AuditWindow; onOpenExchange?: (exchange: ExchangeKey) => boolean | void;
}) {
  const { t } = useI18n();
  const [failedWindow, setFailedWindow] = useState<AskMessageWindow | null>(null);
  const original = window.original;
  return <section className="ask-message-window stack-tight">
    <div className="panel-heading">
      <h4>{t(windowLabels[window.kind])}</h4>
      {onOpenExchange && <button type="button" onClick={() => {
        setFailedWindow(onOpenExchange(original.exchange) === false ? original : null);
      }}>{t('auditWindow.openExchange')}</button>}
    </div>
    <p className="overview-note"><code>{idLabel(original.exchange.flow_instance)} / {original.exchange.sequence}</code></p>
    {window.kind === 'candidate' && <p className="amber">{t('auditWindow.candidateNotice')}</p>}
    {failedWindow === original && <p role="status" className="error">{t('auditWindow.exchangeUnavailable')}</p>}
    <p className="overview-note">{t('auditWindow.earlier', { count: original.earlier_observed_messages })}</p>
    {window.messages.length === 0 && <p className="muted">{t('auditWindow.empty')}</p>}
    <ol className="ask-conversation">
      {window.messages.map((entry, index) => <li key={index} className="stack-tight">
        <RawFacts summaryKey="auditWindow.references" value={entry.reference} />
        {entry.scopeMismatch && <p className="amber">{t('auditWindow.messageScopeMismatch')}</p>}
        {entry.problem && <p className="amber">{t(problemLabels[entry.problem])}</p>}
        {(entry.problem || entry.scopeMismatch) && entry.originals.length > 0
          && <RawFacts summaryKey="chat.originalMessage" value={entry.originals} />}
        {entry.message && !entry.scopeMismatch && <OriginalChatMessage message={entry.message} />}
        {entry.gaps.length > 0 && <RawFacts summaryKey="auditWindow.contextGaps" value={entry.gaps} />}
      </li>)}
    </ol>
    <RawFacts summaryKey="auditWindow.sourceGaps" value={original.source_gaps} />
    <RawFacts summaryKey="auditWindow.references" value={original} />
  </section>;
}

// The parent owns content authorization, the current read slot and navigation eligibility.
export function AskAuditWindows({ context, ask, onOpenExchange }: {
  context: AskAuditContext; ask: Ask; onOpenExchange?: (exchange: ExchangeKey) => boolean | void;
}) {
  const windows = useMemo(() => askAuditWindows(ask, context), [ask, context]);
  return <div className="ask-audit-windows stack-tight">
    {windows.map((window, index) => <MessageWindow key={index} window={window} onOpenExchange={onOpenExchange} />)}
    <RawFacts summaryKey="auditWindow.contextGaps" value={context.view.gaps} />
  </div>;
}
