import { useLayoutEffect, useRef, useState } from 'react';
import type { ReviewSession } from './session';
import type { SettingsReceipt, SettingsState } from './settings-state';
import { changedConfigSections, decodeConfigDocument,
  type ConfigDocument, type ConfigSection, type ConfigSnapshot, type RuntimeCommand } from './settings-wire';
import { protectionKinds } from './driver-wire';
import { commandFacts, type OverviewRead } from './reply-wire';
import { driverFacts, featureView, flowFacts, flowView, overviewCommands, overviewView, sourceLabels } from './overview-model';
import { factsJson, parseUnique } from './wire-json';
import { idLabel, type Access } from './wire';
import type { MessageKey } from './messages';
import { useI18n } from './i18n';
import { RawFacts } from './global/raw-facts';
import { DetailField } from './global/detail-field';
import { byteText } from './global/byte-text';
import { OverviewProblem } from './overview';
import { ConfigurationFailure, ConfigurationReadStatus } from './configuration-receipt';
import { focusOutsideDialog } from './global/focus';

const categoryLabels = { process: 'category.process', file: 'category.file', network: 'category.network' } as const;
const sectionLabels = { queues: 'settings.queues', process_tree: 'settings.processTree', driver: 'receipt.driver',
  middleware: 'settings.middleware', flow: 'receipt.flow', credentials: 'settings.bindings',
  file_close_audit: 'settings.fileCloseAudit', group_observation: 'settings.groupObservation', log_storage: 'settings.logStorage' } satisfies Record<ConfigSection, MessageKey>;
