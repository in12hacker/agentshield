import { useEffect, useState } from 'react';
import type { AttributionRequest, ExchangeKey, ToolAttribution } from './context-wire';
import type { OriginalEvent } from './event-wire';
import { attributionInput } from './attribution-input';
import { useI18n } from './i18n';
import { RawFacts } from './global/raw-facts';
import { ProcessKeyFields } from './process-key-fields';

function ArgumentField({ name, label }: { name: string; label: string }) {
  const { t } = useI18n();
  const [mode, setMode] = useState('');
  return <fieldset className="stack-tight"><legend>{label}</legend>
    <label>{t('attribution.valueSelection')}<select name={`${name}_mode`} required value={mode} onChange={e => setMode(e.target.value)}>
      <option value="" disabled>{t('rules.choose')}</option>
      <option value="whole">{t('attribution.wholeValue')}</option><option value="pointer">{t('attribution.pointerValue')}</option>
    </select></label>
    {mode === 'pointer' && <label>{t('attribution.pointer')}<input name={name} required placeholder={t('attribution.pointerExample')} /></label>}
  </fieldset>;
}

export function AttributionPanel({ event, query, result, loading, completed, allowed, onRead, onOpenExchange }: {
  event: OriginalEvent; query: AttributionRequest | null; result: ToolAttribution | null; loading: boolean; completed: boolean; allowed: boolean;
  onRead: (query: AttributionRequest) => void; onOpenExchange: (exchange: ExchangeKey) => boolean;
}) {
  const { t } = useI18n();
  const [target, setTarget] = useState('');
  const [invalidInput, setInvalidInput] = useState(false);
  const [openFailed, setOpenFailed] = useState(false);
  useEffect(() => setOpenFailed(false), [result]);
  const kind = event.header.kind;
  const applicable = kind === 'file' || kind === 'network' || (kind === 'process' && event.header.operation === 1);
  const actualTarget = kind === 'file' ? 'file_path' : kind === 'network' ? 'network_peer' : target;
  const comparisonLabels = { equal: 'attribution.equal', different: 'attribution.different',
    event_target_unavailable: 'attribution.eventTargetUnavailable', argument_unavailable: 'attribution.argumentUnavailable',
    source_incomplete: 'attribution.sourceIncomplete' } as const;
  return <section className="attribution-panel stack-tight" aria-label={t('attribution.title')}>
    <details>
      <summary>{t('attribution.title')}</summary>
      <p className="overview-note">{t('attribution.notice')}</p>
      {!applicable ? <p>{t('attribution.notApplicable')}</p> : <form className="stack-tight" onSubmit={e => {
        e.preventDefault();
        if (!allowed || loading) return;
        let value: AttributionRequest;
        try { value = attributionInput(new FormData(e.currentTarget)); }
        catch { setInvalidInput(true); return; }
        setInvalidInput(false); onRead(value);
      }}>
        <fieldset disabled={!allowed || loading} className="stack-tight">
          <legend>{t('attribution.conditions')}</legend>
          <label>{t('attribution.actor')}<select name="actor" required defaultValue="">
            <option value="" disabled>{t('rules.choose')}</option>
            <option value="same_process">{t('attribution.sameProcess')}</option>
            <option value="physical_parent">{t('attribution.physicalParent')}</option>
            <option value="same_thread">{t('attribution.sameThread')}</option>
          </select></label>
          <label>{t('attribution.lookback')}<input name="lookback_seconds" inputMode="decimal" required /></label>
          <p className="muted">{t('attribution.lookbackHelp')}</p>
          {kind === 'process' ? <label>{t('attribution.target')}<select name="target" required value={target} onChange={e => setTarget(e.target.value)}>
            <option value="" disabled>{t('rules.choose')}</option>
            <option value="executable">{t('attribution.executable')}</option><option value="exec_argument">{t('attribution.execArgument')}</option>
          </select></label> : <><input name="target" type="hidden" value={actualTarget} />
            <p>{t(kind === 'file' ? 'attribution.filePath' : 'attribution.networkPeer')}</p></>}
          {actualTarget === 'exec_argument' && <label>{t('attribution.argumentIndex')}<input name="argument_index" inputMode="numeric" required /></label>}
          {(actualTarget === 'file_path' || actualTarget === 'executable' || actualTarget === 'exec_argument')
            && <ArgumentField key={actualTarget} name="value_pointer" label={t('attribution.valueField')} />}
          {actualTarget === 'network_peer' && <><ArgumentField name="address_pointer" label={t('attribution.addressField')} />
            <ArgumentField name="port_pointer" label={t('attribution.portField')} /></>}
          <label>{t('chat.pageSize')}<select name="limit" defaultValue="8">{[8, 16].map(limit => <option key={limit} value={limit}>{limit}</option>)}</select></label>
          <button type="submit">{t('attribution.compare')}</button>
        </fieldset>
        {invalidInput && <p className="error" role="alert">{t('attribution.invalidInput')}</p>}
      </form>}
    </details>
    {query && allowed && <RawFacts summaryKey="attribution.originalQuery" value={query} />}
    {query && completed && !result && <p className="amber">{t('attribution.noComparisonSource')}</p>}
    {result && <div className="stack-tight">
      <h4>{t('attribution.results')}</h4><p className="amber">{t('attribution.candidateNotice')}</p>
      <ProcessKeyFields identity={result.required_connection_process} />
      <RawFacts summaryKey="attribution.originalResult" value={result} />
      {result.candidates.length === 0 ? <p>{t('attribution.noCandidates')}</p> : <ul className="stack-tight">
        {result.candidates.map((candidate, index) => <li key={index} className="attribution-candidate stack-tight">
          <p>{t(comparisonLabels[candidate.target_comparison])}</p>
          <p>{t('attribution.observedTime')} <code>{candidate.response_observed_ns}</code></p>
          <RawFacts summaryKey="attribution.originalCandidate" value={candidate} />
          <a href="#chat-history" onClick={e => { if (!allowed || !onOpenExchange(candidate.call.message.exchange)) { e.preventDefault(); setOpenFailed(true); } }}>
            {t('attribution.openExchange')}</a>
        </li>)}
      </ul>}
      {query && result.next_after && <button type="button" disabled={!allowed || loading}
        onClick={() => onRead({ ...query, after: result.next_after })}>{t('processes.nextPage')}</button>}
      {openFailed && <p className="error" role="status">{t('attribution.openFailed')}</p>}
    </div>}
  </section>;
}
