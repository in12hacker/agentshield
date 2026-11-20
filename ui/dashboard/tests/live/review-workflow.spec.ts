import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { test, expect, requireLive, same, LiveFailure, type LiveObservation, type WireObservation } from './fixtures';
import { activityAndChatOriginals, compare, privateOriginals } from './originals';
import { createReviewPolicy, exerciseReviewDrafts, readReviewPolicy, replaceAndRestoreReviewNumber,
  restoreReviewPolicy, reviewRuleId, verifyReviewPolicy } from './review-policy';
import { parseUnique } from '../../src/wire-json';
import type { PolicySnapshot } from '../../src/rules-wire';
import { sameEventKey } from '../../src/event-wire';

const fixture = process.env.AS_LIVE_FIXTURE!;
interface Work {
  caseName: 'file-allow' | 'file-deny'; actorCase: string; actorPage: string; prompt: string; target: string; content: string;
  decision: { type: 'allow' } | { type: 'deny'; terminate: 'none' }; outcome: 'allowed' | 'denied';
}

class OriginalActor {
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly exited: Promise<number | null>;
  private readonly offset: number;
  private submitted = false;
  private exit: { code: number | null; signal: NodeJS.Signals | null } | null = null;
  private spawnFailed = false;
  private eventReadFailed = false;
  private finished = false;
  constructor(private readonly live: LiveObservation, readonly work: Work) {
    const file = join(fixture, 'run/browser-events.jsonl');
    const prior = existsSync(file) ? readFileSync(file) : Buffer.alloc(0);
    requireLive(!prior.toString('utf8').split('\n').filter(Boolean).some(line => {
      const row = JSON.parse(line); return row.case === work.actorCase && row.type === 'request';
    }), 'ACTOR_CASE_ALREADY_SUBMITTED_NO_RESEND');
    this.offset = prior.length;
    this.child = spawn(process.execPath, [join(fixture, 'agent-browser.cjs')], { stdio: ['pipe', 'pipe', 'pipe'] });
    this.child.stdout.resume(); this.child.stderr.resume();
    this.exited = new Promise(resolve => {
      this.child.once('error', () => { this.spawnFailed = true; });
      this.child.once('close', (code, signal) => { this.exit = { code, signal }; resolve(code); });
    });
    this.child.stdin.on('error', () => { this.spawnFailed = true; });
  }
  checkRunning = () => { requireLive(!this.spawnFailed && !this.exit, 'ORIGINAL_ACTOR_PROCESS_ENDED_WHILE_WAITING'); };
  send(value: unknown) { this.child.stdin.write(JSON.stringify(value) + '\n'); }
  events(): any[] {
    const file = join(fixture, 'run/browser-events.jsonl');
    try {
      if (!existsSync(file)) return [];
      return readFileSync(file).subarray(this.offset).toString('utf8').split('\n').slice(0, -1).filter(Boolean).map(line => JSON.parse(line));
    } catch {
      if (!this.eventReadFailed) this.live.record('actor_event_observer', 'OBSERVER_FAILED', { case: this.work.caseName,
        reason: 'event read failed; keep original request alive and observe actual Actor exit' });
      this.eventReadFailed = true;
      return [];
    }
  }
  businessTerminal() {
    return this.events().find(row => row.case === this.work.actorCase && (row.type === 'terminal'
      || row.type === 'reply' && (row.original?.ok === false || row.original?.payload?.runStarted === false || row.original?.payload?.runError != null)));
  }
  async prepare() {
    this.send({ op: 'prepare', pages: [this.work.actorPage] });
    const result = await this.live.until(() => this.events().find(row => row.page === this.work.actorPage && ['prepared', 'browser_error'].includes(row.type)), 'ORIGINAL_ACTOR_PAGE_NOT_PREPARED', this.checkRunning);
    this.live.preserve('actor-prepare', result);
    requireLive(result.type === 'prepared', 'ORIGINAL_ACTOR_PREPARATION_FAILED');
  }
  submit() {
    requireLive(!this.submitted, 'ACTOR_REQUEST_MUST_NOT_BE_REPLAYED');
    this.submitted = true;
    this.send({ op: 'submit', cases: [{ name: this.work.actorCase, page: this.work.actorPage, follow: false, prompt: this.work.prompt }] });
  }
  async terminal() {
    // Dashboard/optional evidence failure must not close a still-running Agent task.
    for (;;) {
      const terminal = this.businessTerminal();
      if (terminal) {
        this.finished = true;
        const rows = this.events().filter(row => row.case === this.work.actorCase);
        this.live.preserve('actor-case-events', { case: this.work.caseName, rows });
        this.live.preserve('actor-terminal', { case: this.work.caseName, original: terminal });
        requireLive(rows.filter(row => row.type === 'request').length === 1, 'ACTOR_SUBMISSION_NOT_EXACTLY_ONCE');
        requireLive(terminal.type === 'terminal', 'ORIGINAL_BUSINESS_START_REJECTED');
        this.live.record('actual_agent_terminal', 'PASS', { case: this.work.caseName, actual_state: terminal.original.payload.state,
          actual_tool_events: rows.filter(row => row.type === 'tool').length, scope: 'terminal and original outputs observed; no guessed wording or tool count' });
        return terminal;
      }
      this.checkRunning();
      await sleep(20);
    }
  }
  async close() {
    try { if (this.submitted && !this.finished && !this.exit) await this.terminal(); }
    finally {
      if (!this.exit) { if (!this.spawnFailed) this.send({ op: 'close' }); this.child.stdin.end(); }
      const code = await this.exited;
      this.live.record('original_actor_retirement', code === 0 && !this.spawnFailed ? 'PASS' : 'OBSERVER_FAILED', {
        case: this.work.caseName, waited: true, exit: code, signal: this.exit?.signal ?? null, spawn_failed: this.spawnFailed });
      requireLive(code === 0 && !this.spawnFailed, 'ORIGINAL_ACTOR_DID_NOT_RETIRE_NORMALLY');
    }
  }
}

