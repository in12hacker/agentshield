import type { Locator, Page } from '@playwright/test';
import { test, expect, unlock, navigate, disclosures, ok, failed, type Peer } from './fixtures';
import { rulePolicy, savedRule, driverStatus } from '../test-support/contract-supply';
import { actorFields, targetFields, editableRuleOperations, ruleOperation, type RuleField } from '../src/rule-fields';
import { decodeChange, operationNames, ruleKinds, type FileTarget, type PolicySnapshot, type ProcessMatch, type RuleKind } from '../src/rules-wire';
import { parseUnique } from '../src/wire-json';
import type { MessageKey } from '../src/messages';

const policy: PolicySnapshot = { ...rulePolicy, document: { groups: [
  { kind: 'process', id: 'process-group', number: 2, enabled: true, execution: { location: 'backend', stage: 'ask' }, rules: [] },
  { ...rulePolicy.document.groups[0]!, rules: rulePolicy.document.groups[0]!.rules.map(rule => ({ ...rule,
    scope: rule.scope.kind === 'containers' ? { ...rule.scope,
      instances: rule.scope.instances.map(instance => ({ ...instance, namespace: 'original-namespace' })) } : rule.scope,
  })) },
  { kind: 'network', id: 'network-group', number: 3, enabled: true, execution: { location: 'driver' }, rules: [] },
] } };

async function openCreateRule(page: Page, peer: Peer, t: (key: MessageKey) => string, kind: RuleKind) {
  await unlock(page, peer);
  const initial = await peer.next(() => navigate(page, 'rules'), 'message', 'rules');
  expect(initial.body.command).toEqual({ type: 'get' });
  peer.reply(initial, ok({ type: 'rules', snapshot: policy }));
  const category = page.locator(`[data-rule-category=${kind}]`);
  await category.locator(':scope > summary').click();
  await category.locator('.rule-group > summary').click();
  await category.getByRole('button', { name: t('rules.viewGroup'), exact: true }).click();
  await page.getByRole('button', { name: t('rules.createRule'), exact: true }).click();
  const dialog = page.locator('.rule-dialog');
  const options = dialog.locator('.rule-options');
  const editable: readonly string[] = editableRuleOperations[kind];
  await expect(options.locator('input[type=checkbox]')).toHaveCount(editable.length);
  expect(await options.locator('input[type=checkbox]').evaluateAll(inputs => inputs.map(input => (input as HTMLInputElement).value))).toEqual(editable);
  await expect(options.locator('label')).toHaveText(editable.map(operation => t(ruleOperation(kind, operation))));
  for (const operation of operationNames[kind].filter(operation => !editable.includes(operation))) {
    await expect(options.locator(`input[type=checkbox][value="${operation}"]`)).toHaveCount(0);
  }
  return dialog;
}

test('disabled rule/group enable confirmations and unknown write results never replay', async ({ page, peer, t }) => {
  const group = rulePolicy.document.groups[0]!;
  const disabled: PolicySnapshot = { ...rulePolicy, document: { groups: [{ ...group, enabled: false, rules: group.rules.map(rule => ({ ...rule, enabled: false })) }] } };
  await unlock(page, peer);
  const first = await peer.next(() => navigate(page, 'rules'), 'message', 'rules'); peer.reply(first, ok({ type: 'rules', snapshot: disabled }));
  for (const kind of ['process', 'network']) {
    const category = page.locator(`[data-rule-category=${kind}]`);
    await peer.local(() => category.locator(':scope > summary').click());
    await expect(category).toContainText(t('rules.empty'));
    await peer.local(() => category.locator(':scope > summary').click());
  }
  for (const target of ['group', 'rule']) {
    await page.locator('[data-rule-category=file] > summary').click();
    await page.locator('.rule-group > summary').click();
    await page.getByRole('button', { name: t('rules.viewGroup'), exact: true }).click();
    if (target === 'rule') await page.locator('.rule-list-select').click();
    await peer.local(() => page.getByRole('button', { name: t('rules.enable'), exact: true }).click());
    const edit = await peer.next(() => page.locator('.rule-dialog').getByRole('button', { name: t('rules.submit'), exact: true }).click(), 'message', 'rules');
    expect(edit.body.command.change.type).toBe(`replace_${target}`);
    expect(target === 'group' ? edit.body.command.change.group.enabled : edit.body.command.change.rule.rule.enabled).toBe(true);
    peer.diagnostic({ type: 'reply_output_unavailable', id: edit.id, code: 'output_too_large' });
    await expect(page.getByRole('alert')).toContainText(t('rules.sendFailed'));
    await disclosures(page.locator('#rules'), peer);
    const read = await peer.next(() => page.locator('.rules-toolbar').getByRole('button', { name: t('common.refresh'), exact: true }).click(), 'message', 'rules');
    peer.reply(read, ok({ type: 'rules', snapshot: disabled }));
    expect(peer.requests.filter(r => r.body.command?.type === 'edit')).toHaveLength(target === 'group' ? 1 : 2);
  }
  peer.sockets.at(-1)!.close({ code: 1000 }); await expect(page.locator('.gate')).toBeVisible();
  await unlock(page, peer); expect(peer.requests.filter(r => r.body.command?.type === 'edit')).toHaveLength(2);
});

