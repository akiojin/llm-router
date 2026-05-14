import { test, expect, type APIRequestContext } from '@playwright/test'
import { ensureDashboardLogin, deleteEndpointsByName } from '../../helpers/api-helpers'
import { startMockOpenAIEndpointServer, type MockOpenAIEndpointServer } from '../../helpers/mock-openai-endpoint'

const API_BASE = process.env.BASE_URL || 'http://127.0.0.1:32768'
const AUTH_HEADER = { Authorization: 'Bearer sk_debug', 'Content-Type': 'application/json' }
const SHARED_MODEL = 'lb-shared-model'

test.describe.configure({ mode: 'serial' })

test.describe('Load Balancer Playground Walkthrough @playground', () => {
  let mockA: MockOpenAIEndpointServer
  let mockB: MockOpenAIEndpointServer
  let endpointNames: string[] = []

  test.beforeAll(async () => {
    mockA = await startMockOpenAIEndpointServer({
      models: [SHARED_MODEL],
      responseDelayMs: 120,
    })
    mockB = await startMockOpenAIEndpointServer({
      models: [SHARED_MODEL],
      responseDelayMs: 120,
    })
  })

  test.afterAll(async () => {
    await mockA.close()
    await mockB.close()
  })

  test.afterEach(async ({ request }) => {
    for (const name of endpointNames) {
      await deleteEndpointsByName(request, name)
    }
    endpointNames = []
  })

  async function createAndPrepareEndpoint(
    request: APIRequestContext,
    name: string,
    baseUrl: string
  ) {
    const createResponse = await request.post(`${API_BASE}/api/endpoints`, {
      headers: AUTH_HEADER,
      data: {
        name,
        base_url: baseUrl,
      },
    })
    expect(createResponse.ok()).toBeTruthy()

    const endpoint = (await createResponse.json()) as { id: string }

    const testResponse = await request.post(`${API_BASE}/api/endpoints/${endpoint.id}/test`, {
      headers: { Authorization: 'Bearer sk_debug' },
    })
    expect(testResponse.ok()).toBeTruthy()

    const syncResponse = await request.post(`${API_BASE}/api/endpoints/${endpoint.id}/sync`, {
      headers: { Authorization: 'Bearer sk_debug' },
    })
    expect(syncResponse.ok()).toBeTruthy()

    return endpoint.id
  }

  test('header -> lb playground -> chat -> load test distribution', async ({
    page,
    request,
  }) => {
    const endpointNameA = `e2e-lb-a-${Date.now()}-${Math.random().toString(16).slice(2)}`
    const endpointNameB = `e2e-lb-b-${Date.now()}-${Math.random().toString(16).slice(2)}`
    endpointNames.push(endpointNameA, endpointNameB)

    await createAndPrepareEndpoint(request, endpointNameA, mockA.baseUrl)
    await createAndPrepareEndpoint(request, endpointNameB, mockB.baseUrl)

    await expect
      .poll(async () => {
        const response = await request.get(`${API_BASE}/v1/models`, {
          headers: { Authorization: 'Bearer sk_debug' },
        })
        if (!response.ok()) {
          return [] as string[]
        }
        const json = (await response.json()) as { data?: Array<{ id: string }> }
        return (json.data ?? []).map((model) => model.id)
      })
      .toContain(SHARED_MODEL)

    await ensureDashboardLogin(page)

    const openLbPlayground = page.locator('#lb-playground-button')
    await expect(openLbPlayground).toBeVisible({ timeout: 10000 })
    await openLbPlayground.click()

    await expect(page).toHaveURL(/#lb-playground/)
    await expect(page.locator('#lb-playground-sidebar')).toBeVisible({ timeout: 10000 })
    await expect(page.locator('#lb-session-auth')).toContainText('Dashboard session')
    await expect(page.locator('#lb-api-key')).toHaveCount(0)

    const modelSelect = page.locator('#lb-model-select')
    await modelSelect.click()
    await page.getByRole('option', { name: SHARED_MODEL }).click()

    await page.fill('#lb-chat-input', 'Hello from LB playground')
    await page.click('#lb-send-chat')

    await expect(page.getByText('MOCK_OK')).toBeVisible({ timeout: 20000 })
    await expect(page.locator('#lb-distribution-panel')).toBeVisible({ timeout: 20000 })

    await page.click('#lb-mode-load-test')

    await page.fill('#lb-total-requests', '12')
    await page.fill('#lb-concurrency', '4')
    await page.fill('#lb-interval-ms', '0')
    await page.fill('#lb-chat-input', 'Load test request')
    await page.click('#lb-start-load-test')

    await expect(page.locator('#lb-load-test-progress')).toContainText('12/12', {
      timeout: 45000,
    })

    await expect(page.locator('[data-testid="lb-distribution-row"]').first()).toBeVisible({
      timeout: 20000,
    })
  })
})
