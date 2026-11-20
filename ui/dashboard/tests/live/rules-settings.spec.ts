import { test, expect, navigate, requireLive, same } from './fixtures';
import { decodeChange } from '../../src/rules-wire';
import { parseUnique } from '../../src/wire-json';
import { readFileSync } from 'node:fs';
import { createReviewPolicy, exerciseReviewDrafts, exerciseReviewToggles, readReviewPolicy, restoreReviewPolicy,
  reviewGroupId as groupId, selectReviewRule, verifyReviewPolicy } from './review-policy';

test('live owned group and rule toggles save adopt and restore', async ({ live }) => {
  await live.safe('owned_group_and_rule_toggle_restore', async () => {
    requireLive(process.env.AS_LIVE_BUSINESS_CONTEXT, 'CURRENT_RESOURCE_CONTEXT_REQUIRED');
    const context = parseUnique(readFileSync(process.env.AS_LIVE_BUSINESS_CONTEXT, 'utf8')) as any;
    await live.unlock();
    const original = await readReviewPolicy(live);
    const configuration = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
    live.preserve('toggle-policy-before', original);
    live.preserve('toggle-configuration-before', configuration);
    let restore = false;
    try {
      await createReviewPolicy(live, original, context.resource_prefix, () => { restore = true; });
      await exerciseReviewToggles(live);
      verifyReviewPolicy(await readReviewPolicy(live), original, context.resource_prefix);
    } finally {
      if (restore) await restoreReviewPolicy(live, original);
      const final = await readReviewPolicy(live);
      const current = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
      live.preserve('toggle-policy-final', final);
      live.preserve('toggle-configuration-final', current);
      requireLive(same(final.document, original.document) && same(current.snapshot, configuration.snapshot), 'TOGGLE_ORIGINAL_POLICY_OR_CONFIGURATION_NOT_RESTORED');
      live.record('rules_toggle_original_policy_and_configuration_restored', 'PASS', { exact_policy_document_equal: true,
        exact_configuration_snapshot_equal: true, final_rule_version: final.version, no_agent_workload: true });
    }
  });
});

test('live owned rule condition drafts preview back and cancel', async ({ live }) => {
  await live.safe('existing_owned_rule_controls', async () => {
    await live.unlock();
    await exerciseReviewDrafts(live);
  });
});

test('live owned rule edits toggles compiler rejection and adoption', async ({ page, live }) => {
  await live.safe('existing_owned_rule_writes', async () => {
    await live.unlock();
    const original = await live.exchange(() => navigate(page, 'rules'), 'message', 'rules');
    const group = live.result(original).data.snapshot.document.groups.find((g: any) => g.id === groupId);
    requireLive(group?.rules.length === 1, 'NOT_RUN_OWNED_RULE_SETUP_REQUIRED');
    const select = (rule: boolean) => selectReviewRule(live, rule);
    const dialog = page.locator('dialog.rule-dialog');
    await exerciseReviewToggles(live);
    await select(true);
    await page.getByRole('button', { name: live.t('rules.editRule'), exact: true }).click();
    await expect(dialog.locator('[name=id]')).toHaveAttribute('readonly', '');
    await dialog.locator('[name=number]').fill('2');
    await live.local(() => dialog.getByRole('button', { name: live.t('rules.preview'), exact: true }).click());
    await live.local(() => dialog.getByRole('button', { name: live.t('rules.back'), exact: true }).click());
    await expect(dialog.locator('[name=number]')).toHaveValue('2');
    await dialog.getByRole('button', { name: live.t('rules.preview'), exact: true }).click();
    const beforeSave = await live.guest({ op: 'request', kind: 'rules', command: { type: 'get' } });
    const changed = await live.exchange(() => dialog.getByRole('button', { name: live.t('rules.submit'), exact: true }).click(), 'message', 'rules');
    const sent = changed.request.frame.message.command;
    const change = decodeChange(sent.change);
    const priorRule = beforeSave.snapshot.document.groups.find((g: any) => g.id === groupId).rules[0];
    requireLive(change.type === 'replace_rule' && same(change.rule, { kind: 'file', rule: { ...priorRule, number: 2 } })
      && same(sent.expected_version, beforeSave.snapshot.version), 'RULE_EDIT_ORIGINAL_CHANGE_DIFFERS');
    const result = live.result(changed);
    requireLive(result.status === 'completed' && result.data.applied && same(result.data.applied.rule_version, result.data.receipt.saved_version), 'RULE_EDIT_NOT_SAVED_AND_ADOPTED');
    const readback = await live.guest({ op: 'request', kind: 'rules', command: { type: 'get' } });
    requireLive(same(readback.snapshot.version, result.data.receipt.saved_version), 'RULE_EDIT_VERSION_READBACK_DIFFERS');
    const expectedDocument = { groups: beforeSave.snapshot.document.groups.map((g: any) => g.id === groupId
      ? { ...g, rules: [{ ...priorRule, number: 2 }] } : g) };
    requireLive(same(readback.snapshot.document, expectedDocument), 'RULE_EDIT_CHANGED_UNRELATED_POLICY');
    live.record('rules_existing_id_readonly_number_edit', 'PASS', { ...live.refs(changed), expected_version: sent.expected_version,
      saved_version: result.data.receipt.saved_version, applied_version: result.data.applied.rule_version, exact_document_readback: true });
    const beforeInvalid = await live.guest({ op: 'request', kind: 'rules', command: { type: 'get' } });
    await select(true);
    await page.getByRole('button', { name: live.t('rules.editRule'), exact: true }).click();
    const action = dialog.locator('fieldset:has(>input[name=action])');
    await action.locator('select').first().selectOption('deny');
    await action.locator('select').nth(1).selectOption('none');
    await dialog.getByRole('button', { name: live.t('rules.preview'), exact: true }).click();
    const invalid = await live.exchange(() => dialog.getByRole('button', { name: live.t('rules.submit'), exact: true }).click(), 'message', 'rules');
    requireLive(live.result(invalid).status === 'failed' && live.result(invalid).code === 'invalid', 'BACKEND_ACCEPTED_DENY_IN_ASK_GROUP');
    const afterInvalid = await live.guest({ op: 'request', kind: 'rules', command: { type: 'get' } });
    requireLive(same(beforeInvalid.snapshot.version, afterInvalid.snapshot.version), 'REJECTED_RULE_CHANGED_SAVED_POLICY');
    live.record('rules_actual_compiler_rejection_without_save', 'PASS', { ...live.refs(invalid), failure_code: live.result(invalid).code, source_version_unchanged: true });
  });
});

