import type { Locator } from '@playwright/test';
import { test, expect, navigate, requireLive, same, type LiveObservation } from './fixtures';
import { disclosures } from '../browser-controls';
import { decodeChatData, matchesChatPage, type ChatData, type ChatQuery, type ChatSessionsData,
  type ChatExchangesData, type ChatMessagesData, type ChatMessage, type ChatContent,
  type ChatMessageScope, type ChatMessageEntry } from '../../src/chat-wire';
import type { NativeChatEntry } from '../../src/native-chat-wire';
import { byteText } from '../../src/global/byte-text';
import { idLabel } from '../../src/wire';
import { factsJson } from '../../src/wire-json';

const roleKeys = { system: 'chat.roleSystem', developer: 'chat.roleDeveloper', user: 'chat.roleUser',
  assistant: 'chat.roleAssistant', tool: 'chat.roleTool', other: 'chat.roleOther' } as const;
const partKeys = { text: 'chat.text', tool_call: 'chat.toolCall', tool_result: 'chat.toolResult',
  opaque: 'chat.opaque', media_ref: 'chat.media' } as const;

async function layout(live: LiveObservation, scope: Locator, name: string) {
  const dimensions = await scope.evaluate(element => {
    const root = document.documentElement, bounds = element.getBoundingClientRect();
    return { viewport: root.clientWidth, page: root.scrollWidth, client: element.clientWidth,
      scroll: element.scrollWidth, left: bounds.left, right: bounds.right };
  });
  requireLive(dimensions.page <= dimensions.viewport && dimensions.scroll <= dimensions.client
    && dimensions.left >= 0 && dimensions.right <= dimensions.viewport, 'SESSION_HORIZONTAL_OVERFLOW');
  live.record(name, 'PASS', dimensions);
}

async function read<Query extends ChatQuery>(live: LiveObservation, action: () => Promise<unknown>, query: Query): Promise<Extract<ChatData, { type: Query['type'] }>> {
  const from = live.frames.length;
  const exchange = await live.exchange(action, 'business', query.type);
  requireLive(same(exchange.request.frame.business.query, query), 'SESSION_QUERY_NOT_ORIGINAL');
  live.source(exchange);
  const result = live.result(exchange);
  if (result.status === 'failed') live.record('session_original_read_failure', 'LIVE_BLOCKED', {
    ...live.refs(exchange), query, result, scope: 'actual read failure; no replacement object or retry',
  });
  requireLive(result.status === 'completed', 'SESSION_ORIGINAL_READ_FAILED');
  const data = decodeChatData(result.data);
  requireLive(matchesChatPage(data, query), 'SESSION_ORIGINAL_PAGE_DIFFERS');
  if (query.type === 'chat_messages' && data.type === 'chat_messages') {
    if (query.scope.kind === 'native_gateway') {
      requireLive(!live.frames.slice(from).some(value => value.direction === 'out'
        && value.frame.business?.query?.type === 'chat_exchanges'), 'SESSION_NATIVE_HTTP_EXCHANGE_FALLBACK');
      if (query.after !== null) {
        requireLive(query.after.kind === 'native_gateway', 'SESSION_NATIVE_CURSOR_SOURCE_DIFFERS');
        requireLive(data.next_after === null || data.next_after.kind === 'native_gateway'
          && data.next_after.read_through === query.after.read_through, 'SESSION_NATIVE_READ_THROUGH_CHANGED');
      }
    }
    live.record('session_source_tagged_message_cursor', 'PASS', { ...live.refs(exchange), scope: query.scope,
      after: query.after, next_after: data.next_after, first_page_read: query.after === null,
      read_through_scope: 'native continuation uses the original cutoff, including the terminal page; a first-page read may acquire a new cutoff' });
  }
  live.record('session_original_query_reply', 'PASS', { ...live.refs(exchange), query, actual_type: data.type });
  return data as Extract<ChatData, { type: Query['type'] }>;
}

