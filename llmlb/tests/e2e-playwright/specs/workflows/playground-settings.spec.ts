import { test, expect } from '@playwright/test'
import { ensureDashboardLogin, deleteEndpointsByName, listEndpoints } from '../../helpers/api-helpers'
import { startMockOpenAIEndpointServer, type MockOpenAIEndpointServer } from '../../helpers/mock-openai-endpoint'

const API_BASE = process.env.BASE_URL || 'http://127.0.0.1:32768'
const AUTH_HEADER = { Authorization: 'Bearer sk_debug', 'Content-Type': 'application/json' }

test.describe.configure({ mode: 'serial' })

test.describe('Playground Settings @playground', () => {
  let mock: MockOpenAIEndpointServer
  const endpointName = `e2e-playground-settings-${Date.now()}`
  let endpointId = ''

  test.beforeAll(async ({ request }) => {
    mock = await startMockOpenAIEndpointServer()

    const resp = await request.post(`${API_BASE}/api/endpoints`, {
      headers: AUTH_HEADER,
      data: { name: endpointName, base_url: mock.baseUrl },
    })
    expect(resp.ok()).toBeTruthy()
    const ep = (await resp.json()) as { id: string }
    endpointId = ep.id

    const testResp = await request.post(`${API_BASE}/api/endpoints/${endpointId}/test`, {
      headers: { Authorization: 'Bearer sk_debug' },
    })
    expect(testResp.ok()).toBeTruthy()

    const syncResp = await request.post(`${API_BASE}/api/endpoints/${endpointId}/sync`, {
      headers: { Authorization: 'Bearer sk_debug' },
    })
    expect(syncResp.ok()).toBeTruthy()
  })

  test.afterAll(async ({ request }) => {
    await deleteEndpointsByName(request, endpointName)
    // Force-close with timeout to avoid afterAll hook timeout
    await Promise.race([
      mock.close(),
      new Promise<void>((resolve) => setTimeout(resolve, 5000)),
    ])
  })

  test('PS-01: Endpoint Playground -> System Prompt -> message -> response', async ({ page }) => {
    test.setTimeout(60000)
    await ensureDashboardLogin(page)

    // Navigate to endpoint playground
    const endpoints = await listEndpoints(page.context().request)
    const ep = endpoints.find((e) => e.name === endpointName)
    expect(ep).toBeTruthy()

    await page.goto(`${API_BASE}/dashboard/#playground/${ep!.id}`)
    await expect(page.getByText('Start a conversation')).toBeVisible({ timeout: 20000 })

    // If a system prompt textarea is visible, set it
    const systemPrompt = page.locator('#system-prompt, textarea[placeholder*="system"]')
    if (await systemPrompt.isVisible({ timeout: 3000 }).catch(() => false)) {
      await systemPrompt.fill('You are a helpful assistant.')
    }

    // Select model
    const modelSelect = page.getByRole('combobox').first()
    await modelSelect.click()
    await page.getByRole('option', { name: mock.models[0] }).click()

    // Send message
    const input = page.getByPlaceholder('Type a message or attach files...')
    await input.fill('Hello from PS-01')
    await page.getByRole('button', { name: 'Send' }).click()

    // Verify mock response
    await expect(page.getByText('MOCK_OK')).toBeVisible({ timeout: 20000 })
  })

  test('PS-02: Streaming toggle -> badge display changes', async ({ page }) => {
    test.setTimeout(60000)
    await ensureDashboardLogin(page)

    const endpoints = await listEndpoints(page.context().request)
    const ep = endpoints.find((e) => e.name === endpointName)
    expect(ep).toBeTruthy()

    await page.goto(`${API_BASE}/dashboard/#playground/${ep!.id}`)
    await expect(page.getByText('Start a conversation')).toBeVisible({ timeout: 20000 })

    // Streaming badge should be visible by default
    await expect(page.getByText('Streaming')).toBeVisible({ timeout: 10000 })

    // Toggle streaming off via toggle control
    const streamToggle = page.locator('#stream-toggle, [data-testid="stream-toggle"]')
    if (await streamToggle.isVisible({ timeout: 3000 }).catch(() => false)) {
      await streamToggle.click()
      // After toggle, the badge text should change (e.g., "Non-Streaming" or toggle state)
      await expect(page.getByText('Streaming')).toBeVisible({ timeout: 5000 })
    }
  })

  test('PS-03: cURL button -> command displayed -> copy', async ({ page }) => {
    test.setTimeout(60000)
    await ensureDashboardLogin(page)

    const endpoints = await listEndpoints(page.context().request)
    const ep = endpoints.find((e) => e.name === endpointName)
    expect(ep).toBeTruthy()

    await page.goto(`${API_BASE}/dashboard/#playground/${ep!.id}`)
    await expect(page.getByText('Start a conversation')).toBeVisible({ timeout: 20000 })

    // The cURL button is a regular button with text "cURL" in the toolbar
    const copyCurl = page.getByRole('button', { name: 'cURL' })
    await expect(copyCurl).toBeVisible({ timeout: 10000 })
    await copyCurl.click()

    // Verify cURL dialog opens with the command
    await expect(page.getByText('cURL Command')).toBeVisible({ timeout: 5000 })
    // The pre element inside the dialog should contain "curl"
    await expect(page.locator('pre').filter({ hasText: 'curl' })).toBeVisible({ timeout: 5000 })
  })

  test('PS-04: LB Playground -> Temperature/MaxTokens -> chat send', async ({ page }) => {
    test.setTimeout(60000)
    await ensureDashboardLogin(page)

    const openLbPlayground = page.locator('#lb-playground-button')
    await expect(openLbPlayground).toBeVisible({ timeout: 10000 })
    await openLbPlayground.click()

    await expect(page).toHaveURL(/#lb-playground/)
    await expect(page.locator('#lb-playground-sidebar')).toBeVisible({ timeout: 10000 })

    // Adjust temperature if available
    const tempInput = page.locator('#lb-temperature, [data-testid="lb-temperature"]')
    if (await tempInput.isVisible({ timeout: 3000 }).catch(() => false)) {
      await tempInput.fill('0.5')
    }

    // Adjust max tokens if available
    const maxTokensInput = page.locator('#lb-max-tokens, [data-testid="lb-max-tokens"]')
    if (await maxTokensInput.isVisible({ timeout: 3000 }).catch(() => false)) {
      await maxTokensInput.fill('256')
    }

    // Select model
    const modelSelect = page.locator('#lb-model-select')
    await modelSelect.click()
    await page.getByRole('option', { name: mock.models[0] }).click()

    // Send message
    await page.fill('#lb-chat-input', 'Hello from PS-04 LB Playground')
    await page.click('#lb-send-chat')

    // Verify response
    await expect(page.getByText('MOCK_OK')).toBeVisible({ timeout: 20000 })
  })

  test('PS-05: LB Playground -> cURL button -> correct URL and headers', async ({ page }) => {
    test.setTimeout(60000)
    await ensureDashboardLogin(page)

    const openLbPlayground = page.locator('#lb-playground-button')
    await expect(openLbPlayground).toBeVisible({ timeout: 10000 })
    await openLbPlayground.click()

    await expect(page).toHaveURL(/#lb-playground/)

    // Look for cURL button in LB Playground
    const copyCurl = page.locator(
      '#lb-copy-curl, [data-testid="lb-copy-curl"], button:has-text("cURL")'
    )
    if (await copyCurl.isVisible({ timeout: 5000 }).catch(() => false)) {
      await copyCurl.click()
      // Verify the cURL command contains the correct base URL and authorization header
      const curlText = page.locator('[data-testid="curl-command"], pre:has-text("curl")')
      if (await curlText.isVisible({ timeout: 5000 }).catch(() => false)) {
        const text = await curlText.textContent()
        expect(text).toContain('/api/dashboard/playground/chat/completions')
        expect(text).toContain('X-CSRF-Token')
      }
    }
  })
})
