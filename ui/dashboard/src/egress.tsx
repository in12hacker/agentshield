import { useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore, type FormEvent, type ReactNode } from 'react';
import type { ReviewSession } from './session';
import type { SettingsState } from './settings-state';
import { ConfigurationReadStatus } from './configuration-receipt';
import type { ConfigSnapshot } from './settings-wire';
import type { OverviewRead } from './reply-wire';
import { flowFacts, flowView, overviewCommands, overviewView } from './overview-model';
import { OverviewProblem } from './overview';
import { idLabel, type Access } from './wire';
import { sameId } from './event-wire';
import { decimalU64 } from './wire-json';
import { parseReference, validSecret, type PrivateOperation } from './credentials-wire';
import type { CredentialReceipt, CredentialState } from './credentials-state';
import { configuredSubjects, issuableBinding, readEgressConfig, type CredentialBinding, type EgressConfig, type ProviderRoute } from './egress-wire';
import { sameJsonValue } from './global/json-value';
import { useI18n } from './i18n';
import type { MessageKey } from './messages';
import { settingsRoute } from './global/navigation';
import { RawFacts } from './global/raw-facts';
import { DetailField } from './global/detail-field';
import { downloadBlob } from './global/download';
import { focusOutsideDialog } from './global/focus';

function Confirmation({ titleKey, askVisible, children }: { titleKey: MessageKey; askVisible: boolean; children: ReactNode }) {
  const { t } = useI18n();
  const heading = useRef<HTMLHeadingElement>(null);
  useLayoutEffect(() => {
    if (!askVisible) focusOutsideDialog(heading.current);
  }, []);
  return <section className="settings-confirm stack-tight" aria-label={t(titleKey)}>
    <h3 ref={heading} tabIndex={-1}>{t(titleKey)}</h3>{children}
  </section>;
}
function BindingScope({ binding, routes }: { binding: CredentialBinding; routes: readonly ProviderRoute[] }) {
  const { t } = useI18n();
  return <div className="egress-scope">
    <div><h3>{t('egress.subjects')}</h3>{binding.subjects.length === 0 ? <p>{t('egress.emptyScope')}</p>
      : binding.subjects.map((s, index) => <dl className="egress-identity" key={index}>
        {s.kind === 'process' ? <><DetailField labelKey="egress.process"><code>{s.value.host_tgid}</code></DetailField>
          <DetailField labelKey="egress.boot"><code>{idLabel(s.value.boot_id)}</code></DetailField>
          <DetailField labelKey="egress.birth"><code>{s.value.birth_boottime_ns}</code></DetailField></>
          : <><DetailField labelKey="egress.runtime"><code>{JSON.stringify(s.value.runtime)}</code></DetailField>
            <DetailField labelKey="egress.namespace"><code>{JSON.stringify(s.value.namespace)}</code></DetailField>
            <DetailField labelKey="egress.container"><code>{JSON.stringify(s.value.id)}</code></DetailField></>}
      </dl>)}</div>
    <div><h3>{t('egress.targets')}</h3>{binding.routes.length === 0 ? <p>{t('egress.emptyScope')}</p>
      : binding.routes.map((id, index) => {
        const route = routes.find(r => sameId(r.id, id));
        return <div className="egress-target" key={index}><code>{idLabel(id)}</code>
          {route ? <><p><code>{route.origin}</code></p><p><code>{route.method}</code> <code>{route.path}</code></p>
            <p><code>{route.profile}</code></p></> : <p className="error">{t('egress.routeMissing')}</p>}
        </div>;
      })}</div>
    <dl><DetailField labelKey="egress.slot"><code>{binding.slot}</code></DetailField>
      <DetailField labelKey="egress.secretRef"><code>{idLabel(binding.secret)}</code></DetailField></dl>
  </div>;
}
function BindSavedSecret({ config, snapshot, ready, state, session, askVisible }: {
  config: EgressConfig; snapshot: ConfigSnapshot; ready: boolean; state: CredentialState;
  session: ReviewSession; askVisible: boolean;
}) {
  const { t } = useI18n();
  const [targetIndex, setTargetIndex] = useState('0'), [routeIndex, setRouteIndex] = useState('0');
  const [slot, setSlot] = useState<CredentialBinding['slot']>('authorization_bearer');
  const [confirm, setConfirm] = useState<CredentialBinding | null>(null);
  const subjects = configuredSubjects(config), routes = config.flow?.routes ?? [];
  const subject = subjects[Number(targetIndex)], route = routes[Number(routeIndex)];
  const secret = state.saved?.secret ?? (config.bindings.length === 1 ? config.bindings[0]!.secret : null);
  const bound = !!secret && !!subject && !!route && config.bindings.some(binding => binding.enabled
    && sameId(binding.secret, secret) && binding.slot === slot && binding.routes.some(id => sameId(id, route.id))
    && binding.subjects.some(value => sameJsonValue(value, subject)));
  return <section className="settings-area stack-tight">
    <h2>{t('egress.setupTitle')}</h2><p className="overview-note">{t('egress.setupNote')}</p>
    {!subjects.length || !routes.length ? <p>{t('egress.setupMissingTarget')}</p> : <>
      <label htmlFor="egress-setup-subject">{t('egress.setupSubject')}</label>
      <select id="egress-setup-subject" value={targetIndex} disabled={!ready || !!confirm}
        onChange={event => setTargetIndex(event.target.value)}>
        {subjects.map((value, index) => <option key={index} value={index}>{value.kind === 'process'
          ? t('egress.setupProcess', { pid: value.value.host_tgid })
          : `${value.value.runtime} ${value.value.id}`}</option>)}
      </select>
      <label htmlFor="egress-setup-route">{t('egress.targets')}</label>
      <select id="egress-setup-route" value={routeIndex} disabled={!ready || !!confirm}
        onChange={event => setRouteIndex(event.target.value)}>
        {routes.map((value, index) => <option key={idLabel(value.id)} value={index}>{value.origin} {value.method} {value.path}</option>)}
      </select>
      <label htmlFor="egress-setup-slot">{t('egress.slot')}</label>
      <select id="egress-setup-slot" value={slot} disabled={!ready || !!confirm}
        onChange={event => setSlot(event.target.value as CredentialBinding['slot'])}>
        <option value="authorization_bearer">{t('egress.setupBearer')}</option><option value="x_api_key">{t('egress.setupApiKey')}</option>
      </select>
      {!secret ? <p>{t('egress.setupImportFirst')}</p> : bound ? <p role="status">{t('egress.setupBound')}</p>
        : !confirm && <button type="button" disabled={!ready || !!state.pending || !subject || !route}
          onClick={() => { if (subject && route) setConfirm({ id: Array.from(crypto.getRandomValues(new Uint8Array(16))),
            secret, enabled: true, subjects: [subject], routes: [route.id], slot }); }}>{t('egress.setupReview')}</button>}
    </>}
    {confirm && <Confirmation titleKey="egress.setupTitle" askVisible={askVisible}>
      <BindingScope binding={confirm} routes={routes} /><div className="actions">
        <button type="button" onClick={() => setConfirm(null)}>{t('settings.cancel')}</button>
        <button type="button" disabled={!ready || !!state.pending} onClick={() => {
          session.saveConfiguration(snapshot, { ...snapshot.document,
            credentials: { bindings: [...snapshot.document.credentials.bindings, confirm] } });
          setConfirm(null);
        }}>{t('egress.setupSave')}</button>
      </div>
    </Confirmation>}
  </section>;
}