test('live advanced configuration cancel restores the original snapshot', async ({ page, live }) => {
  await live.safe('local_configuration_drafts', async () => {
    await live.unlock();
    const config = await live.exchange(() => navigate(page, 'settings'), 'message', 'configuration');
    live.source(config);
    await live.local(() => page.getByRole('button', { name: live.t('settings.edit'), exact: true }).click());
    for (const checkbox of await page.locator('.settings-editor input[type=checkbox]').all()) {
      const initial = await checkbox.isChecked();
      await live.local(() => checkbox.setChecked(!initial)); await live.local(() => checkbox.setChecked(initial));
    }
    await live.local(() => page.getByRole('button', { name: live.t('settings.advancedEdit'), exact: true }).click());
    const advancedCancel = page.getByRole('button', { name: live.t('settings.cancel'), exact: true });
    const cancelControl = await live.describe(advancedCancel);
    await live.local(() => page.locator('#settings-document').fill('{"local_draft":true}'));
    await live.local(() => advancedCancel.click());
    await live.local(() => page.getByRole('button', { name: live.t('settings.edit'), exact: true }).click());
    await live.local(() => page.getByRole('button', { name: live.t('settings.advancedEdit'), exact: true }).click());
    requireLive(same(parseUnique(await page.locator('#settings-document').inputValue()), live.result(config).data.snapshot.document), 'ADVANCED_CANCEL_DID_NOT_RESTORE_SNAPSHOT');
    live.record('settings_cancel', 'PASS', { ...live.refs(config), control: cancelControl, original_document_restored: true });
    await live.local(() => page.locator('#settings-document').fill('{invalid'));
    await live.local(() => page.getByRole('button', { name: live.t('settings.preview'), exact: true }).click());
    await expect(page.getByRole('alert')).toContainText(live.t('settings.invalidDocument'));
    await live.local(() => page.getByRole('button', { name: live.t('settings.discardAdvanced'), exact: true }).click());
    await live.local(() => page.getByRole('button', { name: live.t('settings.cancel'), exact: true }).click());
    const actual = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
    requireLive(same(actual.snapshot, live.result(config).data.snapshot), 'LOCAL_DRAFT_CHANGED_CONFIGURATION');
    live.record('settings_all_local_checkboxes_advanced_invalid_discard_cancel', 'PASS', { ...live.refs(config), actual_saved_version_unchanged: true });
  });
});
