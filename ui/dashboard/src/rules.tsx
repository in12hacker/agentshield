import { byteText } from './global/byte-text';
import { RuleDetails, RuleEffect, ScopeDetails } from './rule-details';
import { ruleActions, ruleCategories, ruleOperation, ruleStages, actorFields, targetFields, editableRuleOperations } from './rule-fields';
import { ActionInput, ConditionsInput, ScopeInput } from './rule-inputs';
import { ScrollPane } from './global/scroll-pane';
import { memo, useEffect, useLayoutEffect, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { useI18n } from './i18n';
import { message, type Message, type MessageKey } from './messages';
import { canReview, idLabel, type Access } from './wire';
import { factsJson, parseUnique } from './wire-json';
import { RawFacts } from './global/raw-facts';
import { definition, decodeRuleGroup, decodeRuleDefinition, placement, placements, ruleKinds,
  type PolicySnapshot, type Rule, type RuleChange, type RuleGroup, type RuleKind } from './rules-wire';
import { adoptionConfirmed, savedFacts, type RulesState } from './rules-state';
import type { ReviewSession } from './session';
import { focusOutsideDialog } from './global/focus';

const changeLabels = {
  create_group: 'rules.createGroup', replace_group: 'rules.editGroup', delete_group: 'rules.deleteGroup',
  create_rule: 'rules.createRule', replace_rule: 'rules.editRule', delete_rule: 'rules.deleteRule',
} as const satisfies Record<RuleChange['type'], MessageKey>;
interface Editor {
  type: RuleChange['type']; base: PolicySnapshot; group: RuleGroup | null; rule: Rule | null;
  prepared?: RuleChange;
}
function RuleDialog({ open, titleKey, close, children }: {
  open: boolean; titleKey: MessageKey; close: () => void; children: ReactNode;
}) {
  const { t } = useI18n();
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const restore = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    const element = dialog.current;
    if (!element) return;
    if (open && !element.open) {
      if (!restore.current) restore.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      element.showModal();
    } else if (!open && element.open) element.close();
  }, [open]);
  useLayoutEffect(() => { if (open) cancel.current?.focus(); }, [open, titleKey]);
  useLayoutEffect(() => {
    const element = dialog.current;
    return () => {
      element?.close();
      if (restore.current?.isConnected) restore.current.focus();
    };
  }, []);
  return <dialog ref={dialog} className="rule-dialog" aria-labelledby="rule-dialog-title"
    aria-describedby={open ? "rule-dialog-help" : undefined} onCancel={event => { event.preventDefault(); close(); }}>
    <div className="rule-dialog-content">
      <header className="review-header"><h2 id="rule-dialog-title">{t(titleKey)}</h2>
        <button ref={cancel} type="button" onClick={close}>{t('rules.close')}</button></header>
      {children}
    </div>
  </dialog>;
}
function StateSelect({ name, labelKey, value }: { name: string; labelKey: MessageKey; value: boolean | undefined }) {
  const { t } = useI18n();
  return <label>{t(labelKey)}<select name={name} defaultValue={value === undefined ? '' : String(value)} required>
    <option value="" disabled>{t('rules.choose')}</option><option value="true">{t('rules.yes')}</option>
    <option value="false">{t('rules.no')}</option>
  </select></label>;
}
function OperationsInput({ kind, value }: { kind: RuleKind; value: readonly string[] | undefined }) {
  const { t } = useI18n();
  const [operations, setOperations] = useState(value ?? []);
  return <fieldset className="rule-options"><legend>{t('rules.operations')}</legend>
    <input type="hidden" name="operations" value={factsJson(operations)} />
    {editableRuleOperations[kind].map(operation => <label key={operation}><input type="checkbox" value={operation} checked={operations.includes(operation)}
      onChange={event => setOperations(event.target.checked ? [...operations, operation] : operations.filter(item => item !== operation))} />
      {t(ruleOperation(kind, operation))}</label>)}
  </fieldset>;
}
function ScopeSummary({ rules }: { rules: readonly Rule[] }) {
  const { t } = useI18n();
  if (!rules.length) return <p className="muted">{t('rules.noScope')}</p>;
  return <ul className="rule-scope-list">
    {rules.map(rule => <li key={rule.id}><code>{rule.id}</code>
      <div><ScopeDetails scope={rule.scope} /></div></li>)}
  </ul>;
}
function stageActions(stage: string): readonly Rule['action']['type'][] {
  return stage === 'ask' ? ['ask'] : stage === 'driver' ? ['allow', 'deny', 'to_user'] : ['allow', 'deny'];
}
function EditDialog({ editor, open, close, session }: {
  editor: Editor; open: boolean; close: () => void; session: ReviewSession;
}) {
  const { t } = useI18n();
  const { group, rule } = editor;
  const form = useRef<HTMLFormElement>(null);
  const [error, setError] = useState<Message | null>(null);
  const [change, setChange] = useState<RuleChange | null>(() => editor.prepared ?? (
    editor.type === 'delete_group' && group ? { type: 'delete_group', group_id: group.id }
      : editor.type === 'delete_rule' && rule ? { type: 'delete_rule', rule_id: rule.id } : null));
  const isGroup = editor.type.endsWith('_group');
  const original = isGroup ? group : rule && group ? definition(group.kind, rule) : null;
  const [kind, setKind] = useState<RuleKind | ''>(group?.kind ?? '');
  const [execution, setExecution] = useState(group ? placement(group) : '');
  const allowedActions = stageActions(execution);
  const converting = Boolean(isGroup && group && execution && execution !== placement(group));
  function proposed(): RuleChange {
    if (!form.current) throw new Error('missing_form');
    const data = new FormData(form.current);
    const get = (name: string) => String(data.get(name) ?? '');
    const json = (name: string) => parseUnique(get(name));
    const flag = (name: string) => {
      const value = get(name); if (value !== 'true' && value !== 'false') throw new Error('explicit_boolean_required');
      return value === 'true';
    };
    let content: unknown;
    if (isGroup) {
      const stage = get('execution');
      if (!stage) throw new Error('explicit_execution_required');
      const rules = group?.rules.map((item, index) => converting && !allowedActions.includes(item.action.type)
        ? { ...item, action: json(`group_action_${index}`), deny_message: get(`group_denial_${index}`) } : item) ?? [];
      content = { kind: get('kind'), id: get('id'), number: Number(get('number')), enabled: flag('enabled'),
        execution: stage === 'driver' ? { location: 'driver' } : { location: 'backend', stage }, rules };
    } else {
      content = { kind: group?.kind, rule: { id: get('id'), number: Number(get('number')), enabled: flag('enabled'),
        scope: json('scope'), operations: json('operations'), source: json('source'), target: json('target'),
        action: json('action'), log: flag('log'), deny_message: get('deny_message') } };
    }
    if (isGroup) {
      const value = decodeRuleGroup(content);
      if (editor.type === 'create_group') return { type: 'create_group', group: value };
      if (!group || value.id !== group.id || value.kind !== group.kind) throw new Error('identity_changed');
      return { type: 'replace_group', group_id: group.id, group: value };
    }
    const value = decodeRuleDefinition(content);
    if (!group || value.kind !== group.kind) throw new Error('family_changed');
    if (editor.type === 'create_rule') return { type: 'create_rule', group_id: group.id, rule: value };
    if (!rule || value.rule.id !== rule.id) throw new Error('identity_changed');
    return { type: 'replace_rule', rule_id: rule.id, rule: value };
  }
  function preview(event: FormEvent) {
    event.preventDefault();
    try { setChange(proposed()); setError(null); }
    catch { setError(message('rules.invalidDraft')); }
  }
  const target = change && 'group' in change ? change.group.id : change && 'rule' in change ? change.rule.rule.id : isGroup ? group?.id : rule?.id;
  const deleting = editor.type === 'delete_group' || editor.type === 'delete_rule';
  const toggledEnabled = editor.prepared?.type === 'replace_group' ? editor.prepared.group.enabled
    : editor.prepared?.type === 'replace_rule' ? editor.prepared.rule.rule.enabled : undefined;
  const before = isGroup ? group?.rules ?? [] : rule ? [rule] : [];
  const proposedRule = change && 'rule' in change ? change.rule.rule : null;
  const after = change && ('group' in change ? change.group.rules : 'rule' in change ? [change.rule.rule] : []);
  const identity = <dl className="receipt-fields">
      <div><dt>{t('rules.target')}</dt><dd>{target ?? t('rules.newTarget')}</dd></div>
      {!isGroup && group && <div><dt>{t('rules.group')}</dt><dd>{group.id}</dd></div>}
      <div><dt>{t('rules.version')}</dt><dd><code>{idLabel(editor.base.version)}</code></dd></div>
  </dl>;
  const help = <p id="rule-dialog-help" className="muted">{t('rules.dialogHelp')}</p>;
  return <RuleDialog open={open} titleKey={change && !deleting ? 'rules.confirm' : changeLabels[editor.type]} close={close}>
    {change && <div className="rule-dialog-step">
      <div className="rule-dialog-body stack-tight">
        {help}
        {identity}
      {change.type === 'delete_group' && <p>{t('rules.deleteGroupHelp', { count: group?.rules.length ?? 0 })}</p>}
      {!deleting && <p>{t(toggledEnabled === undefined ? changeLabels[change.type]
        : toggledEnabled ? 'rules.enable' : 'rules.disable')}</p>}
      <h3>{t(deleting ? 'rules.affectedScope' : 'rules.scopePreview')}</h3>
      {!deleting && <p className="muted">{t('rules.scopeBefore')}</p>}
      <ScopeSummary rules={before} />
      {!deleting && after && <>
        <p className="muted">{t('rules.scopeAfter')}</p>
        <ScopeSummary rules={after} />
      </>}
      {proposedRule && group && !editor.prepared && <details className="rule-settings">
        <summary>{t('rules.previewDetails')}</summary>
        <RuleEffect action={proposedRule.action} />
        <RuleDetails group={group} rule={proposedRule} />
      </details>}
      <details className="rule-settings"><summary>{t('rules.diagnostics')}</summary>
        {original && <RawFacts summaryKey="rules.before" value={original} />}
        <RawFacts summaryKey="rules.exactChange" value={change} />
      </details>
      </div>
      <div className="dialog-actions actions">
        <button type="button" onClick={close}>{t('rules.cancel')}</button>
        {!deleting && !editor.prepared && <button type="button" onClick={() => setChange(null)}>{t('rules.back')}</button>}
        <button type="button" className={deleting ? 'danger' : 'primary'}
          onClick={() => { session.editRules(editor.base, change); close(); }}>{t(deleting ? changeLabels[change.type] : 'rules.submit')}</button>
      </div>
    </div>}
    <form ref={form} onSubmit={preview} className="rule-dialog-step" hidden={Boolean(change)}>
      <div className="rule-dialog-body stack-tight">
      {!change && help}
      {identity}
      <div className="stack-tight">
        <div className="rule-form-grid">
          {isGroup && <label>{t('rules.kind')}<select name="kind" value={kind} required disabled={Boolean(group)}
            onChange={event => setKind(event.target.value as RuleKind)}>
            <option value="" disabled>{t('rules.choose')}</option>{ruleKinds.map(value => <option key={value} value={value}>{t(ruleCategories[value])}</option>)}
          </select>{group && <input type="hidden" name="kind" value={group.kind} />}</label>}
          <label>{t('rules.id')}<input name="id" required defaultValue={(isGroup ? group?.id : rule?.id) ?? ''} readOnly={Boolean(original)} /></label>
          <label>{t('rules.number')}<input name="number" type="number" min={1} max={4294967295} step={1} required defaultValue={(isGroup ? group?.number : rule?.number) ?? ''} /></label>
          <StateSelect name="enabled" labelKey="rules.enabled" value={isGroup ? group?.enabled : rule?.enabled} />
          {isGroup && <label>{t('rules.execution')}<select name="execution" required value={execution} onChange={event => setExecution(event.target.value)}>
            <option value="" disabled>{t('rules.choose')}</option>{placements.map(value => <option key={value} value={value}>{t(ruleStages[value])}</option>)}
          </select></label>}
        </div>
        {converting && group && group.rules.some(item => !allowedActions.includes(item.action.type)) && <section className="stack-tight">
          <p>{t('rules.stageActionsTogether')}</p>
          {group.rules.map((item, index) => allowedActions.includes(item.action.type) ? null : <fieldset key={`${execution}:${item.id}`} className="stack-tight">
            <legend>{item.id}</legend>
            <ActionInput name={`group_action_${index}`} allowed={allowedActions} />
            <label>{t('rules.denyMessage')}<textarea name={`group_denial_${index}`} rows={3} defaultValue={item.deny_message ?? ''} /></label>
          </fieldset>)}
        </section>}
        {!isGroup && group && <>
          <p className="muted">{t('rules.explicitHelp')}</p>
          {group && <OperationsInput kind={group.kind} value={rule?.operations} />}
          <ScopeInput value={rule?.scope} />
          <ConditionsInput name="source" titleKey="rules.sourceCondition" fields={actorFields} value={rule?.source} />
          <ConditionsInput name="target" titleKey="rules.targetCondition" fields={targetFields[group.kind]} value={rule?.target} />
          <ActionInput value={rule?.action} allowed={stageActions(placement(group))} />
          <StateSelect name="log" labelKey="rules.log" value={rule?.log} />
          <label>{t('rules.denyMessage')}<textarea name="deny_message" rows={3} defaultValue={rule?.deny_message ?? ''} /></label>
        </>}
      </div>
      {error && <p className="error" role="alert">{t(error.key, error.params)}</p>}
      </div>
      <div className="dialog-actions actions"><button type="button" onClick={close}>{t('rules.cancel')}</button>
        <button type="submit" className="primary">{t('rules.preview')}</button></div>
    </form>
  </RuleDialog>;
}

