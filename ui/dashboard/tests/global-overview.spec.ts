import { test, expect, unlock, navigate, disclosures, failed } from './fixtures';
import { dashboardPages } from '../src/global/navigation';
import { testToken, overviewResult, driverRead } from '../test-support/contract-supply';

test('direct hash entry and reauthentication keep the skip link bound to the actual workspace', async ({ page, peer, t }) => {
  await unlock(page, peer, '#rules');
  await peer.local(async () => { await page.locator('.skip-link').focus(); await page.keyboard.press('Enter'); });
  await expect(page.locator('#rules-title')).toBeFocused();
  await navigate(page, 'egress');
  await page.getByRole('button', { name: t('header.lock'), exact: true }).click();
  await expect(page.locator('.gate')).toBeVisible();
  await unlock(page, peer);
  await peer.local(async () => { await page.locator('.skip-link').focus(); await page.keyboard.press('Enter'); });
  await expect(page.locator('#egress-title')).toBeFocused();
});

test('gate validation, pending cancel, local preferences, navigation, lock and explicit reconnect', async ({ page, peer, t, locale }) => {
  peer.authenticate = false;
  await page.goto('/');
  await peer.local(async () => {
    await page.locator('.gate select').selectOption('en-US');
    await expect(page.locator('html')).toHaveAttribute('lang', 'en-US');
    await page.locator('.gate select').selectOption('zh-CN');
    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN');
    await page.locator('.gate select').selectOption('ja-JP');
    await expect(page.locator('html')).toHaveAttribute('lang', 'ja-JP');
    await page.locator('.gate select').selectOption(locale!);
    await page.locator('button[type=submit]').click();
    await expect(page.locator('#management-token')).toBeFocused();
    await page.locator('#management-token').fill('invalid');
    await page.locator('button[type=submit]').click();
  });
  await disclosures(page.locator('.gate'), peer);
  await page.locator('#management-token').fill(testToken);
  await page.locator('button[type=submit]').click();
  await expect(page.getByRole('button', { name: t('gate.cancel'), exact: true })).toBeVisible();
  await page.getByRole('button', { name: t('gate.cancel'), exact: true }).click();
  await expect(page.locator('#management-token')).toBeEmpty();
  peer.authenticate = true;
  await unlock(page, peer);
  for (const target of dashboardPages) {
    await navigate(page, target.id);
    await peer.local(async () => { await page.locator('.skip-link').focus(); await page.keyboard.press('Enter'); });
    await expect(page.locator(`#${target.headingId}`)).toBeFocused();
  }
  await page.goBack();
  await expect(page.locator('#settings-title')).toBeFocused();
  await page.goForward();
  await expect(page.locator('#egress-title')).toBeFocused();
  if (await page.locator('.menu-button').isVisible()) {
    await peer.local(async () => {
      await page.locator('.menu-button').click();
      await expect(page.locator('nav a[aria-current=page]')).toBeFocused();
      await page.keyboard.press('Escape');
      await expect(page.locator('.menu-button')).toBeFocused();
      await expect(page.locator('#dashboard-navigation')).toBeHidden();
      await page.locator('.menu-button').click();
      await page.locator('.menu-button').click();
    });
  }
  await navigate(page, 'overview');
  await peer.local(async () => {
    for (const language of ['ja-JP', 'zh-CN', 'en-US', locale!]) {
      await page.locator('.dashboard-header select').selectOption(language);
      await expect(page.locator('html')).toHaveAttribute('lang', language);
    }
    const theme = page.getByRole('button', { name: t('header.theme'), exact: true });
    const before = await theme.getAttribute('aria-pressed');
    await theme.click();
    await expect(theme).toHaveAttribute('aria-pressed', String(before !== 'true'));
    await theme.click();
    await page.locator('.skip-link').focus();
    await page.keyboard.press('Enter');
    await expect(page.locator('#overview-heading')).toBeFocused();
  });
  const refresh = await peer.next(() => page.getByRole('button', { name: t('header.refresh'), exact: true }).click(), 'business', 'protection_overview');
  peer.reply(refresh, overviewResult());
  await page.getByRole('button', { name: t('header.lock'), exact: true }).click();
  await expect(page.locator('.gate')).toBeVisible();
  await expect(page.locator('#management-token')).toBeEmpty();
  expect(peer.sockets).toHaveLength(2);
  await unlock(page, peer);
  expect(peer.sockets).toHaveLength(3);
});