function PublicCa({ config, snapshot, ready, state, session, manager, askVisible }: {
  config: EgressConfig | null; snapshot: ConfigSnapshot | null;
  ready: boolean; state: CredentialState; session: ReviewSession; manager: boolean; askVisible: boolean;
}) {
  const { t } = useI18n();
  const [confirm, setConfirm] = useState(false), [downloadFailed, setDownloadFailed] = useState(false);
  const ca = config?.flow?.ca;
  const downloadable = session.getConfiguredCa(snapshot, state.ca);
  const download = () => {
    session.discardPhantom();
    const certificate = session.getConfiguredCa(snapshot, state.ca);
    if (!certificate) { setDownloadFailed(true); return; }
    try {
      downloadBlob(new Blob([new Uint8Array(certificate.certificate_der)], { type: 'application/pkix-cert' }), 'agentshield-ca.der');
      setDownloadFailed(false);
    } catch { setDownloadFailed(true); }
  };
  return <section className="settings-area stack-tight">
    <h2>{t('egress.ca')}</h2>
    <dl className="settings-fields"><DetailField labelKey="egress.configuredCa"><code>{ca ? idLabel(ca) : t(config ? 'egress.noConfiguredCa' : 'settings.notRead')}</code></DetailField></dl>
    <p className="overview-note">{t('egress.trustNote')}</p>
    {manager && <div className="actions">
      <button type="button" disabled={!ready || !ca || !!state.pending} onClick={() => { if (ca) session.credentialCommand({ type: 'public_ca', ca }); }}>{t('egress.readCa')}</button>
      <button type="button" disabled={!!state.pending} onClick={() => { session.discardPhantom(); setConfirm(true); }}>{t('egress.ensureCa')}</button>
    </div>}
    {confirm && manager && <Confirmation titleKey="egress.ensureCa" askVisible={askVisible}>
      <p>{t('egress.ensureNote')}</p><div className="actions">
        <button type="button" onClick={() => setConfirm(false)}>{t('settings.cancel')}</button>
        <button type="button" disabled={!!state.pending} onClick={() => { setConfirm(false); session.credentialCommand({ type: 'ensure_ca' }); }}>{t('egress.confirmEnsure')}</button>
      </div></Confirmation>}
    {state.ca && <><dl className="settings-fields">
      <DetailField labelKey="egress.publicCaId"><code>{idLabel(state.ca.id)}</code></DetailField>
      <DetailField labelKey="egress.fingerprint"><code>{idLabel(state.ca.fingerprint_sha256)}</code></DetailField>
    </dl><p className="overview-note">{t(downloadable ? 'egress.caBound' : 'egress.caHistorical')}</p>
      {downloadable && <button type="button" onClick={download}>{t('egress.downloadCa')}</button>}</>}
    {downloadFailed && <p className="error" role="status">{t('egress.downloadFailed')}</p>}
  </section>;
}

