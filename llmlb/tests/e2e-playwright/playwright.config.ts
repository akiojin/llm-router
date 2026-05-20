import { defineConfig, devices } from '@playwright/test';

const baseURL = (process.env.BASE_URL || 'http://127.0.0.1:32768').replace(/\/$/, '');
const basePort = (() => {
  try {
    const url = new URL(baseURL);
    if (url.port) return Number(url.port);
    return url.protocol === 'https:' ? 443 : 80;
  } catch {
    return 32768;
  }
})();
const e2eDataDir = `llmlb/tests/e2e-playwright/.llmlb-${basePort}`;
const screenshotProjects =
  process.env.PLAYWRIGHT_SCREENSHOTS === '1'
    ? [
        {
          name: 'screenshots',
          use: {
            ...devices['Desktop Chrome'],
            screenshot: 'on' as const,
            headless: false,
          },
          testMatch: '**/navigation-screenshots.spec.ts',
        },
      ]
    : [];

/**
 * Playwright E2E Test Configuration for LLM Load Balancer
 * @see https://playwright.dev/docs/test-configuration
 */
export default defineConfig({
  testDir: './specs',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 1,
  workers: process.env.CI ? 1 : 4,
  reporter: [
    ['html', { outputFolder: 'reports/html' }],
    ['json', { outputFile: 'reports/results.json' }],
    ['list'],
  ],
  use: {
    baseURL,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
  // Server auto-start (optional - set SKIP_SERVER=1 to disable)
  webServer: process.env.SKIP_SERVER
    ? undefined
    : {
        // The broad E2E suite verifies protected-route behavior, so run its
        // server with auth required even though product default is optional.
        command:
          process.platform === 'win32'
            ? `set LLMLB_API_KEY_REQUIRED=true&& set LLMLB_DATABASE_URL=sqlite:${e2eDataDir.replace(/\//g, '\\\\')}\\\\llmlb.db&& set LLMLB_LOG_DIR=${e2eDataDir.replace(/\//g, '\\\\')}\\\\logs&& cargo run -p llmlb -- serve --no-tray --port ${basePort}`
            : `LLMLB_API_KEY_REQUIRED=true LLMLB_DATABASE_URL=sqlite:${e2eDataDir}/llmlb.db LLMLB_LOG_DIR=${e2eDataDir}/logs cargo run -p llmlb -- serve --no-tray --port ${basePort}`,
        url: `${baseURL}/dashboard`,
        reuseExistingServer: !process.env.CI,
        timeout: 300000,
        cwd: '../../../',
      },
  projects: [
    {
      name: 'chromium',
      use: {
        ...devices['Desktop Chrome'],
        headless: process.env.HEADED !== '1',
      },
      testIgnore: '**/navigation-screenshots.spec.ts',
    },
    ...screenshotProjects,
    // Uncomment for multi-browser testing
    // {
    //   name: 'firefox',
    //   use: { ...devices['Desktop Firefox'] },
    // },
    // {
    //   name: 'webkit',
    //   use: { ...devices['Desktop Safari'] },
    // },
  ],
});