async function directory(live: LiveObservation, data: ChatSessionsData) {
  const scope = live.page.locator('.chat-session-directory'), rows = scope.locator('.process-list > li');
  await expect(rows).toHaveCount(data.sessions.length);
  for (const [index, original] of data.sessions.entries()) {
    const row = rows.nth(index), button = row.locator('.process-row');
    const labels = button.locator('.chat-session-value > bdi');
    requireLive(original.reference.kind === original.kind, 'SESSION_REFERENCE_SOURCE_DIFFERS');
    if (original.kind === 'model_http') {
      await expect(labels).toHaveCount(original.session.values.length);
      for (const [position, value] of original.session.values.entries()) {
        const header = value.kind === 'header' ? byteText(value.value) : null;
        const shown = value.kind === 'json'
          ? value.value.kind === 'json_string' ? JSON.stringify(value.value.value) : value.value.value
          : header!.kind === 'bytes' ? header!.value : JSON.stringify(header!.value);
        requireLive(await labels.nth(position).textContent() === shown, 'SESSION_ORIGINAL_IDENTITY_LABEL_DIFFERS');
      }
      const field = original.session.field;
      await expect(button.locator(':scope > span')).toHaveText([
        live.t(field.kind === 'header' ? 'chat.sessionHeader' : 'chat.sessionPointer', {
          field: field.kind === 'header' ? field.name : field.pointer,
        }), live.t('chat.sessionCount', { count: original.exchanges }), original.source.origin,
      ]);
      if (original.session.issue === null) await expect(button).toBeEnabled();
      else {
        await expect(button).toBeDisabled();
        await expect(row).toContainText(live.t('chat.sessionUnavailable'));
        live.record('session_selection_with_capture_issue', 'NOT_OBSERVED', { reference: original.reference, issue: original.session.issue });
      }
    } else {
      await expect(labels).toHaveCount(1);
      requireLive(await labels.textContent() === JSON.stringify(original.session.session_id), 'SESSION_NATIVE_ID_LABEL_DIFFERS');
      await expect(button.locator(':scope > span')).toHaveText([live.t('chat.nativeGateway'),
        JSON.stringify(original.session.session_key), live.t('chat.observationCount', { count: original.chat_observations })]);
      await expect(button).toBeEnabled();
    }
    const source = row.locator(':scope > details');
    await live.local(() => source.locator(':scope > summary').click());
    const process = original.kind === 'model_http' ? original.source.actor.process : original.namespace.server;
    if (process) await expect(source.locator(':scope > dl').first().locator('dd')).toHaveText([
      idLabel(process.boot_id), String(process.host_tgid), process.birth_boottime_ns,
    ]);
    else await expect(source).toContainText(live.t('chat.sessionSourceMissing'));
    if (original.kind === 'model_http') requireLive(await source.locator(':scope > dl').last().locator('dd').textContent()
      === original.source.route_label, 'SESSION_ORIGINAL_ROUTE_DIFFERS');
    await live.openSummary(source.getByText(live.t('chat.originalSource'), { exact: true }), 'session_complete_original_source', original.source);
    if (original.kind === 'native_gateway') await live.openSummary(source.getByText(live.t('chat.originalMessage'), { exact: true }),
      'session_complete_original_native_directory_row', original);
    await disclosures(source, { local: async action => {
      await live.local(action);
      await layout(live, source, 'session_source_expanded_layout');
    } });
    await live.local(() => source.locator(':scope > summary').click());
    live.record('session_original_row_fields', 'PASS', { reference: original.reference, kind: original.kind,
      identity: original.session, source: original.source,
      count: original.kind === 'model_http' ? original.exchanges : original.chat_observations,
      namespace: original.kind === 'native_gateway' ? original.namespace : null, complete_original_source_compared: true,
      scope: 'observed application session and source only; not Agent identity or provider Conversation' });
  }
  for (const kind of ['model_http', 'native_gateway'] as const) if (!data.sessions.some(row => row.kind === kind)) {
    live.record('session_directory_source_sample', 'NOT_OBSERVED', { kind, reason: 'this original directory page returned no row of this source' });
  }
  const next = scope.getByRole('button', { name: live.t('processes.nextPage'), exact: true });
  if (data.next_before === null) await expect(next).toBeDisabled();
  else await expect(next).toBeEnabled();
  if (data.sessions.length === 0) {
    await expect(scope).toContainText(live.t('chat.noSessions'));
    live.record('session_directory_rows', 'NOT_OBSERVED', { reason: 'this original query returned no Session rows' });
  }
  await layout(live, scope, 'session_directory_layout');
}