test('rule navigation, group CRUD/toggles, rule CRUD/toggles and original save/adoption/conflict', async ({ page, peer, t }) => {
  await unlock(page, peer);
  const initial = await peer.next(() => navigate(page, 'rules'), 'message', 'rules'); peer.reply(initial, ok({ type: 'rules', snapshot: policy }));
  const button = (key: Parameters<typeof t>[0], scope: Locator = page.locator('#rules')) => scope.getByRole('button', { name: t(key), exact: true });
  const dialog = page.locator('.rule-dialog');
  const selectGroup = async () => {
    const category = page.locator('[data-rule-category=file]');
    if (!await category.getAttribute('open').then(value => value !== null)) await category.locator(':scope > summary').click();
    const group = page.locator('[data-rule-group="saved-decisions"]');
    if (!await group.getAttribute('open').then(value => value !== null)) await group.locator(':scope > summary').click();
    await button('rules.viewGroup', group).click();
  };
  const receipt = async (request: Awaited<ReturnType<typeof peer.next>>, adopted = false) => {
    const saved = adopted ? { ...savedRule, applied: { ...driverStatus.snapshot, rule_version: savedRule.receipt.saved_version } } : savedRule;
    const read = await peer.next(async () => peer.reply(request, ok(saved)), 'message', 'rules');
    expect(read.body.command).toEqual({ type: 'get' }); peer.reply(read, ok({ type: 'rules', snapshot: policy }));
    await expect(page.getByText(t(adopted ? 'rules.adopted' : 'rules.unconfirmed'), { exact: true }).first()).toBeVisible();
  };
  await expect(button('rules.createGroup')).toBeEnabled();
  await peer.local(async () => {
    for (const stage of ['driver', 'remembered', 'fast', 'ask', '']) {
      await page.locator('.rule-filters select').selectOption(stage);
      for (const category of await page.locator('.rule-category').all()) {
        await category.locator(':scope > summary').click();
        if (await category.locator('.rule-group').count() === 0) await expect(category).toContainText(t('rules.empty'));
        else await expect(category.locator('.rule-group').first()).toBeVisible();
        await category.locator(':scope > summary').click();
      }
    }
    await disclosures(page.locator('#rules'), peer);
    await selectGroup(); await expect(page.locator('#rule-detail-title')).toBeFocused();
    await disclosures(page.locator('#rule-detail'), peer);
    await button('processes.returnToList', page.locator('#rule-detail')).click();
    await selectGroup(); await button('rules.editGroup').click(); await expect(dialog.locator('input[name=id]')).toHaveAttribute('readonly', '');
    await button('rules.close', dialog).click();
    await button('rules.createGroup').click(); await dialog.press('Escape');
    await button('rules.createGroup').click();
    await dialog.locator('select[name=kind]').selectOption('network');
    await dialog.locator('input[name=id]').fill('new-explicit-group');
    await dialog.locator('input[name=number]').fill('11');
    await dialog.locator('select[name=enabled]').selectOption('false');
    await dialog.locator('select[name=execution]').selectOption('fast');
    await button('rules.preview', dialog).click(); await button('rules.back', dialog).click();
    await expect(dialog.locator('input[name=id]')).toHaveValue('new-explicit-group');
    await button('rules.preview', dialog).click(); await disclosures(dialog, peer);
  });
  const create = await peer.next(() => button('rules.submit', dialog).click(), 'message', 'rules');
  expect(create.body.command).toEqual({ type: 'edit', expected_version: policy.version, change: { type: 'create_group', group: {
    kind: 'network', id: 'new-explicit-group', number: 11, enabled: false, execution: { location: 'backend', stage: 'fast' }, rules: [],
  } } });
  peer.reply(create, { status: 'accepted', data: savedRule });
  await expect(page.getByText(t('rules.accepted'), { exact: true })).toBeVisible();
  await receipt(create);
  for (const operation of ['edit', 'disable', 'delete'] as const) {
    await selectGroup();
    await peer.local(async () => {
      await button(operation === 'edit' ? 'rules.editGroup' : operation === 'disable' ? 'rules.disable' : 'rules.deleteGroup').click();
      await button('rules.cancel', dialog).filter({ visible: true }).click();
      await button(operation === 'edit' ? 'rules.editGroup' : operation === 'disable' ? 'rules.disable' : 'rules.deleteGroup').click();
      if (operation === 'edit') { await dialog.locator('input[name=number]').fill('7'); await button('rules.preview', dialog).click(); }
    });
    const request = await peer.next(() => button(operation === 'delete' ? 'rules.deleteGroup' : 'rules.submit', dialog).click(), 'message', 'rules');
    expect(request.body.command.expected_version).toEqual(policy.version);
    expect(request.body.command.change.type).toBe(operation === 'delete' ? 'delete_group' : 'replace_group');
    expect(request.body.command.change.group_id).toBe('saved-decisions');
    if (operation === 'disable') expect(request.body.command.change.group.enabled).toBe(false);
    await receipt(request, operation === 'disable');
  }
  for (const operation of ['edit', 'disable', 'delete'] as const) for (const namespace of operation === 'edit' ? [null, '  supplied namespace 🧪  '] : [undefined]) {
    let expectedSource: ProcessMatch | undefined;
    let expectedTarget: FileTarget | undefined;
    await selectGroup(); await page.locator('.rule-list-select').click();
    await disclosures(page.locator('#rule-detail'), peer);
    await peer.local(async () => {
      await button(operation === 'edit' ? 'rules.editRule' : operation === 'disable' ? 'rules.disable' : 'rules.deleteRule').click();
      await button('rules.cancel', dialog).filter({ visible: true }).click();
      await button(operation === 'edit' ? 'rules.editRule' : operation === 'disable' ? 'rules.disable' : 'rules.deleteRule').click();
      if (operation === 'edit') {
        const scope = dialog.locator('fieldset:has(>input[name=scope])');
        const namespaceInput = scope.getByRole('textbox', { name: t('processes.namespace'), exact: true });
        await expect(namespaceInput).toHaveValue('original-namespace');
        await expect(namespaceInput).toHaveAccessibleDescription(t('rules.namespaceOptionalHelp'));
        await namespaceInput.fill(namespace ?? '');
        await expect(namespaceInput).toHaveValue(namespace ?? '');
        const source = dialog.locator('fieldset:has(>input[name=source])');
        const sourceDraft = async () => parseUnique(await source.locator('input[name=source]').inputValue());
        const uid = source.getByRole('spinbutton', { name: t('rules.fieldUid'), exact: true });
        await uid.fill('7');
        expect(await sourceDraft()).toEqual({ ...policy.document.groups[1]!.rules[0]!.source, uid: 7 });
        const { uid: _removedUid, ...retainedSource } = policy.document.groups[1]!.rules[0]!.source;
        await source.getByRole('button', { name: t('rules.removeCondition', { field: t('rules.fieldUid') }), exact: true }).click();
        expect(await sourceDraft()).toEqual(retainedSource);
        await expect(uid).toHaveCount(0);
        await source.locator('.rule-add-condition select').selectOption('uid');
        await button('rules.addCondition', source).click();
        await uid.fill('7');
        expect(await sourceDraft()).toEqual({ ...retainedSource, uid: 7 });
        await source.locator('.rule-add-condition select').selectOption('name');
        await button('rules.addCondition', source).click();
        await uid.fill('11');
        expect(await sourceDraft()).toEqual({ ...policy.document.groups[1]!.rules[0]!.source,
          uid: 11, name: { operator: 'exact', value: '' } });
        await source.getByRole('button', { name: t('rules.removeCondition', { field: t('rules.fieldUid') }), exact: true }).click();
        expectedSource = { ...retainedSource, name: { operator: 'exact', value: '' } };
        expect(await sourceDraft()).toEqual(expectedSource);
        const target = dialog.locator('fieldset:has(>input[name=target])');
        const targetDraft = async () => parseUnique(await target.locator('input[name=target]').inputValue());
        await target.getByRole('spinbutton', { name: t('rules.deviceMajor'), exact: true }).fill('7');
        await target.getByRole('spinbutton', { name: t('rules.deviceMinor'), exact: true }).fill('0');
        await target.getByRole('textbox', { name: t('rules.fieldInode'), exact: true }).fill('18446744073709551614');
        const expected: FileTarget = { ...policy.document.groups[1]!.rules[0]!.target, device: [7, 0], inode: 18446744073709551614n };
        expect(await targetDraft()).toEqual(expected);
        for (const [field, label] of [['access_mask', 'rules.fieldAccess'], ['inode', 'rules.fieldInode'], ['device', 'rules.fieldDevice']] as const) {
          await target.getByRole('button', { name: t('rules.removeCondition', { field: t(label) }), exact: true }).click();
          delete expected[field];
          expect(await targetDraft()).toEqual(expected);
        }
        await target.locator('.rule-add-condition select').selectOption('access_mask');
        await button('rules.addCondition', target).click();
        await target.getByRole('combobox', { name: t('rules.fieldAccess'), exact: true }).selectOption('3');
        await target.locator('.rule-add-condition select').selectOption('new_path');
        await button('rules.addCondition', target).click();
        await target.locator(':scope > .rule-input-item').filter({ hasText: t('rules.fieldNewPath') }).locator('.panel-heading button').click();
        expectedTarget = { path: (policy.document.groups[1]!.rules[0]!.target as FileTarget).path, access_mask: 3 };
        expect(await targetDraft()).toEqual(expectedTarget);
        await dialog.locator('textarea').fill(''); await button('rules.preview', dialog).click();
        await button('rules.back', dialog).click();
        expect(await sourceDraft()).toEqual(expectedSource);
        expect(await targetDraft()).toEqual(expectedTarget);
        await button('rules.preview', dialog).click();
      }
      await disclosures(dialog, peer);
    });
    const request = await peer.next(() => button(operation === 'delete' ? 'rules.deleteRule' : 'rules.submit', dialog).click(), 'message', 'rules');
    expect(request.body.command.change.type).toBe(operation === 'delete' ? 'delete_rule' : 'replace_rule');
    expect(request.body.command.change.rule_id).toBe('saved-open');
    if (operation === 'edit') {
      const change = decodeChange(request.body.command.change);
      expect(change.type).toBe('replace_rule');
      if (change.type !== 'replace_rule') throw new Error('expected typed replace_rule');
      expect(change.rule.rule.source).toEqual(expectedSource);
      expect(change.rule.rule.target).toEqual(expectedTarget);
      expect(request.body.command.expected_version).toEqual(policy.version);
      expect(request.body.command.change.rule.rule.deny_message).toBe('');
      expect(request.body.command.change.rule.rule.target.access_mask).toBe(3);
      expect(request.body.command.change.rule.rule.scope).toEqual({ kind: 'containers', instances: [{
        runtime: 'docker', namespace, id: 'full-container-object-id',
      }] });
    }
    if (operation === 'disable') expect(request.body.command.change.rule.rule.enabled).toBe(false);
    if (operation === 'delete') {
      const read = await peer.next(async () => peer.reply(request, failed('conflict', 'original-version-conflict')), 'message', 'rules');
      peer.reply(read, ok({ type: 'rules', snapshot: policy }));
      await expect(page.getByRole('alert').filter({ hasText: t('rules.conflict') })).toBeVisible();
      await disclosures(page.locator('#rules'), peer);
    } else await receipt(request);
  }
  const refresh = await peer.next(() => button('common.refresh').click(), 'message', 'rules'); expect(refresh.body.command.type).toBe('get');
  peer.reply(refresh, ok({ type: 'rules', snapshot: policy }));
});

