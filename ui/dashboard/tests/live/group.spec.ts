import type { Locator } from '@playwright/test';
import { test, expect, navigate, requireLive, same, type Exchange, type LiveObservation } from './fixtures';
import { disclosures } from '../browser-controls';
import { decodeGroupData, matchesGroupPage, type GroupDelegation, type GroupPage } from '../../src/group-wire';
import { messageId } from '../../src/context-wire';
import { processIdentity } from '../../src/processes-model';
import type { ProcessKey } from '../../src/processes-wire';
import { idLabel } from '../../src/wire';

const keys = (row: GroupDelegation) => row.kind === 'a2a' ? [row.parent, row.child] : [row.actor];
const keyFields = (key: ProcessKey) => [idLabel(key.boot_id), String(key.host_tgid), key.birth_boottime_ns];

async function observation(live: LiveObservation, scope: Locator, row: GroupDelegation) {
  await expect(scope.locator(':scope > h3, :scope > h4')).toHaveText(live.t('group.record', { id: row.id }));
  const gaps = row.kind === 'a2a' ? row.delegation.gap === null ? [] : [row.delegation.gap] : row.gaps;
  await expect(scope.locator(':scope > .amber code')).toHaveText([...gaps]);
  if (gaps.length === 0) await expect(scope.getByText(live.t('group.noStageGap'), { exact: true })).toBeVisible();
  if (row.kind === 'openclaw_tool_receipt') {
    await expect(scope.getByText(live.t('group.publicEvidence', { receipts: row.gateway.accepted_spawns.length,
      terminals: row.gateway.child_terminals.length, publications: row.gateway.parent_publications.length }), { exact: true })).toBeVisible();
  }
  await expect(scope.getByRole('button', { name: live.t('group.openReceiptExchange'), exact: true }))
    .toHaveCount(row.kind === 'openclaw_tool_receipt' ? 1 : 0);
  await live.openSummary(scope.getByText(live.t('group.original'), { exact: true }), 'group_original_observation', row);
}

async function groupPage(live: LiveObservation, exchange: Exchange, before: string | null = null, pageNumber = 1): Promise<GroupPage> {
  const result = live.result(exchange);
  requireLive(same(exchange.request.frame.business.query, { type: 'group_delegations', before, limit: 8 }), 'GROUP_ORIGINAL_QUERY_DIFFERS');
  requireLive(result.status === 'completed', 'GROUP_ORIGINAL_QUERY_FAILED');
  const current = decodeGroupData(result.data).page;
  requireLive(matchesGroupPage(current, { before, limit: 8 }), 'GROUP_EXCLUSIVE_PAGE_DIFFERS');
  live.source(exchange);
  const page = live.page, cards = page.locator('#group .group-call');
  await expect(cards).toHaveCount(current.records.length);
  await expect(page.getByText(live.t('group.pageCount', { page: pageNumber, records: current.records.length }), { exact: true })).toBeVisible();
  await live.openSummary(page.locator('#group').getByText(live.t('processes.originalReply'), { exact: true }), 'group_decoded_reply', {
    id: exchange.request.frame.business.id,
    result: { status: 'completed', data: { type: 'group_delegations', page: current } },
  });
  if (current.records.length === 0) {
    await expect(page.getByText(live.t('group.empty'), { exact: true })).toBeVisible();
    live.record('group_positive_nodes_and_navigation', 'NOT_OBSERVED', { ...live.refs(exchange), reason: 'this original page returned no records' });
  }
  for (const kind of ['a2a', 'openclaw_tool_receipt'] as const) if (!current.records.some(row => row.kind === kind)) {
    live.record('group_record_kind', 'NOT_OBSERVED', { ...live.refs(exchange), kind });
  }
  for (const [index, row] of current.records.entries()) {
    const card = cards.nth(index), identities = keys(row);
    await observation(live, card, row);
    await expect(card.locator('.group-node')).toHaveCount(identities.length);
    await expect(card.locator('.group-arrow')).toContainText(live.t(row.kind === 'a2a' ? 'group.observedCall' : 'group.acceptanceObserved'));
    await expect(card.locator('.group-candidate > code')).toHaveText(row.kind === 'a2a' ? [] : [row.child_run_id, row.child_session_key]);
    if (row.kind === 'openclaw_tool_receipt') for (const [stage, values] of Object.entries(row.gateway)) {
      if (values.length === 0) live.record('group_gateway_stage', 'NOT_OBSERVED', { ...live.refs(exchange), row_id: row.id, stage });
    }
    for (const [nodeIndex, key] of identities.entries()) {
      const node = card.locator('.group-node').nth(nodeIndex);
      await expect(node.locator('dd')).toHaveText(keyFields(key));
      await live.local(() => node.click());
      const detail = page.locator('#group-detail');
      await expect(page.locator('#group-detail-title')).toBeFocused();
      await expect(detail.locator(':scope > dl dd')).toHaveText(keyFields(key));
      requireLive(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), 'GROUP_DETAIL_HORIZONTAL_OVERFLOW');
      const related = current.records.filter(value => keys(value).some(value => processIdentity(value) === processIdentity(key)));
      await expect(detail.locator('.group-reference')).toHaveCount(related.length);
      for (const [position, original] of related.entries()) await observation(live, detail.locator('.group-reference').nth(position), original);
      await live.local(() => detail.getByRole('button', { name: live.t('processes.returnToList'), exact: true }).click());
      await expect(node).toBeFocused();
      live.record('group_observed_process_detail_return', 'PASS', { ...live.refs(exchange), row_id: row.id, node_index: nodeIndex,
        key, related_row_ids: related.map(value => value.id), full_key_and_original_rows_compared: true });
    }
    live.record('group_row_fields', 'PASS', { ...live.refs(exchange), row_id: row.id, kind: row.kind,
      all_returned_fields_compared: true, scope: 'original observation, not Agent identity or inferred completion' });
  }
  requireLive(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), 'GROUP_PAGE_HORIZONTAL_OVERFLOW');
  const next = page.locator('#group').getByRole('button', { name: live.t('processes.nextPage'), exact: true });
  if (current.next_before === null) await expect(next).toBeDisabled();
  else await expect(next).toBeEnabled();
  return current;
}

