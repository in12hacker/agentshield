import { navigate, requireLive, same, type Exchange, type LiveObservation } from './fixtures';
import { decodeChange, definition, type FileTarget, type PolicySnapshot, type ProcessMatch, type RuleChange, type RuleDefinition, type RuleGroup } from '../../src/rules-wire';
import { parseUnique } from '../../src/wire-json';

export const reviewGroupId = 'controls-live-file-review';
export const reviewRuleId = 'controls-live-file-open';

export async function openReviewRules(live: LiveObservation) {
  return live.exchange(async () => {
    if (await live.page.locator('#rules').isVisible()) await live.page.locator('.rules-toolbar').getByRole('button', { name: live.t('common.refresh'), exact: true }).click();
    else await navigate(live.page, 'rules');
  }, 'message', 'rules');
}

export async function selectReviewRule(live: LiveObservation, rule = true) {
  const category = live.page.locator('[data-rule-category=file]');
  if (await category.getAttribute('open') === null) await category.locator(':scope > summary').click();
  const branch = live.page.locator(`[data-rule-group="${reviewGroupId}"]`);
  if (await branch.getAttribute('open') === null) await branch.locator(':scope > summary').click();
  await branch.getByRole('button', { name: live.t('rules.viewGroup'), exact: true }).click();
  if (rule) await branch.locator('.rule-list-select').click();
}

export async function readReviewPolicy(live: LiveObservation) {
  const result = await live.guest({ op: 'request', kind: 'rules', command: { type: 'get' } });
  live.preserve('independent-policy-replies', result);
  return result.snapshot as PolicySnapshot;
}

export async function adoptedReviewChange(live: LiveObservation, exchange: Exchange, before: PolicySnapshot, change: RuleChange, name: string) {
  const actual = await readReviewPolicy(live);
  const sent = exchange.request.frame.message.command;
  requireLive(sent.type === 'edit' && same(sent.expected_version, before.version) && same(decodeChange(sent.change), change), 'RULE_ORIGINAL_CHANGE_OR_VERSION_DIFFERS');
  const result = live.result(exchange);
  requireLive(result.status === 'completed' && result.data.type === 'rules_saved', 'UI_RULE_SAVE_NOT_COMPLETED');
  requireLive(result.data.applied !== null && same(result.data.receipt.saved_version, result.data.applied.rule_version), 'UI_RULE_NOT_ACTUALLY_ADOPTED');
  requireLive(same(actual.version, result.data.receipt.saved_version), 'INDEPENDENT_RULE_READBACK_DIFFERS');
  live.record(name, 'PASS', { ...live.refs(exchange), expected_version: sent.expected_version, change,
    saved_version: result.data.receipt.saved_version, applied_version: result.data.applied.rule_version, independent_readback: true });
  return actual;
}

export async function exerciseReviewToggles(live: LiveObservation) {
  const initial = await openReviewRules(live);
  let before = live.result(initial).data.snapshot as PolicySnapshot;
  const page = live.page, dialog = page.locator('dialog.rule-dialog');
  for (const kind of ['group', 'rule'] as const) for (const enabled of [false, true]) {
    const group = before.document.groups.find(group => group.id === reviewGroupId);
    requireLive(group?.kind === 'file' && group.rules.length === 1 && group.rules[0]?.id === reviewRuleId, 'OWNED_TOGGLE_RULE_MISSING');
    const rule = group.rules[0]!;
    const replacement = kind === 'group' ? { ...group, enabled } : { ...group, rules: [{ ...rule, enabled }] };
    const change: RuleChange = kind === 'group' ? { type: 'replace_group', group_id: reviewGroupId, group: replacement }
      : { type: 'replace_rule', rule_id: reviewRuleId, rule: definition(group.kind, { ...rule, enabled }) };
    await selectReviewRule(live, kind === 'rule');
    const toggle = () => page.locator('#rule-detail').getByRole('button', { name: live.t(enabled ? 'rules.enable' : 'rules.disable'), exact: true }).click();
    await live.local(toggle);
    await live.local(() => dialog.getByRole('button', { name: live.t('rules.cancel'), exact: true }).filter({ visible: true }).click());
    await live.local(toggle);
    const edit = await live.exchange(() => dialog.getByRole('button', { name: live.t('rules.submit'), exact: true }).click(), 'message', 'rules');
    const actual = await adoptedReviewChange(live, edit, before, change, `rules_${kind}_${enabled ? 'enable' : 'disable'}`);
    requireLive(same(actual.document, { groups: before.document.groups.map(group => group.id === reviewGroupId ? replacement : group) }), 'TOGGLE_CHANGED_UNRELATED_POLICY');
    before = actual;
  }
}

