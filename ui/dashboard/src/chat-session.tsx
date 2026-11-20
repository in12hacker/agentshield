import { useI18n } from './i18n';
import { byteText } from './global/byte-text';
import { sameJsonValue } from './global/json-value';
import { RawFacts } from './global/raw-facts';
import { ProcessKeyFields } from './process-key-fields';
import type { ReviewSession } from './session';
import { chatData, type ChatState } from './chat-state';
import type { ChatSession } from './chat-wire';
import type { SessionValue } from './session-wire';

function SessionValueLabel({ value }: { value: SessionValue }) {
  if (value.kind === 'header') {
    const text = byteText(value.value);
    return <bdi>{text.kind === 'bytes' ? text.value : JSON.stringify(text.value)}</bdi>;
  }
  return <bdi>{value.value.kind === 'json_string' ? JSON.stringify(value.value.value) : value.value.value}</bdi>;
}

function SessionLabel({ item }: { item: ChatSession }) {
  const { t } = useI18n();
  if (item.kind === 'native_gateway') return <>
    <strong className="chat-session-value"><bdi>{JSON.stringify(item.session.session_id)}</bdi></strong>
    <span>{t('chat.nativeGateway')}</span><span><bdi>{JSON.stringify(item.session.session_key)}</bdi></span>
    <span>{t('chat.observationCount', { count: item.chat_observations })}</span>
  </>;
  const { field } = item.session;
  return <>
    <strong className="chat-session-value">{item.session.values.map((value, index) => <SessionValueLabel key={index} value={value} />)}</strong>
    <span>{t(field.kind === 'header' ? 'chat.sessionHeader' : 'chat.sessionPointer', {
      field: field.kind === 'header' ? field.name : field.pointer,
    })}</span>
    <span>{t('chat.sessionCount', { count: item.exchanges })}</span>
    <span><bdi>{item.source.origin}</bdi></span>
  </>;
}

export function SessionDirectory({ read, allowed, session }: { read: ChatState; allowed: boolean; session: ReviewSession }) {
  const { t } = useI18n();
  const data = chatData(read.sessions);
  const busy = read.sessions.phase === 'loading';
  const failure = read.sessions.result?.status === 'failed' ? read.sessions.result : null;
  return <section className="chat-session-directory stack-tight" aria-label={t('chat.sessions')}>
    <div className="panel-heading">
      <button type="button" disabled={!allowed} aria-pressed={read.sessionSelection === null && read.selection?.kind !== 'native_gateway'}
         onClick={() => session.chatHistory.selectSession(null)}>{t('chat.allExchanges')}</button>
      <button type="button" disabled={!allowed} aria-pressed={read.selection?.kind === 'native_gateway' && read.selection.session === null}
        onClick={() => session.chatHistory.selectNative()}>{t('chat.unassociatedNative')}</button>
      {read.sessionSelection && <p role="status" className="overview-note">{t('chat.sessionFiltered')}</p>}
    </div>
    <details onToggle={event => {
      if (event.target === event.currentTarget && event.currentTarget.open && read.sessions.phase === 'initial' && allowed)
        session.chatHistory.refreshSessions();
    }}>
      <summary>{t('chat.sessions')}</summary>
      {!allowed && <p className="amber">{t('chat.noGrant')}</p>}
      {busy && <p role="status">{t('chat.loading')}</p>}
      {read.sessions.problem && <p className="error" role="alert">{t(read.sessions.problem.key, read.sessions.problem.params)}</p>}
      {failure && <p className="error" role="alert">{t('chat.readFailed')}{' · '}<code>{failure.code}</code>{' · '}{failure.message}</p>}
      <div className="process-controls">
        <button type="button" disabled={!allowed || busy} onClick={() => session.chatHistory.refreshSessions()}>{t('common.refresh')}</button>
        <button type="button" disabled={!allowed || busy || data?.type !== 'chat_sessions' || !data.next_before}
          onClick={session.chatHistory.nextSessions}>{t('processes.nextPage')}</button>
      </div>
      {allowed && data?.type === 'chat_sessions' && <>
        {data.sessions.length === 0 && <p className="muted">{t('chat.noSessions')}</p>}
        <ul className="process-list">{data.sessions.map(item => <li key={JSON.stringify(item.reference)}>
          <button type="button" className="process-row" disabled={item.kind === 'model_http' && item.session.issue !== null}
            aria-pressed={sameJsonValue(read.sessionSelection, item.reference)}
            onClick={() => session.chatHistory.selectSession(item)}><SessionLabel item={item} /></button>
          <details><summary>{t('chat.sessionSource')}</summary>
            {item.kind === 'native_gateway' ? <ProcessKeyFields identity={item.namespace.server} /> : <>
              {item.source.actor.process ? <ProcessKeyFields identity={item.source.actor.process} /> : <p>{t('chat.sessionSourceMissing')}</p>}
              <dl className="process-fields"><div><dt>{t('chat.sessionRoute')}</dt><dd><bdi>{item.source.route_label}</bdi></dd></div></dl></>}
            <RawFacts value={item.source} summaryKey="chat.originalSource" />
            {item.kind === 'native_gateway' && <RawFacts value={item} summaryKey="chat.originalMessage" />}
          </details>
          {item.kind === 'model_http' && item.session.issue && <p className="amber">{t('chat.sessionUnavailable')}</p>}
        </li>)}</ul>
      </>}
    </details>
  </section>;
}