async function exchanges(live: LiveObservation, data: ChatExchangesData, pageNumber: number, limit: number) {
  const scope = live.page.locator('#chat-history'), rows = scope.locator(':scope > .process-layout .process-row');
  await expect(rows).toHaveCount(data.exchanges.length);
  for (const [index, original] of data.exchanges.entries()) await expect(rows.nth(index).locator('dd'))
    .toHaveText([idLabel(original.flow_instance), original.sequence]);
  await expect(scope.locator(':scope > .process-controls select')).toHaveValue(String(limit));
  await expect(scope.locator(':scope > .process-controls')).toContainText(live.t('chat.page', { page: pageNumber }));
  const next = scope.locator(':scope > .process-controls').getByRole('button', { name: live.t('processes.nextPage'), exact: true });
  if (data.next_after === null) await expect(next).toBeDisabled();
  else await expect(next).toBeEnabled();
  if (data.exchanges.length === 0) {
    await expect(scope).toContainText(live.t('chat.emptyExchanges'));
    live.record('session_exchange_rows', 'NOT_OBSERVED', { reason: 'this original page returned no request records' });
  }
  await layout(live, scope, 'session_exchange_page_layout');
}

async function content(scope: Locator, value: ChatContent) {
  const body = scope.locator(':scope > pre.chat-body');
  await expect(body).toHaveCount(1);
  requireLive(await body.textContent() === (value.kind === 'text' ? value.value : factsJson(value.value)), 'SESSION_ORIGINAL_CONTENT_DIFFERS');
}

async function modelMessage(live: LiveObservation, article: Locator, original: ChatMessage) {
  await expect(article.locator(':scope > h3')).toHaveText(live.t(roleKeys[original.role.kind])
    + (original.role.kind === 'other' ? ` · ${JSON.stringify(original.role.value)}` : ''));
  await expect(article.locator(':scope > p.overview-note')).toHaveText(live.t('chat.messageRevision', {
    sequence: original.id.sequence, revision: original.revision,
  }));
  await expect(article.locator(':scope > dl dd')).toHaveText([original.origin, original.state, factsJson(original.quality)]);
  await live.openSummary(article.locator(':scope > details > summary'), 'session_complete_original_message', original);
  const parts = article.locator(':scope > .chat-part');
  await expect(parts).toHaveCount(original.observed_parts.length);
  for (const [index, part] of original.observed_parts.entries()) {
    const shown = parts.nth(index);
    await expect(shown.locator(':scope > h4')).toHaveText(`${live.t(partKeys[part.type])} · ${part.id}`);
    if (part.type === 'text') {
      await expect(shown.locator(':scope > p > code')).toHaveText(part.kind);
      await content(shown, { kind: 'text', value: part.text });
    } else if (part.type === 'tool_call') {
      requireLive(await shown.locator(':scope > p').first().textContent() === `${live.t('chat.toolName')} ${JSON.stringify(part.name)}`,
        'SESSION_ORIGINAL_TOOL_NAME_DIFFERS');
      if (part.arguments === null) {
        await expect(shown.locator(':scope > pre.chat-body')).toHaveCount(0);
        await expect(shown.locator(':scope > .amber')).toHaveText(live.t('chat.argumentsMissing'));
      } else await content(shown, part.arguments);
    } else if (part.type === 'tool_result') await content(shown, part.content);
    else if (part.type === 'opaque') requireLive(await shown.locator(':scope > p > code').textContent()
      === `${JSON.stringify(part.provider_type)} · ${JSON.stringify(part.reason)}`, 'SESSION_ORIGINAL_OPAQUE_PART_DIFFERS');
    else requireLive(await shown.locator(':scope > p').textContent() === `${part.kind} · ${part.safe_label}`, 'SESSION_ORIGINAL_MEDIA_LABEL_DIFFERS');
    if (part.type === 'tool_call' || part.type === 'tool_result') await live.openSummary(
      shown.locator(':scope > details > summary'), 'session_original_part_provider_reference', part.native_id);
  }
  if (original.observed_parts.length === 0) await expect(article).toContainText(live.t('chat.noParts'));
}

