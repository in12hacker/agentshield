import { test, expect, navigate, requireLive, same, type LiveObservation } from './fixtures';
import { factsJson } from '../../src/wire-json';
import { protectionKinds, type DriverStatus } from '../../src/driver-wire';
import type { ConfigDocument, ConfigSnapshot } from '../../src/settings-wire';

test('live persistent protection checkboxes save apply and restore', async ({ page, live }) => {
  await live.safe('persistent_protection_checkboxes', async () => {
    await live.unlock();
    const before = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
    const original = before.snapshot as ConfigSnapshot;
    const initialDriver = (await live.guest({ op: 'request', kind: 'control', command: { type: 'get_status' } })).snapshot as DriverStatus;
    live.preserve('checkbox-configuration-before', original);
    live.preserve('checkbox-driver-before', initialDriver);
    requireLive(initialDriver.self_protection === 'enabled', 'CHECKBOX_INITIAL_SELF_PROTECTION_NOT_ENABLED');
    const changed: ConfigDocument = { ...original.document, driver: { ...original.document.driver,
      startup_protection: { process: !original.document.driver.startup_protection.process,
        file: !original.document.driver.startup_protection.file, network: !original.document.driver.startup_protection.network },
      self_protection: { log: !original.document.driver.self_protection.log } } };
    await live.exchange(() => navigate(page, 'settings'), 'message', 'configuration');
    const save = async (expected: ConfigSnapshot, document: ConfigDocument, phase: 'changed' | 'restored') => {
      const read = await live.exchange(() => page.getByRole('button', { name: live.t('settings.read'), exact: true }).click(), 'message', 'configuration');
      requireLive(live.result(read).status === 'completed' && same(live.result(read).data.snapshot, expected), 'CHECKBOX_UI_SNAPSHOT_DIFFERS');
      await live.local(() => page.getByRole('button', { name: live.t('settings.edit'), exact: true }).click());
      const editor = page.locator('.settings-editor');
      await expect(editor.locator('input[type=checkbox]')).toHaveCount(4);
      for (const [feature, key] of [['process', 'category.process'], ['file', 'category.file'], ['network', 'category.network']] as const) {
        await live.local(() => editor.getByRole('checkbox', { name: live.t(key), exact: true }).setChecked(document.driver.startup_protection[feature]));
      }
      await live.local(() => editor.getByRole('checkbox', { name: live.t('settings.selfLog'), exact: true }).setChecked(document.driver.self_protection.log));
      await live.local(() => page.getByRole('button', { name: live.t('settings.preview'), exact: true }).click());
      await live.openSummary(page.locator('.settings-confirm summary'), `checkbox_${phase}_proposed_document`, document);
      const saved = await live.exchange(() => page.getByRole('button', { name: live.t('settings.confirmSave'), exact: true }).click(), 'message', 'configuration');
      // Read the actual state before asserting the UI receipt; never replay an uncertain mutation.
      const actual = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
      const driver = (await live.guest({ op: 'request', kind: 'control', command: { type: 'get_status' } })).snapshot as DriverStatus;
      live.preserve(`checkbox-${phase}-configuration-readback`, actual);
      live.preserve(`checkbox-${phase}-driver-readback`, driver);
      const sent = saved.request.frame.message.command, result = live.result(saved);
      requireLive(same(sent, { type: 'update', expected_version: expected.version, document }), 'CHECKBOX_ORIGINAL_UPDATE_DIFFERS');
      requireLive(result.status === 'completed' && result.data.type === 'configuration_saved'
        && same(result.data.receipt.changed_sections, ['driver']) && same(result.data.receipt.required_effects, ['apply_driver'])
        && result.data.applied?.source_state === 'running' && result.data.applied.self_protection === 'enabled', 'CHECKBOX_SAVE_OR_DRIVER_ADOPTION_FAILED');
      requireLive(same(actual.snapshot.document, document) && same(actual.snapshot.version, result.data.receipt.saved_version)
        && same(actual.snapshot.paths, original.paths), 'CHECKBOX_FULL_CONFIGURATION_READBACK_DIFFERS');
      requireLive(driver.self_protection === 'enabled' && driver.source_state === 'running'
        && same(driver.driver_config_version, result.data.applied.driver_config_version), 'CHECKBOX_DRIVER_VERSION_OR_DEFAULTS_DIFFER');
      for (const feature of protectionKinds) {
        const override = initialDriver[feature].runtime_override;
        requireLive(driver[feature].startup_default === document.driver.startup_protection[feature]
          && driver[feature].runtime_override === override && same(driver[feature], result.data.applied[feature])
          && driver[feature].effective_state === ((override === 'default' ? document.driver.startup_protection[feature] : override === 'enabled') ? 'enabled' : 'disabled'),
        'CHECKBOX_DRIVER_FEATURE_STATE_DIFFERS');
      }
      live.source(saved);
      await live.openSummary(page.locator('.settings-receipt summary'), `checkbox_${phase}_original_receipt`, result);
      live.record(`configuration_four_checkboxes_${phase}`, 'PASS', { ...live.refs(saved), expected_version: expected.version,
        saved_version: actual.snapshot.version, driver_config_version: driver.driver_config_version,
        startup_protection: document.driver.startup_protection, self_log: document.driver.self_protection.log,
        entire_document_and_paths_compared: true, self_protection: driver.self_protection,
        scope: 'actual save/adoption and driver defaults/effective states; not per-operation HIPS effects' });
    };
    try { await save(original, changed, 'changed'); }
    finally {
      const current = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
      live.preserve('checkbox-before-restore', current);
      if (!same(current.snapshot.document, original.document)) await save(current.snapshot, original.document, 'restored');
      const final = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
      const driver = (await live.guest({ op: 'request', kind: 'control', command: { type: 'get_status' } })).snapshot as DriverStatus;
      live.preserve('checkbox-configuration-final', final);
      live.preserve('checkbox-driver-final', driver);
      requireLive(same(final.snapshot, original) && driver.self_protection === 'enabled'
        && protectionKinds.every(feature => same(driver[feature], initialDriver[feature])), 'CHECKBOX_ORIGINAL_CONFIGURATION_OR_DRIVER_NOT_RESTORED');
      live.record('configuration_four_checkboxes_original_restored', 'PASS', { version: final.snapshot.version,
        entire_original_snapshot_equal: true, original_driver_features_equal: true, self_protection: driver.self_protection });
    }
  });
});

