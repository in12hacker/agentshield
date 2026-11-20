import { expect, requireLive, same, type LiveObservation } from './fixtures';
import type { Locale } from '../../src/i18n-contract';
import { parseUnique } from '../../src/wire-json';
import type { AskAuditContext } from '../../src/context-wire';
import { decodeChatMessage } from '../../src/chat-wire';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { issuableBinding, readEgressConfig } from '../../src/egress-wire';
import { idLabel } from '../../src/wire';
import { navigate } from '../browser-controls';

export async function activityAndChatOriginals(live: LiveObservation, ask: any) {
  const page = live.page;
  await live.viewport(320);
  const key = { source_instance: ask.event.header.source_instance, sequence_domain: ask.event.header.sequence_domain, sequence: ask.event.header.sequence };
  let log = await live.exchange(() => navigate(page, 'activity'), 'message', 'security_log');
  let records: any[], index: number;
  const cursors: unknown[] = [];
  for (;;) {
    requireLive(live.result(log).status === 'completed', 'CURRENT_SECURITY_LOG_READ_FAILED');
    const current = live.result(log).data.page;
    records = current.records;
    index = records.findIndex(record => record.record.event && same(record.record.decision_id, ask.decision_id)
      && same({ source_instance: record.record.event.header.source_instance, sequence_domain: record.record.event.header.sequence_domain, sequence: record.record.event.header.sequence }, key));
    if (index >= 0 || !current.next_before) break;
    requireLive(!cursors.some(cursor => same(cursor, current.next_before)), 'SECURITY_LOG_CURSOR_REPEATED');
    cursors.push(current.next_before);
    log = await live.exchange(() => page.getByRole('button', { name: live.t('activity.older'), exact: true }).click(), 'message', 'security_log');
  }
  requireLive(index >= 0, 'ORIGINAL_EVENT_NOT_IN_CURRENT_SECURITY_LOG');
  await live.local(() => page.locator('.activity-rows .process-row').nth(index).click());
  const ordinary = await live.exchange(() => page.locator('#activity-detail').getByRole('button', { name: live.t('evidence.read'), exact: true }).click(), 'business', 'activity_context');
  const result = live.result(ordinary);
  requireLive(result.status === 'completed' && same(ordinary.request.frame.business.query.anchor, { kind: 'event', value: key })
    && ordinary.request.frame.business.query.attribution === null, 'ORDINARY_ACTIVITY_ANCHOR_DIFFERS');
  const download = page.waitForEvent('download');
  await live.local(() => page.locator('#activity-evidence-download').click());
  const file = await download;
  const exported = parseUnique(readFileSync((await file.path())!, 'utf8')) as any;
  requireLive(same(exported, { schema: 'agentshield.activity-evidence', version: 1, event_key: key,
    selection: { kind: 'log', id: records[index].id, decision_id: ask.decision_id }, view: result.data.view }), 'ACTUAL_EVIDENCE_DOWNLOAD_DIFFERS');
  live.preserve('activity-downloads', exported);
  live.record('activity_original_event_and_download', 'PASS', { ...live.refs(ordinary), event: key, row_id: records[index].id, filename: file.suggestedFilename(), entire_original_view_compared: true });
  for (const locale of ['ja-JP', 'zh-CN', 'en-US'] as const) { await live.language(locale); await compare(live, 'activity', ask, locale); }
  const exchanges = await live.exchange(() => navigate(page, 'chat'), 'business', 'chat_exchanges');
  requireLive(live.result(exchanges).status === 'completed', 'ACTUAL_CHAT_EXCHANGES_FAILED');
  for (const [index, exchange] of live.result(exchanges).data.exchanges.entries()) {
    const messages = await live.exchange(() => page.locator('#chat-history > .process-layout .process-row').nth(index).click(), 'business', 'chat_messages');
    requireLive(live.result(messages).status === 'completed' && same(messages.request.frame.business.query.scope, {kind:'model_http',exchange}), 'CHAT_ORIGINAL_EXCHANGE_DIFFERS');
    live.source(messages);
    for (const locale of ['ja-JP', 'zh-CN', 'en-US'] as const) {
      await live.language(locale);
      for (const [position, entry] of live.result(messages).data.messages.entries()) {
        requireLive(entry.kind === 'model_http', 'CHAT_MODEL_SOURCE_DIFFERS');
        const message = entry.message;
        const article = page.locator('#chat-messages article.chat-message').nth(position);
        await live.openSummary(article.locator(':scope > details > summary'), 'chat_complete_original_message', message);
        for (const [partIndex, part] of message.observed_parts.entries()) if (part.type === 'tool_call' || part.type === 'tool_result') {
          await live.openSummary(article.locator('.chat-part').nth(partIndex).locator('summary'), 'chat_original_provider_part_reference', part.native_id);
        }
        live.record('chat_actual_message_identity', 'PASS', { ...live.refs(messages), id: message.id, revision: message.revision, role: message.role,
          parts: message.observed_parts.map((part: any) => ({ id: part.id, type: part.type })) });
      }
    }
    await live.local(() => page.locator('#chat-messages').getByRole('button', { name: live.t('processes.returnToList'), exact: true }).click());
  }
  const sessions = await live.exchange(() => page.locator('.chat-session-directory > details > summary').click(), 'business', 'chat_sessions');
  requireLive(live.result(sessions).status === 'completed', 'REAL_SESSION_DIRECTORY_FAILED');
  live.record('chat_actual_session_directory', 'PASS', { ...live.refs(sessions), actual_sessions: live.result(sessions).data.sessions.length });
  if (!live.result(sessions).data.sessions.length) live.record('chat_session_sample', 'NOT_OBSERVED', live.refs(sessions));
}

