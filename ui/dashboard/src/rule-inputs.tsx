import { useState } from 'react';
import { useI18n } from './i18n';
import type { MessageKey } from './messages';
import { factsJson, parseUnique } from './wire-json';
import { fileAccessLabels, matchLabels, ruleActions, terminationLabels, type RuleField } from './rule-fields';
import type { ArgumentMatch, RuleAction, RuleDecision, Scope, TextMatch } from './rules-wire';
import type { ContainerInstance } from './workload-wire';

function PatternInput({ value, change }: { value: TextMatch; change: (value: TextMatch) => void }) {
  const { t } = useI18n();
  return <div className="rule-form-grid">
    <label>{t('rules.matchOperator')}<select value={value.operator} onChange={e => change({ ...value, operator: e.target.value as TextMatch['operator'] })}>
      {Object.entries(matchLabels).map(([operator, label]) => <option key={operator} value={operator}>{t(label)}</option>)}
    </select></label>
    <label>{t('rules.conditionValue')}<input value={value.value} onChange={e => change({ ...value, value: e.target.value })} /></label>
  </div>;
}
function IntegerInput({ labelKey, value, change, max, wide }: {
  labelKey: MessageKey; value: unknown; change: (value: string) => void; max?: number; wide?: boolean;
}) {
  const { t } = useI18n();
  return <label>{t(labelKey)}<input required value={String(value ?? '')} inputMode="numeric"
    type={wide ? 'text' : 'number'} min={0} max={max} step={1} pattern={wide ? '[0-9]+' : undefined}
    onChange={e => change(e.target.value)} /></label>;
}
type ArgumentDraft = Omit<ArgumentMatch, 'index'> & { index: number | string };

function ArgumentsInput({ value, change }: { value: readonly ArgumentDraft[]; change: (value: readonly ArgumentDraft[]) => void }) {
  const { t } = useI18n();
  return <div className="stack-tight">
    {value.map((arg, index) => <div key={index} className="rule-input-item stack-tight">
      <IntegerInput labelKey="rules.argumentIndex" value={arg.index} max={Number.MAX_SAFE_INTEGER} change={text =>
        change(value.map((item, i) => i === index ? { ...item, index: text } : item))} />
      <PatternInput value={arg.pattern} change={pattern => change(value.map((item, i) => i === index ? { ...item, pattern } : item))} />
      <button type="button" onClick={() => change(value.filter((_, i) => i !== index))}>{t('rules.removeArgument', { index: String(arg.index) })}</button>
    </div>)}
    <button type="button" onClick={() => change([...value, { index: 0, pattern: { operator: 'exact', value: '' } }])}>{t('rules.addArgument')}</button>
  </div>;
}
const initialCondition = { match: { operator: 'exact', value: '' }, text: '', number: '', u64: '',
  device: ['', ''], access: '', ports: { first: '', last: '' }, arguments: [], mount_effect: 'independent_view' };

function FieldInput({ field, value, change }: { field: RuleField; value: unknown; change: (value: unknown) => void }) {
  const { t } = useI18n();
  const [key, label, format] = field;
  if (format === 'mount_effect') return <label>{t(label)}<select required value={String(value)} onChange={e => change(e.target.value)}>
    <option value="independent_view">{t('rules.mountIndependentView')}</option>
  </select></label>;
  if (format === 'match') return <PatternInput value={value as TextMatch} change={change} />;
  if (format === 'arguments') return <ArgumentsInput value={value as readonly ArgumentDraft[]} change={change} />;
  if (format === 'ports') {
    const range = value as { first: unknown; last: unknown };
    return <div className="rule-form-grid">
      <IntegerInput labelKey="rules.firstPort" value={range.first} max={65535} change={first => change({ ...range, first })} />
      <IntegerInput labelKey="rules.lastPort" value={range.last} max={65535} change={last => change({ ...range, last })} />
    </div>;
  }
  if (format === 'device') {
    const [major, minor] = value as readonly unknown[];
    return <div className="rule-form-grid">
      <IntegerInput labelKey="rules.deviceMajor" value={major} max={0xffffffff} change={next => change([next, minor])} />
      <IntegerInput labelKey="rules.deviceMinor" value={minor} max={0xffffffff} change={next => change([major, next])} />
    </div>;
  }
  if (format === 'access') return <label>{t(label)}<select required value={String(value)} onChange={e => change(e.target.value)}>
    <option value="" disabled>{t('rules.choose')}</option>
    {Object.entries(fileAccessLabels).map(([mask, key]) => <option value={mask} key={mask}>{t(key)}</option>)}
    {typeof value === 'number' && !(value in fileAccessLabels) && <option value={value}>{t('rules.accessHex', { mask: value.toString(16) })}</option>}
  </select></label>;
  if (format === 'text') return <label>{t('rules.conditionValue')}<input required value={String(value)} onChange={e => change(e.target.value)} /></label>;
  return <IntegerInput labelKey={label} value={value} change={change} wide={format === 'u64'} max={key === 'protocol' ? 255 : 0xffffffff} />;
}