const overrideLabels = { default: 'settings.overrideDefault', enabled: 'feature.enabled', disabled: 'feature.disabled' } as const;
function ActionHeading({ id, labelKey }: { id: string; labelKey: MessageKey }) {
  const { t } = useI18n();
  const heading = useRef<HTMLHeadingElement>(null);
  useLayoutEffect(() => { focusOutsideDialog(heading.current); }, [id]);
  return <h3 id={id} ref={heading} tabIndex={-1}>{t(labelKey)}</h3>;
}
function Version({ version, labelKey = 'settings.version' }: { version: readonly number[]; labelKey?: MessageKey }) {
  const { t } = useI18n();
  return <p className="overview-note">{t(labelKey)} <code>{idLabel(version)}</code></p>;
}
function ChangedSections({ sections }: { sections: readonly ConfigSection[] }) {
  const { t } = useI18n();
  return <p>{t('settings.changedSections')} {sections.length
    ? sections.map(section => t(sectionLabels[section])).join(' · ') : t('settings.noChanges')}</p>;
}
function SavedSnapshot({ snapshot }: { snapshot: ConfigSnapshot }) {
  const { t } = useI18n();
  const source = snapshot.paths.configuration_file;
  return <div className="settings-source stack-tight">
    <h3>{t('settings.savedDefaults')}</h3>
    <dl className="settings-fields">{protectionKinds.map(feature => <DetailField key={feature} labelKey={categoryLabels[feature]}>
      {t(snapshot.document.driver.startup_protection[feature] ? 'feature.enabled' : 'feature.disabled')}
    </DetailField>)}<DetailField labelKey="settings.selfLog">{t(snapshot.document.driver.self_protection.log ? 'feature.enabled' : 'feature.disabled')}</DetailField></dl>
    <p className="overview-note">{t('settings.defaultsNote')}</p>
    <p className="overview-note">{t('settings.configurationSource')} <code>{source.encoding === 'unix_bytes'
      ? byteText(source.value).value : source.value.map(unit => String.fromCharCode(unit)).join('')}</code></p>
    <Version version={snapshot.version} />
    <RawFacts summaryKey="settings.savedDocument" value={snapshot.document} />
    <RawFacts summaryKey="settings.paths" value={snapshot.paths} />
  </div>;
}
function AccessAcknowledgement({ checked, change }: { checked: boolean; change: (checked: boolean) => void }) {
  const { t } = useI18n();
  return <label className="settings-check"><input type="checkbox" checked={checked}
    onChange={event => change(event.target.checked)} /><span>{t('settings.accessAcknowledgement')}</span></label>;
}
function ConfigEditor({ snapshot, session, close }: { snapshot: ConfigSnapshot; session: ReviewSession; close: () => void }) {
  const { t } = useI18n();
  const [document, setDocument] = useState(snapshot.document);
  const [advanced, setAdvanced] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ document: ConfigDocument; sections: readonly ConfigSection[] } | null>(null);
  const [invalid, setInvalid] = useState(false);
  const [accessChecked, setAccessChecked] = useState(false);
  const prepare = () => {
    try {
      // Exactly one parse of advanced input; confirmation reuses this validated object.
      const proposed = advanced === null ? document : decodeConfigDocument(parseUnique(advanced, { integerOnly: true }));
      setPreview({ document: proposed, sections: changedConfigSections(snapshot.document, proposed) });
      setInvalid(false); setAccessChecked(false);
    } catch { setInvalid(true); }
  };
  if (preview) {
    const accessChanges = preview.sections.includes('middleware');
    return <section className="settings-confirm stack-tight" aria-labelledby="settings-save-preview">
      <ActionHeading id="settings-save-preview" labelKey="settings.savePreview" />
      <Version version={snapshot.version} />
      <ChangedSections sections={preview.sections} />
      <p className="overview-note">{t('settings.saveNote')}</p>
      {accessChanges && <><p className="amber">{t('settings.accessWarning')}</p>
        <AccessAcknowledgement checked={accessChecked} change={setAccessChecked} /></>}
      <RawFacts summaryKey="settings.proposedDocument" value={preview.document} />
      <div className="actions">
        <button type="button" onClick={() => setPreview(null)}>{t('settings.backToEdit')}</button>
        <button type="button" onClick={close}>{t('settings.cancel')}</button>
        <button type="button" className="primary" disabled={!preview.sections.length || (accessChanges && !accessChecked)}
          onClick={() => session.saveConfiguration(snapshot, preview.document)}>{t('settings.confirmSave')}</button>
      </div>
    </section>;
  }
  return <section className="settings-editor stack-tight" aria-labelledby="settings-editor-title"
    onKeyDown={event => { if (event.key === 'Escape') { event.stopPropagation(); close(); } }}>
    <ActionHeading id="settings-editor-title" labelKey="settings.edit" />
    <Version version={snapshot.version} />
    <fieldset disabled={advanced !== null} className="settings-boolean-fields">
      <legend>{t('settings.savedDefaults')}</legend>
      {protectionKinds.map(feature => <label className="settings-check" key={feature}>
        <input type="checkbox" checked={document.driver.startup_protection[feature]} onChange={event => {
          setDocument({ ...document, driver: { ...document.driver,
            startup_protection: { ...document.driver.startup_protection, [feature]: event.target.checked } } });
        }} /><span>{t(categoryLabels[feature])}</span>
      </label>)}
      <label className="settings-check"><input type="checkbox" checked={document.driver.self_protection.log}
        onChange={event => setDocument({ ...document, driver: { ...document.driver, self_protection: { log: event.target.checked } } })} />
        <span>{t('settings.selfLog')}</span></label>
    </fieldset>
    <p className="overview-note">{t('settings.selfLogNote')}</p>
    {advanced === null ? <button type="button" onClick={() => setAdvanced(factsJson(document))}>{t('settings.advancedEdit')}</button>
      : <div className="stack-tight">
        <label htmlFor="settings-document">{t('settings.completeJson')}</label>
        <p className="overview-note" id="settings-json-help">{t('settings.advancedNote')}</p>
        <textarea id="settings-document" value={advanced} spellCheck={false} autoComplete="off" aria-describedby="settings-json-help"
          onChange={event => { setAdvanced(event.target.value); setInvalid(false); }} />
        <button type="button" onClick={() => { setAdvanced(null); setInvalid(false); }}>{t('settings.discardAdvanced')}</button>
      </div>}
    {invalid && <p className="error" role="alert">{t('settings.invalidDocument')}</p>}
    <div className="actions"><button type="button" onClick={close}>{t('settings.cancel')}</button>
      <button type="button" onClick={prepare}>{t('settings.preview')}</button></div>
  </section>;
}
function ConfigurationActions({ snapshot, session }: { snapshot: ConfigSnapshot; session: ReviewSession }) {
  const { t } = useI18n();
  const [mode, setMode] = useState<'idle' | 'edit' | 'reload'>('idle');
  if (mode === 'edit') return <ConfigEditor snapshot={snapshot} session={session} close={() => setMode('idle')} />;
  if (mode === 'reload') return <section className="settings-confirm stack-tight" aria-labelledby="settings-reload-preview">
    <ActionHeading id="settings-reload-preview" labelKey="settings.reloadPreview" />
    <Version version={snapshot.version} />
    <p>{t('settings.reloadSections')}</p>
    <p className="overview-note">{t('settings.reloadNote')}</p>
    <p className="amber">{t('settings.accessWarning')}</p>
    <div className="actions"><button type="button" onClick={() => setMode('idle')}>{t('settings.cancel')}</button>
      <button type="button" onClick={() => session.reloadConfiguration(snapshot)}>{t('settings.confirmReload')}</button></div>
  </section>;
  return <div className="actions"><button type="button" onClick={() => setMode('edit')}>{t('settings.edit')}</button>
    <button type="button" onClick={() => setMode('reload')}>{t('settings.reload')}</button></div>;
}
function OperationReceipt({ receipt }: { receipt: SettingsReceipt }) {
  const { t } = useI18n();
  const { result, operation } = receipt;
  const facts = receipt.problem || result?.status === 'accepted' ? null : commandFacts(result ?? undefined);
  const saved = facts?.type === 'configuration_saved' ? facts : null;
  const read = facts?.type === 'configuration' || facts?.type === 'configuration_reloaded' ? facts.snapshot : null;
  const applied = saved?.applied ?? (facts?.type === 'configuration_reloaded' ? facts.applied
    : facts?.type === 'driver_status' ? facts.snapshot : null);
  const feature = typeof operation === 'object' ? applied?.[operation.feature] : undefined;
  const controlComplete = typeof operation === 'object' && result?.status === 'completed' && facts?.type === 'driver_status';
  const reloaded = operation === 'reload' && result?.status === 'completed' && read !== null;
  const titleKey: MessageKey = receipt.problem ? 'settings.unconfirmed' : result?.status === 'accepted' ? 'settings.accepted'
    : result?.status === 'failed' ? 'settings.failed' : controlComplete ? 'settings.controlCompleted'
    : saved && result?.status === 'completed' ? 'settings.saved' : reloaded ? 'settings.reloaded' : 'settings.unconfirmed';
  return <section className="settings-receipt stack-tight" aria-labelledby="settings-receipt-title">
    <h3 id="settings-receipt-title">{t('settings.lastOperation')}</h3>
    <p role="status">{result === null && !receipt.problem ? t('settings.writing') : t(titleKey)}</p>
    <p className="overview-note">{t('settings.operation')} <code>{typeof operation === 'string' ? operation : operation.type}</code>
      {typeof operation === 'object' && <> · {t(categoryLabels[operation.feature])} · {t(operation.type === 'resume_runtime_feature'
        ? 'settings.resume' : operation.enabled ? 'settings.enableTemporarily' : 'settings.disableTemporarily')}</>}</p>
    <p className="overview-note">{t('overview.commandId')} <code>{idLabel(receipt.id)}</code></p>
    <ConfigurationFailure result={result} />
    {receipt.problem && <p className="error">{t(receipt.problem.key, receipt.problem.params)}</p>}
    {saved && <>
      {(result?.status === 'failed' || saved.receipt.required_effects.includes('reinitialize_main')
        || (saved.receipt.required_effects.includes('apply_driver') && !saved.applied))
        && <p className="amber">{t('settings.savedPending')}</p>}
      <Version version={saved.receipt.saved_version} labelKey="settings.savedVersion" />
      <ChangedSections sections={saved.receipt.changed_sections} />
      <h3>{t('settings.effects')}</h3>
      {saved.receipt.required_effects.length === 0 ? <p className="overview-note">{t('settings.noEffects')}</p>
        : <ul className="settings-effects">{saved.receipt.required_effects.map((effect, index) => <li key={index}>
          <code>{effect}</code> · {t(effect === 'reinitialize_main' ? 'settings.effectReinitialize'
            : effect === 'reconnect_middleware' ? 'settings.effectReconnect'
            : effect === 'apply_driver' ? saved.applied ? 'settings.driverReceipt' : 'settings.driverUnconfirmed'
            : result?.status === 'completed' ? 'settings.effectCompleted' : 'settings.effectUnconfirmed')}
        </li>)}</ul>}
    </>}
    {read && <>{!reloaded && <p>{t('settings.sourceRead')}</p>}<Version version={read.version} /></>}
    {(applied || saved?.receipt.required_effects.includes('apply_driver')) && <>
      <p className="overview-note">{t(applied ? 'settings.driverReceipt' : 'settings.driverUnconfirmed')}</p>
      {applied && <dl className="settings-fields"><DetailField labelKey="receipt.source">{t(sourceLabels[applied.source_state])}</DetailField>
        <DetailField labelKey="settings.driverVersion"><code>{applied.driver_config_version === null ? t('common.unknown') : idLabel(applied.driver_config_version)}</code></DetailField>
        {feature && <><DetailField labelKey="settings.runtimeOverride">{t(overrideLabels[feature.runtime_override])}</DetailField>
          <DetailField labelKey="settings.effectiveState">{t(featureView(feature.effective_state).label)}</DetailField></>}
      </dl>}
    </>}
    {(saved || read || applied) && <p className="overview-note">{t('settings.versionNote')}</p>}
    {result && <RawFacts summaryKey="receipt.details" value={result} />}
    {receipt.diagnostic && <RawFacts summaryKey="receipt.details" value={receipt.diagnostic} />}
  </section>;
}
function RuntimeDiagnostics({ read, settings, access, session }: {
  read: OverviewRead; settings: SettingsState; access: Access | null; session: ReviewSession;
}) {
  const { t } = useI18n();
  const [confirmation, setConfirmation] = useState<RuntimeCommand | null>(null);
  const driver = driverFacts(read), flow = flowFacts(read), commands = overviewCommands(read);
  const manager = access === 'manage_service';
  const busy = settings.pending !== null;
  return <section className="settings-area stack-tight" aria-labelledby="settings-diagnostics-title">
    <div className="panel-heading"><h2 id="settings-diagnostics-title">{t('settings.diagnostics')}</h2>
      <button type="button" onClick={session.refreshOverview} disabled={read.phase === 'loading'}>{t('settings.refreshDiagnostics')}</button></div>
    <p className="overview-note" role="status">{t(overviewView(read).label)}</p>
    <OverviewProblem read={read} />
    <p className="overview-note">{t('settings.runtimeNote')}</p>
    {commands && <ConfigurationFailure result={commands.driver} />}
    {driver && <>
      {commands?.driver.status !== 'completed' && <p className="amber">{t('settings.partialStatus')}</p>}
      <dl className="settings-fields"><DetailField labelKey="receipt.source">{t(sourceLabels[driver.source_state])}</DetailField>
        <DetailField labelKey="category.self">{t(featureView(driver.self_protection).label)}</DetailField>
        <DetailField labelKey="receipt.pending">{driver.pending}</DetailField><DetailField labelKey="receipt.unread">{driver.pending_receipts}</DetailField>
        <DetailField labelKey="receipt.dropped">{String(driver.dropped)}</DetailField>
        {driver.error !== null && <DetailField labelKey="receipt.error"><code>{JSON.stringify(driver.error)}</code></DetailField>}
      </dl>
      <div className="settings-runtime-features">{protectionKinds.map(feature => <section key={feature} className="settings-feature stack-tight">
        <h3>{t(categoryLabels[feature])}</h3>
        <dl className="settings-fields"><DetailField labelKey="settings.driverDefault">{t(driver[feature].startup_default ? 'feature.enabled' : 'feature.disabled')}</DetailField>
          <DetailField labelKey="settings.runtimeOverride">{t(overrideLabels[driver[feature].runtime_override])}</DetailField>
          <DetailField labelKey="settings.effectiveState">{t(featureView(driver[feature].effective_state).label)}</DetailField></dl>
        {manager && !confirmation && <div className="actions">
          <button type="button" disabled={busy} onClick={() => setConfirmation({ type: 'set_runtime_feature', feature, enabled: true })}>{t('settings.enableTemporarily')}</button>
          <button type="button" disabled={busy} onClick={() => setConfirmation({ type: 'set_runtime_feature', feature, enabled: false })}>{t('settings.disableTemporarily')}</button>
          <button type="button" disabled={busy} onClick={() => setConfirmation({ type: 'resume_runtime_feature', feature })}>{t('settings.resume')}</button>
        </div>}
      </section>)}</div>
    </>}
    {manager && confirmation && <section className="settings-confirm stack-tight" aria-labelledby="settings-control-preview">
      <ActionHeading id="settings-control-preview" labelKey="settings.controlPreview" />
      <p>{t(categoryLabels[confirmation.feature])} · {t(confirmation.type === 'resume_runtime_feature' ? 'settings.resume'
        : confirmation.enabled ? 'settings.enableTemporarily' : 'settings.disableTemporarily')}</p>
      <p className="overview-note">{t('settings.controlNote')}</p>
      {confirmation.type === 'resume_runtime_feature' && <p>{t('settings.resumeNote')}</p>}
      <div className="actions"><button type="button" onClick={() => setConfirmation(null)}>{t('settings.cancel')}</button>
        <button type="button" disabled={busy} onClick={() => { session.controlRuntime(confirmation); setConfirmation(null); }}>{t('settings.confirmControl')}</button></div>
    </section>}
    <h3>{t('receipt.flow')}</h3>
    {commands && <ConfigurationFailure result={commands.flow} />}
    {flow && <>
      {commands?.flow.status !== 'completed' && <p className="amber">{t('settings.partialStatus')}</p>}
      <dl className="settings-fields"><DetailField labelKey="receipt.intake">{t(flowView(flow.intake_state).label)}</DetailField>
        <DetailField labelKey="receipt.exchanges">{flow.active_exchanges}</DetailField><DetailField labelKey="receipt.lost">{flow.lost_deliveries}</DetailField>
        <DetailField labelKey="receipt.parseFailures">{flow.parse_failures}</DetailField></dl>
    </>}
    {commands && <RawFacts summaryKey="receipt.details" value={commands} />}
  </section>;
}
export function SettingsView({ settings, overview, access, session }: {
  settings: SettingsState; overview: OverviewRead; access: Access | null; session: ReviewSession;
}) {
  const { t } = useI18n();
  const ready = settings.phase === 'ready' && !settings.pending;
  return <section id="settings" className="settings-page stack" aria-labelledby="settings-title">
      <p className="overview-note">{t('settings.intro')}</p>
      {access !== 'manage_service' && <p role="status" className="overview-note">{t('settings.noWritePermission')}</p>}
      <section className="settings-area stack-tight" aria-labelledby="settings-service-title">
        <div className="panel-heading"><h2 id="settings-service-title">{t('settings.service')}</h2>
          <button type="button" disabled={settings.pending !== null} onClick={session.refreshSettings}>{t('settings.read')}</button></div>
        <ConfigurationReadStatus settings={settings} />
        {settings.snapshot && <>
          <SavedSnapshot snapshot={settings.snapshot} />
          {ready && access === 'manage_service' && <ConfigurationActions snapshot={settings.snapshot} session={session} />}
        </>}
        {settings.receipt && <OperationReceipt receipt={settings.receipt} />}
      </section>
      <RuntimeDiagnostics read={overview} settings={settings} access={access} session={session} />
  </section>;
}
