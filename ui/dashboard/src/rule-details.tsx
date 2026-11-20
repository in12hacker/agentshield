import { useI18n } from './i18n';
import type { MessageKey } from './messages';
import { DetailField } from './global/detail-field';
import { actorFields, fileAccessLabels, matchLabels, ruleActions, ruleOperation, targetFields, terminationLabels, type RuleField } from './rule-fields';
import type { ArgumentMatch, Rule, RuleAction, RuleGroup, Scope, TextMatch } from './rules-wire';

export function ScopeDetails({ scope }: { scope: Scope }) {
  const { t } = useI18n();
  if (scope.kind === 'all_protected') return <p>{t('rules.allProtected')}</p>;
  return <>
    <p>{t('rules.containerCount', { count: scope.instances.length })}</p>
    <ul className="rule-instances">{scope.instances.map((instance, index) => <li key={index}>
      <dl className="rule-fields">
        <DetailField labelKey="processes.runtime">{instance.runtime}</DetailField>
        <DetailField labelKey="processes.namespace">{instance.namespace ?? t('rules.noNamespace')}</DetailField>
        <DetailField labelKey="processes.containerId"><code>{instance.id}</code></DetailField>
      </dl>
    </li>)}</ul>
  </>;
}

function PatternValue({ pattern }: { pattern: TextMatch }) {
  const { t } = useI18n();
  return <span className="rule-pattern"><span>{t(matchLabels[pattern.operator])}</span>
    <code>{pattern.value === '' ? t('rules.emptyText') : pattern.value}</code></span>;
}

function ConditionValue({ format, value, field }: { format: RuleField[2]; value: unknown; field: string }) {
  const { t } = useI18n();
  if (format === 'mount_effect') return <>{t('rules.mountIndependentView')}</>;
  if (format === 'match') return <PatternValue pattern={value as TextMatch} />;
  if (format === 'arguments') return <ul className="rule-arguments">{(value as readonly ArgumentMatch[]).map((arg, index) =>
    <li key={index}><span>{t('rules.argumentAt', { index: arg.index })}</span> <PatternValue pattern={arg.pattern} /></li>)}</ul>;
  if (format === 'ports') {
    const { first, last } = value as { first: number; last: number };
    return <>{first === last ? first : `${first}–${last}`}</>;
  }
  if (format === 'device') return <code>{(value as readonly number[]).join(':')}</code>;
  if (field === 'protocol' && (value === 6 || value === 17)) return <>{t(value === 6 ? 'rules.protocolTcp' : 'rules.protocolUdp', { number: value })}</>;
  if (format === 'access') {
    const label = fileAccessLabels[value as keyof typeof fileAccessLabels];
    return label ? <>{t(label)}</> : <code>{t('rules.accessHex', { mask: (value as number).toString(16) })}</code>;
  }
  return <code>{String(value)}</code>;
}

function Conditions({ fields, value, titleKey }: { fields: readonly RuleField[]; value: object; titleKey: MessageKey }) {
  const { t } = useI18n();
  const entries = value as Record<string, unknown>;
  const present = fields.filter(([key]) => entries[key] != null && !(Array.isArray(entries[key]) && entries[key].length === 0));
  return <section className="rule-condition">
    <h4>{t(titleKey)}</h4>
    {present.length === 0 ? <p className="muted">{t('rules.noCondition')}</p> : <dl className="rule-fields">
      {present.map(([key, label, format]) => <DetailField key={key} labelKey={label}>
        <ConditionValue format={format} value={entries[key]} field={key} />
      </DetailField>)}
    </dl>}
  </section>;
}

export function RuleEffect({ action }: { action: RuleAction }) {
  const { t } = useI18n();
  return <div className="rule-effect" data-action={action.type}>
    <strong>{t(ruleActions[action.type])}</strong>
    {action.type === 'deny' && action.terminate !== 'none' && <p>{t(terminationLabels[action.terminate])}</p>}
    {action.type === 'ask' && <dl className="rule-fields">
      <DetailField labelKey={'rules.timeout'}>{t('rules.milliseconds', { value: String(action.timeout_ms) })}</DetailField>
      <DetailField labelKey="ask.timeoutDefault">{action.timeout.type === 'allow' ? t('rules.actionAllow') : t(terminationLabels[action.timeout.terminate])}</DetailField>
    </dl>}
  </div>;
}

export function RuleDetails({ group, rule }: { group: RuleGroup; rule: Rule }) {
  const { t } = useI18n();
  return <>
    <ul className="rule-match-flow" aria-label={t('rules.matchFlow')}>
      <li><h4>{t('rules.operations')}</h4><ul className="rule-operation-list">{rule.operations.map((operation, index) =>
        <li key={index}>{t(ruleOperation(group.kind, operation))}</li>)}</ul></li>
      <li><h4>{t('rules.scope')}</h4><ScopeDetails scope={rule.scope} /></li>
      <li><p className="rule-match-note muted">{t('rules.allConditions')}</p><Conditions fields={actorFields} titleKey="rules.sourceCondition" value={rule.source} />
        <Conditions fields={targetFields[group.kind]} titleKey="rules.targetCondition" value={rule.target} /></li>
    </ul>
    <details className="rule-settings"><summary>{t('rules.displaySettings')}</summary>
      <dl className="rule-fields">
        <DetailField labelKey={'rules.number'}>{rule.number}</DetailField>
        <DetailField labelKey={'rules.log'}>{t(rule.log ? 'rules.yes' : 'rules.no')}</DetailField>
        {rule.deny_message && <DetailField labelKey={'rules.denyMessage'}>{rule.deny_message}</DetailField>}
      </dl>
    </details>
  </>;
}