async function nativeMessage(live: LiveObservation, article: Locator, original: NativeChatEntry) {
  await expect(article.locator(':scope > h3')).toHaveText(live.t('chat.nativeGateway'));
  const order = original.order;
  await expect(article.locator(':scope > p.overview-note')).toHaveText(live.t(order.kind === 'session_sequence'
    ? 'chat.sessionOrder' : order.kind === 'history_window' ? 'chat.windowOrder' : 'chat.unresolvedOrder')
    + (order.kind === 'session_sequence' ? ` · ${order.sequence.value}` : order.kind === 'history_window'
      ? ` · ${order.window.message.id}:${order.window.pointer} [${order.index}]` : ''));
  await live.openSummary(article.locator(':scope > details > summary').filter({ hasText: live.t('chat.nativeId') }),
    'session_native_original_message_identity_order', { reference: original.reference, native_message_id: original.native_message_id, order });
  const observations = article.locator(':scope > .chat-part');
  await expect(observations).toHaveCount(original.observations.length);
  for (const [index, observation] of original.observations.entries()) {
    const shown = observations.nth(index), role = observation.role;
    await expect(shown.locator(':scope > h4')).toHaveText(live.t(role && role.kind !== 'json' ? roleKeys[role.value.kind] : 'chat.roleMissing'));
    if (role?.kind === 'string') requireLive(await shown.locator(':scope > p > code').textContent()
      === JSON.stringify(role.original), 'SESSION_NATIVE_ORIGINAL_ROLE_DIFFERS');
    if (role?.kind === 'protocol_input') await expect(shown.locator(':scope > p.overview-note')).toHaveText(live.t('chat.protocolInput'));
    if (observation.content === null) {
      await expect(shown.locator(':scope > pre.chat-body')).toHaveCount(0);
      await expect(shown.locator(':scope > .amber')).toHaveText(live.t('chat.contentMissing'));
    } else await content(shown, observation.content);
    await live.openSummary(shown.locator(':scope > details > summary'), 'session_native_complete_original_observation', observation);
    // Equal bodies remain distinct observations. Report their actual references for Root's UX review.
    const earlier = original.observations.slice(0, index).findIndex(value => observation.content !== null && same(value.content, observation.content));
    if (earlier >= 0) live.record('session_native_repeated_body_review', 'PASS', {
      component: 'ui/dashboard/src/chat-message.tsx:ChatHistoryMessage', message_reference: original.reference,
      observation_indices: [earlier, index], observations: [original.observations[earlier], observation],
      identical_observation: same(original.observations[earlier], observation),
      scope: 'actual equal bodies compared, not a UX acceptance; Root owns review of repeated content versus distinct provenance',
    });
  }
  const gaps = article.locator(':scope > details > summary').filter({ hasText: live.t('chat.observationGaps') });
  await expect(gaps).toHaveCount(original.gaps.length ? 1 : 0);
  if (original.gaps.length) await live.openSummary(gaps, 'session_native_original_observation_gaps', original.gaps);
}