function SecretInput({ state, session, askVisible }: { state: CredentialState; session: ReviewSession; askVisible: boolean }) {
  const { t } = useI18n();
  const form = useRef<HTMLFormElement>(null), input = useRef<HTMLInputElement>(null);
  const [rotate, setRotate] = useState(false), [reference, setReference] = useState(''), [revision, setRevision] = useState('');
  const [confirmation, setConfirmation] = useState<PrivateOperation | null>(null), [invalid, setInvalid] = useState<MessageKey | null>(null);
  useLayoutEffect(() => {
    const element = form.current;
    return () => { element?.reset(); };
  }, []);
  const prepare = () => {
    session.discardPhantom(); setInvalid(null);
    try {
      setConfirmation(rotate ? { type: 'replace_secret', secret: parseReference(reference), expected_revision: decimalU64(revision) }
        : { type: 'import_secret' });
    } catch { setInvalid('egress.invalidReference'); }
  };
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    let value = input.current?.value ?? '';
    event.currentTarget.reset();
    const operation = confirmation;
    setConfirmation(null);
    try {
      if (!validSecret(value)) { setInvalid('egress.invalidSecret'); return; }
      if (operation) session.credentialCommand(operation, value);
    } finally { value = ''; }
  };
  return <section className="settings-area stack-tight">
    <h2>{t('egress.secrets')}</h2><p className="overview-note" id="egress-secret-help">{t('egress.secretNote')}</p>
    <form ref={form} onSubmit={submit} autoComplete="off" className="stack-tight">
      <fieldset disabled={!!state.pending} className="egress-form stack-tight">
        <label className="settings-check"><input type="checkbox" checked={rotate} disabled={confirmation !== null}
          onChange={event => {
            const next = event.currentTarget.checked;
            if (input.current) input.current.value = '';
            session.discardPhantom(); setRotate(next); setInvalid(null);
          }} />
          <span>{t('egress.rotate')}</span></label>
        {rotate && <><label htmlFor="egress-secret-ref">{t('egress.secretRef')}</label>
          <input id="egress-secret-ref" value={reference} autoComplete="off" spellCheck={false} disabled={confirmation !== null}
            onChange={event => setReference(event.target.value)} />
          <label htmlFor="egress-secret-revision">{t('egress.expectedRevision')}</label>
          <input id="egress-secret-revision" value={revision} inputMode="numeric" autoComplete="off" disabled={confirmation !== null}
            onChange={event => setRevision(event.target.value)} />
          {state.saved && !confirmation && <button type="button" onClick={() => {
            setReference(idLabel(state.saved!.secret)); setRevision(state.saved!.revision);
          }}>{t('egress.useSavedReceipt')}</button>}</>}
        <label htmlFor="egress-provider-secret">{t('egress.secretValue')}</label>
        <input ref={input} id="egress-provider-secret" type="password" autoComplete="off" autoCapitalize="none"
          spellCheck={false} aria-describedby="egress-secret-help" onPaste={event => {
            if (/[\r\n\0]/.test(event.clipboardData.getData('text'))) { event.preventDefault(); setInvalid('egress.invalidSecret'); }
          }} onDrop={event => {
            if (/[\r\n\0]/.test(event.dataTransfer.getData('text'))) { event.preventDefault(); setInvalid('egress.invalidSecret'); }
          }} onKeyDown={event => {
            if (event.key === 'Enter' && !confirmation) { event.preventDefault(); prepare(); }
          }} />
        {invalid && <p className="error" role="alert">{t(invalid)}</p>}
        {!confirmation ? <button type="button" onClick={prepare}>{t(rotate ? 'egress.prepareRotate' : 'egress.prepareImport')}</button>
          : <Confirmation titleKey={rotate ? 'egress.prepareRotate' : 'egress.prepareImport'} askVisible={askVisible}>
            {confirmation.type === 'replace_secret' && <dl className="settings-fields">
              <DetailField labelKey="egress.secretRef"><code>{idLabel(confirmation.secret)}</code></DetailField>
              <DetailField labelKey="egress.expectedRevision"><code>{confirmation.expected_revision}</code></DetailField>
            </dl>}
            <p>{t('egress.secretConfirm')}</p><div className="actions">
              <button type="button" onClick={() => { setConfirmation(null); form.current?.reset(); }}>{t('settings.cancel')}</button>
              <button type="submit" className="primary">{t(rotate ? 'egress.confirmRotate' : 'egress.confirmImport')}</button>
            </div></Confirmation>}
      </fieldset>
    </form>
  </section>;
}

