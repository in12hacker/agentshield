import { readFile } from 'node:fs/promises';
import { test, expect, unlock, navigate, disclosures, ok } from './fixtures';
import { egressSnapshot, egressBinding, syntheticCa, syntheticSecret, syntheticPhantom, phantomData,
  overviewResult } from '../test-support/contract-supply';
import { idLabel } from '../src/wire';
import { readEgressConfig } from '../src/egress-wire';

test('egress CA download, secret import/rotate, phantom issue/copy/revoke and local confirmations', async ({ page, peer, t, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await unlock(page, peer);
  const initial = await peer.next(() => navigate(page, 'egress'), 'message', 'configuration');
  const button = (key: Parameters<typeof t>[0]) => page.locator('#egress-trust').getByRole('button', { name: t(key), exact: true });
  // Existing local author controls also work while the configuration read is pending.
  await peer.local(async () => {
    const secret = page.locator('#egress-provider-secret');
    await secret.fill(syntheticSecret);
    expect(await secret.inputValue() === syntheticSecret).toBe(true);
    await page.locator('.egress-form input[type=checkbox]').check();
    await expect(page.locator('#egress-secret-ref')).toBeVisible();
    await expect(secret).toBeEmpty();
    await page.locator('.egress-form input[type=checkbox]').uncheck();
    await expect(page.locator('#egress-secret-ref')).toHaveCount(0);
    await secret.fill(syntheticSecret);
    await button('egress.prepareImport').click();
    await expect(button('egress.confirmImport')).toBeVisible();
    await button('settings.cancel').click();
    await expect(secret).toBeEmpty();
    await expect(button('egress.confirmImport')).toHaveCount(0);
    await page.locator('#egress-revoke-id').fill(idLabel(phantomData.token_id));
    await button('egress.reviewRevoke').click();
    await expect(button('egress.confirmRevoke')).toBeVisible();
    await button('settings.cancel').click();
    await expect(button('egress.confirmRevoke')).toHaveCount(0);
  });
  peer.reply(initial, ok({ type: 'configuration', snapshot: egressSnapshot }));
  const issueConfig = readEgressConfig(egressSnapshot);
  const issueBinding = issueConfig.bindings.find(binding => idLabel(binding.id) === idLabel(phantomData.binding))!;
  const issuanceScope = async () => {
    const summary = page.locator('.settings-receipt > details > summary');
    await peer.local(async () => {
      await summary.click();
      await expect(summary.locator('..')).toHaveAttribute('open', '');
      const scope = summary.locator('..').locator('.egress-scope');
      await expect(scope.locator('dd')).toHaveText([
        ...issueBinding.subjects.flatMap(subject => subject.kind === 'process'
          ? [String(subject.value.host_tgid), idLabel(subject.value.boot_id), subject.value.birth_boottime_ns]
          : [JSON.stringify(subject.value.runtime), JSON.stringify(subject.value.namespace), JSON.stringify(subject.value.id)]),
        issueBinding.slot, idLabel(issueBinding.secret),
      ]);
      await expect(scope.locator('.egress-target code')).toHaveText(issueBinding.routes.flatMap(id => {
        const route = issueConfig.flow!.routes.find(route => idLabel(route.id) === idLabel(id))!;
        return [idLabel(id), route.origin, route.method, route.path, route.profile];
      }));
      await summary.click();
      await expect(summary.locator('..')).not.toHaveAttribute('open', '');
    });
  };
  await expect(button('egress.readCa')).toBeEnabled();
  await peer.local(async () => {
    await button('egress.reviewRevoke').click();
    await expect(button('egress.confirmRevoke')).toBeVisible();
    await button('settings.cancel').click();
    await expect(button('egress.confirmRevoke')).toHaveCount(0);
  });
  await disclosures(page.locator('#egress-trust'), peer);
  const ca = await peer.next(() => button('egress.readCa').click(), 'private', 'public_ca');
  expect(ca.body).toEqual({ type: 'public_ca', ca: syntheticCa.id }); peer.reply(ca, ok({ type: 'ca', info: syntheticCa }));
  const download = page.waitForEvent('download');
  await peer.local(() => button('egress.downloadCa').click());
  const file = await download;
  expect(file.suggestedFilename()).toBe('agentshield-ca.der');
  expect([...await readFile((await file.path())!)]).toEqual(syntheticCa.certificate_der);
  await peer.local(async () => { await button('egress.ensureCa').click(); await button('settings.cancel').click(); await button('egress.ensureCa').click(); });
  const ensure = await peer.next(() => button('egress.confirmEnsure').click(), 'private', 'ensure_ca');
  expect(ensure.body).toEqual({ type: 'ensure_ca' }); peer.reply(ensure, ok({ type: 'ca', info: syntheticCa }));
  await peer.local(async () => {
    await button('egress.prepareImport').click(); await button('egress.confirmImport').click();
    await expect(page.getByRole('alert')).toContainText(t('egress.invalidSecret'));
    await page.locator('#egress-provider-secret').fill(syntheticSecret);
    await button('egress.prepareImport').click(); await button('settings.cancel').click();
    await expect(page.locator('#egress-provider-secret')).toBeEmpty();
    await page.locator('#egress-provider-secret').fill(syntheticSecret);
    await button('egress.prepareImport').click();
  });
  const imported = await peer.next(() => button('egress.confirmImport').click(), 'private', 'import_secret');
  // Boolean checks deliberately avoid printing a private value on assertion failure.
  expect(imported.body.value === syntheticSecret).toBe(true);
  peer.reply(imported, ok({ type: 'secret_saved', receipt: { secret: egressBinding.secret, revision: '7' } }));
  await expect(page.locator('#egress-provider-secret')).toBeEmpty();
  await peer.local(async () => {
    await page.locator('.egress-form input[type=checkbox]').check();
    await page.locator('#egress-secret-ref').fill('bad'); await page.locator('#egress-secret-revision').fill('bad');
    await button('egress.prepareRotate').click(); await expect(page.getByRole('alert')).toContainText(t('egress.invalidReference'));
    await button('egress.useSavedReceipt').click();
    await expect(page.locator('#egress-secret-ref')).toHaveValue(idLabel(egressBinding.secret));
    await expect(page.locator('#egress-secret-revision')).toHaveValue('7');
    // Both dynamically introduced fields are editable and keep original reference/revision.
    await page.locator('#egress-secret-ref').fill(idLabel(egressBinding.secret));
    await page.locator('#egress-secret-revision').fill('7');
    await page.locator('#egress-provider-secret').fill(syntheticSecret);
    await button('egress.prepareRotate').click();
  });
  const rotate = await peer.next(() => button('egress.confirmRotate').click(), 'private', 'replace_secret');
  expect(rotate.body.secret).toEqual(egressBinding.secret); expect(rotate.body.expected_revision).toBe('7');
  expect(rotate.body.value === syntheticSecret).toBe(true);
  peer.reply(rotate, ok({ type: 'secret_saved', receipt: { secret: egressBinding.secret, revision: '8' } }));
  await peer.local(async () => {
    await button('egress.prepareRotate').click(); await button('settings.cancel').click();
    await page.locator('.egress-form input[type=checkbox]').uncheck();
    await page.locator('#egress-binding').selectOption(idLabel(egressBinding.id));
    await button('egress.reviewIssue').click(); await button('settings.cancel').click(); await button('egress.reviewIssue').click();
  });
  const issue = await peer.next(() => button('egress.confirmIssue').click(), 'private', 'issue_phantom');
  expect(issue.body).toEqual({ type: 'issue_phantom', binding: egressBinding.id, expected_config: egressSnapshot.version });
  peer.reply(issue, ok(phantomData));
  await expect(page.locator('#egress-once-value')).toHaveAttribute('type', 'password');
  await issuanceScope();
  await peer.local(async () => {
    await page.locator('#egress-once-value').click();
    await expect(page.locator('#egress-once-value')).toHaveAttribute('readonly', '');
    await button('egress.reveal').click(); await expect(page.locator('#egress-once-value')).toHaveAttribute('type', 'text');
    await button('egress.hide').click(); await button('egress.copy').click();
    await expect(page.getByText(t('egress.copied'), { exact: true })).toBeVisible();
    expect(await page.evaluate(expected => navigator.clipboard.readText().then(value => value === expected), syntheticPhantom)).toBe(true);
    await disclosures(page.locator('.settings-receipt'), peer);
    await button('egress.closeValue').click(); await expect(page.locator('#egress-once-value')).toHaveCount(0);
    await page.locator('#egress-revoke-id').fill('invalid'); await button('egress.reviewRevoke').click();
    await expect(page.getByRole('alert')).toContainText(t('egress.invalidReference'));
    await button('egress.useIssuedReceipt').click();
    await expect(page.locator('#egress-revoke-id')).toHaveValue(idLabel(phantomData.token_id));
    await page.locator('#egress-revoke-id').fill(idLabel(phantomData.token_id));
    await button('egress.reviewRevoke').click(); await button('settings.cancel').click();
  });
  const pendingConfiguration = await peer.next(() => button('settings.read').click(), 'message', 'configuration');
  // Snapshot release removes optional sections; the issued receipt retains its original scope.
  await issuanceScope();
  await peer.local(async () => {
    await button('egress.useIssuedReceipt').click();
    expect(await page.locator('#egress-revoke-id').inputValue() === idLabel(phantomData.token_id)).toBe(true);
    await page.locator('#egress-revoke-id').fill(idLabel(phantomData.token_id));
    await button('egress.reviewRevoke').click();
    await expect(button('egress.confirmRevoke')).toBeVisible();
    await button('settings.cancel').click();
    await expect(button('egress.confirmRevoke')).toHaveCount(0);
  });
  peer.reply(pendingConfiguration, ok({ type: 'configuration', snapshot: egressSnapshot }));
  await expect(button('egress.readCa')).toBeEnabled();
  await peer.local(() => button('egress.reviewRevoke').click());
  const revoke = await peer.next(() => button('egress.confirmRevoke').click(), 'private', 'revoke_phantom');
  expect(revoke.body).toEqual({ type: 'revoke_phantom', token: phantomData.token_id }); peer.reply(revoke, ok({ type: 'phantom_revoked', revision: '9' }));
  await expect(page.locator('.settings-receipt')).toContainText('revoke_phantom');
  const diagnostics = await peer.next(() => button('settings.refreshDiagnostics').click(), 'business', 'protection_overview'); peer.reply(diagnostics, overviewResult());
  const read = await peer.next(() => button('settings.read').click(), 'message', 'configuration'); peer.reply(read, ok({ type: 'configuration', snapshot: egressSnapshot }));
  await peer.local(() => page.locator('#egress-trust a[href="#settings"]').click());
  await expect(page.locator('#settings-title')).toBeFocused();
});

test('private unconfirmed write is not retried; late once value after leave is discarded', async ({ page, peer, t }) => {
  await unlock(page, peer);
  const initial = await peer.next(() => navigate(page, 'egress'), 'message', 'configuration'); peer.reply(initial, ok({ type: 'configuration', snapshot: egressSnapshot }));
  await page.locator('#egress-binding').selectOption(idLabel(egressBinding.id));
  await page.getByRole('button', { name: t('egress.reviewIssue'), exact: true }).click();
  const issue = await peer.next(() => page.getByRole('button', { name: t('egress.confirmIssue'), exact: true }).click(), 'private', 'issue_phantom');
  await navigate(page, 'overview'); peer.reply(issue, ok(phantomData));
  const again = await peer.next(() => navigate(page, 'egress'), 'message', 'configuration'); peer.reply(again, ok({ type: 'configuration', snapshot: egressSnapshot }));
  await expect(page.locator('#egress-once-value')).toHaveCount(0);
  expect(peer.requests.filter(r => r.body.type === 'issue_phantom')).toHaveLength(1);
  peer.sockets.at(-1)!.close({ code: 1000 }); await expect(page.locator('.gate')).toBeVisible();
  await unlock(page, peer); expect(peer.requests.filter(r => r.body.type === 'issue_phantom')).toHaveLength(1);
});

test('clipboard rejection, input paste/drop rejection and private grant revocation', async ({ page, peer, t }) => {
  await unlock(page, peer);
  const initial = await peer.next(() => navigate(page, 'egress'), 'message', 'configuration'); peer.reply(initial, ok({ type: 'configuration', snapshot: egressSnapshot }));
  const input = page.locator('#egress-provider-secret');
  await peer.local(async () => {
    for (const type of ['paste', 'drop']) {
      await input.evaluate((element, type) => {
        const data = new DataTransfer(); data.setData('text', 'invalid\nsynthetic');
        element.dispatchEvent(type === 'paste' ? new ClipboardEvent(type, { clipboardData: data, bubbles: true, cancelable: true })
          : new DragEvent(type, { dataTransfer: data, bubbles: true, cancelable: true }));
      }, type);
      await expect(page.getByRole('alert')).toContainText(t('egress.invalidSecret')); await expect(input).toBeEmpty();
    }
  });
  await page.locator('#egress-binding').selectOption(idLabel(egressBinding.id));
  await page.getByRole('button', { name: t('egress.reviewIssue'), exact: true }).click();
  const issue = await peer.next(() => page.getByRole('button', { name: t('egress.confirmIssue'), exact: true }).click(), 'private', 'issue_phantom');
  peer.reply(issue, ok(phantomData)); await expect(page.locator('#egress-once-value')).toBeVisible();
  await page.evaluate(() => Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: () => Promise.reject(new Error('synthetic clipboard rejection')) } }));
  await peer.local(() => page.getByRole('button', { name: t('egress.copy'), exact: true }).click());
  await expect(page.getByText(t('egress.copyFailed'), { exact: true })).toBeVisible();
  peer.diagnostic({ type: 'private_not_authorized', code: 'unauthorized' });
  await expect(page.locator('#egress-once-value')).toHaveCount(0);
  await expect(page.getByRole('button', { name: t('egress.confirmIssue'), exact: true })).toHaveCount(0);
  await expect(page.locator('#egress-trust')).toContainText(t('egress.permissionRevoked'));
  expect(peer.requests.filter(r => r.lane === 'private')).toHaveLength(1);
});