export async function privateOriginals(live: LiveObservation) {
  const page = live.page;
  const config = await live.exchange(() => navigate(page, 'egress'), 'message', 'configuration');
  const snapshot = live.result(config).data.snapshot;
  const issueConfig = readEgressConfig(snapshot);
  const button = (key: Parameters<typeof live.t>[0]) => page.locator('#egress-trust').getByRole('button', { name: live.t(key), exact: true });
  const ca = await live.privateCommand(() => button('egress.readCa').click(), 'public_ca');
  requireLive(ca.result.status === 'completed' && ca.result.data.type === 'ca', 'CURRENT_CA_READ_FAILED');
  const certificate = ca.result.data.info;
  requireLive(same(certificate.id, issueConfig.flow?.ca)
    && same([...createHash('sha256').update(Buffer.from(certificate.certificate_der)).digest()], certificate.fingerprint_sha256), 'CURRENT_CA_ID_OR_DER_DIGEST_DIFFERS');
  const download = page.waitForEvent('download');
  await live.local(() => button('egress.downloadCa').click());
  const downloaded = await download;
  requireLive(same([...readFileSync((await downloaded.path())!)], certificate.certificate_der), 'CURRENT_CA_DOWNLOAD_DER_DIFFERS');
  live.record('private_current_ca_download', 'PASS', { ...ca.metadata, ca_id: certificate.id, fingerprint_sha256: certificate.fingerprint_sha256, der_bytes: certificate.certificate_der.length });
  const binding = issueConfig.bindings.find(binding => issuableBinding(issueConfig, binding));
  requireLive(binding, 'CURRENT_BINDING_NOT_ISSUABLE');
  await page.locator('#egress-binding').selectOption(idLabel(binding.id));
  await button('egress.reviewIssue').click();
  const issued = await live.privateCommand(() => button('egress.confirmIssue').click(), 'issue_phantom');
  requireLive(issued.result.status === 'completed' && issued.result.data.type === 'phantom_issued', 'ACTUAL_PHANTOM_ISSUE_FAILED');
  const tokenId = issued.result.data.token_id;
  live.record('private_extra_token_issued', 'PASS', { ...issued.metadata, token_id: tokenId, binding: binding.id, never_installed_into_agent: true });
  try {
    requireLive(same(issued.command, { type: 'issue_phantom', binding: binding.id, expected_config: snapshot.version })
      && same(issued.result.data.binding, binding.id), 'ORIGINAL_ISSUE_REQUEST_SCOPE_DIFFERS');
    await expect(page.locator('#egress-once-value')).toHaveAttribute('readonly', '');
    requireLive(await page.locator('#egress-once-value').inputValue() === issued.result.data.value, 'ONCE_VALUE_DIFFERS_FROM_PRIVATE_ORIGINAL');
    await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
    await live.privateLocal('egress_once_copy', button('egress.copy'), () => button('egress.copy').click());
    requireLive(await page.evaluate(value => navigator.clipboard.readText().then(text => text === value), issued.result.data.value), 'ACTUAL_CLIPBOARD_COPY_DIFFERS');
    await page.evaluate(() => navigator.clipboard.writeText(''));
    issued.result.data.value = '';
    await live.privateLocal('egress_once_close', button('egress.closeValue'), () => button('egress.closeValue').click());
    await expect(page.locator('#egress-once-value')).toHaveCount(0);
    for (const locale of ['ja-JP', 'zh-CN', 'en-US'] as const) {
      await live.language(locale);
      const summary = page.locator('.settings-receipt > details > summary');
      await live.privateLocal('egress_scope_open', summary, () => summary.click());
      const scope = summary.locator('..').locator('.egress-scope');
      await expect(scope.locator('dd')).toHaveText([
        ...binding.subjects.flatMap(subject => subject.kind === 'process'
          ? [String(subject.value.host_tgid), idLabel(subject.value.boot_id), subject.value.birth_boottime_ns]
          : [JSON.stringify(subject.value.runtime), JSON.stringify(subject.value.namespace), JSON.stringify(subject.value.id)]),
        binding.slot, idLabel(binding.secret),
      ]);
      await expect(scope.locator('.egress-target code')).toHaveText(binding.routes.flatMap(id => {
        const route = issueConfig.flow?.routes.find(route => same(route.id, id));
        return route ? [idLabel(id), route.origin, route.method, route.path, route.profile] : [idLabel(id)];
      }));
      await live.privateLocal('egress_scope_close', summary, () => summary.click());
      live.record('egress_scope', 'PASS', { ...issued.metadata, configuration_version: snapshot.version, original_binding_and_routes_compared: true });
      await live.privateLocal('egress_issued', button('egress.useIssuedReceipt'), () => button('egress.useIssuedReceipt').click());
      requireLive(await page.locator('#egress-revoke-id').inputValue() === idLabel(tokenId), 'ISSUED_ID_INPUT_DIFFERS');
      live.record('egress_issued_original_id', 'PASS', { ...issued.metadata, matched_same_session_issued_id_in_memory: true });
      await live.privateLocal('egress_revoke_preview', button('egress.reviewRevoke'), () => button('egress.reviewRevoke').click());
      await live.privateLocal('egress_revoke_cancel', button('settings.cancel'), () => button('settings.cancel').click());
    }
  } finally {
    issued.result.data.value = '';
    if (await button('egress.confirmRevoke').isVisible()) await button('settings.cancel').click();
    await page.locator('#egress-revoke-id').fill(idLabel(tokenId));
    await button('egress.reviewRevoke').click();
    const revoked = await live.privateCommand(() => button('egress.confirmRevoke').click(), 'revoke_phantom');
    requireLive(same(revoked.command, { type: 'revoke_phantom', token: tokenId })
      && revoked.result.status === 'completed' && revoked.result.data.type === 'phantom_revoked', 'OWN_EXTRA_TOKEN_REVOKE_NOT_CONFIRMED');
    live.record('private_revoke_only_extra_token', 'PASS', { ...revoked.metadata, token_id: tokenId, revision: revoked.result.data.revision, never_installed_into_agent: true });
  }
}