function IssuePhantom({ snapshot, config, state, session, askVisible, ready }: {
  snapshot: ConfigSnapshot; config: EgressConfig; state: CredentialState; session: ReviewSession; askVisible: boolean; ready: boolean;
}) {
  const { t } = useI18n();
  const [selected, setSelected] = useState(''), [confirm, setConfirm] = useState<CredentialBinding | null>(null);
  const choices = config.bindings.filter(b => issuableBinding(config, b));
  const binding = choices.find(b => idLabel(b.id) === selected);
  return <section className="settings-area stack-tight">
    <h2>{t('egress.issue')}</h2><p className="overview-note">{t('egress.issueNote')}</p>
    {choices.length === 0 ? <p>{t('egress.noEnabledBindings')}</p> : <>
      <label htmlFor="egress-binding">{t('egress.selectBinding')}</label>
      <select id="egress-binding" value={selected} disabled={!!state.pending || !!confirm || !ready}
        onChange={event => { session.discardPhantom(); setSelected(event.target.value); }}>
        <option value="">{t('egress.selectBinding')}</option>
        {choices.map(b => <option key={idLabel(b.id)} value={idLabel(b.id)}>{idLabel(b.id)}</option>)}
      </select>
      {!confirm && <button type="button" disabled={!binding || !!state.pending || !ready}
        onClick={() => { if (binding) { session.discardPhantom(); setConfirm(binding); } }}>{t('egress.reviewIssue')}</button>}
    </>}
    {confirm && <Confirmation titleKey="egress.reviewIssue" askVisible={askVisible}>
      <p>{t('egress.bindingId')} <code>{idLabel(confirm.id)}</code></p>
      <p>{t('settings.version')} <code>{idLabel(snapshot.version)}</code></p>
      <BindingScope binding={confirm} routes={config.flow?.routes ?? []} />
      <p>{t('egress.issueConfirm')}</p><div className="actions">
        <button type="button" onClick={() => setConfirm(null)}>{t('settings.cancel')}</button>
        <button type="button" className="primary" disabled={!!state.pending || !ready}
          onClick={() => { session.issuePhantom(snapshot, confirm); setConfirm(null); }}>{t('egress.confirmIssue')}</button>
      </div></Confirmation>}
  </section>;
}