function WriteReceipt({ state }: { state: RulesState }) {
  const { t } = useI18n();
  const receipt = state.receipt;
  if (!receipt) return null;
  const result = receipt.result;
  const saved = savedFacts(result);
  return <section className="overview-panel stack-tight" aria-label={t('rules.receipt')}>
    <h3>{t('rules.receipt')}</h3>
    <p role="status">{t(result.status === 'failed' ? 'rules.failed' : result.status === 'accepted' ? 'rules.accepted' : 'rules.completed')}</p>

    {result.status === 'failed' && <div className="receipt-failure" role="alert"><p>{t('common.failureCode')}<code>{result.code}</code></p>
      <p>{t('common.originalMessage')}<span>{result.message}</span></p></div>}
    {saved && <dl className="receipt-fields">
      <div><dt>{t('rules.savedVersion')}</dt><dd><code>{idLabel(saved.receipt.saved_version)}</code></dd></div>
      <div><dt>{t('rules.effects')}</dt><dd>{saved.receipt.required_effects.includes('apply_driver') ? t('rules.effectDriver') : t('rules.none')}</dd></div>
      <div><dt>{t('rules.adoption')}</dt><dd>{t(adoptionConfirmed(saved) ? 'rules.adopted' : 'rules.unconfirmed')}</dd></div>
      <div><dt>{t('rules.appliedVersion')}</dt><dd><code>{saved.applied?.rule_version ? idLabel(saved.applied.rule_version) : t('rules.unconfirmed')}</code></dd></div>
    </dl>}
    <RawFacts summaryKey="receipt.details" value={result} />
  </section>;
}