test('overview links, all module tiles, independent receipts, refresh slots and scrolling', async ({ page, peer, t }) => {
  await unlock(page, peer);
  await disclosures(page.locator('#overview'), peer);
  await peer.local(async () => {
    await page.locator('.banner-links a[href="#protection-receipts"]').click();
    await expect(page.locator('#receipts-title')).toBeFocused();
    for (const tile of await page.locator('.protection-tile').all()) {
      await navigate(page, 'overview');
      await tile.click();
      await expect(page.locator('#receipts-title')).toBeFocused();
    }
  });
  const banner = await peer.next(() => page.locator('.protection-banner button').click(), 'business', 'protection_overview');
  await peer.local(async () => {
    for (const tile of await page.locator('.protection-tile').all()) {
      await navigate(page, 'overview'); await tile.click();
      await expect(page.locator('#receipts-title')).toBeFocused();
    }
  });
  const retry = await peer.next(() => page.locator('.protection-banner button').click(), 'business', 'protection_overview');
  const newer = await peer.next(() => page.locator('.receipt-section .panel-heading button').click(), 'business', 'protection_overview');
  peer.reply(banner, { status: 'failed', code: 'unavailable', message: 'stale-overview' });
  peer.reply(retry, { status: 'failed', code: 'unavailable', message: 'stale-overview-retry' });
  peer.reply(newer, overviewResult(driverRead, failed('unavailable', 'flow-unavailable')));
  await expect(page.locator('#overview')).toContainText('flow-unavailable');
  await expect(page.locator('#overview')).not.toContainText('stale-overview');
  await peer.local(async () => {
    await navigate(page, 'overview'); await page.locator('.protection-tile').last().click();
    await expect(page.locator('#receipts-title')).toBeFocused();
  });
  await disclosures(page.locator('#overview'), peer);
  // Establish the same payload before/after the layout check; genuinely changed content has a different reading layout.
  const stable = await peer.next(() => page.getByRole('button', { name: t('header.refresh'), exact: true }).click(), 'business', 'protection_overview');
  peer.reply(stable, overviewResult());
  await expect(page.locator('.receipt-card').last()).toContainText('18446744073709551615');
  const scroller = page.locator('.dashboard-main');
  await scroller.evaluate(e => { e.scrollTop = e.scrollHeight; });
  const position = await scroller.evaluate(e => e.scrollTop);
  const previousHeight = await page.locator('#overview').evaluate(e => e.getBoundingClientRect().height);
  const anchorBefore = await page.locator('#receipts-title').evaluate(e => e.getBoundingClientRect().top);
  const last = await peer.next(() => page.getByRole('button', { name: t('header.refresh'), exact: true }).click(), 'business', 'protection_overview');
  expect(await page.locator('#overview').evaluate(e => e.getBoundingClientRect().height)).toBeGreaterThanOrEqual(previousHeight);
  peer.reply(last, overviewResult());
  await expect(page.locator('.receipt-card').last()).toContainText('18446744073709551615');
  // A different final response can be genuinely shorter; only the native end-of-content clamp is valid.
  const maximum = await scroller.evaluate(e => Math.max(0, e.scrollHeight - e.clientHeight));
  const actual = await scroller.evaluate(e => e.scrollTop);
  const anchorAfter = await page.locator('#receipts-title').evaluate(e => e.getBoundingClientRect().top);
  await test.info().attach('overview-layout.json', { body: JSON.stringify({ position, actual, maximum, anchorBefore, anchorAfter }), contentType: 'application/json' });
  // Wrapping status text above the reader can activate native scroll anchoring. Keep its visual anchor or the actual end clamp.
  expect.soft(Math.abs(anchorAfter - anchorBefore) <= 1 || actual === Math.min(position, maximum),
    'refresh preserves the visual reading anchor or clamps to the actual content end: ' + JSON.stringify({position,actual,maximum,anchorBefore,anchorAfter})).toBe(true);
  const processes = await peer.next(() => page.locator('#protection-range a[href="#protection-targets"]').click(), 'message', 'processes');
  expect(processes.body.command.query.type).toBe('list');
});
