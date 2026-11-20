import { createHash, randomBytes } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { test, expect, navigate, requireLive, same } from './fixtures';
import { idLabel } from '../../src/wire';
import { privateOriginals } from './originals';

test('live private local drafts preview and cancel without private writes', async ({ page, live }) => {
  await live.safe('private_local_drafts', async () => {
    await live.unlock();
    const config = await live.exchange(() => navigate(page, 'egress'), 'message', 'configuration');
    live.source(config);
    const button = (key: Parameters<typeof live.t>[0]) => page.locator('#egress-trust').getByRole('button', { name: live.t(key), exact: true });
    const secret = page.locator('#egress-provider-secret');
    const rotate = page.locator('.egress-form input[type=checkbox]');
    let value = randomBytes(24).toString('hex');
    await live.privateLocal('egress_draft_input', secret, () => live.privateInput(secret, value));
    requireLive(await secret.inputValue() === value, 'PRIVATE_LOCAL_INPUT_DIFFERS');
    await live.privateLocal('egress_draft_rotate_on', rotate, () => rotate.check());
    requireLive(await secret.inputValue() === '', 'ROTATE_SWITCH_DID_NOT_CLEAR_PRIVATE_INPUT');
    await expect(page.locator('#egress-secret-ref')).toBeVisible();
    await live.privateLocal('egress_draft_rotate_off', rotate, () => rotate.uncheck());
    await expect(page.locator('#egress-secret-ref')).toHaveCount(0);
    await live.privateLocal('egress_draft_input', secret, () => live.privateInput(secret, value));
    value = '';
    await live.privateLocal('egress_draft_preview', button('egress.prepareImport'), () => button('egress.prepareImport').click());
    await expect(button('egress.confirmImport')).toBeVisible();
    await live.privateLocal('egress_draft_cancel', button('settings.cancel'), () => button('settings.cancel').click());
    requireLive(await secret.inputValue() === '', 'PRIVATE_DRAFT_CANCEL_DID_NOT_CLEAR_INPUT');
    const token = page.locator('#egress-revoke-id');
    // A local author reference only. It is never claimed to be an issued token.
    let reference = randomBytes(16).toString('hex');
    await live.privateLocal('egress_revoke_draft_input', token, () => token.fill(reference));
    requireLive(await token.inputValue() === reference, 'LOCAL_REVOKE_REFERENCE_DIFFERS');
    reference = '';
    await live.privateLocal('egress_revoke_draft_preview', button('egress.reviewRevoke'), () => button('egress.reviewRevoke').click());
    await expect(button('egress.confirmRevoke')).toBeVisible();
    await live.privateLocal('egress_revoke_draft_cancel', button('settings.cancel'), () => button('settings.cancel').click());
    await expect(button('egress.confirmRevoke')).toHaveCount(0);
    live.record('egress_issued_and_scope', 'NOT_OBSERVED', { reason: 'this local-only scenario has no same-connection issue receipt; no private mutation made to manufacture one' });
  });
});

test('live runtime controls and independent driver state', async ({ page, live }) => {
  await live.safe('real_runtime_and_private_management', async () => {
    await live.unlock();
    const initial = await live.exchange(() => navigate(page, 'settings'), 'message', 'configuration');
    const before = live.result(initial).data.snapshot;
    live.preserve('configuration-before-runtime-controls', before);
    for (const [index, feature] of ['process', 'file', 'network'].entries()) {
      for (const [key, type, enabled] of [
        ['settings.enableTemporarily', 'set_runtime_feature', true],
        ['settings.disableTemporarily', 'set_runtime_feature', false],
        ['settings.resume', 'resume_runtime_feature', null],
      ] as const) {
        live.stage(`runtime_${feature}_${type}_${enabled}`);
        const area = page.locator('.settings-feature').nth(index);
        await live.local(() => area.getByRole('button', { name: live.t(key), exact: true }).click());
        await live.local(() => page.getByRole('button', { name: live.t('settings.cancel'), exact: true }).click());
        await area.getByRole('button', { name: live.t(key), exact: true }).click();
        const command = await live.exchange(() => page.getByRole('button', { name: live.t('settings.confirmControl'), exact: true }).click(), 'message', 'control');
        const sent = command.request.frame.message.command;
        requireLive(sent.type === type && sent.feature === feature && (enabled === null || sent.enabled === enabled), 'RUNTIME_UI_REQUEST_DIFFERS');
        const result = live.result(command);
        requireLive(result.status === 'completed' && result.data.type === 'driver_status', 'RUNTIME_DRIVER_REPLY_NOT_COMPLETED');
        const actual = await live.guest({ op: 'request', kind: 'control', command: { type: 'get_status' } });
        requireLive(same(result.data.snapshot[feature], actual.snapshot[feature]), 'INDEPENDENT_RUNTIME_READBACK_DIFFERS');
        requireLive(actual.snapshot.self_protection === 'enabled', 'SELF_PROTECTION_CHANGED');
        const expected = enabled === null ? before.document.driver.startup_protection[feature] : enabled;
        requireLive(actual.snapshot[feature].effective_state === (expected ? 'enabled' : 'disabled'), 'ACTUAL_RUNTIME_STATE_DIFFERS');
        live.record(`runtime_${feature}_${type}_${enabled}`, 'PASS', { ...live.refs(command), actual_feature: actual.snapshot[feature], independent_readback: true, effect_scope: 'actual driver state; per-operation effect requires workload' });
        await expect(page.locator('#settings-control-preview')).toHaveCount(0);
        live.source(command);
        await live.openSummary(page.locator('.settings-receipt summary'), 'settings_receipt', result);
      }
    }
    const after = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
    requireLive(same(after.snapshot.version, before.version) && same(after.snapshot.document, before.document), 'RUNTIME_OVERRIDE_WAS_PERSISTED');
    live.record('runtime_controls_did_not_change_saved_configuration', 'PASS', { version: before.version });
    live.record('runtime_per_operation_effects', 'LIVE_BLOCKED', { reason: 'current owned control ops provide no existing subject launch interface; no extra model round solely for control toggles' });
  });
});

