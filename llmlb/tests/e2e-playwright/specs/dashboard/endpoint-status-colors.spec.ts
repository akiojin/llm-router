import { test, expect, type Locator, type APIRequestContext } from '@playwright/test';
import { ensureDashboardLogin, deleteEndpointsByName, listEndpoints } from '../../helpers/api-helpers';
import { startMockOpenAIEndpointServer, type MockOpenAIEndpointServer } from '../../helpers/mock-openai-endpoint';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';

const API_BASE = process.env.BASE_URL || 'http://127.0.0.1:32768';
const AUTH_HEADER = { Authorization: 'Bearer sk_debug' };

test.describe.configure({ mode: 'serial' });

function parseAlphaToken(token: string): number {
  const t = token.trim();
  if (t.endsWith('%')) return Number(t.slice(0, -1)) / 100;
  return Number(t);
}

function parseCssColorAlpha(value: string): { alpha: number; explicitAlpha: boolean } {
  const v = value.trim();

  if (v === 'transparent') return { alpha: 0, explicitAlpha: true };

  // Modern browsers generally normalize to rgb()/rgba() but support both comma and space syntax.
  // - rgb(12, 34, 56)
  // - rgba(12, 34, 56, 0.2)
  // - rgb(12 34 56 / 0.2)
  //
  // Newer Chromium may return CSS Color 4 values (e.g. oklab(... / 0.2)).
  // We only need alpha to detect "no CSS generated" regressions, so parse alpha broadly.
  const rgbaComma = v.match(/^rgba\(\s*\d+,\s*\d+,\s*\d+,\s*([\d.]+%?)\s*\)$/);
  if (rgbaComma) return { alpha: parseAlphaToken(rgbaComma[1]), explicitAlpha: true };

  const alphaSlash = v.match(/\/\s*([\d.]+%?)\s*\)$/);
  if (alphaSlash) return { alpha: parseAlphaToken(alphaSlash[1]), explicitAlpha: true };

  // CSS Color 4 color-mix outputs may be returned as opaque oklab()/oklch() colors
  // even when the source utility is a `/20` variant. Class assertions above cover
  // the exact utility; alpha is only useful when the browser exposes it explicitly.
  if (v.endsWith(')')) return { alpha: 1, explicitAlpha: false };

  throw new Error(`Unsupported CSS color format: ${value}`);
}

function statusBadgeClassExpectations(status: 'pending' | 'online' | 'offline' | 'error') {
  switch (status) {
    case 'online':
      return {
        includes: [/bg-success\/20/, /text-success/],
        excludes: [/bg-destructive/, /bg-warning/, /text-warning/, /text-destructive/],
      };
    case 'pending':
      return {
        includes: [/bg-warning\/20/, /text-warning/],
        excludes: [/bg-destructive/, /bg-success/, /text-success/, /text-destructive/],
      };
    case 'offline':
      return {
        includes: [/bg-destructive\/20/, /text-destructive/],
        excludes: [/bg-destructive(?!\/)/, /text-destructive-foreground/],
      };
    case 'error':
      return {
        includes: [/bg-destructive(?!\/)/, /text-destructive-foreground/],
        excludes: [/bg-destructive\/20/, /text-destructive(?!-foreground)/],
      };
  }
}

async function expectStatusBadgeClasses(badge: Locator, status: 'pending' | 'online' | 'offline' | 'error') {
  // This helper exists just to keep the test readable. It is intentionally regex-based,
  // because class ordering is not stable across builds.
  const expectations = statusBadgeClassExpectations(status);
  for (const re of expectations.includes) {
    await expect(badge).toHaveClass(re);
  }
  for (const re of expectations.excludes) {
    await expect(badge).not.toHaveClass(re);
  }
}

