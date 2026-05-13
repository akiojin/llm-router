import { test, expect } from '@playwright/test'
import {
  API_BASE,
  cleanupRuntimeResources,
  createApiKeyViaUi,
  DASHBOARD_ORIGIN,
  getEndpointByName,
  getSseDataLines,
  OLLAMA_BASE,
  openDashboard,
  postStreamingChatCompletion,
  prepareEndpointViaUi,
  probeLocalRuntimes,
  RuntimeSelection,
  searchEndpointRow,
  waitForApiModelVisible,
  waitForEndpointTypeAndStatus,
  waitForModelVisibleInDetails,
} from '../../helpers/real-local-runtime'

const AUTH_HEADERS = {
  Authorization: 'Bearer sk_debug',
  'Content-Type': 'application/json',
}

test.describe.configure({ mode: 'serial' })

test.describe('Real Local Streaming And Failure Paths @workflows @api @real-runtimes', () => {
  let runtimeSelection: RuntimeSelection | null = null
  let skipReason = ''
  let endpointNames: string[] = []
  let apiKeyName = ''
  let apiKeyId = ''

  test.beforeAll(async ({ request }) => {
    const runtimeProbe = await probeLocalRuntimes(request)
    if (!runtimeProbe.ok) {
      skipReason = runtimeProbe.reason
      return
    }
    runtimeSelection = runtimeProbe.selection
  })

  test.afterEach(async ({ request }) => {
    await cleanupRuntimeResources(request, {
      apiKeyId,
      apiKeyName,
      endpointNames,
    })
    endpointNames = []
    apiKeyName = ''
    apiKeyId = ''
  })

  test('streaming works and real API failure paths are surfaced without breaking the endpoint', async ({
    page,
    request,
  }) => {
    test.setTimeout(20 * 60_000)
    test.skip(!runtimeSelection, skipReason)
    const selection = runtimeSelection
    if (!selection) return

    await page.setViewportSize({ width: 1440, height: 960 })
    await page
      .context()
      .grantPermissions(['clipboard-read', 'clipboard-write'], { origin: DASHBOARD_ORIGIN })

    const endpointName = `e2e-real-stream-${Date.now()}`
    endpointNames = [endpointName]
    apiKeyName = `e2e-real-stream-key-${Date.now()}`

    await openDashboard(page)
    const endpoint = await prepareEndpointViaUi(page, request, {
      endpointName,
      baseUrl: OLLAMA_BASE,
      endpointType: 'ollama',
      typeLabel: 'Ollama',
    })
    await waitForModelVisibleInDetails(page, endpointName, selection.ollamaModel)

    const createdApiKey = await createApiKeyViaUi(page, apiKeyName)
    apiKeyId = createdApiKey.id
    await page.keyboard.press('Escape')

    await waitForApiModelVisible(request, createdApiKey.key, selection.ollamaModel)

    const streamingResponse = await postStreamingChatCompletion(
      request,
      createdApiKey.key,
      selection.ollamaModel,
      'Reply with a short confirmation for the streaming test.'
    )
    expect(streamingResponse.response.headers()['content-type']).toContain('text/event-stream')
    expect(streamingResponse.dataLines.length).toBeGreaterThan(1)
    expect(streamingResponse.dataLines[streamingResponse.dataLines.length - 1]).toBe('data: [DONE]')

    const parsedChunks = getSseDataLines(streamingResponse.text)
      .filter((line) => line !== 'data: [DONE]')
      .map((line) => JSON.parse(line.replace(/^data: /, '')) as { choices?: Array<{ delta?: { content?: string } }> })
    expect(parsedChunks.length).toBeGreaterThan(0)
    expect(parsedChunks.some((chunk) => chunk.choices?.[0]?.delta != null)).toBeTruthy()

    const invalidApiKeyModelsResponse = await request.get(`${API_BASE}/v1/models`, {
      headers: { Authorization: 'Bearer sk_invalid' },
    })
    expect(invalidApiKeyModelsResponse.ok()).toBeFalsy()
    expect([401, 403]).toContain(invalidApiKeyModelsResponse.status())

    const missingModelResponse = await request.post(`${API_BASE}/v1/chat/completions`, {
      headers: {
        ...AUTH_HEADERS,
        Authorization: `Bearer ${createdApiKey.key}`,
      },
      data: {
        model: `${selection.ollamaModel}-missing`,
        messages: [{ role: 'user', content: 'This should fail.' }],
        stream: false,
      },
    })
    expect(missingModelResponse.ok()).toBeFalsy()
    expect(missingModelResponse.status()).toBeGreaterThanOrEqual(400)
    const missingModelBody = await missingModelResponse.text()
    expect(missingModelBody.length).toBeGreaterThan(0)

    await page.goto(`${API_BASE}/dashboard`)
    let row = await searchEndpointRow(page, endpointName)
    await row.locator('button[title="Test Connection"]').click()
    await waitForEndpointTypeAndStatus(request, endpointName, 'ollama')
    row = await searchEndpointRow(page, endpointName)
    await expect(row.getByText('Online', { exact: true })).toBeVisible({ timeout: 20000 })

    const endpointAfterFailures = await getEndpointByName(request, endpointName)
    expect(endpointAfterFailures?.base_url).toBe(OLLAMA_BASE)
  })
})
