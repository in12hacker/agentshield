import { test, expect, unlock, navigate, disclosures, ok, failed } from './fixtures';
import { trackedRecord, trackedPage, containerInstance } from '../test-support/contract-supply';
import type { ProcessPage, ProcessRecord } from '../src/processes-wire';
import { idLabel } from '../src/wire';
import { parseUnique } from '../src/wire-json';

test('process list, local views/details, every workload filter, lifecycle, size, paging and expired cursor', async ({ page, peer, t }) => {
  await unlock(page, peer);
  const request = await peer.next(() => navigate(page, 'processes'), 'message', 'processes');
  expect(request.body.command.query).toEqual({ type: 'list', filter: { name: null, executable: null, lifecycle: null, kind: null, workload: null }, page: { limit: 8, cursor: null } });
  const container = trackedRecord();
  const host = trackedRecord('2'); host.latest.process.workload = { kind: 'host' };
  const unknown = trackedRecord('3'); unknown.latest.process.workload = null; unknown.latest.process.name = [];
  const cursor = { instance: Array(16).fill(9), generation: '1', query: Array(32).fill(7), after: container.key, hit_after: null };
  const supply = trackedPage([host, container, unknown], cursor);
  const coverage = async (original: ProcessPage) => {
    const summary = page.locator('.process-coverage summary');
    await peer.local(async () => {
      await summary.click();
      const status = original.status;
      await expect(summary.locator('..')).toHaveAttribute('open', '');
      await expect(summary.locator('..').locator('dd')).toHaveText([
        idLabel(status.instance), status.generation, status.evidence_epoch, original.tree_revision, status.revision,
        String(status.nodes), String(status.retired), String(status.memory_used), status.tracking_epoch,
        status.marker, status.loss_epoch, status.backend_rejected,
      ]);
      await summary.click();
      await expect(summary.locator('..')).not.toHaveAttribute('open', '');
    });
  };
  const originalRecord = async (original: ProcessRecord) => {
    const detail = page.locator('#process-detail');
    await expect(detail.locator(':scope > dl').first().locator('dd')).toHaveText([
      idLabel(original.key.boot_id), String(original.key.host_tgid), original.key.birth_boottime_ns,
    ]);
    const summary = detail.getByText(t('processes.originalRecord'), { exact: true });
    await peer.local(async () => {
      await summary.click();
      expect(parseUnique(await summary.locator('..').locator(':scope > pre').innerText())).toEqual(original);
      await summary.click();
      await expect(summary.locator('..')).not.toHaveAttribute('open', '');
    });
  };
  peer.reply(request, ok({ type: 'processes', result: { type: 'page', data: supply } }));
  await expect(page.locator('.process-row')).toHaveCount(3);
  await coverage(supply);
  await disclosures(page.locator('#protection-targets'), peer);
  for (let i = 0; i < 3; i++) {
    const row = page.locator('.process-row').nth(i);
    await peer.local(() => row.click());
    await expect(page.locator('#process-detail-title')).toBeFocused();
    await originalRecord(supply.records[i]!);
    await disclosures(page.locator('#process-detail'), peer);
    await peer.local(() => page.locator('#process-detail').getByRole('button', { name: t('processes.returnToList'), exact: true }).click());
    await expect(row).toBeFocused();
  }
  await peer.local(async () => {
    await page.getByRole('button', { name: t('processes.containerTab'), exact: true }).click();
    await expect(page.locator('.process-row')).toHaveCount(1);
    const row = page.locator('.process-row');
    await row.click();
    await expect(page.locator('#process-detail-title')).toBeFocused();
    await originalRecord(container);
    await page.locator('#process-detail').getByRole('button', { name: t('processes.returnToList'), exact: true }).click();
    await expect(row).toBeFocused();
    await page.getByRole('button', { name: t('processes.processTab'), exact: true }).click();
    await expect(page.locator('.process-row')).toHaveCount(3);
  });
  for (let i = 0; i < 2; i++) {
    const filter = await peer.next(() => page.getByRole('button', { name: t('processes.filterInstance'), exact: true }).nth(i).click(), 'message', 'processes');
    expect(filter.body.command.query.filter.workload).toEqual(i === 0 ? { kind: 'host' } : { kind: 'container', instance: containerInstance });
    const filtered = trackedPage([i === 0 ? host : container], cursor);
    peer.reply(filter, ok({ type: 'processes', result: { type: 'page', data: filtered } }));
    await coverage(filtered);
    if (i === 0) {
      const row = page.locator('.process-row');
      await peer.local(() => row.click());
      await originalRecord(host);
      await peer.local(() => page.locator('#process-detail').getByRole('button', { name: t('processes.returnToList'), exact: true }).click());
      await expect(row).toBeFocused();
    }
    const clear = await peer.next(() => page.getByRole('button', { name: t('processes.clearFilter'), exact: true }).click(), 'message', 'processes');
    expect(clear.body.command.query.filter.workload).toBeNull();
    peer.reply(clear, ok({ type: 'processes', result: { type: 'page', data: supply } }));
  }
  for (const lifecycle of ['present', 'exit_observed', 'no_longer_present', '']) {
    const change = await peer.next(() => page.getByRole('combobox', { name: t('processes.lifecycle'), exact: true }).selectOption(lifecycle), 'message', 'processes');
    expect(change.body.command.query.filter.lifecycle).toBe(lifecycle || null);
    peer.reply(change, ok({ type: 'processes', result: { type: 'page', data: trackedPage([], cursor) } }));
    await expect(page.getByText(t('processes.emptyPage'), { exact: true })).toBeVisible();
  }
  for (const size of ['1', '4', '16', '32', '8']) {
    const change = await peer.next(() => page.getByRole('combobox', { name: t('processes.pageSize'), exact: true }).selectOption(size), 'message', 'processes');
    expect(change.body.command.query.page).toEqual({ limit: Number(size), cursor: null });
    peer.reply(change, ok({ type: 'processes', result: { type: 'page', data: trackedPage([container], cursor) } }));
    await expect(page.locator('.process-row')).toHaveCount(1);
  }
  const next = await peer.next(() => page.getByRole('button', { name: t('processes.nextPage'), exact: true }).click(), 'message', 'processes');
  expect(next.body.command.query.page.cursor).toEqual(cursor);
  peer.reply(next, failed('cursor_expired', 'original cursor expired'));
  await expect(page.getByRole('alert')).toContainText('original cursor expired');
  const first = await peer.next(() => page.getByRole('button', { name: t('processes.firstPage'), exact: true }).click(), 'message', 'processes');
  expect(first.body.command.query.page.cursor).toBeNull();
  await navigate(page, 'overview');
  peer.reply(first, ok({ type: 'processes', result: { type: 'page', data: supply } }));
  await expect(page.locator('#process-detail')).toHaveCount(0);
});