export function verifyReviewPolicy(actual: PolicySnapshot, original: PolicySnapshot, resource: string) {
  requireLive(same(actual.document.groups.filter(g => g.id !== reviewGroupId), original.document.groups), 'ORIGINAL_POLICY_GROUPS_OR_ORDER_CHANGED');
  const group = actual.document.groups.find(g => g.id === reviewGroupId);
  requireLive(group?.kind === 'file' && group.enabled && group.number === 20 && same(group.execution, { location: 'backend', stage: 'ask' }) && group.rules.length === 1, 'OWNED_GROUP_NOT_EFFECTIVE');
  const rule = group.rules[0]!;
  requireLive(rule.id === reviewRuleId && rule.number === 1 && rule.enabled && same(rule.operations, ['open'])
    && same(rule.scope, { kind: 'all_protected' }) && same(rule.action, { type: 'ask', timeout_ms: 20000, timeout: { type: 'deny', terminate: 'none' } }) && rule.log, 'OWNED_BUSINESS_RULE_DIFFERS');
  requireLive(Object.values(rule.source).every(value => value === null || Array.isArray(value) && value.length === 0), 'WORKLOAD_SOURCE_WAS_NARROWED');
  const target = rule.target as FileTarget;
  requireLive(same(target.path, { operator: 'prefix', value: resource }) && target.access_mask === 2
    && [target.new_path, target.device, target.inode].every(value => value === undefined || value === null), 'WRITE_MASK_OR_RESOURCE_DIFFERS');
  return group;
}

export async function createReviewPolicy(live: LiveObservation, original: PolicySnapshot, resource: string, verified: () => void) {
  const page = live.page;
  const initial = await openReviewRules(live);
  const before = live.result(initial).data.snapshot as PolicySnapshot;
  live.preserve('review-policy-before', before);
  requireLive(same(before.document, original.document), 'PRIOR_ORIGINAL_POLICY_NOT_RESTORED');
  requireLive(!before.document.groups.some(g => g.id === reviewGroupId || g.rules.some(r => r.id === reviewRuleId)), 'OWNED_REVIEW_RULE_ALREADY_EXISTS_NO_REPLAY');
  verified();
  const dialog = page.locator('dialog.rule-dialog');
  await page.getByRole('button', { name: live.t('rules.createGroup'), exact: true }).click();
  await dialog.locator('[name=kind]').selectOption('file');
  await dialog.locator('[name=id]').fill(reviewGroupId);
  await dialog.locator('[name=number]').fill('20');
  await dialog.locator('[name=enabled]').selectOption('true');
  await dialog.locator('[name=execution]').selectOption('ask');
  await dialog.getByRole('button', { name: live.t('rules.preview'), exact: true }).click();
  const group: RuleGroup = { kind: 'file', id: reviewGroupId, number: 20, enabled: true, execution: { location: 'backend', stage: 'ask' }, rules: [] };
  const created = await live.exchange(() => dialog.getByRole('button', { name: live.t('rules.submit'), exact: true }).click(), 'message', 'rules');
  const grouped = await adoptedReviewChange(live, created, before, { type: 'create_group', group }, 'rules_create_group_save_adopt_readback');
  requireLive(same(grouped.document, { groups: [...original.document.groups, group] }), 'GROUP_CREATE_CHANGED_ORIGINAL_POLICY');
  await selectReviewRule(live, false);
  await page.getByRole('button', { name: live.t('rules.createRule'), exact: true }).click();
  await dialog.locator('input[name=id]').fill(reviewRuleId);
  await dialog.locator('input[name=number]').fill('1');
  await dialog.locator('select[name=enabled]').selectOption('true');
  await dialog.locator('.rule-options input[value=open]').check();
  await dialog.locator('fieldset:has(>input[name=scope]) select').selectOption('all_protected');
  const target = dialog.locator('fieldset:has(>input[name=target])');
  for (const field of ['path', 'access_mask']) {
    await target.locator('.rule-add-condition select').selectOption(field);
    await target.getByRole('button', { name: live.t('rules.addCondition'), exact: true }).click();
  }
  const path = target.locator(':scope > .rule-input-item').first();
  await path.locator('select').selectOption('prefix');
  await path.locator('input').fill(resource);
  await target.getByRole('combobox', { name: live.t('rules.fieldAccess'), exact: true }).selectOption('2');
  const action = dialog.locator('fieldset:has(>input[name=action])');
  await action.locator('select').first().selectOption('ask');
  await action.locator('input:not([type=hidden])').fill('20000');
  await action.locator('select').nth(1).selectOption('deny');
  await action.locator('select').nth(2).selectOption('none');
  await dialog.locator('select[name=log]').selectOption('true');
  const rule: RuleDefinition = { kind: 'file', rule: { id: reviewRuleId, number: 1, enabled: true,
    operations: ['open'], scope: { kind: 'all_protected' }, source: {}, target: { path: { operator: 'prefix', value: resource }, access_mask: 2 },
    action: { type: 'ask', timeout_ms: 20000, timeout: { type: 'deny', terminate: 'none' } }, log: true, deny_message: '' } };
  await dialog.getByRole('button', { name: live.t('rules.preview'), exact: true }).click();
  const saved = await live.exchange(() => dialog.getByRole('button', { name: live.t('rules.submit'), exact: true }).click(), 'message', 'rules');
  const actual = await adoptedReviewChange(live, saved, grouped, { type: 'create_rule', group_id: reviewGroupId, rule }, 'rules_create_write_open_save_adopt_readback');
  verifyReviewPolicy(actual, original, resource);
  live.preserve('review-policy-adopted', actual);
}

