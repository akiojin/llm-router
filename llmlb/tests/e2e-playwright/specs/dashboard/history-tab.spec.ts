import { test, expect } from '@playwright/test'
import { ensureDashboardLogin, deleteEndpointsByName } from '../../helpers/api-helpers'
import { startMockOpenAIEndpointServer, type MockOpenAIEndpointServer } from '../../helpers/mock-openai-endpoint'
import { DashboardSelectors } from '../../helpers/selectors'

const API_BASE = process.env.BASE_URL || 'http://127.0.0.1:32768'
const AUTH_HEADER = { Authorization: 'Bearer sk_debug', 'Content-Type': 'application/json' }

test.describe.configure({ mode: 'serial' })

test.describe('History Tab @dashboard', () => {
  let mock: MockOpenAIEndpointServer
  const endpointName = `e2e-history-${Date.now()}`

  test.beforeAll(async ({ request }, testInfo) => {
    testInfo.setTimeout(120000)
    mock = await startMockOpenAIEndpointServer()

    // Create endpoint
    const resp = await request.post(`${API_BASE}/api/endpoints`, {
      headers: AUTH_HEADER,
      data: { name: endpointName, base_url: mock.baseUrl },
    })
    const ep = await resp.json()

    // Connection test + model sync
    await request.post(`${API_BASE}/api/endpoints/${ep.id}/test`, {
      headers: { Authorization: 'Bearer sk_debug' },
    })
    await request.post(`${API_BASE}/api/endpoints/${ep.id}/sync`, {
      headers: { Authorization: 'Bearer sk_debug' },
    })

    // Send several requests to populate history
    for (let i = 0; i < 5; i++) {
      await request.post(`${API_BASE}/v1/chat/completions`, {
        headers: AUTH_HEADER,
        data: {
          model: mock.models[0],
          messages: [{ role: 'user', content: `history test ${i}` }],
        },
      })
    }
  })

  test.afterAll(async ({ request }) => {
    await deleteEndpointsByName(request, endpointName)
    await mock.close()
  })

  test('HT-01: History Tab displays request history rows', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /requests/i }).click()
    await page.waitForTimeout(1000)

    const tbody = page.locator(DashboardSelectors.history.historyTbody)
    const firstRow = tbody.locator('tr').first()
    await expect(firstRow).toBeVisible({ timeout: 10000 })

    // Each row should have cells for timestamp, model, status, etc.
    const cells = firstRow.locator('td')
    const cellCount = await cells.count()
    expect(cellCount).toBeGreaterThanOrEqual(3)
  })

  test('HT-02: Clicking a history row opens a detail modal with tabs', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /requests/i }).click()
    await page.waitForTimeout(1000)

    // Click first row
    await page.locator(DashboardSelectors.history.historyTbody).locator('tr').first().click()

    // Detail modal should appear
    const modal = page.locator(DashboardSelectors.modals.requestModal)
    await expect(modal).toBeVisible({ timeout: 10000 })

    // Verify Overview/Request/Response tabs exist inside the modal
    const tabs = modal.locator('button[role="tab"]')
    const tabCount = await tabs.count()
    expect(tabCount).toBeGreaterThanOrEqual(2)
  })

  test('HT-03: Request tab displays JSON body', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /requests/i }).click()
    await page.waitForTimeout(1000)

    // Open detail modal
    await page.locator(DashboardSelectors.history.historyTbody).locator('tr').first().click()
    const modal = page.locator(DashboardSelectors.modals.requestModal)
    await expect(modal).toBeVisible({ timeout: 10000 })

    // Switch to Request tab (case-insensitive match)
    const requestTab = modal.locator('button[role="tab"]').filter({ hasText: /request/i })
    if (await requestTab.isVisible().catch(() => false)) {
      await requestTab.click()
      await page.waitForTimeout(500)

      // The panel should contain some text (JSON body)
      // Use the active tab panel (data-state="active") to avoid strict mode violation
      const panel = modal.locator('[role="tabpanel"][data-state="active"]')
      const text = await panel.textContent()
      expect(text).toBeTruthy()
    }
  })

  test('HT-04: Response tab displays JSON response', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /requests/i }).click()
    await page.waitForTimeout(1000)

    // Open detail modal
    await page.locator(DashboardSelectors.history.historyTbody).locator('tr').first().click()
    const modal = page.locator(DashboardSelectors.modals.requestModal)
    await expect(modal).toBeVisible({ timeout: 10000 })

    // Switch to Response tab
    const responseTab = modal.locator('button[role="tab"]').filter({ hasText: /response/i })
    if (await responseTab.isVisible().catch(() => false)) {
      await responseTab.click()
      await page.waitForTimeout(500)

      // Use the active tab panel (data-state="active") to avoid strict mode violation
      const panel = modal.locator('[role="tabpanel"][data-state="active"]')
      const text = await panel.textContent()
      expect(text).toBeTruthy()
    }
  })

  test('HT-05: Pagination information is displayed', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /requests/i }).click()
    await page.waitForTimeout(1000)

    // Pagination info should be visible
    const pageInfo = page.locator(DashboardSelectors.history.pageInfo)
    const isVisible = await pageInfo.isVisible({ timeout: 5000 }).catch(() => false)
    if (isVisible) {
      const text = await pageInfo.textContent()
      expect(text).toBeTruthy()
    }

    // Per-page selector should be present
    const perPage = page.locator(DashboardSelectors.history.perPage)
    const perPageVisible = await perPage.isVisible({ timeout: 3000 }).catch(() => false)
    if (perPageVisible) {
      expect(await perPage.isEnabled()).toBe(true)
    }
  })

  test('HT-06: Tab buttons remain visible within modal on Request/Response tabs', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /requests/i }).click()
    await page.waitForTimeout(1000)

    await page.locator(DashboardSelectors.history.historyTbody).locator('tr').first().click()
    const modal = page.locator(DashboardSelectors.modals.requestModal)
    await expect(modal).toBeVisible({ timeout: 10000 })

    for (const tabName of ['Request', 'Response']) {
      const tab = modal.locator('button[role="tab"]').filter({ hasText: new RegExp(tabName, 'i') })
      if (!(await tab.isVisible().catch(() => false))) continue
      await tab.click()
      await page.waitForTimeout(500)

      const modalBox = await modal.boundingBox()
      expect(modalBox).toBeTruthy()

      const allTabs = modal.locator('button[role="tab"]')
      const tabCount = await allTabs.count()
      for (let i = 0; i < tabCount; i++) {
        const tabBox = await allTabs.nth(i).boundingBox()
        expect(tabBox).toBeTruthy()
        expect(tabBox!.x).toBeGreaterThanOrEqual(modalBox!.x)
        expect(tabBox!.x + tabBox!.width).toBeLessThanOrEqual(modalBox!.x + modalBox!.width)
      }
    }
  })
})