export async function compare(live: LiveObservation, owner: 'ask' | 'activity', decision: any, locale: Locale) {
  live.stage(`${owner}_explicit_candidates_${locale}`);
  const page = live.page, t = live.t.bind(live);
  const scope = owner === 'ask' ? page.locator('dialog.review-dialog') : page.locator('#activity-detail');
  if (owner === 'ask' && await scope.locator('.ask-intent').getAttribute('open') === null) {
    await live.local(() => scope.locator('.ask-intent > summary').click());
  }
  const panel = scope.locator('.attribution-panel');
  const conditions = panel.locator(':scope > details').filter({ has: page.locator('form') });
  if (await conditions.getAttribute('open') === null) await live.local(() => conditions.locator(':scope > summary').click());
  const form = panel.locator('form');
  for (const [name, action] of [
    ['actor', () => form.locator('[name=actor]').selectOption('same_process')],
    ['lookback_seconds', () => form.locator('[name=lookback_seconds]').fill('300')],
    ['value_pointer_mode', () => form.locator('[name=value_pointer_mode]').selectOption('pointer')],
    ['value_pointer', () => form.locator('[name=value_pointer]').fill('/path')],
    ['limit', () => form.locator('[name=limit]').selectOption('16')],
  ] as const) {
    await live.local(action);
    live.record(`${owner}_comparison_input_${name}`, 'PASS', { local: true, locale, control: await live.describe(form.locator(`[name=${name}]`)) });
  }
  const exchange = await live.exchange(() => form.getByRole('button', { name: t('attribution.compare'), exact: true }).click(), 'business', owner === 'ask' ? 'ask_context' : 'activity_context');
  live.source(exchange);
  const request = exchange.request.frame.business.query;
  requireLive(request.attribution.limit === 16 && request.attribution.after === null && request.attribution.pattern.lookback_ns === '300000000000', 'ATTRIBUTION_INPUT_NOT_PRESERVED');
  requireLive(owner !== 'ask' || same(request.decision_id, decision.decision_id), 'ASK_CONTEXT_ID_CHANGED');
  const result = live.result(exchange);
  requireLive(result.status === 'completed', 'REAL_EXPLICIT_COMPARISON_FAILED');
  const context = owner === 'ask' ? result.data.context : result.data;
  live.record(`${owner}_comparison_submit`, 'PASS', { ...live.refs(exchange), query: request,
    candidates: context.attribution?.candidates.length ?? null, confirmed: owner === 'ask' ? context.trigger_call : null,
    control: await live.describe(form.getByRole('button', { name: t('attribution.compare'), exact: true })) });
  for (const key of ['attribution.originalQuery', 'attribution.originalResult', 'attribution.originalCandidate'] as const) {
    const summaries = panel.getByText(t(key), { exact: true });
    if (!await summaries.count()) live.record(`${owner}_${key}`, 'NOT_OBSERVED', { ...live.refs(exchange), reason: 'actual reply did not mount this data sample' });
    const values = key === 'attribution.originalQuery' ? [request.attribution]
      : key === 'attribution.originalResult' ? [context.attribution] : context.attribution?.candidates ?? [];
    for (const [index, summary] of (await summaries.all()).entries()) await live.openSummary(summary, `${owner}_${key}`, values[index]);
  }
  if (owner === 'ask') {
    const audit = context as AskAuditContext;
    const rawMessages = audit.view.source_records.flatMap(r => r.kind === 'message' ? [r.value] : []);
    const realMessages = rawMessages.flatMap(value => { try { return [decodeChatMessage(value)]; } catch { return []; } });
    live.record('ask_actual_window_sources', 'PASS', { ...live.refs(exchange), roles: realMessages.map(m => m.role.kind),
      candidate_windows: context.candidate_windows.map((w: any) => w.message_refs.length), confirmed_trigger: context.trigger_call });
    const windows = audit.window ? [audit.window, ...audit.candidate_windows] : audit.candidate_windows;
    const returns = audit.view.source_records.flatMap(source => source.kind === 'request_return'
      && same(source.value.event, audit.event) && same(source.value.decision_id, decision.decision_id) ? [source.value] : []);
    live.record('ask_native_return_original', returns.length ? 'PASS' : 'NOT_OBSERVED', { ...live.refs(exchange), records: returns,
      scope: 'same EventKey/DecisionId native return facts only; independent syscall trace and ToolExecution are separate' });
    if (!windows.length) live.record('ask_actual_window_sample', 'NOT_OBSERVED', { ...live.refs(exchange), reason: 'this original context supplied no confirmed or candidate window' });
    const mounted = scope.locator('.ask-message-window');
    await expect(mounted).toHaveCount(windows.length);
    for (const [windowIndex, window] of windows.entries()) {
      const section = mounted.nth(windowIndex);
      const entries = section.locator('.ask-conversation > li');
      await expect(entries).toHaveCount(window.message_refs.length);
      await live.openSummary(section.locator(':scope > details > summary').filter({ hasText: t('auditWindow.references') }), 'ask_window_original', window);
      for (const [index, reference] of window.message_refs.entries()) {
        const entry = entries.nth(index);
        await live.openSummary(entry.locator(':scope > details > summary').filter({ hasText: t('auditWindow.references') }), 'ask_references', reference);
        const originals = rawMessages.filter(m => same(m.id, reference));
        const source = realMessages.find(m => same(m.id, reference));
        const rendered = source && originals.every(m => same(m, source)) && same(reference.exchange, window.exchange);
        if (!rendered) {
          if (originals.length) await live.openSummary(entry.locator(':scope > details > summary').filter({ hasText: t('chat.originalMessage') }), 'ask_message_unresolved_originals', originals);
          live.record('ask_message_sample', 'NOT_OBSERVED', { ...live.refs(exchange), window_index: windowIndex, position: index + 1,
            reference, reason: !originals.length ? 'original message not supplied' : !source ? 'invalid original message shape' : 'conflicting originals or exchange scope mismatch' });
          continue;
        }
        const message = entry.locator('article.chat-message');
        await live.openSummary(message.locator(':scope > details > summary'), `ask_message_original_${source.role.kind}`, source);
        for (const [partIndex, part] of source.observed_parts.entries()) if (part.type === 'tool_call' || part.type === 'tool_result') {
          await live.openSummary(message.locator('.chat-part').nth(partIndex).locator('summary'), `ask_${part.type}_native_id_part_${part.id}`, part.native_id);
        }
        live.record('ask_message_sample', 'PASS', { ...live.refs(exchange), window_index: windowIndex, position: index + 1,
          reference, revision: source.revision, role: source.role, parts: source.observed_parts.map(p => ({ id: p.id, type: p.type })) });
      }
    }
    const operation = scope.locator('.ask-chain .chain-row[data-stage=operation]');
    const summary = operation.locator('.chain-originals > summary');
    const control = await live.describe(summary);
    await live.local(() => summary.click());
    const originals = await operation.locator('.chain-originals > ul > li > pre').allTextContents();
    requireLive(originals.length > 0, 'ASK_OPERATION_ORIGINALS_NOT_MOUNTED');
    let questionSeen = false;
    for (const text of originals) {
      const shown = parseUnique(text) as any;
      const value = Object.hasOwn(shown, 'references') ? shown.value : shown;
      requireLive(audit.view.source_records.some(source => same(source.value, value)) || audit.view.relations.some(relation => same(relation, value))
        || audit.view.gaps.some(gap => same(gap, value)) || same(value, decision), 'ASK_OPERATION_ORIGINAL_DIFFERS_FROM_SOURCE');
      if (same(value, decision)) {
        requireLive(same(shown.references, [{ kind: 'event', value: audit.event }, { kind: 'question', value: decision.decision_id }]), 'ASK_OPERATION_QUESTION_REFERENCES_DIFFER');
        questionSeen = true;
      }
    }
    requireLive(questionSeen, 'ASK_OPERATION_ORIGINAL_QUESTION_NOT_SHOWN');
    await live.local(() => summary.click());
    live.record('ask_operation', 'PASS', { ...live.refs(exchange), originals: originals.length, control, original_values_compared: true });
    if (!realMessages.some((m: any) => m.role.kind === 'tool')) live.record('ask_tool_result_native_id_and_complete_message', 'NOT_OBSERVED', { ...live.refs(exchange), reason: 'actual candidate window has no Tool-role message; shared Chat renderer evidence does not supply this Ask data sample' });
    if (context.trigger_call === null) live.record('ask_confirmed_original_tool_call', 'LIVE_BLOCKED', { ...live.refs(exchange), reason: 'no confirmed tool execution bridge; pattern candidate does not create upstream confirmed original' });
  }
  return exchange;
}
