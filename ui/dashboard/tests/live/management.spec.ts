import { test, expect, navigate, requireLive, same, type LiveObservation } from './fixtures';
import { disclosures } from '../browser-controls';
import { groupProcesses } from '../../src/processes-model';
import type { ProcessPage, ProcessRecord } from '../../src/processes-wire';
import { idLabel } from '../../src/wire';

async function coverage(live: LiveObservation, original: ProcessPage) {
  const summary = live.page.locator('.process-coverage summary');
  const control = await live.describe(summary);
  await live.local(async () => {
    await summary.click();
    const status = original.status;
    await expect(summary.locator('..').locator('dd')).toHaveText([
      idLabel(status.instance), status.generation, status.evidence_epoch, original.tree_revision, status.revision,
      String(status.nodes), String(status.retired), String(status.memory_used), status.tracking_epoch,
      status.marker, status.loss_epoch, status.backend_rejected,
    ]);
    await summary.click();
  });
  live.record('process_coverage_fields', 'PASS', { control, tree_revision: original.tree_revision, status: original.status });
}

async function processRow(live: LiveObservation, record: ProcessRecord, index: number, grouping: string) {
  const page = live.page, row = page.locator('#protection-targets .process-row').nth(index);
  const control = await live.describe(row);
  await live.local(() => row.click());
  const detail = page.locator('#process-detail');
  await expect(page.locator('#process-detail-title')).toBeFocused();
  await expect(detail.locator(':scope > dl').first().locator('dd')).toHaveText([
    idLabel(record.key.boot_id), String(record.key.host_tgid), record.key.birth_boottime_ns,
  ]);
  await live.openSummary(detail.getByText(live.t('processes.originalRecord'), { exact: true }), 'process_record_original', record);
  await live.openSummary(detail.getByText(live.t('processes.initialRecord'), { exact: true }), 'process_initial_original', record.initial);
  await live.local(() => detail.getByRole('button', { name: live.t('processes.returnToList'), exact: true }).click());
  await expect(row).toBeFocused();
  live.record('process_local', 'PASS', { key: record.key, grouping, control, complete_record_compared: true, returned_to_trigger: true });
}