export async function exerciseReviewDrafts(live: LiveObservation) {
  const original = await openReviewRules(live);
  const snapshot = live.result(original).data.snapshot as PolicySnapshot;
  await selectReviewRule(live);
  const dialog = live.page.locator('dialog.rule-dialog');
  await live.local(() => live.page.getByRole('button', { name: live.t('rules.editRule'), exact: true }).click());
  const source = dialog.locator('fieldset:has(>input[name=source])'), target = dialog.locator('fieldset:has(>input[name=target])');
  const sourceDraft = async () => parseUnique(await source.locator('input[name=source]').inputValue()) as ProcessMatch;
  const targetDraft = async () => parseUnique(await target.locator('input[name=target]').inputValue()) as FileTarget;
  const add = async (name: 'source' | 'target', field: string) => {
    const area = name === 'source' ? source : target;
    const draft = name === 'source' ? await sourceDraft() : await targetDraft();
    if ((draft as Record<string, unknown>)[field] == null) {
      await live.local(() => area.locator('.rule-add-condition select').selectOption(field));
      await live.local(() => area.getByRole('button', { name: live.t('rules.addCondition'), exact: true }).click());
    }
  };
  for (const [index, value] of ['7', '11'].entries()) {
    await add('source', 'uid');
    if (index === 1) await add('source', 'name');
    const before = await sourceDraft(), uid = source.getByRole('spinbutton', { name: live.t('rules.fieldUid'), exact: true });
    const control = await live.describe(uid);
    await live.local(() => uid.fill(value));
    requireLive(same(await sourceDraft(), { ...before, uid: Number(value) }), 'UID_DRAFT_INPUT_DIFFERS');
    const { uid: _uid, ...retained } = before;
    await live.local(() => source.getByRole('button', { name: live.t('rules.removeCondition', { field: live.t('rules.fieldUid') }), exact: true }).click());
    requireLive(same(await sourceDraft(), retained), 'UID_REMOVAL_CHANGED_OTHER_FIELDS');
    live.record('rule_conditions_uid', 'PASS', { ...live.refs(original), control, layout_iteration: index, local: true });
  }
  for (const field of ['device', 'inode', 'access_mask']) await add('target', field);
  const expected: FileTarget = { ...await targetDraft(), device: [7, 0], inode: 18446744073709551614n, access_mask: 3 };
  await live.local(() => target.getByRole('spinbutton', { name: live.t('rules.deviceMajor'), exact: true }).fill('7'));
  await live.local(() => target.getByRole('spinbutton', { name: live.t('rules.deviceMinor'), exact: true }).fill('0'));
  await live.local(() => target.getByRole('textbox', { name: live.t('rules.fieldInode'), exact: true }).fill('18446744073709551614'));
  await live.local(() => target.getByRole('combobox', { name: live.t('rules.fieldAccess'), exact: true }).selectOption('3'));
  requireLive(same(await targetDraft(), expected), 'FILE_CONDITION_DRAFT_DIFFERS');
  for (const [field, label] of [['access_mask', 'rules.fieldAccess'], ['inode', 'rules.fieldInode'], ['device', 'rules.fieldDevice']] as const) {
    const remove = target.getByRole('button', { name: live.t('rules.removeCondition', { field: live.t(label) }), exact: true });
    const control = await live.describe(remove);
    await live.local(() => remove.click());
    delete expected[field];
    requireLive(same(await targetDraft(), expected), 'FILE_CONDITION_REMOVAL_CHANGED_OTHER_FIELDS');
    live.record(`rule_conditions_${field}`, 'PASS', { ...live.refs(original), control, local: true });
  }
  const retainedSource = await sourceDraft();
  await live.local(() => dialog.getByRole('button', { name: live.t('rules.preview'), exact: true }).click());
  await live.local(() => dialog.getByRole('button', { name: live.t('rules.back'), exact: true }).click());
  requireLive(same(await sourceDraft(), retainedSource) && same(await targetDraft(), expected), 'RULE_PREVIEW_BACK_CHANGED_DRAFT');
  await live.local(() => dialog.getByRole('button', { name: live.t('rules.cancel'), exact: true }).click());
  requireLive(same(await readReviewPolicy(live), snapshot), 'LOCAL_RULE_DRAFT_CHANGED_POLICY');
  live.record('rule_conditions_preview_back_cancel', 'PASS', { ...live.refs(original), independent_snapshot_unchanged: true });
}

