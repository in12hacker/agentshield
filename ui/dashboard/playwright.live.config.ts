import { defineConfig } from '@playwright/test';
import { isAbsolute, resolve } from 'node:path';

const fixture = process.env.AS_LIVE_FIXTURE;
const output = process.env.AS_LIVE_OUTPUT;
const http = process.env.AS_LIVE_HTTP_URL;
const websocket = process.env.AS_LIVE_WS_URL;
if (!fixture || !output || !isAbsolute(fixture) || !isAbsolute(output)) throw new Error('NOT_RUN: AS_LIVE_FIXTURE and AS_LIVE_OUTPUT require explicit absolute paths');
if (!http || !websocket) throw new Error('NOT_RUN: current owner HTTP/WS endpoints required');
if (process.env.DEBUG || process.env.PWDEBUG) throw new Error('NOT_RUN: live debug payload logging is forbidden');

export default defineConfig({
  testDir: './tests/live',
  fullyParallel: false, workers: 1, retries: 0,
  timeout: 0, expect: { timeout: 0 },
  outputDir: resolve(output, 'playwright'),
  reporter: [['./tests/live/safe-reporter.ts']],
  use: {
    baseURL: http, browserName: 'chromium',
    trace: 'off', screenshot: 'off', video: 'off', actionTimeout: 0, navigationTimeout: 0,
    launchOptions: { timeout: 0 },
  },
  projects: ['ja-JP', 'zh-CN', 'en-US'].flatMap(locale => [320, 1280].map(width => ({
    name: `${locale}-${width}`, use: { locale, viewport: { width, height: 900 } },
  }))),
});
