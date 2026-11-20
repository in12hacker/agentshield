import { test, expect, unlock, navigate, disclosures, ok, type Request } from './fixtures';
import { historyExchange, anotherExchange, historyMessage, exchangesResult, messagesResult,
  originalSessions, sessionReference } from '../test-support/contract-supply';
import type { ChatMessage } from '../src/chat-wire';

test('chat sessions, exchanges and messages use original cursors, local disclosures and clear late reads', async ({ page, peer, t }) => {
  await unlock(page, peer);
  const exchanges = await peer.next(() => navigate(page, 'chat'), 'business', 'chat_exchanges');
  expect(exchanges.body).toEqual({ type: 'chat_exchanges', session: null, after: null, limit: 8 });
  peer.reply(exchanges, exchangesResult());
  const directory = page.locator('.chat-session-directory');
  const sessions = await peer.next(() => directory.locator('summary').first().click(), 'business', 'chat_sessions');
  expect(sessions.body).toEqual({ type: 'chat_sessions', before: null, limit: 8 }); peer.reply(sessions, ok(originalSessions()));
  await expect(directory.locator('.process-row')).toHaveCount(2);
  // Source disclosure has no query; the directory summary itself is the explicit query owner.
  await disclosures(directory.locator('li').first(), peer);
  await disclosures(directory.locator('li').last(), peer);
  for (let i = 0; i < 2; i++) {
    const filtered = await peer.next(() => directory.locator('.process-row').nth(i).click(), 'business', 'chat_exchanges');
    expect(filtered.body.session).toEqual(sessionReference(String(i)));
    peer.reply(filtered, exchangesResult());
    const read = await peer.next(() => page.locator('#chat-history > .process-layout .process-row').first().click(), 'business', 'chat_messages');
    expect(read.body.scope).toEqual({kind:'model_http',exchange:historyExchange});
    if (i === 0) {
      const message: ChatMessage = { ...historyMessage(), observed_parts: [
        { type: 'text', id: 0, kind: 'plain', text: 'long-original-'.repeat(150) },
        { type: 'tool_call', id: 1, name: '', arguments: null, native_id: null },
        { type: 'tool_result', id: 2, content: { kind: 'json', value: { empty: '', values: [null, '', 0, false] } }, native_id: null },
        { type: 'opaque', id: 3, provider_type: '', reason: '' },
        { type: 'media_ref', id: 4, kind: 'image', safe_label: 'original-media-label' },
      ] };
      peer.reply(read, messagesResult([message], message.id));
      await expect(page.locator('.chat-body').first()).toHaveText('long-original-'.repeat(150));
      await disclosures(page.locator('#chat-messages'), peer);
      const tools = page.locator('#chat-messages .process-controls');
      const next = await peer.next(() => tools.getByRole('button', { name: t('processes.nextPage'), exact: true }).click(), 'business', 'chat_messages');
      expect(next.body.after).toEqual({kind:'model_http',message:message.id}); peer.reply(next, messagesResult([{ ...historyMessage('10'), observed_parts: [] }]));
      await expect(page.locator('#chat-messages')).toContainText(t('chat.noParts'));
      const size = await peer.next(() => tools.locator('select').selectOption('16'), 'business', 'chat_messages');
      expect(size.body).toEqual({ type: 'chat_messages', scope:{kind:'model_http',exchange: historyExchange}, after: null, limit: 16 });
      peer.reply(size, messagesResult([])); await expect(page.locator('#chat-messages')).toContainText(t('chat.emptyMessages'));
      const first = await peer.next(() => tools.getByRole('button', { name: t('processes.firstPage'), exact: true }).click(), 'business', 'chat_messages');
      expect(first.body.after).toBeNull(); peer.reply(first, messagesResult());
    }
    await peer.local(() => page.locator('#chat-messages').getByRole('button', { name: t('processes.returnToList'), exact: true }).click());
    if (i === 1) peer.reply(read, messagesResult());
    await expect(page.locator('#chat-messages')).toHaveCount(0);
  }
  const all = await peer.next(() => directory.getByRole('button', { name: t('chat.allExchanges'), exact: true }).click(), 'business', 'chat_exchanges');
  expect(all.body.session).toBeNull(); peer.reply(all, exchangesResult());
  const other = await peer.next(() => page.locator('#chat-history > .process-layout .process-row').last().click(), 'business', 'chat_messages');
  expect(other.body.scope).toEqual({kind:'model_http',exchange:anotherExchange}); peer.reply(other, messagesResult([historyMessage('0', anotherExchange)]));
  await expect(page.locator('#chat-messages')).toContainText('original user text');
  await peer.local(() => page.locator('#chat-messages').getByRole('button', { name: t('processes.returnToList'), exact: true }).click());
  const topTools = page.locator('#chat-history > .process-controls');
  const nextExchange = await peer.next(() => topTools.getByRole('button', { name: t('processes.nextPage'), exact: true }).click(), 'business', 'chat_exchanges');
  expect(nextExchange.body.after).toEqual(anotherExchange); peer.reply(nextExchange, exchangesResult([], null));
  await expect(page.locator('#chat-history')).toContainText(t('chat.emptyExchanges'));
  const size = await peer.next(() => topTools.locator('select').selectOption('16'), 'business', 'chat_exchanges');
  expect(size.body.limit).toBe(16); peer.reply(size, exchangesResult());
  const first = await peer.next(() => topTools.getByRole('button', { name: t('processes.firstPage'), exact: true }).click(), 'business', 'chat_exchanges');
  expect(first.body.after).toBeNull(); peer.reply(first, exchangesResult());
  const nextSession = await peer.next(() => directory.getByRole('button', { name: t('processes.nextPage'), exact: true }).click(), 'business', 'chat_sessions');
  expect(nextSession.body.before).toEqual(sessionReference('1')); peer.reply(nextSession, ok({ type: 'chat_sessions', sessions: [], next_before: null }));
  await expect(directory).toContainText(t('chat.noSessions'));
  const refresh = await peer.next(() => directory.getByRole('button', { name: t('common.refresh'), exact: true }).click(), 'business', 'chat_sessions');
  expect(refresh.body.before).toBeNull();
  await navigate(page, 'overview'); peer.reply(refresh, ok(originalSessions()));
  await expect(page.locator('.chat-message')).toHaveCount(0);
});

