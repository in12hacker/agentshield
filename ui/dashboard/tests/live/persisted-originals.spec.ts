import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join } from 'node:path';
import { test, navigate, requireLive, same } from './fixtures';
import { activityAndChatOriginals } from './originals';
import { restoreReviewPolicy, reviewRuleId } from './review-policy';
import { parseFrame } from '../../src/wire';
import { parseUnique } from '../../src/wire-json';
import type { PolicySnapshot } from '../../src/rules-wire';

test('live current owned originals and policy restoration without new business', async ({ page, live }) => {
  await live.safe('existing_owned_originals_only', async () => {
    requireLive(process.env.AS_LIVE_ORIGINAL_FRAMES && process.env.AS_LIVE_BUSINESS_CONTEXT, 'CURRENT_ORIGINAL_FRAMES_AND_CONTEXT_REQUIRED');
    const context = parseUnique(readFileSync(process.env.AS_LIVE_BUSINESS_CONTEXT, 'utf8')) as any;
    const original = parseUnique(readFileSync(context.baseline_policy, 'utf8')) as PolicySnapshot;
    const configuration = parseUnique(readFileSync(join(dirname(process.env.AS_LIVE_BUSINESS_CONTEXT), 'configuration-initial.json'), 'utf8')) as any;
    const bytes = readFileSync(process.env.AS_LIVE_ORIGINAL_FRAMES);
    const frames = bytes.toString('utf8').split('\n').filter(Boolean).map(line => parseUnique(line) as any)
      .filter(row => row.direction === 'in' && typeof row.wire === 'string').map(row => {
        requireLive(createHash('sha256').update(row.wire).digest('hex') === row.sha256, 'ORIGINAL_FRAME_BYTES_CHANGED');
        return { row, frame: parseFrame(row.wire) };
      });
    const questions = frames.flatMap(({ row, frame }) => frame.kind === 'ask' && frame.ask.hit.rule_id === reviewRuleId ? [{ row, ask: frame.ask }] : []);
    requireLive(questions.length > 0, 'CURRENT_OWNED_QUESTIONS_NOT_IN_ORIGINALS');
    live.preserve('source-original-frames', { path: process.env.AS_LIVE_ORIGINAL_FRAMES, sha256: createHash('sha256').update(bytes).digest('hex'),
      scope: 'read original objects for query anchors; no frames sent to or injected into Dashboard' });
    await live.unlock();
    try {
      for (const { row, ask } of questions) {
        const completion = frames.find(item => item.frame.kind === 'operation_result'
          && same(item.frame.completion.decision_id, ask.decision_id) && same(item.frame.completion.event, ask.event));
        requireLive(completion, 'ORIGINAL_COMPLETION_NOT_RETAINED_FOR_THIS_QUESTION');
        live.record('original_query_anchor', 'PASS', { question_source_line: row.line, completion_source_line: completion.row.line,
          decision_id: ask.decision_id, event: ask.event.header, scope: 'original EventKey/DecisionId only, not a ToolExecution bridge' });
        await activityAndChatOriginals(live, ask);
      }
      const config = await live.exchange(() => navigate(page, 'egress'), 'message', 'configuration');
      requireLive(live.result(config).status === 'completed', 'CURRENT_CONFIGURATION_READ_FAILED');
      const summary = page.locator('#egress-trust details > summary').first();
      if (await summary.count()) {
        const wasOpen = await summary.locator('..').getAttribute('open') !== null;
        await live.privateLocal('actual_summary_metadata_serialization', summary, () => summary.click());
        requireLive((await summary.locator('..').getAttribute('open') !== null) !== wasOpen, 'ACTUAL_SUMMARY_DID_NOT_TOGGLE');
        await live.privateLocal('actual_summary_metadata_serialization_restore', summary, () => summary.click());
      } else live.record('actual_summary_metadata_serialization', 'NOT_OBSERVED', live.refs(config));
    } finally {
      await restoreReviewPolicy(live, original);
      const current = await live.guest({ op: 'request', kind: 'configuration', command: { type: 'get' } });
      live.preserve('configuration-final-independent-reply', current);
      requireLive(same(current.snapshot, configuration.snapshot), 'ORIGINAL_CONFIGURATION_SNAPSHOT_CHANGED');
      live.record('configuration_original_snapshot_unchanged', 'PASS', { version: current.snapshot.version, independent_get: true });
    }
    live.record('completed_ask_dialog_after_observer_interruption', 'NOT_OBSERVED', {
      reason: 'fresh UI reads the original activity/chat; it does not recreate a retired browser Ask dialog or inject a historical Ask' });
  });
});
