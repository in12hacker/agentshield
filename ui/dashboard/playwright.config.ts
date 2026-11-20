import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  testIgnore: '**/live/**',
  fullyParallel: true,
  workers: 2,
  retries: 0,
  timeout: 60_000,
  expect: { timeout: 5_000 },
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL: 'http://127.0.0.1:5186',
    browserName: 'chromium',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    actionTimeout: 8_000,
  },
  projects: ['ja-JP', 'zh-CN', 'en-US'].flatMap(locale => [1280, 320].map(width => ({
    name: `${locale}-${width}`, use: { locale, viewport: { width, height: 900 } },
  }))),
  webServer: {
    command: 'npm run dev -- --port 5186',
    url: 'http://127.0.0.1:5186',
    env: { VITE_AS_WS_URL: 'ws://127.0.0.1:21854/' },
    reuseExistingServer: false,
    gracefulShutdown: { signal: 'SIGTERM', timeout: 5000 },
  },
});
