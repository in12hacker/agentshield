import { SessionDirectory } from './chat-session';
import { memo, useLayoutEffect, useRef } from 'react';
import type { ReviewSession } from './session';
import { chatData, chatDenied, type ChatRead, type ChatState } from './chat-state';
import type { ExchangeKey } from './context-wire';
import { sameJsonValue } from './global/json-value';
import { RawFacts } from './global/raw-facts';
import { ChatHistoryMessage } from './chat-message';
import type { ChatMessageScope } from './chat-wire';
import { idLabel } from './wire';
import { useI18n } from './i18n';
import { focusOutsideDialog } from './global/focus';

function ExchangeIdentity({ exchange }: { exchange: ExchangeKey }) {
  const { t } = useI18n();
  return <dl className="process-fields">
    <div><dt>{t('chat.flowInstance')}</dt><dd><code>{idLabel(exchange.flow_instance)}</code></dd></div>
    <div><dt>{t('chat.exchangeSequence')}</dt><dd><code>{exchange.sequence}</code></dd></div>
  </dl>;
}
function ReadStatus({ read }: { read: ChatRead }) {
  const { t } = useI18n();
  const failure = read.result?.status === 'failed' ? read.result : null;
  return <div role="status" className="stack-tight">
    {read.phase === 'initial' && <p className="muted">{t('chat.initial')}</p>}
    {read.phase === 'loading' && <p>{t('chat.loading')}</p>}
    {read.problem && <p className="error">{t(read.problem.key, read.problem.params)}</p>}
    {failure && <div className="receipt-failure stack-tight"><p>{t('chat.readFailed')}</p>
      <p>{t('common.failureCode')}<code>{failure.code}</code></p>
      <p>{t('common.originalMessage')}<code>{JSON.stringify(failure.message)}</code></p></div>}
  </div>;
}
function PageTools({ read, allowed, refresh, next }: {
  read: ChatRead; allowed: boolean; refresh: (limit?: number) => void; next: () => void;
}) {
  const { t } = useI18n();
  const busy = read.phase === 'loading', data = chatData(read);
  const nextCursor = data?.type === 'chat_sessions' ? data.next_before : data?.next_after;
  return <div className="process-controls">
    <button type="button" disabled={!allowed || busy} onClick={() => refresh()}>{t('processes.firstPage')}</button>
    <label>{t('chat.pageSize')}<select disabled={!allowed || busy} value={read.query?.limit ?? 8}
      onChange={event => refresh(Number(event.target.value))}>
      {[8, 16].map(limit => <option key={limit} value={limit}>{limit}</option>)}
    </select></label>
    <button type="button" disabled={!allowed || busy || !nextCursor} onClick={next}>{t('processes.nextPage')}</button>
    {data && <p className="overview-note">{t('chat.page', { page: read.pageNumber })}</p>}
  </div>;
}
function ExchangeMessages({ read, scope, session, allowed, close }: {
  read: ChatRead; scope: ChatMessageScope; session: ReviewSession; allowed: boolean; close: () => void;
}) {
  const { t } = useI18n();
  const heading = useRef<HTMLHeadingElement>(null);
  useLayoutEffect(() => { focusOutsideDialog(heading.current); }, [scope]);
  const data = chatData(read);
  return <section id="chat-messages" className="process-detail stack-tight" aria-labelledby="chat-messages-heading">
    <div className="panel-heading"><h2 id="chat-messages-heading" tabIndex={-1} ref={heading}>{t('chat.messages')}</h2>
      <button type="button" onClick={close}>{t('processes.returnToList')}</button></div>
    {scope.kind === 'model_http' ? <ExchangeIdentity exchange={scope.exchange} /> : <p>{t('chat.nativeGateway')}</p>}
    <PageTools read={read} allowed={allowed} refresh={session.chatHistory.refreshMessages} next={session.chatHistory.nextMessages} />
    <ReadStatus read={read} />
    {data?.type === 'chat_messages' && <>
      {data.messages.length === 0 && <p>{t('chat.emptyMessages')}</p>}
      {data.messages.map(message => <ChatHistoryMessage key={JSON.stringify(message.kind === 'model_http' ? message.message.id : message.reference)} entry={message} />)}
      <p className="overview-note">{t(data.next_after ? 'chat.moreMessages' : 'chat.lastPage')}</p>
    </>}
  </section>;
}
export const ChatHistoryView = memo(function ChatHistoryView({ read, chatContent, session }: { read: ChatState; chatContent: boolean; session: ReviewSession }) {
  const { t } = useI18n();
  useLayoutEffect(() => { session.chatHistory.enter(); return session.chatHistory.leave; }, [session]);
  const trigger = useRef<HTMLButtonElement | null>(null);
  const data = chatData(read.exchanges);
  useLayoutEffect(() => { trigger.current = null; }, [data]);
  const allowed = chatContent && !chatDenied(read);
  const native = read.selection?.kind === 'native_gateway' || read.sessionSelection?.kind === 'native_gateway';
  return <section id="chat-history" className="chat-workspace stack-tight" aria-labelledby="chat-heading">
    <p className="overview-note">{t('chat.scope')}</p><p className="overview-note">{t('chat.identityGap')}</p>
    {!allowed && <p className="amber" role="status">{t('chat.noGrant')}</p>}
    <SessionDirectory read={read} allowed={allowed} session={session} />
    {!native && <><PageTools read={read.exchanges} allowed={allowed} refresh={session.chatHistory.refreshExchanges} next={session.chatHistory.nextExchanges} />
      <ReadStatus read={read.exchanges} /></>}
    <div className={`process-layout${read.selection ? ' has-detail' : ''}`}>
      {allowed && data?.type === 'chat_exchanges' && <div>
        {data.exchanges.length === 0 && <p className="process-empty">{t('chat.emptyExchanges')}</p>}
        <ul className="process-list">{data.exchanges.map(exchange => <li key={`${idLabel(exchange.flow_instance)}:${exchange.sequence}`}>
          <button type="button" className="process-row" aria-controls="chat-messages" aria-expanded={sameJsonValue(read.selection, { kind: 'model_http', exchange })}
            onClick={event => { trigger.current = event.currentTarget; session.chatHistory.select(exchange); }}>
            <strong>{t('chat.exchange')}</strong><ExchangeIdentity exchange={exchange} />
          </button></li>)}</ul>
        <p className="overview-note">{t(data.next_after ? 'chat.moreExchanges' : 'chat.lastPage')}</p>
      </div>}
      {allowed && read.selection && <ExchangeMessages scope={read.selection} read={read.messages} session={session} allowed={allowed} close={() => {
        session.chatHistory.close();
        focusOutsideDialog(trigger.current ?? document.getElementById('chat-heading'), { preventScroll: true });
      }} />}
    </div>
    {chatContent && read.exchanges.result?.status === 'failed' && <RawFacts summaryKey="receipt.details" value={read.exchanges.result} />}
  </section>;
});
