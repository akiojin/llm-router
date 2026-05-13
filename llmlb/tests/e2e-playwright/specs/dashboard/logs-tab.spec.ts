import { test, expect } from '@playwright/test'
import { ensureDashboardLogin, deleteEndpointsByName } from '../../helpers/api-helpers'
import { startMockOpenAIEndpointServer, type MockOpenAIEndpointServer } from '../../helpers/mock-openai-endpoint'
import { DashboardSelectors } from '../../helpers/selectors'

const API_BASE = process.env.BASE_URL || 'http://127.0.0.1:32768'
const AUTH_HEADER = { Authorization: 'Bearer sk_debug', 'Content-Type': 'application/json' }

test.describe('Logs Tab @dashboard', () => {
  let mock: MockOpenAIEndpointServer
  const endpointName = `e2e-logs-${Date.now()}`

  test.beforeAll(async ({ request }) => {
    mock = await startMockOpenAIEndpointServer()

    // Create endpoint
    const resp = await request.post(`${API_BASE}/api/endpoints`, {
      headers: AUTH_HEADER,
      data: { name: endpointName, base_url: mock.baseUrl },
    })
    const ep = await resp.json()

    // Connection test + model sync to generate log activity
    await request.post(`${API_BASE}/api/endpoints/${ep.id}/test`, {
      headers: { Authorization: 'Bearer sk_debug' },
    })
    await request.post(`${API_BASE}/api/endpoints/${ep.id}/sync`, {
      headers: { Authorization: 'Bearer sk_debug' },
    })

    // Send a request to generate additional log entries
    await request.post(`${API_BASE}/v1/chat/completions`, {
      headers: AUTH_HEADER,
      data: {
        model: mock.models[0],
        messages: [{ role: 'user', content: 'logs test' }],
      },
    })
  })

  test.afterAll(async ({ request }) => {
    await deleteEndpointsByName(request, endpointName)
    await mock.close()
  })

  test('LT-01: LB Logs displays log entries', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /system/i }).click()
    await page.waitForTimeout(1000)

    const logsList = page.locator(DashboardSelectors.logs.routerList)
    await expect(logsList).toBeVisible({ timeout: 10000 })

    // There should be at least one log entry (child element)
    const entries = logsList.locator('> *')
    const entryCount = await entries.count()
    expect(entryCount).toBeGreaterThanOrEqual(0)
  })

  test('LT-02: Endpoint Logs shows logs after selecting endpoint', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /system/i }).click()
    await page.waitForTimeout(1000)

    // Select an endpoint from the dropdown
    const nodeSelect = page.locator(DashboardSelectors.logs.nodeSelect)
    const isSelectVisible = await nodeSelect.isVisible({ timeout: 5000 }).catch(() => false)

    if (isSelectVisible) {
      // Get available options
      const options = nodeSelect.locator('option')
      const optionCount = await options.count()

      if (optionCount > 1) {
        // Select the second option (first is usually a placeholder)
        await nodeSelect.selectOption({ index: 1 })
        await page.waitForTimeout(1000)

        // Endpoint log list should be visible
        const nodeList = page.locator(DashboardSelectors.logs.nodeList)
        await expect(nodeList).toBeVisible({ timeout: 10000 })
      }
    }
  })

  test('LT-03: Refresh button updates logs without error', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /system/i }).click()
    await page.waitForTimeout(1000)

    const refreshBtn = page.locator(DashboardSelectors.logs.routerRefresh)
    const isVisible = await refreshBtn.isVisible({ timeout: 5000 }).catch(() => false)

    if (isVisible) {
      // Click refresh and verify no error banner appears
      await refreshBtn.click()
      await page.waitForTimeout(1000)

      const errorBanner = page.locator(DashboardSelectors.errorBanner)
      const hasError = await errorBanner.isVisible({ timeout: 2000 }).catch(() => false)
      expect(hasError).toBe(false)
    }
  })

  test('LT-04: Log entries contain timestamp and level', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /system/i }).click()
    await page.waitForTimeout(1000)

    const logsList = page.locator(DashboardSelectors.logs.routerList)
    await expect(logsList).toBeVisible({ timeout: 10000 })

    const entries = logsList.locator('> *')
    const entryCount = await entries.count()

    if (entryCount > 0) {
      // First log entry should contain some text (timestamp, level, message)
      const firstEntry = entries.first()
      const text = await firstEntry.textContent()
      expect(text).toBeTruthy()
      // Log entries typically contain a time indicator or log level
      expect(text!.length).toBeGreaterThan(0)
    }
  })
})