async function expectStatusBadgeStyles(badge: Locator, status: 'pending' | 'online' | 'offline' | 'error') {
  // This catches "class is present but CSS isn't generated" regressions (the original bug report).
  const bg = await badge.evaluate((el) => getComputedStyle(el).backgroundColor);
  const { alpha, explicitAlpha } = parseCssColorAlpha(bg);

  // Badge backgrounds should never be fully transparent.
  expect(alpha, `backgroundColor was ${bg}`).toBeGreaterThan(0);

  if (status === 'error') {
    // Error uses solid destructive background.
    expect(alpha, `backgroundColor was ${bg}`).toBeGreaterThanOrEqual(0.95);
  } else if (explicitAlpha) {
    // online/pending/offline use `/20` variants (tinted background).
    expect(alpha, `backgroundColor was ${bg}`).toBeLessThan(0.95);
  }
}

async function expectStatusBadge(badge: Locator, status: 'pending' | 'online' | 'offline' | 'error') {
  await expectStatusBadgeClasses(badge, status);
  await expectStatusBadgeStyles(badge, status);
}

test.describe('Endpoint Status Colors @dashboard', () => {
  let mock: MockOpenAIEndpointServer;
  let mockBad: MockOpenAIEndpointServer;
  let mockBadClosed = false;

  test.beforeAll(async () => {
    // Delay /v1/models so newly created endpoints remain Pending long enough to assert color mapping.
    mock = await startMockOpenAIEndpointServer({ responseDelayMs: 3000 });
    mockBad = await startMockOpenAIEndpointServer();
  });

  test.afterAll(async () => {
    await mock.close();
    if (!mockBadClosed) {
      await mockBad.close();
    }
  });

  test('S-EP-01: status colors are consistent across list/detail/playground', async ({ page, request }) => {
    test.setTimeout(240_000);

    const baseName = `e2e-status-colors-${Date.now()}-${Math.random().toString(16).slice(2)}`;
    const endpointOkName = `${baseName}-ok`;
    const endpointBadName = `${baseName}-bad`;
    const endpointBadUrl = mockBad.baseUrl;

    const artifactDir = path.join('test-results', 'status-colors');
    await mkdir(artifactDir, { recursive: true });

    const shot = async (name: string) => {
      const filePath = path.join(artifactDir, `${baseName}-${name}.png`);
      await page.screenshot({ path: filePath, fullPage: true });
    };

    try {
      await ensureDashboardLogin(page);

      // ---------------------------------------------------------------------
      // Pending (OK endpoint, immediately after creation)
      // ---------------------------------------------------------------------
      await page.getByRole('button', { name: 'Add Endpoint' }).click();
      await page.fill('#endpoint-name', endpointOkName);
      await page.fill('#endpoint-url', mock.baseUrl);
      await page.getByRole('button', { name: 'Create Endpoint' }).click();

      // Use search filter first to find the endpoint reliably when many endpoints exist
      await page.getByPlaceholder('Search by name or URL...').fill(baseName);
      const okRow = page.getByRole('row').filter({ hasText: endpointOkName });
      await expect(okRow).toBeVisible({ timeout: 20000 });

      const okStatusCell = okRow.locator('td').nth(3);
      const okBadge = okStatusCell.locator('div').first();
      await expect(okBadge).toHaveText('Pending', { timeout: 20000 });
      await expectStatusBadge(okBadge, 'pending');
      await shot('dashboard-pending');

      // Detail modal (pending)
      await okRow.locator('button[title="Details"]').click();
      const pendingDialog = page.getByRole('dialog').filter({ hasText: endpointOkName });
      await expect(pendingDialog).toBeVisible({ timeout: 20000 });
      const pendingDialogStatusBadge = pendingDialog.locator('div.rounded-full').filter({ hasText: /^Pending$/ }).first();
      await expectStatusBadge(pendingDialogStatusBadge, 'pending');
      await shot('detail-pending');
      await page.keyboard.press('Escape');

      // Playground (pending)
      const pendingEndpoints = await listEndpoints(request);
      const okEndpoint = pendingEndpoints.find((e) => e.name === endpointOkName);
      expect(okEndpoint?.id).toBeTruthy();
      await page.goto(`/dashboard/#playground/${okEndpoint!.id}`);
      await expect(page.getByText('Start a conversation')).toBeVisible({ timeout: 20000 });

      const pendingPlaygroundBadge = page.locator('div.rounded-full').filter({ hasText: /^Pending$/ }).first();
      await expectStatusBadge(pendingPlaygroundBadge, 'pending');
      await shot('playground-pending');

      // Back to Dashboard
      await page.getByRole('button', { name: 'Back to Dashboard' }).click();
      await page.getByPlaceholder('Search by name or URL...').fill(baseName);
      await expect(okRow).toBeVisible({ timeout: 20000 });

      // ---------------------------------------------------------------------
      // Online (run explicit connection test for deterministic transition)
      // ---------------------------------------------------------------------
      // Use API-level test connection for reliability (UI button may fail
      // silently if CSRF token is stale or the request takes too long).
      await request.post(`${API_BASE}/api/endpoints/${okEndpoint!.id}/test`, {
        headers: AUTH_HEADER,
      });
      // Reload dashboard to pick up the new status.
      await page.click('#refresh-button');
      await page.waitForLoadState('load');
      await page.getByPlaceholder('Search by name or URL...').fill(baseName);
      await expect(okRow).toBeVisible({ timeout: 20000 });
      await expect(okRow.getByText('Online', { exact: true })).toBeVisible({ timeout: 20000 });

      const okBadgeOnline = okRow.locator('td').nth(3).locator('div').first();
      await expectStatusBadge(okBadgeOnline, 'online');
      await shot('dashboard-online');

      // Detail modal (online)
      await okRow.locator('button[title="Details"]').click();
      const onlineDialog = page.getByRole('dialog').filter({ hasText: endpointOkName });
      await expect(onlineDialog).toBeVisible({ timeout: 20000 });
      const onlineDialogStatusBadge = onlineDialog.locator('div.rounded-full').filter({ hasText: /^Online$/ }).first();
      await expectStatusBadge(onlineDialogStatusBadge, 'online');
      await shot('detail-online');
      await page.keyboard.press('Escape');

      // Playground (online)
      await page.goto(`/dashboard/#playground/${okEndpoint!.id}`);
      await expect(page.getByText('Start a conversation')).toBeVisible({ timeout: 20000 });
      const onlinePlaygroundBadge = page.locator('div.rounded-full').filter({ hasText: /^Online$/ }).first();
      await expectStatusBadge(onlinePlaygroundBadge, 'online');
      await shot('playground-online');

      // Back to Dashboard
      await page.getByRole('button', { name: 'Back to Dashboard' }).click();
      await page.getByPlaceholder('Search by name or URL...').fill(baseName);
      await expect(okRow).toBeVisible({ timeout: 20000 });

      // ---------------------------------------------------------------------
      // Error (explicit test connection failure)
      // ---------------------------------------------------------------------
      await page.getByRole('button', { name: 'Add Endpoint' }).click();
      await page.fill('#endpoint-name', endpointBadName);
      await page.fill('#endpoint-url', endpointBadUrl);
      await page.getByRole('button', { name: 'Create Endpoint' }).click();

      // Ensure search filter is applied for both ok and bad endpoints
      await page.getByPlaceholder('Search by name or URL...').fill(baseName);
      const badRow = page.getByRole('row').filter({ hasText: endpointBadName });
      await expect(badRow).toBeVisible({ timeout: 20000 });

      // Use API test connection, then look up the ID to reload
      const preErrorEndpoints = await listEndpoints(request);
      const badEndpointForTest = preErrorEndpoints.find((e) => e.name === endpointBadName);
      expect(badEndpointForTest?.id).toBeTruthy();
      if (!mockBadClosed) {
        await mockBad.close();
        mockBadClosed = true;
      }
      await request.post(`${API_BASE}/api/endpoints/${badEndpointForTest!.id}/test`, {
        headers: AUTH_HEADER,
      });
      // Reload dashboard to show updated status
      await page.click('#refresh-button');
      await page.waitForLoadState('load');
      await page.getByPlaceholder('Search by name or URL...').fill(baseName);
      await expect(badRow).toBeVisible({ timeout: 20000 });

      const badBadgeError = badRow.locator('td').nth(3).locator('div').first();
      await expectStatusBadge(badBadgeError, 'error');
      await shot('dashboard-error');

      // Detail modal (error)
      await badRow.locator('button[title="Details"]').click();
      const errorDialog = page.getByRole('dialog').filter({ hasText: endpointBadName });
      await expect(errorDialog).toBeVisible({ timeout: 20000 });
      const errorDialogStatusBadge = errorDialog.locator('div.rounded-full').filter({ hasText: /^Error$/ }).first();
      await expectStatusBadge(errorDialogStatusBadge, 'error');
      await shot('detail-error');
      await page.keyboard.press('Escape');

      const errorEndpoints = await listEndpoints(request);
      const badEndpoint = errorEndpoints.find((e) => e.name === endpointBadName);
      expect(badEndpoint?.id).toBeTruthy();

      // Playground (error)
      await page.goto(`/dashboard/#playground/${badEndpoint!.id}`);
      await expect(page.getByText('Start a conversation')).toBeVisible({ timeout: 20000 });
      const errorPlaygroundBadge = page.locator('div.rounded-full').filter({ hasText: /^Error$/ }).first();
      await expectStatusBadge(errorPlaygroundBadge, 'error');
      await shot('playground-error');

      // Back to Dashboard
      await page.getByRole('button', { name: 'Back to Dashboard' }).click();
      await page.getByPlaceholder('Search by name or URL...').fill(baseName);
      await expect(badRow).toBeVisible({ timeout: 20000 });

      // ---------------------------------------------------------------------
      // Offline (health checker transitions error -> offline after next failure)
      // ---------------------------------------------------------------------
      await expect.poll(async () => {
        const endpoints = await listEndpoints(request);
        return endpoints.find((e) => e.name === endpointBadName)?.status;
      }, {
        timeout: 90_000,
        intervals: [1000, 2000, 5000],
      }).toBe('offline');

      const offlineText = badRow.getByText('Offline', { exact: true });
      const offlineVisible = await offlineText.isVisible({ timeout: 20000 }).catch(() => false);
      if (!offlineVisible) {
        // Force a refresh if polling/websocket updates don't reach the UI in time.
        await page.click('#refresh-button');
        await page.waitForLoadState('load');
        await page.getByPlaceholder('Search by name or URL...').fill(baseName);
      }
      await expect(offlineText).toBeVisible({ timeout: 20000 });
      const badBadgeOffline = badRow.locator('td').nth(3).locator('div').first();
      await expectStatusBadge(badBadgeOffline, 'offline');
      await shot('dashboard-offline');

      // Detail modal (offline)
      await badRow.locator('button[title="Details"]').click();
      const offlineDialog = page.getByRole('dialog').filter({ hasText: endpointBadName });
      await expect(offlineDialog).toBeVisible({ timeout: 20000 });
      const offlineDialogStatusBadge = offlineDialog.locator('div.rounded-full').filter({ hasText: /^Offline$/ }).first();
      await expectStatusBadge(offlineDialogStatusBadge, 'offline');
      await shot('detail-offline');
      await page.keyboard.press('Escape');

      // Playground (offline)
      await page.goto(`/dashboard/#playground/${badEndpoint!.id}`);
      await expect(page.getByText('Start a conversation')).toBeVisible({ timeout: 20000 });
      const offlinePlaygroundBadge = page.locator('div.rounded-full').filter({ hasText: /^Offline$/ }).first();
      await expectStatusBadge(offlinePlaygroundBadge, 'offline');
      await shot('playground-offline');
    } finally {
      await deleteEndpointsByName(request, endpointOkName);
      await deleteEndpointsByName(request, endpointBadName);
    }
  });
});
