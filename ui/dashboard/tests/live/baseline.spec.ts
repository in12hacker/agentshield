import { test, expect, navigate, requireLive } from './fixtures';
import { idLabel } from '../../src/wire';

test('live authentication and original protection overview', async ({ page, live }) => {
  await live.safe('minimal_real_authentication_and_overview', async () => {
    await live.unlock();
    const initial = await live.exchangeAfter(0, 'business', 'protection_overview');
    const result = live.result(initial);
    requireLive(result.status === 'completed' && result.data.type === 'protection_overview', 'OVERVIEW_NOT_COMPLETED');
    requireLive(result.data.driver.status === 'completed' && result.data.flow.status === 'completed', 'OVERVIEW_COMPONENT_NOT_COMPLETED');
    const driver = result.data.driver.data.snapshot;
    requireLive(driver.source_state === 'running' && driver.self_protection === 'enabled', 'REAL_NATIVE_PROTECTION_NOT_READY');
    await expect(page.locator('#overview .receipt-card')).toHaveCount(2);
    await expect(page.locator('html')).toHaveAttribute('lang', test.info().project.use.locale!);
    live.record('overview_initial', 'PASS', { ...live.refs(initial), source_state: driver.source_state,
      self_protection: driver.self_protection, driver_instance: driver.instance, rule_version: driver.rule_version,
      flow_result_status: result.data.flow.status, scope: 'real status receipt, not original operation effect' });
    const refresh = await live.exchange(() => page.locator('.receipt-section .panel-heading button').click(), 'business', 'protection_overview');
    requireLive(live.result(refresh).status === 'completed', 'OVERVIEW_REFRESH_FAILED');
    live.record('overview_receipt_refresh', 'PASS', live.refs(refresh));
    live.source(refresh);
    const command = page.locator('#protection-receipts > details > summary');
    await live.local(() => command.click());
    await expect(command.locator('..').locator('code')).toHaveText(idLabel(refresh.request.frame.business.id));
    await live.local(() => command.click());
    live.record('overview_original_command_id', 'PASS', live.refs(refresh));
    const summaries = page.locator('#overview .receipt-card summary');
    await live.openSummary(summaries.nth(0), 'overview_original_driver_receipt', live.result(refresh).data.driver);
    await live.openSummary(summaries.nth(1), 'overview_original_flow_receipt', live.result(refresh).data.flow);
    for (const link of await page.locator('#overview .protection-tile, #overview .protection-banner a').all()) {
      await live.local(() => navigate(page, 'overview'));
      await live.local(() => link.click());
      await expect(page.locator('#receipts-title')).toBeFocused();
    }
    const theme = page.getByRole('button', { name: live.t('header.theme'), exact: true });
    const dark = await page.locator('html').evaluate(element => element.classList.contains('dark'));
    await live.local(() => theme.click());
    requireLive(await page.locator('html').evaluate(element => element.classList.contains('dark')) !== dark, 'LOCAL_THEME_DID_NOT_CHANGE');
    await live.local(() => theme.click());
    requireLive(await page.locator('html').evaluate(element => element.classList.contains('dark')) === dark, 'LOCAL_THEME_DID_NOT_RESTORE');
    const menu = page.locator('.menu-button');
    if (await menu.isVisible()) {
      await live.local(() => menu.click());
      await expect(menu).toHaveAttribute('aria-expanded', 'true');
      await expect(page.locator('#dashboard-navigation')).toBeVisible();
      await live.local(() => page.keyboard.press('Escape'));
      await expect(menu).toHaveAttribute('aria-expanded', 'false');
      await expect(menu).toBeFocused();
    }
    live.record('overview_links_and_local_shell_controls', 'PASS', { ...live.refs(refresh), theme_restored: true });
    const from = live.frames.length;
    const connection = refresh.request.connection;
    await page.getByRole('button', { name: live.t('header.lock'), exact: true }).click();
    await expect(page.locator('.gate')).toBeVisible();
    await live.until(() => live.connectionEnded(connection), 'LOCKED_CONNECTION_NOT_CLOSED');
    await expect(page.locator('#management-token')).toBeEmpty();
    await expect(page.locator('.review-dialog')).toHaveCount(0);
    await live.unlock();
    const reread = await live.exchangeAfter(from, 'business', 'protection_overview');
    requireLive(reread.request.connection !== connection && live.result(reread).status === 'completed', 'EXPLICIT_REAUTHENTICATION_NOT_NEW_CONNECTION');
    requireLive(!live.frames.slice(from).some(row => row.direction === 'out' && (row.frame.private
      || row.frame.message && !['review_availability'].includes(row.frame.message.type))), 'REAUTHENTICATION_REPLAYED_BUSINESS_WRITE');
    live.record('management_lock_and_explicit_reauthentication', 'PASS', { ...live.refs(reread), old_connection_closed: true,
      new_connection: reread.request.connection, token_cleared: true, business_write_replayed: false });
  });
});