async function messages(live: LiveObservation, data: ChatMessagesData, messageScope: ChatMessageScope, pageNumber: number, limit: number) {
  const scope = live.page.locator('#chat-messages'), articles = scope.locator('article.chat-message');
  if (messageScope.kind === 'model_http') await expect(scope.locator(':scope > dl dd')).toHaveText([
    idLabel(messageScope.exchange.flow_instance), messageScope.exchange.sequence,
  ]);
  else {
    await expect(scope.locator(':scope > dl')).toHaveCount(0);
    await expect(scope.locator(':scope > p').first()).toHaveText(live.t('chat.nativeGateway'));
  }
  await expect(scope.locator(':scope > .process-controls select')).toHaveValue(String(limit));
  await expect(scope.locator(':scope > .process-controls')).toContainText(live.t('chat.page', { page: pageNumber }));
  await expect(articles).toHaveCount(data.messages.length);
  for (const [position, original] of data.messages.entries()) {
    if (original.kind === 'model_http') await modelMessage(live, articles.nth(position), original.message);
    else await nativeMessage(live, articles.nth(position), original);
    live.record('session_original_message_fields', 'PASS', { scope: messageScope, position,
      reference: original.kind === 'model_http' ? { kind: 'model_http', message: original.message.id } : original.reference,
      complete_original_value_compared: true, header_role_content_order_and_metadata_compared: true });
  }
  if (data.messages.length === 0) {
    await expect(scope).toContainText(live.t('chat.emptyMessages'));
    live.record('session_message_rows', 'NOT_OBSERVED', { scope: messageScope, reason: 'this original page returned no messages' });
  }
  const next = scope.getByRole('button', { name: live.t('processes.nextPage'), exact: true });
  if (data.next_after === null) await expect(next).toBeDisabled();
  else await expect(next).toBeEnabled();
  await disclosures(scope, { local: async action => {
    await live.local(action);
    await layout(live, scope, 'session_message_expanded_detail_layout');
  } });
  await layout(live, scope, 'session_message_detail_layout');
}

async function messagePages(live: LiveObservation, first: ChatMessagesData, scope: ChatMessageScope, controls: boolean) {
  const page = live.page, tools = page.locator('#chat-messages > .process-controls');
  let observed = first.messages.length > 0;
  await expect(page.locator('#chat-messages-heading')).toBeFocused();
  await messages(live, first, scope, 1, 8);
  if (first.next_after !== null) {
    const next = await read(live, () => tools.getByRole('button', { name: live.t('processes.nextPage'), exact: true }).click(),
      { type: 'chat_messages', scope, after: first.next_after, limit: 8 });
    const reference = (entry: ChatMessageEntry) => entry.kind === 'model_http' ? { kind: 'model_http', message: entry.message.id } : entry.reference;
    requireLive(next.messages.every(entry => !first.messages.some(previous => same(reference(previous), reference(entry)))),
      'SESSION_MESSAGE_REPEATED_ACROSS_ORIGINAL_CURSOR');
    await messages(live, next, scope, 2, 8);
    observed = observed || next.messages.length > 0;
  } else live.record('session_message_next_cursor', 'NOT_OBSERVED', { scope, reason: 'no original next_after; disabled control checked' });
  if (controls) {
    let current = await read(live, () => tools.locator('select').selectOption('16'),
      { type: 'chat_messages', scope, after: null, limit: 16 });
    await messages(live, current, scope, 1, 16);
    observed = observed || current.messages.length > 0;
    current = await read(live, () => tools.getByRole('button', { name: live.t('processes.firstPage'), exact: true }).click(),
      { type: 'chat_messages', scope, after: null, limit: 16 });
    await messages(live, current, scope, 1, 16);
    observed = observed || current.messages.length > 0;
  }
  await live.local(() => page.locator('#chat-messages > .panel-heading').getByRole('button', {
    name: live.t('processes.returnToList'), exact: true,
  }).click());
  await expect(page.locator('#chat-messages')).toHaveCount(0);
  return observed;
}