async function saveContentGrant(live: LiveObservation, enabled: boolean) {
  const page = live.page;
  await navigate(page, 'settings');
  const current = await live.exchange(() => page.getByRole('button', { name: live.t('settings.read'), exact: true }).click(), 'message', 'configuration');
  const snapshot = live.result(current).data.snapshot;
  requireLive(snapshot.document.middleware.chat_content !== enabled, 'CONFIGURATION_ALREADY_AT_TARGET_NO_WRITE_REPLAY');
  const document = { ...snapshot.document, middleware: { ...snapshot.document.middleware, chat_content: enabled } };
  await page.getByRole('button', { name: live.t('settings.edit'), exact: true }).click();
  await page.getByRole('button', { name: live.t('settings.advancedEdit'), exact: true }).click();
  await page.locator('#settings-document').fill(factsJson(document));
  await live.local(() => page.getByRole('button', { name: live.t('settings.preview'), exact: true }).click());
  const confirm = page.getByRole('button', { name: live.t('settings.confirmSave'), exact: true });
  await expect(confirm).toBeDisabled();
  await page.locator('.settings-confirm input[type=checkbox]').check();
  await expect(confirm).toBeEnabled();
  const control = await live.describe(confirm);
  const from = live.frames.length;
  await confirm.click();
  const request = await live.until(() => live.frames.slice(from).find(r => r.direction === 'out' && r.frame.message?.type === 'configuration' && r.frame.message.command.type === 'update'), 'CONFIGURATION_UI_UPDATE_NOT_OBSERVED');
  requireLive(same(request.frame.message.command.expected_version, snapshot.version) && same(request.frame.message.command.document, document), 'CONFIGURATION_UI_ORIGINAL_REQUEST_DIFFERS');
  const observed = await live.until(() => {
    const response = live.frames.find(r => r.direction === 'in' && r.connection === request.connection
      && same(r.frame.message?.id, request.frame.message.id) && r.frame.message?.result?.status !== 'accepted');
    return response ? { response } : live.connectionEnded(request.connection) ? { response: undefined } : null;
  }, 'CONFIGURATION_REPLY_OR_ORIGINAL_CONNECTION_END_NOT_OBSERVED');
  const response = observed.response;
  // Authorization adoption may close the original writer. Read back once from a
  // freshly authenticated original Unix owner; never resend the UI mutation.
  await live.guest({ op: 'reconnect_readonly' });
  const actual = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
  requireLive(same(actual.snapshot.document, document), 'ACTUAL_SAVED_CONTENT_GRANT_DIFFERS');
  live.record(`configuration_save_chat_content_${enabled}`, 'PASS', { request_ref: request.raw_ref,
    reply_ref: response?.raw_ref ?? null, original_reply_status: response?.frame.message.result.status ?? 'UNCONFIRMED_CONNECTION_ENDED',
    saved_version_readback: actual.snapshot.version, exact_document_readback: true, update_sent_once: true, control });
  if (await page.locator('.dashboard-shell').isVisible()) await page.getByRole('button', { name: live.t('header.lock'), exact: true }).click();
  await live.unlock(enabled);
  return actual.snapshot;
}

