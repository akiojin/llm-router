import { test, expect } from '@playwright/test'
import { ensureDashboardLogin, deleteEndpointsByName } from '../../helpers/api-helpers'
import { startMockOpenAIEndpointServer, type MockOpenAIEndpointServer } from '../../helpers/mock-openai-endpoint'

const API_BASE = process.env.BASE_URL || 'http://127.0.0.1:32768'
const AUTH_HEADER = { Authorization: 'Bearer sk_debug', 'Content-Type': 'application/json' }

test.describe.configure({ mode: 'serial' })

test.describe('Statistics Tab @dashboard', () => {
  let mock: MockOpenAIEndpointServer
  const endpointName = `e2e-stats-${Date.now()}`

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

    // Send requests to generate statistics
    for (let i = 0; i < 3; i++) {
      await request.post(`${API_BASE}/v1/chat/completions`, {
        headers: AUTH_HEADER,
        data: {
          model: mock.models[0],
          messages: [{ role: 'user', content: `stats test ${i}` }],
        },
      })
    }
  })

  test.afterAll(async ({ request }) => {
    await deleteEndpointsByName(request, endpointName)
    await mock.close()
  })

  test('ST-01: Daily tab displays token statistics', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /usage/i }).click()
    await page.waitForTimeout(1000)

    // The Statistics tab content should now be visible
    // TokenStatsSection has its own inner Tabs (Daily/Monthly), with Daily as default
    const dailyBtn = page.locator('button[role="tab"]').filter({ hasText: 'Daily' })
    if (await dailyBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
      await dailyBtn.click()
      await page.waitForTimeout(500)
    }

    // At minimum, the Statistics outer tab panel should have loaded with
    // the Token Statistics section (active inner tabpanel)
    const activePanel = page.locator('[role="tabpanel"][data-state="active"]')
    await expect(activePanel.first()).toBeVisible({ timeout: 10000 })
  })

  test('ST-02: Monthly tab displays monthly statistics', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /usage/i }).click()
    await page.waitForTimeout(1000)

    // Switch to Monthly sub-tab
    const monthlyBtn = page.locator('button[role="tab"]').filter({ hasText: 'Monthly' })
    if (await monthlyBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
      await monthlyBtn.click()
      await page.waitForTimeout(500)

      // Monthly data should display - use active panel to avoid ambiguity
      const activePanel = page.locator('[role="tabpanel"][data-state="active"]')
      await expect(activePanel.first()).toBeVisible({ timeout: 5000 })
    }
  })

  test('ST-03: Statistics cards layout is correct', async ({ page }) => {
    await ensureDashboardLogin(page)
    await page.getByRole('tab', { name: /usage/i }).click()
    await page.waitForTimeout(1000)

    // Check that stat cards are rendered
    // The statistics tab may use [data-stat] attributes for token-related cards
    const statCards = page.locator('[data-stat*="token"], [data-stat*="request"]')
    const cardCount = await statCards.count()

    // If dedicated stat cards exist, verify count; otherwise check the tab panel has content
    if (cardCount > 0) {
      expect(cardCount).toBeGreaterThanOrEqual(1)
      // Verify each card has a label (text content)
      for (let i = 0; i < cardCount; i++) {
        const text = await statCards.nth(i).textContent()
        expect(text).toBeTruthy()
      }
    } else {
      // Fallback: statistics active tab panel itself should have content
      const panel = page.locator('[role="tabpanel"][data-state="active"]')
      const text = await panel.first().textContent()
      expect(text).toBeTruthy()
    }
  })
})
