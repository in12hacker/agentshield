import { readFile } from 'node:fs/promises';
import { test, expect, unlock, navigate, disclosures, ok, contextRead, contextUnsubscribe, type Request } from './fixtures';
import { logPage, eventAnchor, contextResult, contextView, conversationContext, longOriginal, historyMessage, id } from '../test-support/contract-supply';
import type { ContextView } from '../src/context-wire';
import { parseUnique } from '../src/wire-json';
import { idLabel } from '../src/wire';

test('partial native references open the original Activity read slot without an execution bridge', async ({ page, peer, t }) => {
  await unlock(page, peer);
  const initial = await peer.next(() => navigate(page, 'activity'), 'message', 'security_log');
  peer.reply(initial, ok({ type: 'security_log', page: logPage(['10'], false) }));
  const { read, subscription } = await contextRead(peer, () => page.locator('.activity-rows .process-row').click(),
    { type: 'activity_context', anchor: { kind: 'event', value: eventAnchor }, attribution: null });
  peer.reply(subscription, { status: 'subscribed', subscription_id: subscription.id });
  const opening = { ...eventAnchor, sequence_domain: '2', sequence: '1000' };
  const leaving = { ...opening, sequence: '1001' };
  const from = { kind: 'native_observation' as const, value: leaving }, to = { kind: 'native_observation' as const, value: opening };
  const view: ContextView = { ...contextView(), source_records: [{ kind: 'request_handoff', value: {
    observation: leaving, request: opening, operation: 'file_open', actor: { tgid: 7, tid: 8, birth_boottime_ns: '9' },
    observed_boottime_ns: '10', phase: { type: 'work_leave', work_address: '2048' },
  } }], relations: [{ kind: 'native_request_reference', from, to, evidence: [from] }], gaps: [{ at: to, reason: 'not_provided' }] };
  const catchup = await peer.next(async () => peer.reply(read, contextResult(view)), 'business', 'activity_context');
  expect(catchup.body).toEqual(read.body); peer.reply(catchup, contextResult(view));
  const originals = page.locator('.activity-originals');
  await expect(originals).toHaveAttribute('open', '');
  await peer.local(async () => {
    await originals.locator(':scope > summary').click(); await expect(originals).not.toHaveAttribute('open', '');
    await originals.locator(':scope > summary').click(); await expect(originals).toHaveAttribute('open', '');
  });
  const native = page.getByRole('region', { name: t('evidence.nativeLinks') });
  await expect(native).toContainText(t('evidence.originalReference'));
  await expect(native).toContainText(t('evidence.notProvided'));
  let request!: Request;
  const retired = await contextUnsubscribe(peer, subscription, async () => {
    request = await peer.next(() => native.getByRole('button', { name: t('evidence.readNativeObservation'), exact: true }).nth(1).click(), 'business', 'activity_context');
  });
  expect(request.body).toEqual({ type: 'activity_context', anchor: to, attribution: null });
  const nextSubscription = await peer.next(async () => peer.reply(retired, { status: 'unsubscribed', subscription_id: subscription.id }), 'stream', 'subscribe');
  expect(nextSubscription.body).toEqual({ type: 'subscribe', filter: {}, after: null });
  peer.reply(nextSubscription, { status: 'subscribed', subscription_id: nextSubscription.id });
  const nativeView: ContextView = { anchor: to, source_records: [], relations: [], gaps: [{ at: to, reason: 'not_provided' }] };
  const nativeCatchup = await peer.next(async () => peer.reply(request, contextResult(nativeView)), 'business', 'activity_context');
  expect(nativeCatchup.body).toEqual(request.body); peer.reply(nativeCatchup, contextResult(nativeView));
  await expect(page.locator('#activity-detail-title')).toHaveText(t('evidence.nativeObservation'));
  await expect(page.getByRole('region', { name: t('evidence.nativeLinks') })).toContainText(t('evidence.notProvided'));
  await expect(page.locator('#activity-evidence-download')).toHaveCount(0);
  await expect(page.locator('#activity-detail .attribution-panel')).toHaveCount(0);
  const closed = await contextUnsubscribe(peer, nextSubscription, () => page.locator('#activity-detail').getByRole('button', { name: t('processes.returnToList'), exact: true }).click());
  peer.reply(closed, { status: 'unsubscribed', subscription_id: nextSubscription.id });
});