function RevokePhantom({ state, session, askVisible }: { state: CredentialState; session: ReviewSession; askVisible: boolean }) {
  const { t } = useI18n();
  const [token, setToken] = useState(''), [confirm, setConfirm] = useState<readonly number[] | null>(null), [invalid, setInvalid] = useState(false);
  return <section className="settings-area stack-tight">
    <h2>{t('egress.revoke')}</h2><p className="overview-note">{t('egress.revokeNote')}</p>
    <label htmlFor="egress-revoke-id">{t('egress.tokenId')}</label>
    <input id="egress-revoke-id" value={token} autoComplete="off" spellCheck={false} disabled={!!confirm || !!state.pending}
      onChange={event => { setToken(event.target.value); setInvalid(false); }} />
    {!confirm && <div className="actions">
      {state.issued && <button type="button" disabled={!!state.pending} onClick={() => setToken(idLabel(state.issued!.token_id))}>{t('egress.useIssuedReceipt')}</button>}
      <button type="button" disabled={!!state.pending} onClick={() => {
        session.discardPhantom();
        try { setConfirm(parseReference(token)); setInvalid(false); } catch { setInvalid(true); }
      }}>{t('egress.reviewRevoke')}</button>
    </div>}
    {invalid && <p className="error" role="alert">{t('egress.invalidReference')}</p>}
    {confirm && <Confirmation titleKey="egress.reviewRevoke" askVisible={askVisible}>
      <p>{t('egress.tokenId')} <code>{idLabel(confirm)}</code></p><p>{t('egress.revokeConfirm')}</p>
      <div className="actions"><button type="button" onClick={() => setConfirm(null)}>{t('settings.cancel')}</button>
        <button type="button" disabled={!!state.pending} onClick={() => {
          session.credentialCommand({ type: 'revoke_phantom', token: confirm }); setConfirm(null);
        }}>{t('egress.confirmRevoke')}</button></div>
    </Confirmation>}
  </section>;
}
function OnceValue({ session }: { session: ReviewSession }) {
  const { t } = useI18n();
  const input = useRef<HTMLInputElement>(null);
  const [revealed, setRevealed] = useState(false), [copied, setCopied] = useState<boolean | null>(null);
  useLayoutEffect(() => {
    const element = input.current;
    if (element) element.value = session.readPhantomValue() ?? '';
    return () => { if (element) element.value = ''; };
  }, [session]);
  return <section className="egress-once stack-tight" aria-labelledby="egress-once-title">
    <h3 id="egress-once-title">{t('egress.onceValue')}</h3><p className="overview-note">{t('egress.onceNote')}</p>
    <label htmlFor="egress-once-value">{t('egress.onceValue')}</label>
    <input ref={input} id="egress-once-value" type={revealed ? 'text' : 'password'} readOnly autoComplete="off" spellCheck={false} />
    <div className="actions">
      <button type="button" aria-pressed={revealed} onClick={() => setRevealed(!revealed)}>{t(revealed ? 'egress.hide' : 'egress.reveal')}</button>
      <button type="button" onClick={() => {
        if (!navigator.clipboard) { setCopied(false); return; }
        void session.copyPhantomValue(value => navigator.clipboard.writeText(value)).then(setCopied);
      }}>{t('egress.copy')}</button>
      <button type="button" onClick={session.discardPhantom}>{t('egress.closeValue')}</button>
    </div>
    {copied !== null && <p role="status">{t(copied ? 'egress.copied' : 'egress.copyFailed')}</p>}
  </section>;
}
function Receipt({ receipt }: { receipt: CredentialReceipt }) {
  const { t } = useI18n();
  const { data, operation } = receipt;
  return <section className="settings-receipt stack-tight">
    <h3>{t('egress.lastReceipt')}</h3>
    <p role="status">{t(receipt.problem ? 'egress.unconfirmed' : receipt.code ? 'egress.operationFailed' : 'egress.completed')}</p>
    <dl className="settings-fields"><DetailField labelKey="settings.operation"><code>{operation.type}</code></DetailField>
      <DetailField labelKey="overview.commandId"><code>{idLabel(receipt.id)}</code></DetailField>
      {receipt.code && <DetailField labelKey="common.failureCode"><code>{receipt.code}</code></DetailField>}
      {operation.type === 'public_ca' && <DetailField labelKey="egress.configuredCa"><code>{idLabel(operation.ca)}</code></DetailField>}
      {operation.type === 'replace_secret' && <><DetailField labelKey="egress.secretRef"><code>{idLabel(operation.secret)}</code></DetailField>
        <DetailField labelKey="egress.expectedRevision"><code>{operation.expected_revision}</code></DetailField></>}
      {operation.type === 'issue_phantom' && <DetailField labelKey="settings.version"><code>{idLabel(operation.expected_config)}</code></DetailField>}
      {operation.type === 'revoke_phantom' && <DetailField labelKey="egress.tokenId"><code>{idLabel(operation.token)}</code></DetailField>}
      {data?.type === 'ca' && <DetailField labelKey="egress.publicCaId"><code>{idLabel(data.info.id)}</code></DetailField>}
      {data?.type === 'secret_saved' && <><DetailField labelKey="egress.secretRef"><code>{idLabel(data.receipt.secret)}</code></DetailField>
        <DetailField labelKey="egress.revision"><code>{data.receipt.revision}</code></DetailField></>}
      {data?.type === 'phantom_issued' && <><DetailField labelKey="egress.tokenId"><code>{idLabel(data.token_id)}</code></DetailField>
        <DetailField labelKey="egress.bindingId"><code>{idLabel(data.binding)}</code></DetailField></>}
      {(data?.type === 'phantom_issued' || data?.type === 'phantom_revoked') && <DetailField labelKey="egress.revision"><code>{data.revision}</code></DetailField>}
    </dl>
    {receipt.problem && <p className="error">{t(receipt.problem.key, receipt.problem.params)}</p>}
    {receipt.scope && <details><summary>{t('egress.receiptScope')}</summary>
      <BindingScope binding={receipt.scope.binding} routes={receipt.scope.routes} />
    </details>}
  </section>;
}