async function answerOriginal(live: LiveObservation, row: WireObservation, work: Work) {
  const ask = row.frame.message;
  const choices = ask.choices as Work['decision'][];
  requireLive(choices.some(choice => same(choice, work.decision)), 'ORIGINAL_ASK_DOES_NOT_OFFER_SELECTED_CHOICE');
  const first = choices.findIndex(choice => same(choice, ask.timeout));
  const ordered = first > 0 ? [choices[first]!, ...choices.slice(0, first), ...choices.slice(first + 1)] : choices;
  const index = ordered.findIndex(choice => same(choice, work.decision));
  const button = live.page.locator('.choice-buttons > button').nth(index);
  const label = await button.evaluate(element => {
    const copy = element.cloneNode(true) as Element;
    copy.querySelector('.ask-default-meta')?.remove();
    return copy.textContent?.trim();
  });
  requireLive(label === live.t(work.decision.type === 'allow' ? 'choice.allow' : 'choice.deny'), 'ACTUAL_TYPED_CHOICE_BUTTON_LABEL_DIFFERS');
  await button.click();
  live.record('actual_ask_answer_sent', 'PASS', { case: work.caseName, ask_ref: row.raw_ref, decision_id: ask.decision_id,
    decision: work.decision, original_choices: choices, selected_button_index: index });
}

