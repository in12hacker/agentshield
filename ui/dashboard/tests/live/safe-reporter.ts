import type { Reporter, TestCase, TestResult, FullResult } from '@playwright/test/reporter';

/** Live reporter never serializes Playwright errors, call logs or parameter values. */
export default class SafeReporter implements Reporter {
  onTestEnd(test: TestCase, result: TestResult) {
    console.log(JSON.stringify({ test: test.title, project: test.parent.project()?.name, status: result.status, duration_ms: result.duration,
      observer_failures: Number(test.annotations.find(a => a.type === 'live_observer_failures')?.description ?? '0'),
      error: result.error ? 'CHECK_FAILED_SEE_SAFE_LIVE_RECORD' : null }));
  }
  onError() { console.log(JSON.stringify({ status: 'RUNNER_ERROR', details: 'NOT_CAPTURED_SENSITIVE_BOUNDARY' })); }
  onEnd(result: FullResult) { console.log(JSON.stringify({ status: result.status })); }
}