for (const access of ['read_only', 'manage_rules']) test(`${access}: content permission blocks chat/Group, service permission gates activity and writes`, async ({ page, peer, t }) => {
  peer.access = access; peer.content = false;
  await unlock(page, peer);
  await peer.local(() => navigate(page, 'chat'));
  await peer.local(() => page.locator('.chat-session-directory summary').click());
  for (const control of await page.locator('#chat-history button, #chat-history select').all()) await expect(control).toBeDisabled();
  await expect(page.locator('#chat-history')).toContainText(t('chat.noGrant'));
  await peer.local(() => navigate(page, 'group'));
  await expect(page.locator('#group button')).toBeDisabled();
  await peer.local(() => navigate(page, 'activity'));
  await expect(page.locator('#activity')).toContainText(t('activity.noGrant'));
  const rules = await peer.next(() => navigate(page, 'rules'), 'message', 'rules');
  expect(rules.body.command.type).toBe('get');
  if (access === 'read_only') await expect(page.getByRole('button', { name: t('rules.createGroup'), exact: true })).toHaveCount(0);
  const settings = await peer.next(() => navigate(page, 'settings'), 'message', 'configuration');
  expect(settings.body.command.type).toBe('get');
  await expect(page.getByRole('button', { name: t('settings.edit'), exact: true })).toHaveCount(0);
  await navigate(page, 'egress');
  await expect(page.getByRole('button', { name: t('egress.ensureCa'), exact: true })).toHaveCount(0);
  expect(peer.requests.some(r => r.lane === 'private')).toBe(false);
});