function encodeField(format: RuleField[2], value: unknown): unknown {
  const numeric = (input: unknown): unknown => typeof input === 'string'
    ? /^[0-9]+$/.test(input) ? parseUnique(input.replace(/^0+(?=\d)/, '')) : input : input;
  if (format === 'number' || format === 'u64' || format === 'access') return numeric(value);
  if (format === 'arguments') return (value as readonly ArgumentDraft[]).map(arg => ({ ...arg, index: numeric(arg.index) }));
  if (format === 'device') return (value as readonly unknown[]).map(numeric);
  if (format === 'ports') {
    const { first, last } = value as { first: unknown; last: unknown };
    return { first: numeric(first), last: numeric(last) };
  }
  return value;
}

export function ConditionsInput({ name, titleKey, fields, value }: { name: string; titleKey: MessageKey; fields: readonly RuleField[]; value?: object }) {
  const { t } = useI18n();
  const [values, setValues] = useState<Record<string, unknown>>(value as Record<string, unknown> ?? {});
  const [next, setNext] = useState('');
  const [addingArguments, setAddingArguments] = useState(false);
  const present = fields.filter(([key]) => values[key] != null && (!Array.isArray(values[key]) || (values[key] as unknown[]).length > 0 || (key === 'arguments' && addingArguments)));
  const available = fields.filter(([key]) => !present.some(([existing]) => existing === key));
  const encoded = { ...values };
  for (const [key, , format] of present) encoded[key] = encodeField(format, values[key]);
  return <fieldset className="stack-tight"><legend>{t(titleKey)}</legend>
    <input type="hidden" name={name} value={factsJson(encoded)} />
    {present.length === 0 && <p className="muted">{t('rules.noCondition')}</p>}
    {present.map(field => <div className="rule-input-item stack-tight" key={field[0]}>
      <div className="panel-heading"><strong>{t(field[1])}</strong>
        <button type="button" aria-label={t('rules.removeCondition', { field: t(field[1]) })} onClick={() => {
            if (field[0] === 'arguments') setAddingArguments(false);
            setValues(current => { const next = { ...current }; delete next[field[0]]; return next; });
          }}>{t('rules.removeField')}</button></div>
      <FieldInput field={field} value={values[field[0]]} change={newValue => setValues(current => ({ ...current, [field[0]]: newValue }))} />
    </div>)}
    {available.length > 0 && <div className="rule-add-condition">
      <label>{t('rules.conditionField')}<select value={next} onChange={e => setNext(e.target.value)}>
        <option value="">{t('rules.choose')}</option>
        {available.map(([key, label]) => <option key={key} value={key}>{t(label)}</option>)}
      </select></label>
      <button type="button" disabled={!next} onClick={() => {
        const selected = available.find(([key]) => key === next);
        if (selected?.[2] === 'arguments') setAddingArguments(true);
        if (selected) setValues(current => ({ ...current, [selected[0]]: initialCondition[selected[2]] }));
        setNext('');
      }}>{t('rules.addCondition')}</button>
    </div>}
  </fieldset>;
}