function EgressWorkspace({ settings, overview, access, session, askVisible }: {
  settings: SettingsState; overview: OverviewRead; access: Access | null; session: ReviewSession; askVisible: boolean;
}) {
  const { t } = useI18n();
  const state = useSyncExternalStore(session.subscribeCredentials, session.getCredentials, session.getCredentials);
  const snapshot = settings.snapshot;
  const config = useMemo(() => {
    if (!snapshot) return null;
    try { return readEgressConfig(snapshot); } catch { return null; }
  }, [snapshot]);
  const ready = settings.phase === 'ready' && !settings.pending;
  const manager = access === 'manage_service' && !state.revoked;
  const flow = flowFacts(overview), commands = overviewCommands(overview);
  return <div className="stack">
    <p className="overview-note">{t('egress.intro')}</p>
    <div className="actions"><button type="button" disabled={!!settings.pending} onClick={() => {
      session.discardPhantom(); session.refreshSettings();
    }}>{t('settings.read')}</button>{manager && <a href={settingsRoute}>{t('egress.editBindings')}</a>}</div>
    <ConfigurationReadStatus settings={settings} />
    {snapshot && !config && <p className="error" role="alert">{t('egress.invalidConfig')}</p>}
    {!manager && <p className="overview-note" role="status">{t(state.revoked ? 'egress.permissionRevoked' : 'egress.noManagementPermission')}</p>}
    <section className="settings-area stack-tight">
      <h2>{t('egress.mode')}</h2>
      <dl className="settings-fields">
        <DetailField labelKey="egress.savedMode">{config ? config.flow ? <><code>{config.flow.mode}</code> · {t(config.flow.enabled ? 'feature.enabled' : 'feature.disabled')}</>
          : t('egress.noFlow') : t('settings.notRead')}</DetailField>
        <DetailField labelKey="egress.actualMode">{flow ? <><code>{flow.mode}</code> · {t(flowView(flow.intake_state).label)}</> : t('common.unknownUnsupplied')}</DetailField>
      </dl>
      <p className="overview-note" role="status">{t(overviewView(overview).label)}</p>
      <OverviewProblem read={overview} />
      {flow && commands?.flow.status !== 'completed' && <p className="amber">{t('settings.partialStatus')}</p>}
      <p className="overview-note">{t('egress.modeNote')}</p>
      <button type="button" disabled={overview.phase === 'loading'} onClick={() => {
        session.discardPhantom(); session.refreshOverview();
      }}>{t('settings.refreshDiagnostics')}</button>
      {snapshot && <p className="overview-note">{t('settings.version')} <code>{idLabel(snapshot.version)}</code></p>}
      {config && snapshot?.document.flow && <RawFacts summaryKey="egress.originalFlowScope" value={snapshot.document.flow.scope} />}
    </section>
    <PublicCa config={config} snapshot={snapshot} ready={ready} state={state} session={session} manager={manager} askVisible={askVisible} />
    {config?.flow && <section className="settings-area stack-tight">
      <h2>{t('egress.routes')}</h2>
      {config.flow.routes.length === 0 ? <p>{t('egress.noRoutes')}</p> : config.flow.routes.map((route, index) => <div className="egress-target" key={index}>
        <p><code>{idLabel(route.id)}</code></p><p><code>{route.origin}</code></p>
        <p><code>{route.method}</code> <code>{route.path}</code></p><p><code>{route.profile}</code></p>
      </div>)}
    </section>}
    {config && <section className="settings-area stack-tight">
      <h2>{t('egress.bindings')}</h2><p className="overview-note">{t('egress.bindingsNote')}</p>
      {config.bindings.length === 0 ? <p>{t('egress.noBindings')}</p> : config.bindings.map((binding, index) => <article className="egress-binding stack-tight" key={index}>
        <div className="panel-heading"><h3><code>{idLabel(binding.id)}</code></h3>
          <span>{t(binding.enabled ? 'feature.enabled' : 'feature.disabled')}</span></div>
        <BindingScope binding={binding} routes={config.flow?.routes ?? []} />
      </article>)}
    </section>}
    {manager && <>
      <SecretInput state={state} session={session} askVisible={askVisible} />
      {snapshot && config && <BindSavedSecret key={idLabel(snapshot.version)} snapshot={snapshot} config={config}
        ready={ready} state={state} session={session} askVisible={askVisible} />}
      {snapshot && config && <IssuePhantom key={idLabel(snapshot.version)} snapshot={snapshot} config={config} state={state}
        session={session} askVisible={askVisible} ready={ready} />}
      <RevokePhantom state={state} session={session} askVisible={askVisible} />
    </>}
    <section className="settings-area stack-tight">
      <h2>{t('egress.sessionReceipts')}</h2><p className="overview-note">{t('egress.receiptsNote')}</p>
      {state.pending && <p role="status">{t('egress.pending')} <code>{state.pending.operation.type}</code></p>}
      {state.problem && <p className="error" role="status">{t(state.problem.key, state.problem.params)}</p>}
      {state.saved && state.receipt?.data?.type !== 'secret_saved' && <dl className="settings-fields"><DetailField labelKey="egress.secretRef"><code>{idLabel(state.saved.secret)}</code></DetailField>
        <DetailField labelKey="egress.revision"><code>{state.saved.revision}</code></DetailField></dl>}
      {state.issued && state.receipt?.data?.type !== 'phantom_issued' && <dl className="settings-fields"><DetailField labelKey="egress.tokenId"><code>{idLabel(state.issued.token_id)}</code></DetailField>
        <DetailField labelKey="egress.bindingId"><code>{idLabel(state.issued.binding)}</code></DetailField>
        <DetailField labelKey="egress.revision"><code>{state.issued.revision}</code></DetailField></dl>}
      {state.receipt && <Receipt receipt={state.receipt} />}
      {!state.receipt && !state.pending && !state.saved && !state.issued && <p>{t('egress.noReceipts')}</p>}
      {state.hasValue && manager && <OnceValue key={idLabel(state.issued!.token_id)} session={session} />}
    </section>
  </div>;
}

export function EgressView(props: {
  settings: SettingsState; overview: OverviewRead; access: Access | null; session: ReviewSession; askVisible: boolean;
}) {
  return <section id="egress-trust" className="settings-page egress-page stack" aria-labelledby="egress-title">
    <EgressWorkspace {...props} />
  </section>;
}