export const RulesView = memo(function RulesView({ rules, access, session, askVisible }: {
  rules: RulesState; access: Access | null; session: ReviewSession; askVisible: boolean;
}) {
  const { t } = useI18n();
  const snapshot = rules.snapshot;
  const [stage, setStage] = useState('');
  const [selected, setSelected] = useState<{ group: string; rule: string | null } | null>(null);
  const detailTitle = useRef<HTMLHeadingElement>(null);
  const detailTrigger = useRef<HTMLButtonElement | null>(null);
  const [editor, setEditor] = useState<Editor | null>(null);
  useEffect(() => { setEditor(null); setSelected(null); detailTrigger.current = null; }, [snapshot]);
  useLayoutEffect(() => {
    if (selected && !document.querySelector('dialog[open]')) {
      const title = detailTitle.current;
      if (title) {
        title.closest('.scroll-pane')?.scrollTo(0, 0);
        title.focus();
      }
    }
  }, [selected]);
  const write = canReview(access) && rules.phase === 'ready' && Boolean(snapshot) && !rules.pending;
  const groups = snapshot?.document.groups.filter(group => !stage || placement(group) === stage) ?? [];
  const selectedGroup = groups.find(group => group.id === selected?.group);
  function clearSelection() {
    setSelected(null);
    detailTrigger.current = null;
  }
  const selectedRule = selectedGroup?.rules.find(rule => rule.id === selected?.rule);
  const open = (type: RuleChange['type'], group: RuleGroup | null, rule: Rule | null = null, prepared?: RuleChange) => {
    if (write && snapshot) setEditor({ type, group, rule, base: snapshot, prepared });
  };
  function toggle(group: RuleGroup, rule: Rule | null) {
    if (rule) open('replace_rule', group, rule, { type: 'replace_rule', rule_id: rule.id,
      rule: definition(group.kind, { ...rule, enabled: !rule.enabled }) });
    else open('replace_group', group, null, { type: 'replace_group', group_id: group.id, group: { ...group, enabled: !group.enabled } });
  }
  return <section id="rules" className="rules-workspace stack" aria-labelledby="rules-title">
    <div className="panel-heading rules-toolbar">
      <div className="actions"><button type="button" disabled={Boolean(rules.pending)} onClick={session.refreshRules}>{t('common.refresh')}</button>
        {canReview(access) && <button type="button" disabled={!write} onClick={() => open('create_group', null)}>{t('rules.createGroup')}</button>}</div>
    </div>
    {access === 'read_only' && <p className="muted">{t('rules.readOnly')}</p>}
    {rules.conflict && <p role="alert" className="error">{t('rules.conflict')}</p>}
    {rules.pending && <p role="status">{t(rules.pending.command === 'edit' ? 'rules.saving' : 'rules.loading')}</p>}
    {rules.problem && <p role="alert" className="error">{t(rules.problem.key, rules.problem.params)}</p>}
    {rules.readFailure && <RawFacts summaryKey="rules.readReply" value={rules.readFailure} />}
    <WriteReceipt state={rules} />
    {snapshot && <>
      <details><summary>{t('rules.snapshot')}</summary><dl className="receipt-fields">
        <div><dt>{t('rules.source')}</dt><dd><code>{snapshot.source.encoding === 'unix_bytes' ? byteText(snapshot.source.value).value
          : snapshot.source.value.map(unit => String.fromCharCode(unit)).join('')}</code></dd></div>
        <div><dt>{t('rules.version')}</dt><dd><code>{idLabel(snapshot.version)}</code></dd></div>
      </dl><p className="overview-note">{t('rules.versionNote')}</p></details>
      <div className="rule-filters"><label>{t('rules.stageFilter')}<select value={stage} onChange={event => { setStage(event.target.value); clearSelection(); }}>
        <option value="">{t('rules.allStages')}</option>{placements.map(value => <option key={value} value={value}>{t(ruleStages[value])}</option>)}</select></label>
      </div>
      <div className="rules-layout"><ScrollPane className="rule-groups" label={t('rules.list')}>
        {ruleKinds.map(kind => {
          const categoryGroups = groups.filter(group => group.kind === kind);
          return <details className="rule-category" data-rule-category={kind} key={kind} onToggle={event => {
            if (!event.currentTarget.open && selectedGroup?.kind === kind) clearSelection();
          }}>
            <summary>
              <strong>{t(ruleCategories[kind])}</strong>
              <span className="rule-branch-meta">{t('rules.groupCount', { count: categoryGroups.length })}</span>
            </summary>
            <div className="rule-category-body">
              {categoryGroups.length === 0 && <p className="muted">{t('rules.empty')}</p>}
              {categoryGroups.map(group => <details key={group.id} className="rule-group" data-rule-group={group.id}
                onToggle={event => {
                  if (!event.currentTarget.open && selected?.group === group.id) clearSelection();
                }}>
                <summary>
                  <span className="rule-group-title">{group.id}</span>
                  <span className="rule-branch-meta">
                    {t(ruleStages[placement(group)])} · {t(group.enabled ? 'feature.enabled' : 'feature.disabled')} · {t('rules.count', { count: group.rules.length })}
                  </span>
                </summary>
                <div className="rule-group-body">
                  <button type="button" className="rule-group-detail" aria-controls="rule-detail"
                    aria-pressed={selected?.group === group.id && selected.rule === null}
                    onClick={event => { detailTrigger.current = event.currentTarget; setSelected({ group: group.id, rule: null }); }}>
                    {t('rules.viewGroup')}
                  </button>
                  <ul className="rule-list">{group.rules.map(rule => <li key={rule.id}>
                    <button type="button" className="rule-list-select" aria-controls="rule-detail"
                      onClick={event => { detailTrigger.current = event.currentTarget; setSelected({ group: group.id, rule: rule.id }); }}
                      aria-pressed={selected?.group === group.id && selected.rule === rule.id}>
                      <strong>{rule.id}</strong>
                      <span>{t(!group.enabled ? 'rules.groupDisabled' : rule.enabled ? 'feature.enabled' : 'feature.disabled')} · {t(ruleActions[rule.action.type])}</span>
                    </button>
                  </li>)}</ul>
                </div>
              </details>)}
            </div>
          </details>;
        })}
      </ScrollPane><ScrollPane as="aside" id="rule-detail" className="rule-detail stack-tight" labelledBy="rule-detail-title">
        <div className="panel-heading"><h3 id="rule-detail-title" ref={detailTitle} tabIndex={-1}>{selectedRule?.id ?? selectedGroup?.id ?? t('rules.detail')}</h3>
          {selectedGroup && <button type="button" onClick={() => {
            setSelected(null);
            focusOutsideDialog(detailTrigger.current?.isConnected ? detailTrigger.current : document.getElementById('rules-title'));
            detailTrigger.current = null;
          }}>{t('processes.returnToList')}</button>}</div>
        {!selectedGroup ? <p className="muted">{t('rules.selectDetail')}</p> : <>
          <p className="rule-meta">
            <span>{t(ruleCategories[selectedGroup.kind])}</span>
            <span>{t(ruleStages[placement(selectedGroup)])}</span>
            <span>{t(!selectedGroup.enabled && selectedRule ? 'rules.groupDisabled'
              : (selectedRule ?? selectedGroup).enabled ? 'feature.enabled' : 'feature.disabled')}</span>
          </p>
          {selectedRule ? <>
            <RuleEffect action={selectedRule.action} />
            {canReview(access) && <div className="actions">
              <button type="button" disabled={!write} onClick={() => open('replace_rule', selectedGroup, selectedRule)}>{t('rules.editRule')}</button>
              <button type="button" disabled={!write} onClick={() => toggle(selectedGroup, selectedRule)}>{t(selectedRule.enabled ? 'rules.disable' : 'rules.enable')}</button>
              <button type="button" className="danger-text" disabled={!write} onClick={() => open('delete_rule', selectedGroup, selectedRule)}>{t('rules.deleteRule')}</button>
            </div>}
            <div key={selectedRule.id} className="stack-tight">
              <RuleDetails group={selectedGroup} rule={selectedRule} />
              <RawFacts summaryKey="rules.diagnostics" value={definition(selectedGroup.kind, selectedRule)} />
            </div>
          </> : <>
            <p>{t('rules.count', { count: selectedGroup.rules.length })}</p>
          {canReview(access) && <div className="actions rule-row-actions">
            <button type="button" disabled={!write} onClick={() => open('replace_group', selectedGroup)}>{t('rules.editGroup')}</button>
            <button type="button" disabled={!write} onClick={() => toggle(selectedGroup, null)}>{t(selectedGroup.enabled ? 'rules.disable' : 'rules.enable')}</button>
            <button type="button" disabled={!write} onClick={() => open('create_rule', selectedGroup)}>{t('rules.createRule')}</button>
            <button type="button" className="danger-text" disabled={!write} onClick={() => open('delete_group', selectedGroup)}>{t('rules.deleteGroup')}</button>
          </div>}
            <RawFacts summaryKey="rules.diagnostics" value={selectedGroup} />
          </>}
        </>}
      </ScrollPane></div>
    </>}
    {editor && editor.base === snapshot && <EditDialog editor={editor} open={!askVisible}
      close={() => setEditor(null)} session={session} />}
  </section>;
});