// All four query families consume the one Main ContentAccess grant. This is a browser/transport test.
for (const target of ['group', 'chat', 'ask', 'activity'] as const) test(`${target}: a matched content denial stops all content pages while Ask and management remain available`, async ({ page, peer, t }) => {
  const supply = await import('../test-support/contract-supply');
  await unlock(page, peer);
  let denied: Request | undefined;
  let subscription: Request | undefined;
  const failure = { status: 'failed', code: 'unauthorized', message: 'original content grant denial' };
  const sample = { ...supply.ask, remaining_ms: 60_000 };
  if (target === 'group') denied = await peer.next(() => navigate(page, 'group'), 'business', 'group_delegations');
  if (target === 'chat') denied = await peer.next(() => navigate(page, 'chat'), 'business', 'chat_exchanges');
  if (target === 'activity') {
    const log = await peer.next(() => navigate(page, 'activity'), 'message', 'security_log');
    const logPage = supply.logPage();
    peer.reply(log, ok({ type: 'security_log', page: logPage }));
    await expect(page.locator('#activity .process-row')).toHaveCount(logPage.records.length);
    for (const row of await page.locator('#activity .process-row').all()) {
      let context: Request | undefined;
      const subscribed = await peer.next(async () => {
        context = await peer.next(() => row.click(), 'business', 'activity_context');
      }, 'stream', 'subscribe');
      expect(subscribed.body).toEqual({ type: 'subscribe', filter: {}, after: null });
      expect(context!.body).toEqual({ type: 'activity_context', anchor: { kind: 'event', value: supply.eventAnchor }, attribution: null });
      peer.reply(subscribed, { status: 'subscribed', subscription_id: subscribed.id });
      await expect(page.locator('#activity-detail-title')).toBeFocused();
      await expect(row).toHaveAttribute('aria-expanded', 'true');
      const retired = await peer.next(() => page.locator('#activity-detail').getByRole('button', { name: t('processes.returnToList'), exact: true }).click(), 'stream', 'unsubscribe');
      expect(retired.body).toEqual({ type: 'unsubscribe', subscription_id: subscribed.id });
      peer.reply(retired, { status: 'unsubscribed', subscription_id: subscribed.id });
      peer.reply(context!, supply.contextResult()); // The closed selection cannot consume this late SQL reply.
      await expect(row).toBeFocused();
    }
    subscription = await peer.next(async () => {
      denied = await peer.next(() => page.locator('#activity .process-row').first().click(), 'business', 'activity_context');
    }, 'stream', 'subscribe');
  }
  if (target === 'ask') {
    subscription = await peer.next(async () => {
      denied = await peer.next(async () => {
        peer.push(sample); await expect(page.locator('.review-dialog')).toBeVisible();
      }, 'business', 'ask_context');
    }, 'stream', 'subscribe');
    expect(denied!.body).toEqual({ type: 'ask_context', decision_id: sample.decision_id, attribution: null });
    await peer.local(() => page.locator('.ask-intent > summary').click());
  }
  if (!denied) throw Error('missing test query');
  const denial = denied;
  if (subscription) {
    expect(subscription.body).toEqual({ type: 'subscribe', filter: {}, after: null });
    peer.reply(subscription, { status: 'subscribed', subscription_id: subscription.id });
    const retired = await peer.next(async () => peer.reply(denial, failure), 'stream', 'unsubscribe');
    expect(retired.body).toEqual({ type: 'unsubscribe', subscription_id: subscription.id });
    peer.reply(retired, { status: 'unsubscribed', subscription_id: subscription.id });
  } else peer.reply(denied, failure);
  if (target === 'activity') await page.locator('#activity-detail').getByText(t('activity.reply'), { exact: true }).click();
  await expect(page.getByText('original content grant denial', { exact: false }).first()).toBeVisible();
  const contentQueries = new Set(['group_delegations', 'chat_sessions', 'chat_exchanges', 'chat_messages', 'ask_context', 'activity_context']);
  const contentCount = () => peer.requests.filter(request => request.lane === 'business' && contentQueries.has(request.body.type)).length;
  const before = contentCount();
  if (target !== 'ask') { peer.push(sample); await expect(page.locator('.review-dialog')).toBeVisible(); }
  await page.locator('.choice-buttons').getByRole('button', { name: t('choice.allow'), exact: true }).click();
  expect(peer.events.filter((event: any) => event.payload?.type === 'answer')).toHaveLength(1);
  peer.push(supply.completed());
  await page.getByRole('button', { name: t('ask.closeCompleted'), exact: true }).click();
  for (const destination of ['group', 'chat', 'group', 'chat'] as const) {
    await navigate(page, destination);
    await expect(page.locator(destination === 'group' ? '#group' : '#chat-history')).toContainText(t(destination === 'group' ? 'group.noGrant' : 'chat.noGrant'));
  }
  expect(contentCount()).toBe(before);
  const config = await peer.next(() => navigate(page, 'settings'), 'message', 'configuration');
  peer.reply(config, ok({ type: 'configuration', snapshot: supply.configSnapshot }));
  await expect(page.getByRole('button', { name: t('settings.edit'), exact: true })).toBeEnabled();
  await page.getByRole('button', { name: t('header.lock'), exact: true }).click();
  await unlock(page, peer);
  const fresh = await peer.next(() => navigate(page, 'chat'), 'business', 'chat_exchanges');
  peer.reply({ ...denied, socket: peer.sockets.at(-1)! }, failure);
  peer.reply(fresh, exchangesResult());
  await expect(page.locator('#chat-history > .process-layout .process-row')).toHaveCount(2);
  await expect(page.locator('#chat-history')).not.toContainText(t('chat.noGrant'));
});