test('live current snapshots process pagination and existing empty branches', async ({ page, live, t }) => {
  await live.safe('current_snapshots_and_runtime_reads', async () => {
    await live.unlock();
    const policy = await live.exchange(() => navigate(page, 'rules'), 'message', 'rules');
    requireLive(live.result(policy).status === 'completed', 'CURRENT_RULE_READ_FAILED');
    live.preserve('rules-before', live.result(policy).data.snapshot);
    live.record('rules_current_snapshot', 'PASS', { ...live.refs(policy), groups: live.result(policy).data.snapshot.document.groups.length });
    await disclosures(page.locator('#rules'), live);
    const config = await live.exchange(() => navigate(page, 'settings'), 'message', 'configuration');
    requireLive(live.result(config).status === 'completed', 'CURRENT_CONFIGURATION_READ_FAILED');
    live.preserve('configuration-before', live.result(config).data.snapshot);
    live.record('configuration_current_snapshot', 'PASS', live.refs(config));
    await disclosures(page.locator('#settings'), live);
    const processes = await live.exchange(() => navigate(page, 'processes'), 'message', 'processes');
    requireLive(live.result(processes).status === 'completed', 'REAL_PROCESS_LIST_FAILED');
    const first = live.result(processes).data.result.data as ProcessPage;
    live.source(processes);
    await coverage(live, first);
    const rows = page.locator('#protection-targets .process-row');
    await expect(rows).toHaveCount(first.records.length);
    const groups = groupProcesses(first);
    const ordered = [...groups.host, ...groups.containers.flatMap(group => group.records), ...groups.unknown];
    for (const [index, record] of ordered.entries()) await processRow(live, record, index, 'processes');
    if (!groups.host.length) live.record('process_host_row', 'NOT_OBSERVED', { ...live.refs(processes), reason: 'no known Host workload in this page' });
    await live.local(() => page.getByRole('button', { name: t('processes.containerTab'), exact: true }).click());
    const containers = groups.containers.flatMap(group => group.records);
    await expect(rows).toHaveCount(containers.length);
    for (const [index, record] of containers.entries()) await processRow(live, record, index, 'containers');
    if (!containers.length) live.record('process_container_row', 'NOT_OBSERVED', { ...live.refs(processes), reason: 'no known container workload in this page' });
    await live.local(() => page.getByRole('button', { name: t('processes.processTab'), exact: true }).click());
    if (groups.host.length) {
      const filtered = await live.exchange(() => page.locator('.process-group').first().getByRole('button', { name: t('processes.filterInstance'), exact: true }).click(), 'message', 'processes');
      requireLive(same(filtered.request.frame.message.command.query.filter.workload, { kind: 'host' }), 'HOST_FILTER_NOT_ORIGINAL_WORKLOAD');
      live.source(filtered);
      const filteredPage = live.result(filtered).data.result.data as ProcessPage;
      await coverage(live, filteredPage);
      const filteredGroups = groupProcesses(filteredPage);
      for (const [index, record] of filteredGroups.host.entries()) await processRow(live, record, index, 'host-filter');
      await live.exchange(() => page.getByRole('button', { name: t('processes.clearFilter'), exact: true }).click(), 'message', 'processes');
    }
    const size = await live.exchange(() => page.getByRole('combobox', { name: t('processes.pageSize'), exact: true }).selectOption('1'), 'message', 'processes');
    const one = live.result(size).data.result.data;
    requireLive(one.records.length === 1, 'REAL_PROCESS_PAGE_SIZE_NOT_APPLIED');
    live.record('process_page_size', 'PASS', live.refs(size));
    if (one.next_cursor) {
      const next = await live.exchange(() => page.getByRole('button', { name: t('processes.nextPage'), exact: true }).click(), 'message', 'processes');
      const query = next.request.frame.message.command.query;
      requireLive(same(query.page.cursor, one.next_cursor), 'PROCESS_CURSOR_NOT_ORIGINAL');
      live.record('process_next_original_cursor', 'PASS', { ...live.refs(next), actual_status: live.result(next).status });
    } else live.record('process_next_original_cursor', 'NOT_OBSERVED', { reason: 'actual first page has no next_cursor' });
    const group = await live.exchange(() => navigate(page, 'group'), 'business', 'group_delegations');
    const groupResult = live.result(group);
    requireLive(groupResult.status === 'completed', 'EXISTING_GROUP_QUERY_FAILED');
    live.record('existing_group_query', 'PASS', { ...live.refs(group), actual_records: groupResult.data.records.length });
    if (groupResult.data.records.length === 0) live.record('group_node_and_receipt_branches', 'NOT_OBSERVED', { reason: 'this query returned no Group records; implemented passive sources and their delivery evidence do not supply a sample in this instance' });
    const history = await live.exchange(() => navigate(page, 'chat'), 'business', 'chat_exchanges');
    requireLive(live.result(history).status === 'completed', 'REAL_CHAT_EXCHANGES_FAILED');
    live.record('chat_exchange_directory', 'PASS', { ...live.refs(history), actual_exchanges: live.result(history).data.exchanges.length });
    const sessions = await live.exchange(() => page.locator('.chat-session-directory > details > summary').click(), 'business', 'chat_sessions');
    requireLive(live.result(sessions).status === 'completed', 'REAL_SESSION_DIRECTORY_FAILED');
    live.record('chat_session_directory', 'PASS', { ...live.refs(sessions), actual_sessions: live.result(sessions).data.sessions.length });
    if (!live.result(sessions).data.sessions.length) live.record('chat_session_selection', 'NOT_OBSERVED', { reason: 'current service returned no explicit Session records' });
  });
});