test('live configuration save authorization narrowing and restore', async ({ page, live }) => {
  await live.safe('last_configuration_mutations_and_authorization', async () => {
    requireLive(process.env.AS_LIVE_FINAL_CONFIG === '1', 'NOT_RUN_FINAL_CONFIGURATION_PHASE_NOT_SELECTED');
    await live.unlock();
    const before = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
    live.preserve('configuration-before-final-ui-writes', before.snapshot);
    let attempted = false;
    try {
      attempted = true;
      await saveContentGrant(live, false);
      const from = live.frames.length;
      await navigate(page, 'chat');
      await expect(page.locator('#chat-history')).toContainText(live.t('chat.noGrant'));
      await expect(page.locator('#chat-history > .process-controls button').first()).toBeDisabled();
      requireLive(!live.frames.slice(from).some(r => r.direction === 'out' && r.frame.business), 'CONTENT_QUERY_SENT_WITHOUT_ACTUAL_GRANT');
      live.record('new_real_content_grant_false_gates_queries', 'PASS', { actual_authenticated_grant: false, no_content_query: true });
    } finally {
      if (attempted) {
        await live.guest({ op: 'reconnect_readonly' });
        const actual = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
        if (actual.snapshot.document.middleware.chat_content === false) {
          if (await page.locator('.dashboard-shell').isVisible()) await page.getByRole('button', { name: live.t('header.lock'), exact: true }).click();
          await live.unlock(false);
          await saveContentGrant(live, true);
        }
      }
    }
    const after = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
    requireLive(same(after.snapshot.document, before.snapshot.document), 'ORIGINAL_CONFIGURATION_NOT_RESTORED');
    live.record('configuration_original_document_restored', 'PASS', { version: after.snapshot.version, actual_content_grant: true });
  });
});

test('live configuration reload original reply and readback', async ({ page, live }) => {
  await live.safe('real_configuration_reload', async () => {
    await live.unlock();
    const before = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
    await navigate(page, 'settings');
    await page.getByRole('button', { name: live.t('settings.reload'), exact: true }).click();
    await live.local(() => page.getByRole('button', { name: live.t('settings.cancel'), exact: true }).click());
    await page.getByRole('button', { name: live.t('settings.reload'), exact: true }).click();
    const reload = await live.exchange(() => page.getByRole('button', { name: live.t('settings.confirmReload'), exact: true }).click(), 'message', 'configuration');
    const result = live.result(reload);
    requireLive(result.status === 'completed' && result.data.type === 'configuration', 'UNCHANGED_RELOAD_MUST_RETURN_CURRENT_CONFIGURATION');
    requireLive(same(result.data.snapshot.document, before.snapshot.document), 'RELOAD_CHANGED_ORIGINAL_DOCUMENT');
    const actual = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
    requireLive(same(actual.snapshot, result.data.snapshot), 'RELOAD_READBACK_DIFFERS');
    await expect(page.locator('.settings-receipt')).toContainText(live.t('settings.reloaded'));
    live.source(reload);
    await live.openSummary(page.locator('.settings-receipt summary'), 'settings_receipt', result);
    live.record('configuration_actual_reload', 'PASS', { ...live.refs(reload), actual_reply_type: result.data.type,
      saved_version: actual.snapshot.version, applied: result.data.applied ?? null, independent_readback: true });
  });
});
