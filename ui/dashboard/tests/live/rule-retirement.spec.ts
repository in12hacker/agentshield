import { readFileSync } from 'node:fs';
import { test, requireLive } from './fixtures';
import { restoreReviewPolicy } from './review-policy';

test('live retire owned rule if present and verify original policy document', async ({ live }) => {
  await live.safe('owned_rule_cleanup_through_ui', async () => {
    requireLive(process.env.AS_LIVE_ORIGINAL_POLICY, 'NOT_RUN_ORIGINAL_POLICY_EVIDENCE_REQUIRED');
    const original = JSON.parse(readFileSync(process.env.AS_LIVE_ORIGINAL_POLICY, 'utf8'));
    await live.unlock();
    await restoreReviewPolicy(live, original);
  });
});
