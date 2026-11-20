import { test, expect, unlock, navigate, disclosures, ok, contextRead, contextUnsubscribe, type Request } from './fixtures';
import { ask, completed, allow, deny, askContext, askContextResult, conversationContext, longOriginal, rulePolicy, historyMessage, historyExchange, eventAnchor, id } from '../test-support/contract-supply';
import type { ChatMessage } from '../src/chat-wire';
import { bytes, decimalU64, integer, parseUnique, record } from '../src/wire-json';
import type { Locator } from '@playwright/test';
import type { AttributionRequest, ToolAttribution } from '../src/context-wire';
import { dashboardPages } from '../src/global/navigation';

const lateDenial = { status: 'failed', code: 'unauthorized', message: 'late closed content read' };

test('Ask all original choices, context cancellation, long messages, one send and actual completion/evidence', async ({ page, peer, t }) => {
  await unlock(page, peer);
  for (const [index, target] of dashboardPages.entries()) {
    await navigate(page, target.id);
    if (target.id === 'rules') {
      const read = peer.requests.filter(request => request.body.type === 'rules').at(-1)!;
      peer.reply(read, ok({ type: 'rules', snapshot: rulePolicy }));
      await peer.local(() => page.getByRole('button', { name: t('rules.createGroup'), exact: true }).click());
      await expect(page.locator('.rule-dialog')).toBeVisible();
    }
    const sample = { ...ask, decision_id: Array(16).fill(20 + index), remaining_ms: 60_000 };
    const selected = await contextRead(peer, async () => {
      peer.push(sample); await expect(page.locator('.review-dialog')).toBeVisible();
    }, { type: 'ask_context', decision_id: sample.decision_id, attribution: null });
    await expect(page.locator('#review-title')).toBeFocused();
    if (target.id === 'rules') await expect(page.locator('.rule-dialog')).not.toBeVisible();
    await peer.local(async () => { await page.keyboard.press('Escape'); await expect(page.locator('.review-dialog')).toBeVisible(); });
    const completionRead = await peer.next(async () => peer.push({ ...completed(), decision_id: sample.decision_id }), 'business', 'ask_context');
    expect(completionRead.body).toEqual(selected.read.body);
    await expect(page.getByRole('button', { name: t('ask.closeCompleted'), exact: true })).toBeVisible();
    await peer.local(() => page.getByRole('button', { name: t('ask.closeCompleted'), exact: true }).click());
    await expect(page.locator('.review-dialog')).not.toBeVisible();
    // Completion and closing do not wait for the original SQL or subscribe acknowledgement.
    const retired = await contextUnsubscribe(peer, selected.subscription, async () => peer.reply(selected.subscription,
      { status: 'subscribed', subscription_id: selected.subscription.id }));
    peer.reply(retired, { status: 'unsubscribed', subscription_id: selected.subscription.id });
    peer.reply(selected.read, lateDenial);
    peer.reply(completionRead, lateDenial);
    if (target.id === 'rules') {
      await expect(page.locator('.rule-dialog')).toBeVisible();
      await expect(page.locator('.rule-dialog').getByRole('button', { name: t('rules.close'), exact: true })).toBeFocused();
      await peer.local(() => page.locator('.rule-dialog').getByRole('button', { name: t('rules.close'), exact: true }).click());
    }
  }
  // A fresh original identity for each explicit choice; no retry and no fabricated result on send.
  const choices = [allow, deny, { type: 'deny', terminate: 'actor' } as const, { type: 'deny', terminate: 'actor_and_parent' } as const];
  for (let i = 0; i < choices.length; i++) {
    const sample = { ...ask, decision_id: Array(16).fill(i + 50), choices, remaining_ms: 60_000 };
    const selected = await contextRead(peer, async () => {
      peer.push(sample); await expect(page.locator('.review-dialog')).toBeVisible();
    }, { type: 'ask_context', decision_id: sample.decision_id, attribution: null });
    let subscription = selected.subscription;
    if (i === 0) {
      await peer.local(() => page.locator('.ask-intent > summary').click());
      peer.reply(subscription, { status: 'subscribed', subscription_id: subscription.id });
      const cancelled = await contextUnsubscribe(peer, subscription, () => page.getByRole('button', { name: t('evidence.cancelWait'), exact: true }).click());
      peer.reply(cancelled, { status: 'unsubscribed', subscription_id: subscription.id });
      const context = conversationContext(); context.decision_id = sample.decision_id;
      context.view = { ...context.view, anchor: { kind: 'question', value: sample.decision_id }, source_records: context.view.source_records.map(s => s.kind === 'question' ? { ...s, value: sample } : s) };
      peer.reply(selected.read, askContextResult(context));
      await expect(page.locator('div.ask-context-status')).toContainText(t('evidence.cancelled'));
      const current = await contextRead(peer, () => page.getByRole('button', { name: t('evidence.reread'), exact: true }).click(),
        { type: 'ask_context', decision_id: sample.decision_id, attribution: null });
      subscription = current.subscription;
      peer.reply(subscription, { status: 'subscribed', subscription_id: subscription.id });
      const catchup = await peer.next(async () => peer.reply(current.read, askContextResult(context)), 'business', 'ask_context');
      expect(catchup.body).toEqual(selected.read.body); peer.reply(catchup, askContextResult(context));
      await expect(page.locator('.ask-audit-windows .chat-message')).toHaveCount(2);
      const originals = page.locator('.ask-audit-windows .chat-body').filter({ hasText: longOriginal });
      await expect(originals).toHaveCount(2);
      for (const body of await originals.all()) expect(await body.textContent()).toBe(longOriginal);
      await disclosures(page.locator('.ask-audit-windows'), peer);
      await disclosures(page.locator('.ask-chain'), peer);
      await peer.local(() => page.locator('.ask-intent > summary').click());
    }
    const label = i === 0 ? 'choice.allow' : i === 1 ? 'choice.deny' : i === 2 ? 'choice.terminateActor' : 'choice.terminateParent';
    const begin = peer.events.length;
    await page.locator('.choice-buttons').getByRole('button', { name: t(label), exact: i !== 1 }).click();
    await expect(page.locator('.choice-buttons button:disabled')).toHaveCount(4);
    const answers = peer.events.slice(begin).filter((e: any) => e.payload?.type === 'answer') as any[];
    expect(answers).toHaveLength(1); expect(answers[0].payload).toEqual({ type: 'answer', decision_id: sample.decision_id, decision: choices[i] });
    await expect(page.locator('.review-dialog')).toContainText(t('review.sent'));
    const result = { ...completed(i === 0 ? 'allowed' : 'denied'), decision_id: sample.decision_id };
    peer.push({ ...result, event: { ...ask.event, header: { ...ask.event.header, sequence: '7' } } });
    await expect(page.getByRole('button', { name: t('ask.closeCompleted'), exact: true })).toHaveCount(0);
    let completionRead!: Request;
    let retired: Request | undefined;
    const complete = async () => {
      completionRead = await peer.next(async () => peer.push(result), 'business', 'ask_context');
      expect(completionRead.body).toEqual(selected.read.body);
    };
    if (i === 0) retired = await contextUnsubscribe(peer, subscription, complete);
    else await complete();
    if (i === choices.length - 1) {
      const evidence = await peer.next(() => page.getByRole('button', { name: t('evidence.view'), exact: true }).click(), 'business', 'activity_context');
      expect(evidence.body).toEqual({ type: 'activity_context', anchor: { kind: 'event', value: eventAnchor }, attribution: null });
      await expect(page.locator('#activity-detail')).toBeVisible();
      await expect(page.locator('.review-dialog')).not.toBeVisible();
      await peer.local(() => page.locator('#activity-detail').getByRole('button', { name: t('processes.returnToList'), exact: true }).click());
      peer.reply(evidence, lateDenial);
    } else await peer.local(() => page.getByRole('button', { name: t('ask.closeCompleted'), exact: true }).click());
    if (!retired) retired = await contextUnsubscribe(peer, subscription, async () => peer.reply(subscription,
      { status: 'subscribed', subscription_id: subscription.id }));
    peer.reply(retired, { status: 'unsubscribed', subscription_id: subscription.id });
    peer.reply(selected.read, lateDenial);
    peer.reply(completionRead, lateDenial);
  }
});