async function openMessages(live: LiveObservation, data: ChatExchangesData, controls: boolean) {
  const page = live.page;
  let observed = false;
  for (const [index, exchange] of data.exchanges.entries()) {
    const row = page.locator('#chat-history > .process-layout .process-row').nth(index);
    const scope = { kind: 'model_http', exchange } as const;
    const current = await read(live, () => row.click(), { type: 'chat_messages', scope, after: null, limit: 8 });
    await expect(row).toHaveAttribute('aria-expanded', 'true');
    const seen = await messagePages(live, current, scope, controls && index === 0);
    observed = observed || seen;
    await expect(row).toBeFocused();
    await expect(row).toHaveAttribute('aria-expanded', 'false');
    live.record('session_original_exchange_return', 'PASS', { exchange, completed_reads_before_local_return: true });
  }
  return observed;
}

async function selectSessions(live: LiveObservation, data: ChatSessionsData) {
  const page = live.page;
  let observed = false, modelControls = true, nativeControls = true;
  for (const [index, original] of data.sessions.entries()) {
    if (original.kind === 'model_http' && original.session.issue !== null) continue;
    const row = page.locator('.chat-session-directory .process-row').nth(index);
    const session = original.reference;
    if (original.kind === 'native_gateway') {
      const scope = { kind: 'native_gateway', session } as const;
      const first = await read(live, () => row.click(), { type: 'chat_messages', scope, after: null, limit: 8 });
      await expect(row).toHaveAttribute('aria-pressed', 'true');
      await expect(page.locator('#chat-history > .process-layout .process-row')).toHaveCount(0);
      await expect(page.locator('#chat-history > .process-controls')).toHaveCount(0);
      const seen = await messagePages(live, first, scope, nativeControls);
      observed = observed || seen;
      nativeControls = false;
      await expect(page.locator('#chat-heading')).toBeFocused();
      live.record('session_original_native_return', 'PASS', { session, focus: 'chat-heading', completed_reads_before_local_return: true });
      continue;
    }
    const first = await read(live, () => row.click(), { type: 'chat_exchanges', session, after: null, limit: 8 });
    await expect(row).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('.chat-session-directory').getByRole('button', {
      name: live.t('chat.allExchanges'), exact: true,
    })).toHaveAttribute('aria-pressed', 'false');
    await exchanges(live, first, 1, 8);
    const seen = await openMessages(live, first, modelControls);
    observed = observed || seen;
    if (first.next_after !== null) {
      const after = first.next_after;
      const next = await read(live, () => page.locator('#chat-history > .process-controls').getByRole('button', {
        name: live.t('processes.nextPage'), exact: true,
      }).click(), { type: 'chat_exchanges', session, after, limit: 8 });
      await exchanges(live, next, 2, 8);
      const seen = await openMessages(live, next, false);
      observed = observed || seen;
    } else live.record('session_filtered_exchange_next_cursor', 'NOT_OBSERVED', { session, reason: 'no original next_after; disabled control checked' });
    if (modelControls) {
      const tools = page.locator('#chat-history > .process-controls');
      let resized = await read(live, () => tools.locator('select').selectOption('16'),
        { type: 'chat_exchanges', session, after: null, limit: 16 });
      await exchanges(live, resized, 1, 16);
      const seen = await openMessages(live, resized, false);
      observed = observed || seen;
      resized = await read(live, () => tools.getByRole('button', { name: live.t('processes.firstPage'), exact: true }).click(),
        { type: 'chat_exchanges', session, after: null, limit: 16 });
      await exchanges(live, resized, 1, 16);
      const refreshed = await openMessages(live, resized, false);
      observed = observed || refreshed;
      modelControls = false;
    }
  }
  return observed;
}