test('activity logs, each row kind, evidence cancel/late/read/download, long excerpts and scroll return', async ({ page, peer, t }) => {
  await unlock(page, peer);
  const initial = await peer.next(() => navigate(page, 'activity'), 'message', 'security_log');
  expect(initial.body.command).toEqual({ before: null, limit: 8 });
  const logs = logPage(['10'], true);
  logs.records = [...logs.records,
    { id: '9', record: { kind: 'file_write_close', reporter_tid: 0, path: [], path_error: null, device_major: 0, device_minor: 0, inode: '0' } },
    { id: '8', record: { kind: 'file_write_close', reporter_tid: 1, path: null, path_error: -2, device_major: 0, device_minor: 0, inode: '1' } },
    { id: '7', record: { kind: 'file_write_close', reporter_tid: 2, path: [255], path_error: null, device_major: 0, device_minor: 0, inode: '2' } }];
  logs.next_before = '7';
  peer.reply(initial, ok({ type: 'security_log', page: logs }));
  await expect(page.locator('.activity-rows .process-row')).toHaveCount(4);
  await disclosures(page.locator('.activity-list-tools'), peer);
  for (let i = 1; i < 4; i++) {
    const row = page.locator('.activity-rows .process-row').nth(i);
    await peer.local(() => row.click());
    await expect(page.locator('#activity-detail-title')).toBeFocused();
    await disclosures(page.locator('#activity-detail'), peer);
    await expect(page.locator('#activity-detail')).toContainText(t('evidence.noAnchor'));
    await peer.local(() => page.locator('#activity-detail').getByRole('button', { name: t('processes.returnToList'), exact: true }).click());
    await expect(row).toBeFocused();
  }
  const row = page.locator('.activity-rows .process-row').first();
  await row.scrollIntoViewIfNeeded();
  const scrollPosition = await page.locator('.dashboard-main').evaluate(e => e.scrollTop);
  const { read, subscription } = await contextRead(peer, () => row.click(),
    { type: 'activity_context', anchor: { kind: 'event', value: eventAnchor }, attribution: null });
  peer.reply(subscription, { status: 'subscribed', subscription_id: subscription.id });
  const cancelled = await contextUnsubscribe(peer, subscription, () => page.getByRole('button', { name: t('evidence.cancelWait'), exact: true }).click());
  peer.reply(cancelled, { status: 'unsubscribed', subscription_id: subscription.id });
  peer.reply(read, contextResult());
  await expect(page.locator('#activity-detail')).toContainText(t('evidence.cancelled'));
  const { read: reread, subscription: currentSubscription } = await contextRead(peer,
    () => page.getByRole('button', { name: t('evidence.reread'), exact: true }).click(),
    { type: 'activity_context', anchor: { kind: 'event', value: eventAnchor }, attribution: null });
  peer.reply(currentSubscription, { status: 'subscribed', subscription_id: currentSubscription.id });
  const context = conversationContext();
  const view = { ...context.view, anchor: { kind: 'event' as const, value: eventAnchor } };
  const catchup = await peer.next(async () => peer.reply(reread, contextResult(view)), 'business', 'activity_context');
  expect(catchup.body).toEqual(read.body); peer.reply(catchup, contextResult(view));
  await expect(page.locator('.conversation-excerpt')).toHaveCount(2);
  const originalMessage = page.locator('[data-stage="upstream"] .chain-message-details');
  await peer.local(async () => {
    await originalMessage.locator(':scope > summary').click();
    await expect(originalMessage.locator('.chat-message')).toContainText(longOriginal);
    await expect(originalMessage.locator('.chat-message')).toContainText(t('chat.toolCall'));
    await expect(originalMessage.getByRole('button', { name: t('auditWindow.openExchange'), exact: true })).toBeVisible();
    await originalMessage.locator(':scope > summary').click();
  });
  for (const excerpt of await page.locator('.conversation-excerpt').all()) {
    await peer.local(async () => {
      await excerpt.getByRole('button', { name: t('ask.expandMessage'), exact: true }).click();
      await expect(excerpt.locator('p')).toHaveText(longOriginal);
      await excerpt.getByRole('button', { name: t('ask.collapseMessage'), exact: true }).click();
      await expect(excerpt.locator('p')).not.toHaveText(longOriginal);
    });
  }
  await disclosures(page.locator('#activity-detail'), peer);
  const download = page.waitForEvent('download'); await peer.local(() => page.locator('#activity-evidence-download').click());
  const file = await download;
  expect(file.suggestedFilename()).toBe(`agentshield-event-${idLabel(eventAnchor.source_instance)}-${eventAnchor.sequence_domain}-${eventAnchor.sequence}.json`);
  const data = parseUnique(await readFile((await file.path())!, 'utf8')) as any;
  expect(data.event_key).toEqual(eventAnchor); expect(data.selection).toEqual({ kind: 'log', id: '10', decision_id: null });
  expect(data.view).toEqual(view);
  const closed = await contextUnsubscribe(peer, currentSubscription, () => page.locator('#activity-detail').getByRole('button', { name: t('processes.returnToList'), exact: true }).click());
  peer.reply(closed, { status: 'unsubscribed', subscription_id: currentSubscription.id });
  await expect(row).toBeFocused();
  expect(await page.locator('.dashboard-main').evaluate(e => e.scrollTop)).toBe(scrollPosition);
  const older = await peer.next(() => page.getByRole('button', { name: t('activity.older'), exact: true }).click(), 'message', 'security_log');
  expect(older.body.command).toEqual({ before: '7', limit: 8 }); peer.reply(older, ok({ type: 'security_log', page: logPage(['6'], false) }));
  await expect(page.getByRole('button', { name: t('activity.older'), exact: true })).toBeDisabled();
  const olderRead = await contextRead(peer, () => page.locator('.activity-rows .process-row').click(),
    { type: 'activity_context', anchor: { kind: 'event', value: eventAnchor }, attribution: null });
  await expect(page.locator('#activity-detail-title')).toBeFocused();
  await peer.local(() => page.locator('#activity-detail').getByRole('button', { name: t('processes.returnToList'), exact: true }).click());
  // A late subscribe ack retires the closed selection instead of starting another SQL read.
  const olderClosed = await contextUnsubscribe(peer, olderRead.subscription, async () => peer.reply(olderRead.subscription,
    { status: 'subscribed', subscription_id: olderRead.subscription.id }));
  peer.reply(olderClosed, { status: 'unsubscribed', subscription_id: olderRead.subscription.id });
  peer.reply(olderRead.read, contextResult());
  await expect(page.locator('#activity-detail')).toHaveCount(0);
  const size = await peer.next(() => page.locator('.activity-controls select').selectOption('16'), 'message', 'security_log');
  expect(size.body.command).toEqual({ before: null, limit: 16 }); peer.reply(size, ok({ type: 'security_log', page: logPage([], false) }));
  await expect(page.locator('#activity')).toContainText(t('activity.empty'));
  const refresh = await peer.next(() => page.locator('.activity-controls button').first().click(), 'message', 'security_log');
  peer.reply(refresh, ok({ type: 'security_log', page: logPage(['10'], false) }));
  const late = await contextRead(peer, () => row.click(),
    { type: 'activity_context', anchor: { kind: 'event', value: eventAnchor }, attribution: null });
  peer.reply(late.subscription, { status: 'subscribed', subscription_id: late.subscription.id });
  const leaving = await contextUnsubscribe(peer, late.subscription, () => navigate(page, 'overview'));
  peer.reply(leaving, { status: 'unsubscribed', subscription_id: late.subscription.id });
  const count = peer.requests.length;
  peer.reply(late.read, contextResult(contextView()));
  peer.push({ type: 'chat', subscription_id: late.subscription.id, envelope: {
    cursor: { service_instance: id, sequence: '1' }, event: { type: 'finished', data: historyMessage() },
  } });
  await expect(page.locator('#activity-detail')).toHaveCount(0);
  await peer.local(async () => {});
  expect(peer.requests).toHaveLength(count);
});