async function runBusiness(live: LiveObservation, work: Work, resource: string) {
  const baseline = await live.guest({ op: 'case_baseline', case: work.caseName });
  live.preserve('actual-file-before', { case: work.caseName, original: baseline });
  requireLive(baseline.path === work.target, 'ORIGINAL_BASELINE_TARGET_DIFFERS');
  const actor = new OriginalActor(live, work);
  const answered: WireObservation[] = [];
  try {
    await actor.prepare();
    const from = live.frames.length;
    const connection = live.frames.at(-1)?.connection;
    requireLive(connection !== undefined, 'ORIGINAL_CASE_CONNECTION_NOT_OBSERVED');
    const checkRunning = () => { live.checkConsumer(connection); actor.checkRunning(); };
    actor.submit();
    for (;;) {
      const next = await live.until(() => {
        const row = live.frames.slice(from).find(value => value.connection === connection && value.direction === 'in' && value.frame.message?.type === 'ask'
          && value.frame.message.hit.rule_id === reviewRuleId && !answered.some(prior => same(prior.frame.message.decision_id, value.frame.message.decision_id)));
        return row ? { row } : actor.businessTerminal() ? { row: null } : null;
      }, 'ASK_OR_ACTUAL_BUSINESS_TERMINAL_NOT_OBSERVED', checkRunning);
      if (!next.row) break;
      await answerOriginal(live, next.row, work);
      answered.push(next.row);
    }
    await actor.terminal();
    const effect = await live.guest({ op: 'case_finish', case: work.caseName });
    live.preserve('actual-file-after-and-syscall-return', { case: work.caseName, original: effect });
    const target = effect.effect;
    const bytes = [...Buffer.from(work.content)];
    const sameDescriptor = same([baseline.inode, baseline.device], [target.inode, target.device]);
    const samePathObject = same([target.inode, target.device], [target.current_path?.inode, target.current_path?.device]);
    const effectOk = work.decision.type === 'allow'
      ? same(target.current_path?.bytes, bytes) && (!samePathObject || same(target.bytes, bytes))
      : same(target.bytes, baseline.bytes) && same(target.current_path, baseline.current_path);
    live.record('actual_file_effect', sameDescriptor && effectOk ? 'PASS' : 'FAIL', { case: work.caseName,
      original_fd_and_current_path_preserved: true, original_descriptor_identity_preserved: sameDescriptor,
      descriptor_and_path_same_object: samePathObject, expected: work.decision.type === 'allow' ? 'requested current path bytes' : 'FD/path bytes and identity unchanged',
      source_ref: 'actual-file-after-and-syscall-return.jsonl', scope: 'physical effect; control receipt checked independently below' });
    requireLive(sameDescriptor && effectOk, 'ACTUAL_WRITE_EFFECT_DIFFERS');
    requireLive(answered.length > 0, 'NORMAL_WRITE_PRODUCED_NO_OWNED_ASK');
    for (const row of answered) {
      const ask = row.frame.message;
      live.preserve('actual-original-asks', { case: work.caseName, ask });
      requireLive(ask.event.header.kind === 'file' && ask.event.header.operation === 1
        && ask.remaining_ms <= 20000 && same(ask.timeout, { type: 'deny', terminate: 'none' })
        && typeof ask.event.payload.access_mask === 'number' && (ask.event.payload.access_mask & 2) === 2
        && Array.isArray(ask.event.payload.path) && Buffer.from(ask.event.payload.path).toString('utf8').startsWith(resource), 'ORIGINAL_WRITE_ASK_FACTS_DIFFER');
      const completion = await live.until(() => live.frames.slice(from).find(value => value.connection === row.connection && value.direction === 'in'
        && value.frame.message?.type === 'operation_result' && same(value.frame.message.decision_id, ask.decision_id)),
      'ORIGINAL_OPERATION_COMPLETION_NOT_RECEIVED', () => live.checkConsumer(row.connection));
      const actual = completion.frame.message;
      requireLive(same(actual.event, ask.event) && same(actual.hit, ask.hit) && actual.result.outcome === work.outcome
        && actual.result.proposal?.reason === 'user' && same(actual.result.proposal.decision, work.decision), 'ORIGINAL_OPERATION_COMPLETION_DIFFERS');
      const replies = live.frames.slice(from).filter(value => value.connection === row.connection && value.direction === 'out'
        && value.frame.message?.type === 'answer' && same(value.frame.message.decision_id, ask.decision_id));
      requireLive(replies.length === 1 && same(replies[0]!.frame.message.decision, work.decision), 'ORIGINAL_ANSWER_NOT_EXACTLY_ONCE');
      live.record('original_ask_answer_completion', 'PASS', { case: work.caseName, ask_ref: row.raw_ref, answer_ref: replies[0]!.raw_ref,
        completion_ref: completion.raw_ref, connection: row.connection, decision_id: ask.decision_id, event: ask.event.header,
        timeout: ask.timeout, remaining_ms: ask.remaining_ms });
    }
    return answered.at(-1)!.frame.message;
  } finally { await actor.close(); }
}

async function inspectAsk(live: LiveObservation, ask: any) {
  await live.viewport(320);
  const dialog = live.page.locator('dialog.review-dialog');
  if (await dialog.locator('.ask-intent').getAttribute('open') === null) {
    await live.local(() => dialog.locator('.ask-intent > summary').click());
  }
  const ordinary = await live.exchange(() => dialog.getByRole('button', { name: live.t('ask.readContext'), exact: true }).click(), 'business', 'ask_context');
  requireLive(same(ordinary.request.frame.business.query, { type: 'ask_context', decision_id: ask.decision_id, attribution: null })
    && live.result(ordinary).status === 'completed', 'ORIGINAL_ASK_CONTEXT_READ_DIFFERS');
  const context = live.result(ordinary).data.context;
  requireLive(same(context.decision_id, ask.decision_id) && sameEventKey(context.event, ask.event.header), 'ORIGINAL_ASK_CONTEXT_IDENTITY_DIFFERS');
  live.source(ordinary);
  await live.openSummary(dialog.locator('.ask-audit-windows > details > summary'), 'ask_original_context_gaps', context.view.gaps);
  live.record('ask_original_context_read', 'PASS', { ...live.refs(ordinary), decision_id: ask.decision_id, event: context.event });
  for (const width of [320, 1280]) for (const locale of ['ja-JP', 'zh-CN', 'en-US'] as const) {
    await live.viewport(width);
    await live.language(locale);
    const original = await compare(live, 'ask', ask, locale);
    const links = live.page.locator('dialog.review-dialog .attribution-candidate a');
    if (await links.count()) {
      const exchange = await live.exchange(() => links.first().click(), 'business', 'chat_messages');
      const candidate = live.result(original).data.context.attribution.candidates[0];
      requireLive(live.result(exchange).status === 'completed' && same(exchange.request.frame.business.query.scope, {kind:'model_http',exchange:candidate.call.message.exchange})
        && exchange.request.frame.business.query.after === null, 'ORIGINAL_CANDIDATE_EXCHANGE_READ_FAILED');
      live.record('ask_candidate_original_http_exchange_link', 'PASS', { ...live.refs(exchange), context_reply_ref: original.reply.raw_ref, exchange: exchange.request.frame.business.query.scope.exchange });
      await live.page.getByRole('button', { name: live.t('ask.returnToQuestion'), exact: true }).click();
      await expect(dialog).toBeVisible();
    } else live.record('ask_candidate_original_http_exchange_link', 'NOT_OBSERVED', live.refs(original));
  }
}