test('live Session original directory filtering messages and only observed cursor branches', async ({ page, live, t }) => {
  let messageSample = false;
  await live.safe('session_original_directory_and_navigation', async () => {
    await live.unlock();
    const initial = await read(live, () => navigate(page, 'chat'), { type: 'chat_exchanges', session: null, after: null, limit: 8 });
    await exchanges(live, initial, 1, 8);
    const scope = page.locator('.chat-session-directory');
    let current = await read(live, () => scope.locator(':scope > details > summary').click(),
      { type: 'chat_sessions', before: null, limit: 8 });
    await directory(live, current);
    if (!current.sessions.some(original => original.kind === 'native_gateway' || original.session.issue === null)) {
      live.record('session_positive_directory_and_navigation', 'NOT_OBSERVED', { reason: 'original directory returned no selectable rows; no repeat-until-positive query' });
    }
    await live.local(async () => {
      await scope.locator(':scope > details > summary').click();
      await scope.locator(':scope > details > summary').click();
    });
    messageSample = await selectSessions(live, current);
    if (current.next_before !== null) {
      const before = current.next_before;
      current = await read(live, () => scope.getByRole('button', { name: t('processes.nextPage'), exact: true }).click(),
        { type: 'chat_sessions', before, limit: 8 });
      await expect(page.locator('#chat-messages')).toHaveCount(0);
      await directory(live, current);
      const seen = await selectSessions(live, current);
      messageSample = messageSample || seen;
    } else live.record('session_directory_next_cursor', 'NOT_OBSERVED', { reason: 'no original next_before; disabled control checked' });
    // The explicit unassociated source is read even when the Session directory is empty.
    const nativeScope = { kind: 'native_gateway', session: null } as const;
    const native = await read(live, () => scope.getByRole('button', { name: t('chat.unassociatedNative'), exact: true }).click(),
      { type: 'chat_messages', scope: nativeScope, after: null, limit: 8 });
    await expect(scope.getByRole('button', { name: t('chat.unassociatedNative'), exact: true })).toHaveAttribute('aria-pressed', 'true');
    await expect(scope.getByRole('button', { name: t('chat.allExchanges'), exact: true })).toHaveAttribute('aria-pressed', 'false');
    const nativeSeen = await messagePages(live, native, nativeScope, true);
    messageSample = messageSample || nativeSeen;
    await expect(page.locator('#chat-heading')).toBeFocused();
    const tools = page.locator('#chat-history > .process-controls');
    let all = await read(live, () => scope.getByRole('button', { name: t('chat.allExchanges'), exact: true }).click(),
      { type: 'chat_exchanges', session: null, after: null, limit: 8 });
    await expect(scope.getByRole('button', { name: t('chat.allExchanges'), exact: true })).toHaveAttribute('aria-pressed', 'true');
    await exchanges(live, all, 1, 8);
    const allSeen = await openMessages(live, all, true);
    messageSample = messageSample || allSeen;
    all = await read(live, () => tools.locator('select').selectOption('16'), { type: 'chat_exchanges', session: null, after: null, limit: 16 });
    await exchanges(live, all, 1, 16);
    all = await read(live, () => tools.getByRole('button', { name: t('processes.firstPage'), exact: true }).click(),
      { type: 'chat_exchanges', session: null, after: null, limit: 16 });
    await exchanges(live, all, 1, 16);
    if (all.next_after !== null) {
      const after = all.next_after;
      all = await read(live, () => tools.getByRole('button', { name: t('processes.nextPage'), exact: true }).click(),
        { type: 'chat_exchanges', session: null, after, limit: 16 });
      await exchanges(live, all, 2, 16);
    } else live.record('session_all_exchange_next_cursor', 'NOT_OBSERVED', { reason: 'no original next_after; disabled control checked' });
    current = await read(live, () => scope.getByRole('button', { name: t('common.refresh'), exact: true }).click(),
      { type: 'chat_sessions', before: null, limit: 8 });
    await directory(live, current);
    await expect(scope.locator('.process-row[aria-pressed=true]')).toHaveCount(0);
    live.record('session_pending_cancel_and_late_receipts', 'NOT_OBSERVED', {
      reason: 'reads completed before local return/navigation; no pending cancellation or late consumption witnessed',
    });
    await layout(live, page.locator('#chat-history'), 'session_final_page_layout');
    if (live.steps.some(step => step.status === 'NOT_OBSERVED')) test.info().annotations.push({ type: 'NOT_OBSERVED',
      description: 'Missing directory/source/message/cursor samples are recorded individually in private steps; this is not a full-feature PASS' });
  });
  test.skip(!messageSample, 'NOT_OBSERVED: no original messages in the observed source branches; positive message workflow not accepted');
});