export async function replaceAndRestoreReviewNumber(live: LiveObservation, original: PolicySnapshot, resource: string) {
  for (const number of [2, 1]) {
    const ui = await openReviewRules(live);
    const before = live.result(ui).data.snapshot as PolicySnapshot;
    const prior = before.document.groups.find(g => g.id === reviewGroupId)!.rules[0]!;
    const replacement: RuleDefinition = { kind: 'file', rule: { ...prior, number } } as RuleDefinition;
    await selectReviewRule(live);
    await live.page.getByRole('button', { name: live.t('rules.editRule'), exact: true }).click();
    const dialog = live.page.locator('dialog.rule-dialog');
    await dialog.locator('[name=number]').fill(String(number));
    await dialog.getByRole('button', { name: live.t('rules.preview'), exact: true }).click();
    const saved = await live.exchange(() => dialog.getByRole('button', { name: live.t('rules.submit'), exact: true }).click(), 'message', 'rules');
    const actual = await adoptedReviewChange(live, saved, before, { type: 'replace_rule', rule_id: reviewRuleId, rule: replacement }, `rules_replace_number_${number}`);
    requireLive(same(actual.document, { groups: before.document.groups.map(g => g.id === reviewGroupId ? { ...g, rules: [replacement.rule] } : g) }), 'REPLACE_CHANGED_OTHER_POLICY_FIELDS');
  }
  const actual = await readReviewPolicy(live);
  verifyReviewPolicy(actual, original, resource);
  live.preserve('business-policy-reverified', actual);
}

export async function restoreReviewPolicy(live: LiveObservation, original: any) {
  const page = live.page;
  const dialog = page.locator('dialog.rule-dialog');
  if (await dialog.isVisible()) await dialog.press('Escape');
  await openReviewRules(live);
  let current = await live.guest({ op: 'request', kind: 'rules', command: { type: 'get' } });
  let group = current.snapshot.document.groups.find((g: any) => g.id === reviewGroupId);
  if (!group) {
    requireLive(same(current.snapshot.document, original.document), 'POLICY_DIFFERS_WITHOUT_OWNED_GROUP');
    live.record('original_policy_document_already_restored', 'PASS', { current_version: current.snapshot.version });
    return;
  }
  for (const kind of [...(group.rules.length ? ['rule' as const] : []), 'group' as const]) {
    requireLive(group.rules.length <= 1 && (!group.rules.length || group.rules[0].id === reviewRuleId), 'OWNED_GROUP_HAS_UNEXPECTED_RULES');
    await selectReviewRule(live, kind === 'rule');
    const key = kind === 'rule' ? 'rules.deleteRule' : 'rules.deleteGroup';
    await page.locator('#rule-detail').getByRole('button', { name: live.t(key), exact: true }).click();
    const dialog = page.locator('dialog.rule-dialog');
    await live.local(() => dialog.getByRole('button', { name: live.t('rules.cancel'), exact: true }).filter({ visible: true }).click());
    await page.locator('#rule-detail').getByRole('button', { name: live.t(key), exact: true }).click();
    const deleted = await live.exchange(() => dialog.getByRole('button', { name: live.t(key), exact: true }).filter({ visible: true }).click(), 'message', 'rules');
    const change: RuleChange = kind === 'rule' ? { type: 'delete_rule', rule_id: reviewRuleId } : { type: 'delete_group', group_id: reviewGroupId };
    current = { snapshot: await adoptedReviewChange(live, deleted, current.snapshot, change, `rules_delete_owned_${kind}`) };
    group = current.snapshot.document.groups.find((g: any) => g.id === reviewGroupId);
  }
  requireLive(same(current.snapshot.document, original.document), 'ORIGINAL_POLICY_DOCUMENT_NOT_RESTORED');
  live.preserve('original-policy-restored', current.snapshot);
  live.record('original_policy_document_restored', 'PASS', { current_version: current.snapshot.version, original_version: original.version, exact_document_equal: true });
}
