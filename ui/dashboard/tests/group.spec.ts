import { test, expect, unlock, navigate, disclosures, ok } from './fixtures';
import { delegationRow, historyReceipt, groupResult, parentKey, childKey, instanceDetails, trackedRecord,
  historyExchange, messagesResult } from '../test-support/contract-supply';
import { idLabel } from '../src/wire';
import { parseUnique } from '../src/wire-json';

test('Group all nodes/details, ProcessKey Get, receipt links, refresh and pagination', async ({ page, peer, t }) => {
  await unlock(page, peer);
  const request = await peer.next(() => navigate(page, 'group'), 'business', 'group_delegations');
  expect(request.body).toEqual({ type: 'group_delegations', before: null, limit: 8 });
  // Contract supply only. Public observations retain their original JSON values and IDs.
  const receipt = historyReceipt();
  if (receipt.kind !== 'openclaw_tool_receipt') throw Error('expected receipt supply');
  const reference = { id: '1', capture: Array(32).fill(3), from_client: false as const,
    frames: [{ offset: '4294967297', length: '81' }] };
  receipt.gateway = {
    accepted_spawns: [{ reference, fact: { parent_run_id: 'parent-run', parent_session_key: 'parent-session',
      parent_agent_id: 'main', tool_call_id: 'original-call|gateway', child_run_id: receipt.child_run_id,
      child_session_key: receipt.child_session_key, event_sequence: 0 } }],
    child_terminals: [{ reference: { ...reference, id: '2' }, fact: { source: 'lifecycle', run_id: receipt.child_run_id,
      session_key: receipt.child_session_key, phase: 'end', execution_settled: true, status: null,
      outcome: { empty: '', values: [0, false, [], ['']] }, aborted: false, stop_reason: '',
      sequence: 18446744073709551615n } }],
    parent_publications: [{ reference: { ...reference, id: '3' }, fact: {
      run_id: `announce:v1:${receipt.child_session_key}:${receipt.child_run_id}`, session_key: 'parent-session', yielded: false } }],
  };
  receipt.gaps = [];
  const a2a = delegationRow('10');
  a2a.delegation = { ...a2a.delegation, gap: 'trace_handoff_unavailable' };
  const records = [a2a, receipt];
  const result = groupResult({ records, next_before: '9' });
  peer.reply(request, result);
  await expect(page.locator('.group-call')).toHaveCount(2);
  await expect(page.locator('.group-node')).toHaveCount(3);
  await expect(page.locator('.group-call').first()).toContainText(t('group.gapHandoff'));
  await expect(page.locator('.group-call').last()).toContainText(t('group.publicEvidence', { receipts: 1, terminals: 1, publications: 1 }));
  await expect(page.locator('.group-call').last()).toContainText(t('group.noStageGap'));
  await expect(page.locator('.group-candidate > code')).toHaveText([receipt.child_run_id, receipt.child_session_key]);
  for (const [index, original] of records.entries()) {
    const summary = page.locator('.group-call').nth(index).getByText(t('group.original'), { exact: true });
    await peer.local(async () => {
      await summary.click();
      expect(parseUnique(await summary.locator('..').locator(':scope > pre').innerText())).toEqual(original);
      await summary.click();
      await expect(summary.locator('..')).not.toHaveAttribute('open', '');
    });
  }
  const replySummary = page.locator('#group').getByText(t('processes.originalReply'), { exact: true });
  await peer.local(async () => {
    await replySummary.click();
    expect(parseUnique(await replySummary.locator('..').locator(':scope > pre').innerText())).toEqual({
      id: request.id, result: { status: 'completed', data: { type: 'group_delegations', page: { records, next_before: '9' } } },
    });
    await replySummary.click();
  });
  await disclosures(page.locator('#group'), peer);
  for (const [i, identity] of [parentKey, childKey, parentKey].entries()) {
    const node = page.locator('.group-node').nth(i);
    const keyFields = [idLabel(identity.boot_id), String(identity.host_tgid), identity.birth_boottime_ns];
    await expect(node.locator('dd')).toHaveText(keyFields);
    await peer.local(() => node.click());
    await expect(page.locator('#group-detail-title')).toBeFocused();
    await expect(page.locator('#group-detail > dl dd')).toHaveText(keyFields);
    const related = i === 1 ? [a2a] : records;
    await expect(page.locator('#group-detail .group-reference')).toHaveCount(related.length);
    for (const [index, original] of related.entries()) {
      const summary = page.locator('#group-detail .group-reference').nth(index).getByText(t('group.original'), { exact: true });
      await peer.local(async () => {
        await summary.click();
        expect(parseUnique(await summary.locator('..').locator(':scope > pre').innerText())).toEqual(original);
        await summary.click();
      });
    }
    await disclosures(page.locator('#group-detail'), peer);
    await peer.local(() => page.locator('#group-detail').getByRole('button', { name: t('processes.returnToList'), exact: true }).click());
    await expect(node).toBeFocused();
  }
  await page.locator('.group-node').nth(1).click();
  const get = await peer.next(() => page.getByRole('button', { name: t('group.openProcess'), exact: true }).click(), 'message', 'processes');
  expect(get.body.command.query).toEqual({ type: 'get', key: childKey });
  const record = { ...trackedRecord(), key: childKey };
  peer.reply(get, ok({ type: 'processes', result: { type: 'details', data: instanceDetails(record) } }));
  await expect(page.locator('#process-detail')).toBeVisible();
  await disclosures(page.locator('#protection-targets'), peer);
  const reread = await peer.next(() => page.getByRole('button', { name: t('processes.rereadInstance'), exact: true }).click(), 'message', 'processes');
  expect(reread.body.command.query).toEqual({ type: 'get', key: childKey });
  peer.reply(reread, ok({ type: 'processes', result: { type: 'details', data: instanceDetails(record) } }));
  const list = await peer.next(() => page.locator('#process-detail').getByRole('button', { name: t('processes.returnToList'), exact: true }).click(), 'message', 'processes');
  expect(list.body.command.query.type).toBe('list');
  const revisit = await peer.next(() => navigate(page, 'group'), 'business', 'group_delegations'); peer.reply(revisit, result);
  await page.locator('.group-node').last().click();
  const getAgain = await peer.next(() => page.getByRole('button', { name: t('group.openProcess'), exact: true }).click(), 'message', 'processes');
  expect(getAgain.body.command.query).toEqual({ type: 'get', key: parentKey });
  peer.reply(getAgain, ok({ type: 'processes', result: { type: 'details', data: instanceDetails({ ...record, key: parentKey }) } }));
  const headerReturn = await peer.next(() => page.locator('#protection-targets > .panel-heading').getByRole('button', { name: t('processes.returnToList'), exact: true }).click(), 'message', 'processes');
  expect(headerReturn.body.command.query.type).toBe('list');
  for (const detail of [false, true]) {
    const again = await peer.next(() => navigate(page, 'group'), 'business', 'group_delegations'); peer.reply(again, result);
    await expect(page.locator('.group-node')).toHaveCount(3);
    if (detail) await page.locator('.group-node').last().click();
    const scope = detail ? page.locator('#group-detail') : page.locator('.group-call').last();
    const messages = await peer.next(() => scope.getByRole('button', { name: t('group.openReceiptExchange'), exact: true }).click(), 'business', 'chat_messages');
    expect(messages.body).toEqual({ type: 'chat_messages', scope:{kind:'model_http',exchange: historyExchange}, after: null, limit: 8 });
    expect(peer.requests.slice(peer.requests.indexOf(messages)).some(r => r.body.type === 'chat_exchanges')).toBe(false);
    peer.reply(messages, messagesResult());
    await expect(page.locator('#chat-messages')).toContainText('original user text');
    await peer.local(() => page.locator('#chat-messages').getByRole('button', { name: t('processes.returnToList'), exact: true }).click());
    await expect(page.locator('#chat-messages')).toHaveCount(0);
  }
  const again = await peer.next(() => navigate(page, 'group'), 'business', 'group_delegations'); peer.reply(again, result);
  const next = await peer.next(() => page.getByRole('button', { name: t('processes.nextPage'), exact: true }).click(), 'business', 'group_delegations');
  expect(next.body.before).toBe('9'); peer.reply(next, groupResult({ records: [], next_before: null }));
  await expect(page.getByText(t('group.empty'), { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: t('processes.nextPage'), exact: true })).toBeDisabled();
  const first = await peer.next(() => page.getByRole('button', { name: t('processes.firstPage'), exact: true }).click(), 'business', 'group_delegations');
  expect(first.body.before).toBeNull(); peer.reply(first, result);
});