export function ScopeInput({ value }: { value?: Scope }) {
  const { t } = useI18n();
  const [kind, setKind] = useState(value?.kind ?? '');
  const [instances, setInstances] = useState<readonly ContainerInstance[]>(value?.kind === 'containers' ? value.instances : []);
  return <fieldset className="stack-tight"><legend>{t('rules.scope')}</legend>
    <label>{t('rules.scopeKind')}<select required value={kind} onChange={event => setKind(event.target.value as typeof kind)}>
      <option value="" disabled>{t('rules.choose')}</option><option value="all_protected">{t('rules.allProtected')}</option>
      <option value="containers">{t('rules.containers')}</option></select></label>
    {kind === 'containers' && <>
      {instances.map((instance, index) => <fieldset key={index} className="rule-input-item stack-tight">
        <legend>{t('rules.containerAt', { index: index + 1 })}</legend>
        {([['runtime', 'processes.runtime'], ['namespace', 'processes.namespace'], ['id', 'processes.containerId']] as const).map(([field, label]) =>
          <label key={field}>{t(label)}<input required={field !== 'namespace'} value={instance[field] ?? ''}
            aria-describedby={field === 'namespace' ? `rule-namespace-help-${index}` : undefined}
            onChange={e => setInstances(items => items.map((item, i) => i === index
              ? { ...item, [field]: field === 'namespace' && e.target.value === '' ? null : e.target.value } : item))} /></label>)}
        <p className="overview-note" id={`rule-namespace-help-${index}`}>{t('rules.namespaceOptionalHelp')}</p>
        <button type="button" onClick={() => setInstances(items => items.filter((_, i) => i !== index))}>{t('rules.removeContainer', { index: index + 1 })}</button>
      </fieldset>)}
      <button type="button" onClick={() => setInstances(items => [...items, { runtime: '', namespace: null, id: '' }])}>{t('rules.addContainer')}</button>
    </>}
    <input type="hidden" name="scope" value={kind === 'all_protected' ? factsJson({ kind }) : kind === 'containers' && instances.length > 0 ? factsJson({ kind, instances }) : ''} />
  </fieldset>;
}

export function ActionInput({ value, name = 'action', allowed }: { value?: RuleAction; name?: string; allowed?: readonly RuleAction['type'][] }) {
  const { t } = useI18n();
  const [kind, setKind] = useState(value?.type ?? '');
  const [terminate, setTerminate] = useState(value?.type === 'deny' ? value.terminate : '');
  const [timeout, setTimeout] = useState(value?.type === 'ask' ? String(value.timeout_ms) : '');
  const [fallback, setFallback] = useState(value?.type === 'ask' ? value.timeout.type : '');
  const [fallbackTerminate, setFallbackTerminate] = useState(value?.type === 'ask' && value.timeout.type === 'deny' ? value.timeout.terminate : '');
  const termination = (current: string, set: (value: Extract<RuleDecision, { type: 'deny' }>['terminate'] | '') => void) =>
    <label>{t('rules.termination')}<select required value={current} onChange={e => set(e.target.value as Extract<RuleDecision, { type: 'deny' }>['terminate'] | '')}>
      <option value="" disabled>{t('rules.choose')}</option>
      {Object.entries(terminationLabels).map(([key, label]) => <option key={key} value={key}>{t(label)}</option>)}
    </select></label>;
  const encoded = kind === 'allow' || kind === 'to_user' ? factsJson({ type: kind }) : kind === 'deny'
    ? factsJson({ type: kind, terminate }) : kind === 'ask' && /^[1-9][0-9]*$/.test(timeout)
      ? `{"type":"ask","timeout_ms":${timeout},"timeout":${factsJson(fallback === 'deny' ? { type: fallback, terminate: fallbackTerminate } : { type: fallback })}}` : '';
  return <fieldset className="stack-tight"><legend>{t('rules.action')}</legend>
    <label>{t('rules.actionKind')}<select required value={kind} onChange={e => setKind(e.target.value as typeof kind)}>
      <option value="" disabled>{t('rules.choose')}</option>
      {Object.entries(ruleActions).filter(([key]) => !allowed || allowed.includes(key as RuleAction['type']))
        .map(([key, label]) => <option key={key} value={key}>{t(label)}</option>)}
    </select></label>
    {kind === 'deny' && termination(terminate, setTerminate)}
    {kind === 'ask' && <>
      <label>{t('rules.timeout')}<input required inputMode="numeric" pattern="[1-9][0-9]*" value={timeout} onChange={e => setTimeout(e.target.value)} /></label>
      <label>{t('ask.timeoutDefault')}<select required value={fallback} onChange={e => setFallback(e.target.value as typeof fallback)}>
        <option value="" disabled>{t('rules.choose')}</option>
        <option value="allow">{t('rules.actionAllow')}</option><option value="deny">{t('rules.actionDeny')}</option>
      </select></label>
      {fallback === 'deny' && termination(fallbackTerminate, setFallbackTerminate)}
    </>}
    <input type="hidden" name={name} value={encoded} />
  </fieldset>;
}