test('live private CA import and rotation receipts', async ({ page, live }) => {
  await live.safe('real_private_management', async () => {
    await live.unlock();
    live.stage('private_ca_and_nonproduction_secret');
    await navigate(page, 'egress');
    const config = await live.exchange(() => page.locator('#egress-trust').getByRole('button', { name: live.t('settings.read'), exact: true }).click(), 'message', 'configuration');
    const snapshot = live.result(config).data.snapshot;
    const button = (key: Parameters<typeof live.t>[0]) => page.locator('#egress-trust').getByRole('button', { name: live.t(key), exact: true });
    const ca = await live.privateCommand(() => button('egress.readCa').click(), 'public_ca');
    requireLive(ca.result.status === 'completed' && ca.result.data.type === 'ca', 'ACTUAL_CA_READ_FAILED');
    const cert = ca.result.data.info;
    requireLive(same(cert.id, snapshot.document.flow.egress.ca), 'PUBLIC_CA_NOT_CONFIGURED_ID');
    requireLive(same([...createHash('sha256').update(Buffer.from(cert.certificate_der)).digest()], cert.fingerprint_sha256), 'ACTUAL_CA_FINGERPRINT_DIFFERS');
    const download = page.waitForEvent('download'); await live.local(() => button('egress.downloadCa').click());
    const file = await download;
    requireLive(same([...readFileSync((await file.path())!)], cert.certificate_der), 'ACTUAL_CA_DOWNLOAD_DIFFERS');
    live.record('private_public_ca_actual_download', 'PASS', { ...ca.metadata, ca_id: cert.id, fingerprint_sha256: cert.fingerprint_sha256, actual_der_bytes: cert.certificate_der.length });
    await live.local(() => button('egress.ensureCa').click());
    await live.local(() => button('settings.cancel').click());
    await button('egress.ensureCa').click();
    const ensured = await live.privateCommand(() => button('egress.confirmEnsure').click(), 'ensure_ca');
    requireLive(ensured.result.status === 'completed' && same(ensured.result.data.info, cert), 'ENSURE_CA_CHANGED_CONFIGURED_CERTIFICATE');
    live.record('private_ensure_existing_ca', 'PASS', { ...ensured.metadata, ca_id: cert.id });
    let secret = randomBytes(24).toString('hex');
    await live.privateInput(page.locator('#egress-provider-secret'), secret);
    await live.local(() => button('egress.prepareImport').click());
    const imported = await live.privateCommand(() => button('egress.confirmImport').click(), 'import_secret');
    secret = '';
    requireLive(imported.result.status === 'completed' && imported.result.data.type === 'secret_saved', 'NONPRODUCTION_SECRET_IMPORT_FAILED');
    const reference = imported.result.data.receipt;
    await expect(page.locator('#egress-provider-secret')).toBeEmpty();
    live.record('private_import_nonproduction_secret', 'PASS', { ...imported.metadata, secret_reference: reference.secret, revision: reference.revision });
    await page.locator('.egress-form input[type=checkbox]').check();
    await button('egress.useSavedReceipt').click();
    await expect(page.locator('#egress-secret-ref')).toHaveValue(idLabel(reference.secret));
    await expect(page.locator('#egress-secret-revision')).toHaveValue(reference.revision);
    secret = randomBytes(24).toString('hex');
    await live.privateInput(page.locator('#egress-provider-secret'), secret);
    await button('egress.prepareRotate').click();
    const rotated = await live.privateCommand(() => button('egress.confirmRotate').click(), 'replace_secret');
    secret = '';
    requireLive(rotated.result.status === 'completed' && same(rotated.result.data.receipt.secret, reference.secret)
      && BigInt(rotated.result.data.receipt.revision) > BigInt(reference.revision), 'NONPRODUCTION_SECRET_ROTATION_NOT_COMMITTED');
    live.record('private_replace_nonproduction_secret_revision', 'PASS', { ...rotated.metadata, secret_reference: reference.secret, revision: rotated.result.data.receipt.revision });
  });
});

test('live same-session issued ID and original issuance scope', async ({ live }) => {
  await live.safe('same_session_issuance_receipt', async () => {
    await live.unlock();
    await privateOriginals(live);
  });
});
