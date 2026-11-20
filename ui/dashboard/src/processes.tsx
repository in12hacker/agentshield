import { ScrollPane } from './global/scroll-pane';
import { memo, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { ReviewSession } from './session';
import { processDetails, type ProcessesState } from './processes-state';
import { lifecycles, processPageSizes, type GetQuery, type ListQuery, type Lifecycle, type ProcessPage,
  type ProcessRecord, type TreeStatus } from './processes-wire';
import { containerIdentity, groupProcesses, observationState, observedValue, processIdentity,
  type ObservationState, type ObservedField } from './processes-model';
import type { ContainerInstance, Workload } from './workload-wire';
import { idLabel } from './wire';
import { useI18n } from './i18n';
import type { MessageKey } from './messages';
import { RawFacts } from './global/raw-facts';
import { DetailField } from './global/detail-field';
import { byteText } from './global/byte-text';
import { ProcessKeyFields } from './process-key-fields';
import { focusOutsideDialog } from './global/focus';

const lifecycleLabels = { present: 'processes.present', exit_observed: 'processes.exitObserved',
  no_longer_present: 'processes.noLongerPresent' } satisfies Record<Lifecycle, MessageKey>;
const phaseLabels = { building: 'processes.building', ready: 'processes.ready', uncertain: 'processes.uncertain',
  closed: 'processes.closed' } satisfies Record<TreeStatus['phase'], MessageKey>;
const phaseNotes = { building: 'processes.buildingNote', ready: 'processes.readyNote', uncertain: 'processes.uncertainNote',
  closed: 'processes.closedNote' } satisfies Record<TreeStatus['phase'], MessageKey>;
const observationLabels = { known: 'processes.known', missing: 'processes.missing', inaccessible: 'processes.inaccessible',
  truncated: 'processes.truncated', stale: 'processes.stale', old_epoch: 'processes.oldEpoch', no_stamp: 'processes.noStamp',
  no_value: 'processes.noValue' } satisfies Record<ObservationState, MessageKey>;
const originLabels = { startup_scan: 'processes.startupScan', refresh: 'processes.sourceRefresh', driver_event: 'processes.driverEvent' } as const;

function OriginalString({ value }: { value: string | null }) {
  const { t } = useI18n();
  return value === null ? <>{t('common.null')}</> : value === '' ? <>{t('processes.emptyValue')}</> : <code>{value}</code>;
}
function ByteValue({ value }: { value: readonly number[] }) {
  const { t } = useI18n();
  const text = byteText(value);
  return text.kind === 'empty' ? <>{t('processes.emptyValue')}</> : <span className="process-byte-value">
    {text.kind === 'bytes' && <span className="muted">{t('processes.byteValue')} </span>}<code>{text.value}</code>
  </span>;
}
function ContainerIdentity({ instance }: { instance: ContainerInstance }) {
  return <dl className="process-fields process-container-identity">
    <DetailField labelKey="processes.runtime"><OriginalString value={instance.runtime} /></DetailField>
    <DetailField labelKey="processes.namespace"><OriginalString value={instance.namespace} /></DetailField>
    <DetailField labelKey="processes.containerId"><OriginalString value={instance.id} /></DetailField>
  </dl>;
}
function ProcessName({ record, epoch }: { record: ProcessRecord; epoch: string }) {
  const { t } = useI18n();
  const name = observedValue(record, 'name', epoch);
  return name === null ? <>{t('processes.instance')}</> : <ByteValue value={name} />;
}
function Observation({ record, epoch, field, labelKey }: {
  record: ProcessRecord; epoch: string; field: ObservedField; labelKey: MessageKey;
}) {
  const { t } = useI18n();
  const state = observationState(record, field, epoch);
  const stamp = record.latest.stamps[field];
  const workload = field === 'workload' ? observedValue(record, 'workload', epoch) : null;
  const text = field !== 'workload' ? observedValue(record, field, epoch) : null;
  return <section className="process-observation stack-tight">
    <h4>{t(labelKey)}</h4>
    <p className={state === 'known' ? 'muted' : 'amber'}>{t(observationLabels[state])}</p>
    {text !== null && <ByteValue value={text} />}
    {workload?.kind === 'host' && <p>{t('processes.hostMembership')}</p>}
    {workload?.kind === 'container' && <ContainerIdentity instance={workload.instance} />}
    {stamp && <p className="overview-note">{t('processes.observationSource', {
      source: t(originLabels[stamp.origin.kind]), epoch: stamp.evidence_epoch,
      begin: stamp.window.begin_boottime_ns, end: stamp.window.end_boottime_ns,
    })}</p>}
  </section>;
}
function Coverage({ page }: { page: ProcessPage }) {
  const { t } = useI18n();
  const status = page.status;
  return <section className="process-coverage stack-tight" aria-label={t('processes.coverage')}>
    <div className="panel-heading"><h3>{t('processes.coverage')}</h3>
      <strong className={status.phase === 'ready' ? '' : 'amber'}>{t(phaseLabels[status.phase])}</strong></div>
    <p className="overview-note">{t(phaseNotes[status.phase])}</p>
    {status.historical_gap && <p className="amber">{t('processes.historicalGap')}</p>}
    <details><summary>{t('processes.coverageDetails')}</summary><dl className="process-fields">
      <DetailField labelKey="processes.treeInstance"><code>{idLabel(status.instance)}</code></DetailField>
      <DetailField labelKey="processes.generation">{status.generation}</DetailField>
      <DetailField labelKey="processes.evidenceEpoch">{status.evidence_epoch}</DetailField>
      <DetailField labelKey="processes.pageRevision">{page.tree_revision}</DetailField>
      <DetailField labelKey="processes.statusRevision">{status.revision}</DetailField>
      <DetailField labelKey="processes.nodes">{String(status.nodes)}</DetailField>
      <DetailField labelKey="processes.retired">{String(status.retired)}</DetailField>
      <DetailField labelKey="processes.memoryBytes">{String(status.memory_used)}</DetailField>
      <DetailField labelKey="processes.trackingEpoch">{status.tracking_epoch}</DetailField>
      <DetailField labelKey="processes.marker">{status.marker}</DetailField>
      <DetailField labelKey="processes.lossEpoch">{status.loss_epoch}</DetailField>
      <DetailField labelKey="processes.backendRejected">{status.backend_rejected}</DetailField>
    </dl></details>
  </section>;
}
function ProcessDetail({ record, epoch, source, close }: { record: ProcessRecord; epoch: string; source: 'list' | 'get'; close: () => void }) {
  const { t } = useI18n();
  const title = useRef<HTMLHeadingElement>(null);
  useLayoutEffect(() => {
    if (source === 'list') focusOutsideDialog(title.current);
  }, [record, source]);
  return <ScrollPane as="aside" id="process-detail" className="process-detail stack-tight" labelledBy="process-detail-title">
    <div className="panel-heading"><h3 id="process-detail-title" ref={title} tabIndex={-1}>{t('processes.detail')}</h3>
      <button type="button" onClick={close}>{t('processes.returnToList')}</button></div>
    <p className="overview-note">{t(source === 'get' ? 'processes.getDetailNote' : 'processes.listDetailNote')}</p>
    <ProcessKeyFields identity={record.key} />
    <dl className="process-fields">
      <DetailField labelKey="processes.lifecycle">{t(lifecycleLabels[record.lifecycle])}</DetailField>
      <DetailField labelKey="processes.recordRevision">{record.revision}</DetailField>
      <DetailField labelKey="processes.firstObserved">{t('processes.sampleWindow', {
        begin: record.first_observed.begin_boottime_ns, end: record.first_observed.end_boottime_ns,
      })}</DetailField>
      <DetailField labelKey="processes.exitWindow">{record.exit_observed ? t('processes.sampleWindow', {
        begin: record.exit_observed.begin_boottime_ns, end: record.exit_observed.end_boottime_ns,
      }) : t('common.null')}</DetailField>
      <DetailField labelKey="processes.exitCode">{record.exit_code === null ? t('common.null') : record.exit_code}</DetailField>
    </dl>
    <Observation record={record} epoch={epoch} field="name" labelKey="processes.name" />
    <Observation record={record} epoch={epoch} field="executable" labelKey="processes.executable" />
    <Observation record={record} epoch={epoch} field="workload" labelKey="processes.membership" />
    {record.creator && <section className="stack-tight"><h4>{t('processes.creator')}</h4><ProcessKeyFields identity={record.creator} /></section>}
    <RawFacts summaryKey="processes.initialRecord" value={record.initial} />
    <RawFacts summaryKey="processes.originalRecord" value={record} />
  </ScrollPane>;
}
function ProcessRows({ records, page, selected, select }: {
  records: readonly ProcessRecord[]; page: ProcessPage; selected: string | null;
  select: (record: ProcessRecord, trigger: HTMLButtonElement) => void;
}) {
  const { t } = useI18n();
  const epoch = page.status.evidence_epoch;
  return <ul className="process-list">{records.map(record => {
    const identity = processIdentity(record.key);
    const executable = observedValue(record, 'executable', epoch);
    return <li key={identity}><button type="button" className="process-row" aria-pressed={selected === identity}
      aria-controls="process-detail" onClick={event => select(record, event.currentTarget)}>
      <strong><ProcessName record={record} epoch={epoch} /></strong>
      <span className="process-row-meta"><span>{t('processes.pidValue', { pid: record.key.host_tgid })}</span>
        <span>{t(lifecycleLabels[record.lifecycle])}</span><span>{t('processes.birth')} <code>{record.key.birth_boottime_ns}</code></span></span>
      {executable !== null && <span><ByteValue value={executable} /></span>}
      {observationState(record, 'workload', epoch) !== 'known' && <span className="amber">
        {t('processes.membershipUnknown')} · {t(observationLabels[observationState(record, 'workload', epoch)])}
      </span>}
    </button></li>;
  })}</ul>;
}
const views = [{ value: 'processes', key: 'processes.processTab' }, { value: 'containers', key: 'processes.containerTab' }] as const satisfies readonly { value: string; key: MessageKey }[];

function ProcessListView({ read, query, session }: {
  read: ProcessesState; query: ListQuery; session: ReviewSession;
}) {
  const { t } = useI18n();
  const [view, setView] = useState<typeof views[number]['value']>('processes');
  const [selection, setSelection] = useState<{ request: readonly number[]; key: string } | null>(null);
  const trigger = useRef<HTMLButtonElement | null>(null);
  const page = read.page;
  useLayoutEffect(() => { setSelection(null); trigger.current = null; }, [page]);
  const groups = useMemo(() => page ? groupProcesses(page) : null, [page]);
  const selectedKey = selection?.request === read.receipt?.id ? selection?.key ?? null : null;
  const selected = page?.records.find(record => processIdentity(record.key) === selectedKey);
  const busy = read.phase === 'loading';
  const filter = query.filter;
  const failure = read.receipt?.result.status === 'failed' ? read.receipt.result : null;
  const select = (record: ProcessRecord, element: HTMLButtonElement) => {
    if (!read.receipt) return;
    trigger.current = element;
    setSelection({ request: read.receipt.id, key: processIdentity(record.key) });
  };
  const exactWorkload = (workload: Workload) => session.refreshProcesses(query.page.limit, filter.lifecycle, workload);
  const rows = (records: readonly ProcessRecord[]) => page && <ProcessRows records={records} page={page}
    selected={selectedKey} select={select} />;
  return <section id="protection-targets" className="process-workspace stack-tight" aria-labelledby="processes-title">
    <div className="panel-heading"><p className="muted">{t('processes.subtitle')}</p>
      <button type="button" disabled={busy} onClick={() => session.refreshProcesses()}>
        {t(read.phase === 'initial' ? 'processes.load' : 'processes.firstPage')}
       </button></div>
    <div className="process-view-switch" aria-label={t('processes.views')}>{views.map(item =>
      <button type="button" key={item.value} aria-pressed={view === item.value}
        onClick={() => {
          setView(item.value); setSelection(null); trigger.current = null;
        }}>{t(item.key)}</button>)}</div>
      <div className="process-controls">
        <label>{t('processes.lifecycle')}<select value={filter.lifecycle ?? ''} disabled={busy} onChange={event =>
          session.refreshProcesses(query.page.limit, event.target.value === '' ? null : event.target.value as Lifecycle)}>
          <option value="">{t('processes.allLifecycles')}</option>{lifecycles.map(value =>
            <option key={value} value={value}>{t(lifecycleLabels[value])}</option>)}
        </select></label>
        <label>{t('processes.pageSize')}<select value={query.page.limit} disabled={busy} onChange={event => session.refreshProcesses(Number(event.target.value))}>
          {processPageSizes.map(size => <option key={size} value={size}>{size}</option>)}
        </select></label>
      </div>
      {filter.workload && <section className="process-filter stack-tight"><div className="panel-heading">
        <h3>{t('processes.exactFilter')}</h3><button type="button" disabled={busy} onClick={() => session.refreshProcesses(query.page.limit, filter.lifecycle, null)}>
          {t('processes.clearFilter')}</button></div>
        {filter.workload.kind === 'host' ? <p>{t('processes.hostFilter')}</p> : <ContainerIdentity instance={filter.workload.instance} />}
        <p className="overview-note">{t('processes.rawFilterNote')}</p>
      </section>}
      {read.phase === 'initial' && <p className="process-empty">{t('processes.notRead')}</p>}
      {busy && <p role="status">{t(read.receipt?.result.status === 'accepted' ? 'receipt.acceptedNote' : 'processes.loading')}</p>}
      {read.problem && <div role="alert" className="receipt-failure stack-tight">
        <p>{t(read.problem.key, read.problem.params)}</p>
        {failure && <><p>{t('common.failureCode')}<code>{failure.code}</code></p>
          <p>{t('common.originalMessage')}<code>{JSON.stringify(failure.message)}</code></p></>}
        {read.diagnostic && <p><code>{read.diagnostic.code}</code></p>}
        {failure?.code === 'cursor_expired' && <p>{t('processes.cursorExpired')}</p>}
      </div>}
      {page && groups && <>
        <Coverage page={page} />
        <p role="status" className="overview-note">{t('processes.pageCount', { page: read.pageNumber, count: page.records.length })}</p>
        {read.changed && <p className="amber">{t('processes.pageChanged')}</p>}
        {page.records.length === 0 && <p className="process-empty">{t('processes.emptyPage')}</p>}
        <div className={`process-layout${selected ? ' has-detail' : ''}`}><ScrollPane className="process-groups stack-tight" labelledBy="processes-title">
          {view === 'containers' && <p className="overview-note">{t('processes.containerScope')}</p>}
          {view === 'containers' && groups.containers.length === 0 && <p className="process-empty">{t('processes.noContainers')}</p>}
          {view === 'processes' && groups.host.length > 0 && <section className="process-group">
            <div className="panel-heading"><h3>{t('processes.hostGroup')}</h3>
              <button type="button" onClick={() => exactWorkload({ kind: 'host' })}>{t('processes.filterInstance')}</button></div>{rows(groups.host)}
          </section>}
          {groups.containers.map(group => <section className="process-group" key={containerIdentity(group.instance)}>
            <div className="panel-heading"><h3>{t('processes.containerGroup')}</h3>
              <button type="button" onClick={() => exactWorkload({ kind: 'container', instance: group.instance })}>{t('processes.filterInstance')}</button></div>
            <ContainerIdentity instance={group.instance} />
            <p className="overview-note">{t('processes.onPageCount', { count: group.records.length })}</p>{rows(group.records)}
          </section>)}
          {view === 'processes' && groups.unknown.length > 0 && <section className="process-group">
            <h3>{t('processes.unknownGroup')}</h3>{rows(groups.unknown)}
          </section>}
          {view === 'containers' && <p className="overview-note">{t('processes.unknownCount', { count: groups.unknown.length })}</p>}
        </ScrollPane>{selected && <ProcessDetail record={selected} epoch={page.status.evidence_epoch} source="list" close={() => {
          setSelection(null);
          focusOutsideDialog(trigger.current?.isConnected ? trigger.current : document.getElementById('processes-title'));
          trigger.current = null;
        }} />}</div>
        <div className="process-pagination"><p className="overview-note">{t(page.next_cursor ? 'processes.morePages' : 'processes.lastPage')}</p>
          <button type="button" disabled={!page.next_cursor || busy} onClick={session.nextProcesses}>{t('processes.nextPage')}</button></div>
      </>}
    {read.receipt && <RawFacts key={idLabel(read.receipt.id)} summaryKey="processes.originalReply" value={read.receipt} />}
    {read.diagnostic && <RawFacts summaryKey="processes.originalReply" value={read.diagnostic} />}
  </section>;
}

function ExactProcessView({ read, query, session }: { read: ProcessesState; query: GetQuery; session: ReviewSession }) {
  const { t } = useI18n();
  const details = processDetails(read);
  const failure = read.receipt?.result.status === 'failed' ? read.receipt.result : null;
  const busy = read.phase === 'loading';
  const showList = () => {
    session.refreshProcesses();
    focusOutsideDialog(document.getElementById('processes-title'));
  };
  return <section id="protection-targets" className="process-workspace stack-tight" aria-labelledby="processes-title">
    <div className="panel-heading"><h2>{t('processes.getTitle')}</h2><div className="actions">
      <button type="button" disabled={busy} onClick={session.reloadProcess}>{t('processes.rereadInstance')}</button>
      <button type="button" onClick={showList}>{t('processes.returnToList')}</button>
    </div></div>
    <p className="overview-note">{t('processes.getNote')}</p>
    <ProcessKeyFields identity={query.key} />
    {busy && <p role="status">{t('processes.loading')}</p>}
    {read.problem && <div className="receipt-failure stack-tight" role="alert">
      <p>{t(read.problem.key, read.problem.params)}</p>
      {failure && <><p>{t('common.failureCode')}<code>{failure.code}</code></p>
        <p>{t('common.originalMessage')}<code>{JSON.stringify(failure.message)}</code></p></>}
      {failure?.code === 'not_found' && <p>{t('processes.instanceNotFound')}</p>}
      {read.diagnostic && <p><code>{read.diagnostic.code}</code></p>}
    </div>}
    {details && <>
      <ProcessDetail record={details.record} epoch={details.evidence_epoch} source="get" close={showList} />
      <RawFacts summaryKey="processes.lookupDetails" value={details} />
    </>}
    {read.receipt && <RawFacts key={idLabel(read.receipt.id)} summaryKey="processes.originalReply" value={read.receipt} />}
    {read.diagnostic && <RawFacts summaryKey="processes.originalReply" value={read.diagnostic} />}
  </section>;
}

export const ProcessesView = memo(function ProcessesView({ read, session }: { read: ProcessesState; session: ReviewSession }) {
  return read.query.type === 'get' ? <ExactProcessView read={read} query={read.query} session={session} />
    : <ProcessListView read={read} query={read.query} session={session} />;
});