test('Ask local timeout is not an Answer or a completion; disconnect removes review and never replays', async ({ page, peer, t }) => {
  peer.content = false;
  await unlock(page, peer);
  peer.push({ ...ask, remaining_ms: 1 });
  await expect(page.locator('.choice-buttons button:disabled')).toHaveCount(2);
  expect(peer.events.some((e: any) => e.payload?.type === 'answer')).toBe(false);
  await expect(page.locator('.review-dialog')).toContainText(t('review.expired'));
  peer.push(completed('cancelled'));
  await expect(page.getByRole('button', { name: t('evidence.view'), exact: true })).toBeDisabled();
  await page.locator('.ask-intent > summary').click();
  await expect(page.locator('.ask-chat')).toContainText(t('ask.noGrant'));
  await expect(page.getByRole('button', { name: t('ask.readContext'), exact: true })).toHaveCount(0);
  await disclosures(page.locator('.ask-chain'), peer);
  await page.getByRole('button', { name: t('ask.closeCompleted'), exact: true }).click();
  peer.push({ ...ask, decision_id: Array(16).fill(90), remaining_ms: 60_000 });
  await expect(page.locator('.review-dialog')).toBeVisible();
  peer.sockets.at(-1)!.close({ code: 1000 }); await expect(page.locator('.gate')).toBeVisible();
  await unlock(page, peer); await expect(page.locator('.review-dialog')).not.toBeVisible();
  expect(peer.events.some((e: any) => e.payload?.type === 'answer')).toBe(false);
  expect(peer.requests.some(request => request.lane === 'stream' || request.body.type === 'ask_context')).toBe(false);
});