test('live current context writes with owned rules private receipts and original views', async ({ page, live }) => {
  await live.safe('current_owned_write_workflow', async () => {
    requireLive(process.env.AS_LIVE_GENERATE_WORKLOAD === '1' && process.env.AS_LIVE_BUSINESS_CONTEXT, 'CURRENT_AUTHORIZED_BUSINESS_CONTEXT_REQUIRED');
    const context = parseUnique(readFileSync(process.env.AS_LIVE_BUSINESS_CONTEXT, 'utf8')) as any;
    const original = parseUnique(readFileSync(context.baseline_policy, 'utf8')) as PolicySnapshot;
    const configuration = parseUnique(readFileSync(join(dirname(process.env.AS_LIVE_BUSINESS_CONTEXT), 'configuration-initial.json'), 'utf8')) as any;
    await live.unlock();
    let restore = false;
    const viewFailures: string[] = [];
    try {
      await createReviewPolicy(live, original, context.resource_prefix, () => { restore = true; });
      for (const caseName of Object.keys(context.cases)) {
        requireLive(caseName === 'file-allow' || caseName === 'file-deny', 'UNKNOWN_CURRENT_BUSINESS_CASE');
        const input = context.cases[caseName];
        const work: Work = { caseName, actorCase: `controls-live-${caseName}`, actorPage: `controls-live-${caseName}`,
          prompt: input.prompt, target: input.target, content: input.args.content, decision: input.decision,
          outcome: caseName === 'file-allow' ? 'allowed' : 'denied' };
        verifyReviewPolicy(await readReviewPolicy(live), original, context.resource_prefix);
        live.stage(caseName);
        const ask = await runBusiness(live, work, context.resource_prefix);
        try { await inspectAsk(live, ask); }
        catch (error) {
          const code = error instanceof LiveFailure ? error.code : 'BROWSER_ORIGINAL_VIEW_ASSERTION';
          viewFailures.push(code); live.record('completed_ask_original_views', 'FAIL', { case: caseName, code });
        }
        const returnToAsk = page.getByRole('button', { name: live.t('ask.returnToQuestion'), exact: true });
        if (await returnToAsk.isVisible()) await live.local(() => returnToAsk.click());
        while (await page.locator('dialog.review-dialog').isVisible()) {
          await live.local(() => page.getByRole('button', { name: live.t('ask.closeCompleted'), exact: true }).click());
        }
        try { await activityAndChatOriginals(live, ask); }
        catch (error) {
          const code = error instanceof LiveFailure ? error.code : 'BROWSER_ACTIVITY_CHAT_ASSERTION';
          viewFailures.push(code); live.record('completed_activity_chat_views', 'FAIL', { case: caseName, code });
        }
      }
      await live.viewport(320);
      for (const locale of ['ja-JP', 'zh-CN', 'en-US'] as const) { await live.language(locale); await exerciseReviewDrafts(live); }
      await replaceAndRestoreReviewNumber(live, original, context.resource_prefix);
      await privateOriginals(live);
    } finally {
      if (restore) await restoreReviewPolicy(live, original);
      const current = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
      live.preserve('configuration-final-independent-reply', current);
      requireLive(same(current.snapshot, configuration.snapshot), 'CONTROLLED_WORKFLOW_CHANGED_CONFIGURATION');
      live.record('configuration_original_snapshot_unchanged', 'PASS', { version: current.snapshot.version, independent_get: true });
    }
    requireLive(viewFailures.length === 0, 'COMPLETED_BUSINESS_ORIGINAL_VIEW_CHECKS_FAILED');
    await expect(page.locator('#rules')).toBeVisible();
  });
});