for (const kind of ruleKinds) test(`minimal legal ${kind} rule: eligible operation, group action and receipt`, async ({ page, peer, t }) => {
  const dialog = await openCreateRule(page, peer, t, kind);
  const operation = editableRuleOperations[kind][0];
  const action = dialog.locator('fieldset:has(>input[name=action])');
  await peer.local(async () => {
    await dialog.locator('input[name=id]').fill(`minimal-${kind}`);
    await dialog.locator('input[name=number]').fill('2');
    await dialog.locator('select[name=enabled]').selectOption('true');
    await dialog.locator(`.rule-options input[value="${operation}"]`).check();
    await dialog.locator('fieldset:has(>input[name=scope]) select').selectOption('all_protected');
    await action.locator('select').first().selectOption(kind === 'process' ? 'ask' : kind === 'file' ? 'deny' : 'to_user');
    if (kind === 'process') {
      await action.locator('input:not([type=hidden])').fill('1000');
      await action.locator('select').nth(1).selectOption('deny');
      await action.locator('select').nth(2).selectOption('none');
    } else if (kind === 'file') await action.locator('select').nth(1).selectOption('none');
    await dialog.locator('select[name=log]').selectOption('false');
    await dialog.getByRole('button', { name: t('rules.preview'), exact: true }).click();
  });
  const request = await peer.next(() => dialog.getByRole('button', { name: t('rules.submit'), exact: true }).click(), 'message', 'rules');
  expect(request.body.command).toEqual({ type: 'edit', expected_version: policy.version, change: { type: 'create_rule',
    group_id: policy.document.groups.find(group => group.kind === kind)!.id,
    rule: { kind, rule: { id: `minimal-${kind}`, number: 2, enabled: true, operations: [operation],
      scope: { kind: 'all_protected' }, source: {}, target: {}, log: false, deny_message: '',
      action: kind === 'process' ? { type: 'ask', timeout_ms: 1000, timeout: { type: 'deny', terminate: 'none' } }
        : kind === 'file' ? { type: 'deny', terminate: 'none' } : { type: 'to_user' },
    } },
  } });
  const read = await peer.next(async () => peer.reply(request, ok(savedRule)), 'message', 'rules');
  peer.reply(read, ok({ type: 'rules', snapshot: policy }));
  const receipt = page.getByRole('region', { name: t('rules.receipt'), exact: true });
  await expect(receipt).toContainText(t('rules.completed'));
  await expect(receipt).toContainText(t('rules.savedVersion'));
  await expect(receipt).toContainText(t('rules.unconfirmed'));
});