test('explicit candidates retain all original window messages and navigation keeps the original Ask answer', async ({ page, peer, t }) => {
  await unlock(page,peer);
  const question = { ...ask, remaining_ms: 180000 };
  const dialog=page.locator('dialog.review-dialog');
  const selected = await contextRead(peer, async () => {
    peer.push(question); await expect(dialog).toBeVisible();
  }, { type: 'ask_context', decision_id: question.decision_id, attribution: null });
  peer.reply(selected.subscription, { status: 'subscribed', subscription_id: selected.subscription.id });
  const initialContext = askContext();
  initialContext.view = { ...initialContext.view, source_records: [{ kind: 'question', value: question }] };
  const initialCatchup = await peer.next(async () => peer.reply(selected.read, askContextResult(initialContext)), 'business', 'ask_context');
  expect(initialCatchup.body).toEqual(selected.read.body); peer.reply(initialCatchup, askContextResult(initialContext));
  await dialog.locator('summary').filter({hasText:t('ask.intentDetails')}).click();
  const panel=dialog.locator('.attribution-panel');await panel.locator('summary').click();
  await panel.locator('[name=actor]').selectOption('same_process');
  await panel.locator('[name=lookback_seconds]').fill('0.000000001');
  await panel.locator('[name=value_pointer_mode]').selectOption('pointer');
  await panel.locator('[name=value_pointer]').fill('/path');
  const request=await peer.next(()=>panel.getByRole('button',{name:t('attribution.compare')}).click(),'business','ask_context');
  const query=request.body.attribution as AttributionRequest;
  expect(query).toMatchObject({pattern:{lookback_ns:'1',actor:'same_process',target:{type:'file_path',pointer:'/path'}},after:null,limit:8});
  const actor = { boot_id: bytes(ask.event.host_boot_id, 16),
    host_tgid: integer(ask.event.header.actor_tgid, 0, 0xffffffff),
    birth_boottime_ns: decimalU64(ask.event.header.actor_birth_boottime_ns) };
  const observed = decimalU64(ask.event.header.source_boottime_ns);
  const target = record(ask.event.payload).path as readonly number[];
  const messages: ChatMessage[]=Array.from({length:20},(_,i)=>{
    const original=historyMessage(String(i),historyExchange);
    return { ...original, quality: [], last_seen_boottime_ns: observed,
      source: { ...original.source, actor: { ...record(original.source.actor), process: actor } } };
  });
  messages[0]!.observed_parts=[{type:'text',id:0,kind:'plain',text:''}];
  messages[1]!.role={kind:'tool'};messages[1]!.observed_parts=[{type:'tool_result',id:0,native_id:null,content:{kind:'text',value:'original tool result'}}];
  messages[19] = { ...messages[19]!, role:{kind:'assistant'},origin:'response_output',
    source:{ ...messages[19]!.source, side:'upstream_response' },
    observed_parts:[0,1].map(part_id=>({type:'tool_call',id:part_id,name:'read',
      native_id:{namespace:'tool_call',field_path:`/output/${part_id}/call_id`,value:{kind:'json_string',value:`synthetic-call-${part_id}`}},
      arguments:{kind:'json',value:{path:'/tmp/a'}}})) };
  const response = messages[19]!;
  const attribution: ToolAttribution={event:askContext().event,pattern:query.pattern,
    required_connection_process:actor,required_connection_thread:null,
    event_boottime_ns:observed,event_target:{type:'bytes',value:target},candidates:response.observed_parts.map(part=>({call:{message:response.id,part_id:part.id},
      message_revision:response.revision,response_observed_ns:response.last_seen_boottime_ns,target_comparison:'equal'})),next_after:null};
  const context=askContext();context.attribution=attribution;context.candidate_windows=[{exchange:historyExchange,
    message_refs:messages.map(m=>m.id),earlier_observed_messages:4,source_gaps:[]}];
  const questionSource = { kind: 'question' as const, value: question };
  context.view={...context.view,source_records:[questionSource,
    ...context.view.source_records.filter(source => source.kind !== 'question'),
    ...messages.map(value=>({kind:'message' as const,value}))]};
  // Chat is an invalidation only: pending notifications coalesce, retaining the explicit query.
  const beforeUpdates = peer.requests.length;
  for (const sequence of ['1', '2']) peer.push({ type: 'chat', subscription_id: selected.subscription.id, envelope: {
    cursor: { service_instance: id, sequence }, event: { type: 'finished', data: { ...historyMessage(),
      observed_parts: [{ type: 'text', id: 0, kind: 'plain', text: 'notification is not a SQL window' }] } },
  } });
  await peer.local(async () => {});
  expect(peer.requests).toHaveLength(beforeUpdates);
  const candidateCatchup = await peer.next(async () => peer.reply(request,askContextResult(context)), 'business', 'ask_context');
  expect(candidateCatchup.id).not.toEqual(request.id);
  expect(candidateCatchup.body).toEqual(request.body);
  peer.reply(candidateCatchup, askContextResult(context));
  await expect(dialog.locator('.attribution-candidate')).toHaveCount(2);
  await expect(dialog.locator('.ask-audit-windows .chat-message')).toHaveCount(20);
  await expect(dialog.locator('.ask-audit-windows')).toContainText('original tool result');
  await expect(dialog.locator('.ask-audit-windows')).toContainText(t('auditWindow.candidateNotice'));
  await expect(dialog.locator('.ask-audit-windows')).not.toContainText(t('auditWindow.confirmed'));
  await expect(dialog.locator('.ask-audit-windows')).not.toContainText('notification is not a SQL window');
  expect(peer.requests).toHaveLength(beforeUpdates + 1);
  expect(await dialog.evaluate(el=>el.scrollWidth<=el.clientWidth+1)).toBe(true);
  expect(context.trigger_call).toBeNull();
  expect(context.view.relations.some(relation => relation.kind === 'tool_execution')).toBe(false);
  const operation = dialog.locator('.ask-chain .chain-row[data-stage=operation]');
  const operationOriginal = operation.locator('.chain-originals > summary');
  await peer.local(async () => {
    await operationOriginal.click();
    await expect(operationOriginal.locator('..')).toHaveAttribute('open', '');
    const originals = operation.locator('.chain-originals > ul > li > pre');
    await expect(originals).toHaveCount(1);
    expect(parseUnique(await originals.innerText())).toEqual({ references: [
      { kind: 'event', value: context.event }, { kind: 'question', value: questionSource.value.decision_id },
    ], value: questionSource.value });
    await operationOriginal.click();
    await expect(operationOriginal.locator('..')).not.toHaveAttribute('open', '');
  });
  // Check every position against this reply, including the repeated User rows.
  // This is synthetic contract evidence, not evidence of a live 20-message sample.
  const original = async (summary: Locator, expected: unknown) => {
    await peer.local(async () => {
      await summary.click();
      await expect(summary.locator('..')).toHaveAttribute('open', '');
      expect(parseUnique(await summary.locator('..').locator(':scope > pre').innerText())).toEqual(expected);
      await summary.click();
      await expect(summary.locator('..')).not.toHaveAttribute('open', '');
    });
  };
  const window = dialog.locator('.ask-message-window');
  const entries = window.locator('.ask-conversation > li');
  await expect(entries).toHaveCount(messages.length);
  for (const [index, message] of messages.entries()) {
    const entry = entries.nth(index);
    await original(entry.locator(':scope > details > summary'), message.id);
    await original(entry.locator('article > details > summary'), message);
    for (const [partIndex, part] of message.observed_parts.entries()) {
      if (part.type === 'tool_call' || part.type === 'tool_result') {
        await original(entry.locator('.chat-part').nth(partIndex).locator('summary'), part.native_id);
      }
    }
  }
  await original(window.locator(':scope > details > summary').filter({ hasText: t('auditWindow.references') }), context.candidate_windows[0]);
  let navigation!: Request;
  const retired = await contextUnsubscribe(peer, selected.subscription, async () => {
    navigation = await peer.next(()=>dialog.locator('.ask-message-window').getByRole('button',{name:t('auditWindow.openExchange')}).click(),'business','chat_messages');
  });
  peer.reply(retired, { status: 'unsubscribed', subscription_id: selected.subscription.id });
  expect(navigation.body.scope).toEqual({ kind: 'model_http', exchange: historyExchange });
  peer.reply(navigation,{status:'completed',data:{type:'chat_messages',messages:[{kind:'model_http',message:messages[1]}],next_after:null}});
  await expect(dialog).not.toBeVisible();
  await expect(page.getByRole('button',{name:t('ask.returnToQuestion')})).toBeVisible();
  expect(peer.events.filter((v:any)=>v.direction==='out' && v.lane==='message' && v.payload?.type==='answer')).toHaveLength(0);
  const returning = await contextRead(peer, async () => {
    await page.getByRole('button',{name:t('ask.returnToQuestion')}).click(); await expect(dialog).toBeVisible();
  }, { type: 'ask_context', decision_id: question.decision_id, attribution: null });
  peer.reply(returning.subscription, { status: 'subscribed', subscription_id: returning.subscription.id });
  await dialog.getByRole('button',{name:t('choice.allow'),exact:true}).click();
  await expect.poll(()=>peer.events.filter((v:any)=>v.direction==='out' && v.lane==='message' && v.payload?.type==='answer').length).toBe(1);
  let completionRead!: Request;
  const completedSubscription = await contextUnsubscribe(peer, returning.subscription, async () => {
    completionRead = await peer.next(async () => peer.push(completed('allowed')), 'business', 'ask_context');
  });
  expect(completionRead.body).toEqual(returning.read.body);
  await expect(dialog.getByRole('button',{name:t('ask.closeCompleted')})).toBeVisible();
  await page.screenshot({path:test.info().outputPath('audit-ask.png'),fullPage:true});
  await peer.local(() => dialog.getByRole('button', { name: t('ask.closeCompleted'), exact: true }).click());
  peer.reply(completedSubscription, { status: 'unsubscribed', subscription_id: returning.subscription.id });
  peer.reply(returning.read, askContextResult(initialContext));
  peer.reply(completionRead, askContextResult(initialContext));
  await expect(dialog).not.toBeVisible();
});
