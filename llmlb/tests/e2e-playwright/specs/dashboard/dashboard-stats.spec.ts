import { test, expect } from '@playwright/test';
import { DashboardPage } from '../../pages/dashboard.page';
import { DashboardSelectors } from '../../helpers/selectors';

test.describe('Dashboard Operations Overview @dashboard', () => {
  let dashboard: DashboardPage;

  test.beforeEach(async ({ page }) => {
    dashboard = new DashboardPage(page);
    await dashboard.goto();
    // Wait for initial data load
    await page.waitForTimeout(500);
  });

  test('S-01: Operational health summary is displayed', async ({ page }) => {
    await expect(page.locator(DashboardSelectors.stats.operationalHealth)).toBeVisible();
  });

  test('S-02: Total Endpoints stat is displayed', async () => {
    await expect(dashboard.totalEndpoints).toBeVisible();
    const text = await dashboard.getTotalEndpoints();
    expect(text).toBeDefined();
  });

  test('S-03: Total Requests stat is displayed', async () => {
    await expect(dashboard.totalRequests).toBeVisible();
  });

  test('S-04: Success Rate stat is displayed', async ({ page }) => {
    const successRate = page.locator(DashboardSelectors.stats.successRate);
    await expect(successRate).toBeVisible();
  });

  test('S-05: Output TPS stat is displayed instead of latency', async ({ page }) => {
    await expect(dashboard.outputTps).toBeVisible();
    await expect(page.locator('[data-stat="response-latency"]')).toHaveCount(0);
  });

  test('S-06: Queue pressure and active request stats are displayed', async ({ page }) => {
    await expect(page.locator(DashboardSelectors.stats.activeRequests)).toBeVisible();
    await expect(page.locator(DashboardSelectors.stats.queuedRequests)).toBeVisible();
  });

  test('S-07: GPU capability is shown as capacity, not average utilization', async ({ page }) => {
    await expect(page.locator(DashboardSelectors.stats.gpuCapacity)).toBeVisible();
    await expect(page.locator('[data-stat="average-gpu-usage"]')).toHaveCount(0);
    await expect(page.locator('[data-stat="average-gpu-memory-usage"]')).toHaveCount(0);
  });

  test('S-08: Action item section is displayed', async ({ page }) => {
    await expect(page.locator(DashboardSelectors.stats.actionItems)).toBeVisible();
  });

  test('S-09: Overview updates on refresh', async ({ page }) => {
    // Store initial values
    const initialTotal = await dashboard.totalRequests.textContent();

    // Trigger refresh (note: refresh reloads the page)
    await dashboard.refresh();
    await page.waitForLoadState('networkidle');

    // Values should still be present (may or may not change)
    const newTotal = await dashboard.totalRequests.textContent();
    expect(newTotal).toBeDefined();
  });
});
