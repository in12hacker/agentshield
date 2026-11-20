import { memo, useLayoutEffect, useRef, useState } from 'react';
import type { ReviewSession } from './session';
import { groupDenied, groupPage, type GroupState } from './group-state';
import type { GroupDelegation, GroupPage } from './group-wire';
import type { ProcessKey } from './processes-wire';
import { processIdentity } from './processes-model';
import { ProcessKeyFields } from './process-key-fields';
import { RawFacts } from './global/raw-facts';
import { chatRoute, navigateDashboard, processesRoute } from './global/navigation';
import { useI18n } from './i18n';
import { focusOutsideDialog } from './global/focus';

const gapKeys = {
  original_task_unavailable: 'group.gapTask', task_terminal_unavailable: 'group.gapTerminal',
  trace_handoff_unavailable: 'group.gapHandoff', parent_tool_facts_unavailable: 'group.gapParentTool',
  child_model_exchange_unavailable: 'group.gapChildModel', child_terminal_unobserved: 'group.gapChildTerminal',
  parent_publication_unobserved: 'group.gapParentDelivery', parent_publication_unverified: 'group.gapPublicationUnverified',
  gateway_receipt_unobserved: 'group.gapGatewayReceipt', gateway_receipt_conflict: 'group.gapGatewayConflict',
  child_terminal_conflict: 'group.gapChildConflict', parent_publication_conflict: 'group.gapPublicationConflict',
} as const;
function Gaps({ row }: { row: GroupDelegation }) {
  const { t } = useI18n();
  return row.kind === 'a2a' ? <p className={row.delegation.gap ? 'amber' : 'muted'}>
    {t('group.stageGap')} {row.delegation.gap ? <>{t(gapKeys[row.delegation.gap])} <code>{row.delegation.gap}</code></> : t('group.noStageGap')}
  </p> : <div className={row.gaps.length ? 'amber' : 'muted'}><p>{t('group.publicEvidence', {
    receipts: row.gateway.accepted_spawns.length, terminals: row.gateway.child_terminals.length,
    publications: row.gateway.parent_publications.length,
  })}</p>{row.gaps.length === 0 && <p>{t('group.noStageGap')}</p>}<ul>
    {row.gaps.map(gap => <li key={gap}>{t(gapKeys[gap])} <code>{gap}</code></li>)}
  </ul></div>;
}
function ReceiptExchange({ row, session }: { row: GroupDelegation; session: ReviewSession }) {
  const { t } = useI18n();
  return row.kind === 'openclaw_tool_receipt' ? <div className="stack-tight">
    <button type="button" onClick={() => {
      if (session.openGroupReceipt(row)) navigateDashboard(chatRoute);
    }}>{t('group.openReceiptExchange')}</button>
    <p className="overview-note">{t('group.receiptExchangeScope')}</p>
  </div> : null;
}
function ProcessNode({ identity, select }: { identity: ProcessKey; select: (key: ProcessKey, trigger: HTMLButtonElement) => void }) {
  const { t } = useI18n();
  return <button type="button" className="group-node stack-tight" aria-controls="group-detail"
    onClick={event => select(identity, event.currentTarget)}>
    <strong>{t('group.observedProcess')}</strong><ProcessKeyFields identity={identity} />
  </button>;
}
function GroupDetail({ identity, rows, close, session }: { identity: ProcessKey; rows: readonly GroupDelegation[]; close: () => void; session: ReviewSession }) {
  const { t } = useI18n();
  const heading = useRef<HTMLHeadingElement>(null);
  useLayoutEffect(() => { focusOutsideDialog(heading.current); }, [identity]);
  return <aside id="group-detail" className="process-detail stack-tight" aria-labelledby="group-detail-title">
    <div className="panel-heading"><h3 id="group-detail-title" ref={heading} tabIndex={-1}>{t('processes.detail')}</h3>
      <button type="button" onClick={close}>{t('processes.returnToList')}</button></div>
    <p className="muted">{t('group.unidentified')}</p><ProcessKeyFields identity={identity} />
    <button type="button" onClick={() => { session.openProcess(identity); navigateDashboard(processesRoute); }}>{t('group.openProcess')}</button>
    {rows.map(row => <section key={row.id} className="group-reference stack-tight">
      <h4>{t('group.record', { id: row.id })}</h4><Gaps row={row} />
      <ReceiptExchange row={row} session={session} />
      <RawFacts summaryKey="group.original" value={row} />
    </section>)}
  </aside>;
}
export const GroupDelegationsView = memo(function GroupDelegationsView({ read, chatContent, session }: {
  read: GroupState; chatContent: boolean; session: ReviewSession;
}) {
  const { t } = useI18n();
  useLayoutEffect(() => { session.enterGroupDelegations(); return session.leaveGroupDelegations; }, [session]);
  const page = groupPage(read);
  const [selection, setSelection] = useState<{ page: GroupPage; key: ProcessKey } | null>(null);
  const trigger = useRef<HTMLButtonElement | null>(null);
  useLayoutEffect(() => { setSelection(null); trigger.current = null; }, [page]);
  const selected = selection?.page === page ? selection.key : null;
  const busy = read.phase === 'loading';
  const allowed = chatContent && !groupDenied(read);
  const failure = read.reply?.result.status === 'failed' ? read.reply.result : null;
  const select = (key: ProcessKey, element: HTMLButtonElement) => {
    if (!page) return;
    trigger.current = element; setSelection({ page, key });
  };
  const related = selected ? page?.records.filter(row => (row.kind === 'a2a' ? [row.parent, row.child] : [row.actor])
    .some(key => processIdentity(key) === processIdentity(selected))) ?? [] : [];
  return <section id="group" className="group-workspace stack-tight" aria-labelledby="group-heading">
    <div className="panel-heading"><p className="overview-note">{t('group.scope')}</p>
      <button type="button" disabled={!allowed || busy} onClick={session.refreshGroupDelegations}>
        {t(read.phase === 'initial' ? 'processes.load' : 'processes.firstPage')}</button></div>
    <p className="overview-note">{t('group.coverage')}</p>
    {!allowed && <p role="status" className="amber">{t('group.noGrant')}</p>}
    {allowed && read.phase === 'initial' && <p>{t('group.initial')}</p>}
    {busy && <p role="status">{t('group.loading')}</p>}
    {read.problem && <div role="alert" className="receipt-failure stack-tight">
      <p>{t(read.problem.key, read.problem.params)}</p>
      {failure && <><p>{t('common.failureCode')}<code>{failure.code}</code></p>
        <p>{t('common.originalMessage')}<code>{JSON.stringify(failure.message)}</code></p></>}
    </div>}
    {allowed && page && <>
      <p role="status" className="overview-note">{t('group.pageCount', { page: read.pageNumber, records: page.records.length })}</p>
      {page.records.length === 0 && <p className="process-empty">{t('group.empty')}</p>}
      <div className={`process-layout${selected ? ' has-detail' : ''}`}><ul className="process-list">
        {page.records.map(row => <li className="group-call stack-tight" key={row.id}>
          <h3>{t('group.record', { id: row.id })}</h3>
          <div className="group-direction">
            <ProcessNode identity={row.kind === 'a2a' ? row.parent : row.actor} select={select} />
            <span className="group-arrow"><span aria-hidden="true">→</span>{t(row.kind === 'a2a' ? 'group.observedCall' : 'group.acceptanceObserved')}</span>
            {row.kind === 'a2a' ? <ProcessNode identity={row.child} select={select} />
              : <div className="group-candidate stack-tight"><strong>{t('group.childRun')}</strong>
                <code>{row.child_run_id}</code><code>{row.child_session_key}</code></div>}
          </div><Gaps row={row} />
          <ReceiptExchange row={row} session={session} />
          <RawFacts summaryKey="group.original" value={row} />
        </li>)}
      </ul>{selected && <GroupDetail identity={selected} rows={related} session={session} close={() => {
        setSelection(null);
        focusOutsideDialog(trigger.current, { preventScroll: true });
      }} />}</div>
      <div className="process-pagination"><p className="overview-note">{t(page.next_before ? 'group.morePages' : 'group.lastPage')}</p>
        <button type="button" disabled={!page.next_before || busy} onClick={session.nextGroupDelegations}>{t('processes.nextPage')}</button></div>
    </>}
    {chatContent && read.reply && <RawFacts summaryKey="processes.originalReply" value={read.reply} />}
  </section>;
});