test('live Group current original observations and only observed navigation branches', async ({ page, live, t }) => {
  await live.safe('group_original_observations_and_navigation', async () => {
    await live.unlock();
    const enter = async () => {
      const exchange = await live.exchange(() => navigate(page, 'group'), 'business', 'group_delegations');
      return groupPage(live, exchange);
    };
    let current = await enter();
    if (current.records.length === 0) {
      test.info().annotations.push({ type: 'NOT_OBSERVED', description: 'Group returned no current sample; positive functionality not accepted' });
      live.record('group_refresh_and_cursor_branch', 'NOT_OBSERVED', { reason: 'no positive sample; no repeat-until-green query' });
      return;
    }
    // A finite action plan from this page. Returning from another workspace reads once;
    // an absent original row is recorded, never replaced by another row or older page.
    const originals = current.records;
    const locate = (id: string) => {
      const index = current.records.findIndex(row => row.id === id);
      if (index < 0) live.record('group_original_row_on_return', 'NOT_OBSERVED', { row_id: id, reason: 'original row is not in the current returned page' });
      return index;
    };
    for (const original of originals) {
      for (const [nodeIndex, originalKey] of keys(original).entries()) {
        const index = locate(original.id);
        if (index < 0) continue;
        const key = keys(current.records[index]!)[nodeIndex];
        requireLive(same(key, originalKey), 'GROUP_ORIGINAL_ROW_PROCESS_CHANGED');
        await live.local(() => page.locator('.group-call').nth(index).locator('.group-node').nth(nodeIndex).click());
        const get = await live.exchange(() => page.getByRole('button', { name: t('group.openProcess'), exact: true }).click(), 'message', 'processes');
        requireLive(same(get.request.frame.message.command.query, { type: 'get', key }), 'GROUP_PROCESS_GET_NOT_ORIGINAL_KEY');
        const result = live.result(get);
        live.source(get);
        await expect(page.locator('#protection-targets > dl dd')).toHaveText(keyFields(originalKey));
        await live.openSummary(page.locator('#protection-targets').getByText(t('processes.originalReply'), { exact: true }),
          'group_process_original_reply', { id: get.request.frame.message.id, result });
        if (result.status === 'failed') {
          requireLive(result.code === 'not_found', 'GROUP_PROCESS_GET_FAILED');
          await expect(page.locator('#protection-targets .receipt-failure')).toContainText('not_found');
          await expect(page.locator('#process-detail')).toHaveCount(0);
          live.record('group_process_details', 'NOT_OBSERVED', { ...live.refs(get), key, actual_code: result.code, reason: 'original instance not retained; no PID substitution' });
        } else {
          requireLive(result.status === 'completed' && result.data.type === 'processes' && result.data.result.type === 'details'
            && same(result.data.result.data.record.key, key), 'GROUP_PROCESS_DETAILS_NOT_ORIGINAL_KEY');
          const record = result.data.result.data.record;
          await expect(page.locator('#process-detail > dl').first().locator('dd')).toHaveText(keyFields(originalKey));
          await live.openSummary(page.locator('#process-detail').getByText(t('processes.originalRecord'), { exact: true }), 'group_process_original_record', record);
          live.record('group_process_details', 'PASS', { ...live.refs(get), key, entire_original_record_compared: true });
        }
        const list = await live.exchange(() => page.locator('#protection-targets > .panel-heading')
          .getByRole('button', { name: t('processes.returnToList'), exact: true }).click(), 'message', 'processes');
        requireLive(list.request.frame.message.command.query.type === 'list', 'GROUP_PROCESS_RETURN_NOT_LIST');
        live.record('group_process_get_and_list_return', 'PASS', { ...live.refs(get), key, actual_status: result.status,
          list: live.refs(list), list_actual_status: live.result(list).status });
        current = await enter();
      }
      if (original.kind !== 'openclaw_tool_receipt') continue;
      for (const inDetail of [false, true]) {
        const index = locate(original.id);
        if (index < 0) continue;
        const row = current.records[index]!;
        requireLive(row.kind === 'openclaw_tool_receipt' && same(row.receipt, original.receipt), 'GROUP_ORIGINAL_RECEIPT_CHANGED');
        const card = page.locator('.group-call').nth(index);
        if (inDetail) await live.local(() => card.locator('.group-node').click());
        const scope = inDetail ? page.locator('#group-detail .group-reference').filter({
          has: page.getByRole('heading', { name: t('group.record', { id: row.id }), exact: true }),
        }) : card;
        const exchangeKey = messageId(row.receipt.value).exchange, from = live.frames.length;
        const chat = await live.exchange(() => scope.getByRole('button', { name: t('group.openReceiptExchange'), exact: true }).click(), 'business', 'chat_messages');
        requireLive(same(chat.request.frame.business.query, { type: 'chat_messages', scope:{kind:'model_http',exchange: exchangeKey}, after: null, limit: 8 }), 'GROUP_RECEIPT_CHAT_NOT_ORIGINAL_EXCHANGE');
        requireLive(!live.frames.slice(from).some(value => value.direction === 'out' && value.frame.business?.query.type === 'chat_exchanges'), 'GROUP_RECEIPT_CHAT_LIST_FALLBACK');
        await expect(page.locator('#chat-messages > dl dd')).toHaveText([idLabel(exchangeKey.flow_instance), exchangeKey.sequence]);
        const result = live.result(chat);
        live.source(chat);
        if (result.status === 'failed') {
          requireLive(result.code === 'not_found', 'GROUP_RECEIPT_CHAT_FAILED');
          await expect(page.locator('#chat-messages .receipt-failure')).toContainText('not_found');
          live.record('group_receipt_chat_messages', 'NOT_OBSERVED', { ...live.refs(chat), row_id: row.id, in_detail: inDetail, actual_code: result.code });
        } else {
          requireLive(result.data.type === 'chat_messages', 'GROUP_RECEIPT_CHAT_WRONG_TYPE');
          await expect(page.locator('#chat-messages article.chat-message')).toHaveCount(result.data.messages.length);
           for (const [position, entry] of result.data.messages.entries()) {
             requireLive(entry.kind === 'model_http', 'GROUP_RECEIPT_CHAT_SOURCE_DIFFERS');
             await live.openSummary(page.locator('#chat-messages article.chat-message').nth(position).locator(':scope > details > summary'), 'group_receipt_chat_original_message', entry.message);
           }
          if (result.data.messages.length === 0) live.record('group_receipt_chat_messages', 'NOT_OBSERVED', { ...live.refs(chat), row_id: row.id, in_detail: inDetail, reason: 'original exchange returned no messages' });
          await disclosures(page.locator('#chat-messages'), live);
        }
        await live.local(() => page.locator('#chat-messages > .panel-heading').getByRole('button', { name: t('processes.returnToList'), exact: true }).click());
        await expect(page.locator('#chat-messages')).toHaveCount(0);
        live.record('group_receipt_original_exchange_navigation', 'PASS', { ...live.refs(chat), row_id: row.id, in_detail: inDetail,
          exchange: exchangeKey, actual_status: result.status, returned_without_history_fallback: true });
        current = await enter();
      }
    }
    const refresh = await live.exchange(() => page.locator('#group > .panel-heading').getByRole('button', { name: t('processes.firstPage'), exact: true }).click(), 'business', 'group_delegations');
    current = await groupPage(live, refresh);
    live.record('group_explicit_first_page_refresh', 'PASS', live.refs(refresh));
    if (current.next_before !== null) {
      const before = current.next_before;
      const next = await live.exchange(() => page.locator('#group').getByRole('button', { name: t('processes.nextPage'), exact: true }).click(), 'business', 'group_delegations');
      await groupPage(live, next, before, 2);
      live.record('group_original_next_cursor', 'PASS', { ...live.refs(next), before });
    } else {
      await expect(page.locator('#group').getByRole('button', { name: t('processes.nextPage'), exact: true })).toBeDisabled();
      live.record('group_original_next_cursor', 'NOT_OBSERVED', { ...live.refs(refresh), reason: 'no original next_before; disabled control checked' });
    }
  });
});