for (const kind of ruleKinds) test(`create ${kind} rule: every dynamic condition/input, operation, scope and action`, async ({ page, peer, t }) => {
  const dialog = await openCreateRule(page, peer, t, kind);
  const group = policy.document.groups.find(group => group.kind === kind)!;
  const actions = group.execution.location === 'driver' ? ['allow', 'deny', 'to_user']
    : group.execution.stage === 'ask' ? ['ask'] : ['allow', 'deny'];
  const button = (key: Parameters<typeof t>[0], scope = dialog) => scope.getByRole('button', { name: t(key), exact: true });
  const operationLabels = await dialog.locator('.rule-options input[type=checkbox]').evaluateAll(inputs => inputs.map(input => (input as HTMLInputElement).labels?.[0]?.textContent?.trim()));
  expect(new Set(operationLabels).size, 'different operation identities need distinguishable labels').toBe(operationLabels.length);
  const expected: Record<string, Record<string, unknown>> = { source: {}, target: {} };
  async function conditionFields(name: string, fields: readonly RuleField[]) {
    const area = dialog.locator(`fieldset:has(> input[name=${name}])`);
    for (const [key, , format] of fields) {
      // Add/remove then add again exercises both dynamic controls without a backend write.
      await area.locator('.rule-add-condition select').selectOption(key); await button('rules.addCondition', area).click();
      let item = area.locator(':scope > .rule-input-item').last();
      await item.locator('.panel-heading button').click();
      await area.locator('.rule-add-condition select').selectOption(key); await button('rules.addCondition', area).click();
      item = area.locator(':scope > .rule-input-item').last();
      if (format === 'match') {
        for (const operator of ['exact', 'prefix', 'glob', 'regex']) await item.locator('select').selectOption(operator);
        await item.locator('input').fill('explicit-draft');
        await item.locator('input').fill(''); expected[name]![key] = { operator: 'regex', value: '' };
      } else if (format === 'arguments') {
        await button('rules.addArgument', item).click();
        await item.locator('input[type=number]').fill('0'); await item.locator('select').selectOption('prefix');
        await item.locator('input:not([type=number])').fill('first');
        await item.getByRole('button', { name: t('rules.removeArgument', { index: '0' }), exact: true }).click();
        await button('rules.addArgument', item).click(); await item.locator('input[type=number]').fill('1');
        await item.locator('select').selectOption('exact'); await item.locator('input:not([type=number])').fill('second');
        await item.getByRole('button', { name: t('rules.removeArgument', { index: '1' }), exact: true }).click();
        await button('rules.addArgument', item).click(); await item.locator('input[type=number]').fill('1');
        await item.locator('input:not([type=number])').fill('preserved-empty-next');
        await item.locator('input:not([type=number])').fill('');
        expected[name]![key] = [{ index: 1, pattern: { operator: 'exact', value: '' } }];
      } else if (format === 'device' || format === 'ports') {
        await item.locator('input').nth(0).fill('0'); await item.locator('input').nth(1).fill(format === 'ports' ? '65535' : '1');
        expected[name]![key] = format === 'ports' ? { first: 0, last: 65535 } : [0, 1];
      } else if (format === 'access') {
        for (const mask of ['1', '2', '3']) await item.locator('select').selectOption(mask); expected[name]![key] = 3;
      } else if (format === 'mount_effect') {
        expect(await item.locator('select option').evaluateAll(options =>
          options.map(option => (option as HTMLOptionElement).value))).toEqual(['independent_view']);
        await item.locator('select').selectOption('independent_view');
        expected[name]![key] = 'independent_view';
      } else {
        const value = format === 'u64' ? '18446744073709551615' : format === 'text' ? '127.0.0.0/8' : key === 'protocol' ? '6' : '0';
        await item.locator('input').fill(value);
        expected[name]![key] = format === 'text' ? value : format === 'u64' ? BigInt(value) : Number(value);
      }
    }
  }
  const long = '固定拒绝 original denial '.repeat(50);
  await peer.local(async () => {
    await dialog.locator('input[name=id]').fill(`new-${kind}`); await dialog.locator('input[name=number]').fill('12');
    await dialog.locator('select[name=enabled]').selectOption('true');
    for (const check of await dialog.locator('.rule-options input[type=checkbox]').all()) { await check.check(); await check.uncheck(); await check.check(); }
    const scope = dialog.locator('fieldset:has(> input[name=scope])');
    await scope.locator('select').selectOption('containers'); await button('rules.addContainer', scope).click();
    await scope.getByRole('textbox').nth(0).fill('docker');
    await scope.getByRole('textbox').nth(1).fill('temporary-namespace');
    await scope.getByRole('textbox').nth(1).fill('');
    await expect(scope.getByRole('textbox').nth(1)).toHaveAccessibleDescription(t('rules.namespaceOptionalHelp'));
    await scope.getByRole('textbox').nth(2).fill('complete-instance');
    await button('rules.addContainer', scope).click();
    const second = scope.locator(':scope > fieldset').last();
    await second.getByRole('textbox').nth(0).fill('docker');
    await second.getByRole('textbox').nth(1).fill('second-namespace');
    await second.getByRole('textbox').nth(2).fill('second-instance');
    await scope.getByRole('button', { name: t('rules.removeContainer', { index: 2 }), exact: true }).click();
    await scope.getByRole('button', { name: t('rules.removeContainer', { index: 1 }), exact: true }).click();
    await scope.locator('select').selectOption('all_protected');
    if (kind === 'file') {
      await scope.locator('select').selectOption('containers'); await button('rules.addContainer', scope).click();
      await scope.getByRole('textbox').nth(0).fill('docker');
      await scope.getByRole('textbox').nth(2).fill('complete-instance');
    }
    await conditionFields('source', actorFields); await conditionFields('target', targetFields[kind]);
    const action = dialog.locator('fieldset:has(> input[name=action])');
    expect(await action.locator('select').first().locator('option').evaluateAll(options =>
      options.map(option => (option as HTMLOptionElement).value))).toEqual(['', ...actions]);
    for (const value of actions) {
      await action.locator('select').first().selectOption(value);
      if (value === 'deny') for (const termination of ['actor', 'actor_and_parent', 'none']) await action.locator('select').nth(1).selectOption(termination);
    }
    if (actions.includes('ask')) {
      await action.locator('input:not([type=hidden])').fill('5000');
      await action.locator('select').nth(1).selectOption('allow'); await action.locator('select').nth(1).selectOption('deny');
      await action.locator('select').nth(2).selectOption('actor_and_parent');
    }
    await dialog.locator('select[name=log]').selectOption('false'); await dialog.locator('textarea').fill(long);
    await button('rules.preview').click(); await disclosures(dialog, peer);
    await button('rules.back').click(); await expect(dialog.locator('textarea')).toHaveValue(long);
    await button('rules.preview').click();
  });
  const request = await peer.next(() => button('rules.submit').click(), 'message', 'rules');
  expect(request.body.command).toEqual({ type: 'edit', expected_version: policy.version, change: { type: 'create_rule', group_id: group.id,
    rule: { kind, rule: { id: `new-${kind}`, number: 12, enabled: true, operations: editableRuleOperations[kind], scope: kind === 'file'
      ? { kind: 'containers', instances: [{ runtime: 'docker', namespace: null, id: 'complete-instance' }] } : { kind: 'all_protected' },
      source: expected.source, target: expected.target, action: kind === 'process'
        ? { type: 'ask', timeout_ms: 5000, timeout: { type: 'deny', terminate: 'actor_and_parent' } }
        : kind === 'network' ? { type: 'to_user' } : { type: 'deny', terminate: 'none' }, log: false, deny_message: long } } } });
  // This is a transport/dynamic-control draft, not a policy accepted by the backend compiler.
  const code = 'invalid';
  const reason = kind === 'file' ? 'new_path requires rename-only; access_mask requires open-only'
    : 'Synthetic backend rejection of combined conditions; the action matches the selected group';
  const read = await peer.next(async () => peer.reply(request, failed(code, reason)), 'message', 'rules');
  expect(read.body.command).toEqual({ type: 'get' });
  peer.reply(read, ok({ type: 'rules', snapshot: policy }));
  const receipt = page.getByRole('region', { name: t('rules.receipt'), exact: true });
  await expect(receipt).toContainText(t('rules.failed'));
  await expect(receipt).toContainText(code);
  await expect(receipt).toContainText(reason);
  await expect(receipt).not.toContainText(t('rules.savedVersion'));
  await disclosures(receipt, peer);
  await navigate(page, 'overview');
  const reread = await peer.next(() => navigate(page, 'rules'), 'message', 'rules');
  peer.reply(reread, ok({ type: 'rules', snapshot: policy }));
  expect(peer.requests.filter(r => r.body.type === 'rules' && r.body.command?.type === 'edit')).toHaveLength(1);
});
