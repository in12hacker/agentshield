import { test, expect, unlock, navigate, disclosures, ok } from './fixtures';
import { configSnapshot, savedConfiguration, driverStatus, overviewResult } from '../test-support/contract-supply';
import { factsJson, parseUnique } from '../src/wire-json';

test('configuration local drafts, JSON validation, preview/back/cancel, save vs adoption, reload and runtime controls', async ({ page, peer, t }) => {
  await unlock(page, peer);
  const initial = await peer.next(() => navigate(page, 'settings'), 'message', 'configuration');
  expect(initial.body.command).toEqual({ type: 'get' });
  peer.reply(initial, ok({ type: 'configuration', snapshot: configSnapshot }));
  const button = (key: Parameters<typeof t>[0]) => page.locator('#settings').getByRole('button', { name: t(key), exact: true });
  await expect(button('settings.edit')).toBeVisible();
  await disclosures(page.locator('#settings'), peer);
  await peer.local(async () => {
    await button('settings.edit').click();
    await button('settings.advancedEdit').click();
    await page.locator('#settings-document').fill(factsJson({ ...configSnapshot.document,
      driver: { ...configSnapshot.document.driver, self_protection: { log: true } } }));
    await button('settings.cancel').click();
    await expect(page.locator('.settings-editor')).toHaveCount(0);
    await button('settings.edit').click();
    await button('settings.advancedEdit').click();
    expect(parseUnique(await page.locator('#settings-document').inputValue())).toEqual(configSnapshot.document);
    await button('settings.discardAdvanced').click();
    for (const checkbox of await page.locator('.settings-editor input[type=checkbox]').all()) { await checkbox.check(); await checkbox.uncheck(); }
    await button('settings.advancedEdit').click();
    await page.locator('#settings-document').fill('{invalid');
    await button('settings.preview').click();
    await expect(page.getByRole('alert')).toContainText(t('settings.invalidDocument'));
    await button('settings.discardAdvanced').click();
    await expect(page.locator('#settings-document')).toHaveCount(0);
    await button('settings.preview').click();
    await expect(page.locator('#settings-save-preview')).toBeFocused();
    await button('settings.backToEdit').click();
    await button('settings.cancel').click();
    await button('settings.edit').click();
    await page.locator('.settings-editor').press('Escape');
    await expect(button('settings.edit')).toBeVisible();
    await button('settings.edit').click();
    await button('settings.advancedEdit').click();
    const proposed = { ...configSnapshot.document, middleware: { ...configSnapshot.document.middleware, chat_content: true } };
    await page.locator('#settings-document').fill(factsJson(proposed));
    await button('settings.preview').click();
    await expect(button('settings.confirmSave')).toBeDisabled();
    await page.locator('.settings-confirm input[type=checkbox]').check();
    await expect(button('settings.confirmSave')).toBeEnabled();
    await page.locator('.settings-confirm input[type=checkbox]').uncheck();
    await button('settings.cancel').click();
    await button('settings.edit').click();
    await page.locator('.settings-editor input[type=checkbox]').nth(1).check();
    await button('settings.preview').click();
    await disclosures(page.locator('.settings-confirm'), peer);
  });
  const save = await peer.next(() => button('settings.confirmSave').click(), 'message', 'configuration');
  expect(save.body.command.type).toBe('update'); expect(save.body.command.expected_version).toEqual(configSnapshot.version);
  expect(save.body.command.document.driver.startup_protection).toEqual({ process: true, file: true, network: true });
  peer.reply(save, { status: 'accepted', data: savedConfiguration });
  await expect(page.locator('.settings-receipt')).toContainText(t('settings.accepted'));
  expect(peer.requests.filter(r => r.body.command?.type === 'update')).toHaveLength(1);
  const readAfter = await peer.next(async () => { peer.reply(save, ok(savedConfiguration)); }, 'message', 'configuration');
  await expect(page.locator('.settings-receipt')).toContainText(t('settings.savedPending'));
  expect(readAfter.body.command.type).toBe('get');
  peer.reply(readAfter, ok({ type: 'configuration', snapshot: configSnapshot }));
  await expect(button('settings.edit')).toBeVisible();
  await peer.local(async () => {
    await button('settings.edit').click();
    const summary = page.locator('.settings-receipt > details > summary');
    await summary.click();
    await expect(summary.locator('..')).toHaveAttribute('open', '');
    expect(parseUnique(await summary.locator('..').locator(':scope > pre').innerText())).toEqual(ok(savedConfiguration));
    await summary.click();
    await expect(summary.locator('..')).not.toHaveAttribute('open', '');
    await button('settings.cancel').click();
  });
  await peer.local(async () => {
    await button('settings.reload').click(); await button('settings.cancel').click();
    await button('settings.reload').click();
  });
  const reload = await peer.next(() => button('settings.confirmReload').click(), 'message', 'configuration');
  expect(reload.body.command).toEqual({ type: 'reload', expected_version: configSnapshot.version });
  const reloadRead = await peer.next(async () => peer.reply(reload, ok({ type: 'configuration_reloaded', snapshot: configSnapshot, applied: driverStatus.snapshot })), 'message', 'configuration');
  peer.reply(reloadRead, ok({ type: 'configuration', snapshot: configSnapshot }));
  await expect(page.locator('.settings-receipt')).toContainText(t('settings.reloaded'));
  // Terminal writes request fresh actual diagnostics; answer the latest overview slot explicitly.
  const latestOverview = peer.requests.filter(r => r.body.type === 'protection_overview').at(-1)!;
  peer.reply(latestOverview, overviewResult());
  for (const feature of ['process', 'file', 'network']) {
    const area = page.locator('.settings-feature').nth(['process', 'file', 'network'].indexOf(feature));
    for (const [key, command] of [
      ['settings.enableTemporarily', { type: 'set_runtime_feature', feature, enabled: true }],
      ['settings.disableTemporarily', { type: 'set_runtime_feature', feature, enabled: false }],
      ['settings.resume', { type: 'resume_runtime_feature', feature }],
    ] as const) {
      await peer.local(async () => {
        await area.getByRole('button', { name: t(key), exact: true }).click();
        await button('settings.cancel').click();
        await area.getByRole('button', { name: t(key), exact: true }).click();
      });
      const control = await peer.next(() => button('settings.confirmControl').click(), 'message', 'control');
      expect(control.body.command).toEqual(command);
      peer.reply(control, ok(driverStatus));
      await expect(page.locator('.settings-receipt')).toContainText(t('settings.controlCompleted'));
      await expect.poll(() => peer.requests.filter(r => r.body.type === 'protection_overview').at(-1)?.id).not.toEqual(latestOverview.id);
      peer.reply(peer.requests.filter(r => r.body.type === 'protection_overview').at(-1)!, overviewResult());
    }
  }
  const diagnostics = await peer.next(() => button('settings.refreshDiagnostics').click(), 'business', 'protection_overview');
  peer.reply(diagnostics, overviewResult());
  const read = await peer.next(() => button('settings.read').click(), 'message', 'configuration');
  peer.reply(read, ok({ type: 'configuration', snapshot: configSnapshot }));
  await disclosures(page.locator('.settings-receipt'), peer);
});

test('configuration write survives leaving, no repeat on lost receipt or disconnect', async ({ page, peer, t }) => {
  await unlock(page, peer);
  const initial = await peer.next(() => navigate(page, 'settings'), 'message', 'configuration');
  peer.reply(initial, ok({ type: 'configuration', snapshot: configSnapshot }));
  await page.getByRole('button', { name: t('settings.edit'), exact: true }).click();
  await page.locator('.settings-editor input[type=checkbox]').nth(1).check();
  await page.getByRole('button', { name: t('settings.preview'), exact: true }).click();
  const save = await peer.next(() => page.getByRole('button', { name: t('settings.confirmSave'), exact: true }).click(), 'message', 'configuration');
  await navigate(page, 'overview'); peer.reply(save, ok(savedConfiguration));
  expect(peer.requests.filter(r => r.body.command?.type === 'update')).toHaveLength(1);
  peer.sockets.at(-1)!.close({ code: 1000 });
  await expect(page.locator('.gate')).toBeVisible();
  await unlock(page, peer);
  expect(peer.requests.filter(r => r.body.command?.type === 'update')).toHaveLength(1);
});
