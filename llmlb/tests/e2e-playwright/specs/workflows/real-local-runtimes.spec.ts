import { test, expect, type APIRequestContext, type Page } from '@playwright/test'
import { DashboardPage } from '../../pages/dashboard.page'
import {
  deleteApiKey,
  deleteEndpointsByName,
  listApiKeys,
  listEndpoints,
} from '../../helpers/api-helpers'
import {
  REAL_LOCAL_RUNTIME_JIT_TIMEOUT_MS,
  REAL_LOCAL_RUNTIME_READINESS_TIMEOUT_MS,
} from '../../helpers/real-local-runtime'

const API_BASE = process.env.BASE_URL || 'http://127.0.0.1:32768'
const DASHBOARD_ORIGIN = new URL(API_BASE).origin
const OLLAMA_BASE = 'http://127.0.0.1:11434'
const LM_STUDIO_BASE = 'http://127.0.0.1:1234'

type OllamaTagsResponse = {
  models?: Array<{ name?: string }>
}

type LmStudioApiModelsResponse = {
  models?: Array<{ key?: string; type?: string; size_bytes?: number }>
}

type RuntimeSelection = {
  ollamaModel: string
  lmStudioModel: string
}

type OpenAiModelEntry = {
  id?: string
  aliases?: string[]
  canonical_name?: string | null
}

function formatRuntimePreflightError(runtime: string, error: unknown): string {
  const message =
    error instanceof Error ? error.message.replace(/^apiRequestContext\.\w+:\s*/, '') : String(error)
  return `${runtime} preflight failed: ${message}`
}

function chooseOllamaModel(names: string[]): string | null {
  const preferred = ['qwen3.5:latest', 'glm-4.7-flash:latest', 'gpt-oss:20b']
  for (const candidate of preferred) {
    if (names.includes(candidate)) return candidate
  }
  return names[0] ?? null
}

function chooseLmStudioModel(
  models: Array<{ key?: string; type?: string; size_bytes?: number }>
): string | null {
  const llmModels = models.filter(
    (model): model is { key: string; type?: string; size_bytes?: number } =>
      model.type === 'llm' && typeof model.key === 'string' && model.key.length > 0
  )

  const preferred = [
    'zai-org/glm-4.7-flash',
    'nvidia/nemotron-3-nano',
    'qwen3.5-35b-a3b',
    'nvidia-nemotron-3-super-120b-a12b',
  ]
  for (const candidate of preferred) {
    if (llmModels.some((model) => model.key === candidate)) return candidate
  }

  llmModels.sort(
    (left, right) =>
      (left.size_bytes ?? Number.MAX_SAFE_INTEGER) -
      (right.size_bytes ?? Number.MAX_SAFE_INTEGER)
  )
  return llmModels[0]?.key ?? null
}

async function probeLocalRuntimes(
  request: APIRequestContext
): Promise<{ ok: true; selection: RuntimeSelection } | { ok: false; reason: string }> {
  let ollamaResponse
  try {
    ollamaResponse = await request.get(`${OLLAMA_BASE}/api/tags`, { timeout: 5000 })
  } catch (error) {
    return { ok: false, reason: formatRuntimePreflightError('Ollama', error) }
  }
  if (!ollamaResponse.ok()) {
    return { ok: false, reason: `Ollama preflight failed: HTTP ${ollamaResponse.status()}` }
  }
  const ollamaJson = (await ollamaResponse.json()) as OllamaTagsResponse
  const ollamaModels = (ollamaJson.models ?? [])
    .map((model) => model.name?.trim() ?? '')
    .filter((name) => name.length > 0)
  const ollamaModel = chooseOllamaModel(ollamaModels)
  if (!ollamaModel) {
    return { ok: false, reason: 'Ollama preflight failed: no local models found' }
  }

  let lmStudioResponse
  try {
    lmStudioResponse = await request.get(`${LM_STUDIO_BASE}/api/v1/models`, {
      timeout: 5000,
    })
  } catch (error) {
    return { ok: false, reason: formatRuntimePreflightError('LM Studio', error) }
  }
  if (!lmStudioResponse.ok()) {
    return {
      ok: false,
      reason: `LM Studio preflight failed: HTTP ${lmStudioResponse.status()}`,
    }
  }
  const lmStudioJson = (await lmStudioResponse.json()) as LmStudioApiModelsResponse
  const lmStudioModel = chooseLmStudioModel(lmStudioJson.models ?? [])
  if (!lmStudioModel) {
    return { ok: false, reason: 'LM Studio preflight failed: no chat-capable models found' }
  }

  return {
    ok: true,
    selection: {
      ollamaModel,
      lmStudioModel,
    },
  }
}

async function searchEndpointRow(page: Page, endpointName: string) {
  const searchInput = page.getByPlaceholder('Search by name or URL...')
  await expect(searchInput).toBeVisible({ timeout: 20000 })
  await searchInput.fill(endpointName)
  const row = page.getByRole('row').filter({ hasText: endpointName })
  await expect(row).toBeVisible({ timeout: 20000 })
  return row
}

async function registerEndpointViaUi(page: Page, endpointName: string, baseUrl: string) {
  const searchInput = page.getByPlaceholder('Search by name or URL...')
  if (await searchInput.isVisible().catch(() => false)) {
    await searchInput.fill('')
  }

  await page.getByRole('button', { name: 'Add Endpoint' }).click()
  await page.fill('#endpoint-name', endpointName)
  await page.fill('#endpoint-url', baseUrl)
  const createResponse = page.waitForResponse(
    (response) =>
      response.url().includes('/api/endpoints') &&
      response.request().method() === 'POST' &&
      response.status() >= 200 &&
      response.status() < 300
  )
  await page.getByRole('button', { name: 'Create Endpoint' }).click()
  await createResponse
  return searchEndpointRow(page, endpointName)
}

async function waitForEndpointTypeAndStatus(
  request: APIRequestContext,
  endpointName: string,
  endpointType: string
) {
  await expect
    .poll(
      async () => {
        const endpoints = await listEndpoints(request)
        const endpoint = endpoints.find((entry) => entry.name === endpointName)
        if (!endpoint) return 'missing'
        return `${endpoint.status}|${endpoint.endpoint_type ?? ''}`
      },
      { timeout: REAL_LOCAL_RUNTIME_READINESS_TIMEOUT_MS, intervals: [1000, 2000, 5000] }
    )
    .toBe(`online|${endpointType}`)
}

async function waitForModelVisibleInDetails(
  page: Page,
  endpointName: string,
  expectedModel: string
) {
  const row = await searchEndpointRow(page, endpointName)
  await row.locator('button[title="Details"]').click()
  const detailsDialog = page.getByRole('dialog').filter({ hasText: endpointName })
  await expect(detailsDialog).toBeVisible({ timeout: 20000 })
  await expect(detailsDialog.getByText(expectedModel)).toBeVisible({
    timeout: REAL_LOCAL_RUNTIME_READINESS_TIMEOUT_MS,
  })
  await page.keyboard.press('Escape')
  await expect(detailsDialog).toBeHidden({ timeout: 20000 })
}

async function waitForApiModelVisible(
  request: APIRequestContext,
  apiKey: string,
  modelId: string
) {
  let resolvedModelId = ''
  await expect
    .poll(
      async () => {
        const response = await request.get(`${API_BASE}/v1/models`, {
          headers: { Authorization: `Bearer ${apiKey}` },
          timeout: 10000,
        })
        if (!response.ok()) return ''
        const json = (await response.json()) as { data?: OpenAiModelEntry[] }
        const match = (json.data ?? []).find((model) => {
          const candidates = [
            model.id?.trim() ?? '',
            model.canonical_name?.trim() ?? '',
            ...(model.aliases ?? []).map((alias) => alias.trim()),
          ].filter((candidate) => candidate.length > 0)
          return candidates.includes(modelId)
        })
        resolvedModelId = match?.id?.trim() ?? ''
        return resolvedModelId
      },
      { timeout: REAL_LOCAL_RUNTIME_READINESS_TIMEOUT_MS, intervals: [1000, 2000, 5000] }
    )
    .not.toBe('')

  return resolvedModelId
}

async function createApiKeyViaUi(page: Page, keyName: string): Promise<{ id: string; key: string }> {
  await page.click('#api-keys-button')
  const apiKeysModal = page.locator('#api-keys-modal')
  await expect(apiKeysModal).toBeVisible({ timeout: 10000 })

  await apiKeysModal.locator('#create-api-key').click()
  const createDialog = page.getByRole('dialog').filter({ hasText: 'Create API Key' })
  await expect(createDialog).toBeVisible({ timeout: 10000 })

  await createDialog.locator('#api-key-name').fill(keyName)

  const createApiKeyResponse = page.waitForResponse(
    (response) =>
      response.url().includes('/api/me/api-keys') &&
      response.request().method() === 'POST' &&
      response.status() >= 200 &&
      response.status() < 300
  )

  await createDialog.getByRole('button', { name: 'Create', exact: true }).click()
  await expect(createDialog).toBeHidden({ timeout: 20000 })

  const createResp = await createApiKeyResponse
  const createRespBody = (await createResp.json()) as { id?: string; key?: string }
  const apiKey = createRespBody.key?.trim() ?? ''
  expect(apiKey).toMatch(/^sk_/)

  const createdAlert = apiKeysModal.getByText('API Key Created Successfully').locator('..')
  await expect(createdAlert).toBeVisible({ timeout: 10000 })
  await createdAlert.locator('#copy-api-key').click()
  await expect(page.getByText('Failed to copy')).toHaveCount(0)
  await expect(createdAlert.locator('#copy-api-key')).toHaveAttribute('data-copied', 'true', {
    timeout: 10000,
  })
  await expect(page.getByText('Auto copy unavailable')).toHaveCount(0)
  await expect
    .poll(async () => page.evaluate(() => navigator.clipboard.readText()), { timeout: 10000 })
    .toBe(apiKey)

  return {
    id: createRespBody.id ?? '',
    key: apiKey,
  }
}

async function deleteApiKeysByName(request: APIRequestContext, keyName: string): Promise<void> {
  const apiKeys = await listApiKeys(request)
  const targets = apiKeys.filter((key) => key.name === keyName)
  for (const target of targets) {
    await deleteApiKey(request, target.id).catch(() => {})
  }
}

async function expectChatCompletion(
  request: APIRequestContext,
  apiKey: string,
  model: string
) {
  const response = await request.post(`${API_BASE}/v1/chat/completions`, {
    data: {
      model,
      messages: [{ role: 'user', content: 'Reply with a short confirmation.' }],
      max_tokens: 32,
      temperature: 0,
      stream: false,
    },
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${apiKey}`,
    },
    timeout: REAL_LOCAL_RUNTIME_JIT_TIMEOUT_MS,
  })
  expect(response.ok()).toBeTruthy()

  const json = (await response.json()) as {
    choices?: Array<{
      message?: {
        content?: string
        role?: string
        reasoning?: string
        reasoning_content?: string
      }
    }>
  }
  const message = json.choices?.[0]?.message
  const assistantContent = message?.content?.trim() ?? ''
  const assistantReasoning = message?.reasoning?.trim() ?? ''
  const assistantReasoningContent = message?.reasoning_content?.trim() ?? ''

  expect(message?.role).toBe('assistant')
  expect(
    assistantContent.length > 0 ||
      assistantReasoning.length > 0 ||
      assistantReasoningContent.length > 0
  ).toBeTruthy()
}

test.describe.configure({ mode: 'serial' })

test.describe('Real Local Runtimes @workflows @real-runtimes', () => {
  let runtimeSelection: RuntimeSelection | null = null
  let skipReason = ''
  let ollamaEndpointName = ''
  let lmStudioEndpointName = ''
  let apiKeyName = ''
  let createdApiKeyId = ''

  test.beforeAll(async ({ request }) => {
    const runtimeProbe = await probeLocalRuntimes(request)
    if (!runtimeProbe.ok) {
      skipReason = runtimeProbe.reason
      return
    }
    runtimeSelection = runtimeProbe.selection
  })

  test.afterEach(async ({ request }) => {
    if (createdApiKeyId) {
      await deleteApiKey(request, createdApiKeyId).catch(() => {})
      createdApiKeyId = ''
    }
    if (apiKeyName) {
      await deleteApiKeysByName(request, apiKeyName)
      apiKeyName = ''
    }
    if (ollamaEndpointName) {
      await deleteEndpointsByName(request, ollamaEndpointName)
      ollamaEndpointName = ''
    }
    if (lmStudioEndpointName) {
      await deleteEndpointsByName(request, lmStudioEndpointName)
      lmStudioEndpointName = ''
    }
  })

  test('register Ollama and LM Studio, create API key, and complete real inference', async ({
    page,
    request,
  }) => {
    test.setTimeout(30 * 60_000)
    test.skip(!runtimeSelection, skipReason)
    const selection = runtimeSelection
    if (!selection) return

    await page.setViewportSize({ width: 1440, height: 960 })
    await page
      .context()
      .grantPermissions(['clipboard-read', 'clipboard-write'], { origin: DASHBOARD_ORIGIN })

    ollamaEndpointName = `e2e-real-ollama-${Date.now()}`
    lmStudioEndpointName = `e2e-real-lmstudio-${Date.now()}`
    apiKeyName = `e2e-real-key-${Date.now()}`

    const dashboard = new DashboardPage(page)
    await dashboard.goto()

    // Register Ollama via the dashboard.
    let row = await registerEndpointViaUi(page, ollamaEndpointName, OLLAMA_BASE)
    await row.locator('button[title="Test Connection"]').click()
    await waitForEndpointTypeAndStatus(request, ollamaEndpointName, 'ollama')
    row = await searchEndpointRow(page, ollamaEndpointName)
    await expect(row.getByText('Ollama', { exact: true })).toBeVisible({ timeout: 20000 })
    await expect(row.getByText('Online', { exact: true })).toBeVisible({ timeout: 20000 })
    await row.locator('button[title="Sync Models"]').click()

    // Register LM Studio via the dashboard.
    row = await registerEndpointViaUi(page, lmStudioEndpointName, LM_STUDIO_BASE)
    await row.locator('button[title="Test Connection"]').click()
    await waitForEndpointTypeAndStatus(request, lmStudioEndpointName, 'lm_studio')
    row = await searchEndpointRow(page, lmStudioEndpointName)
    await expect(row.getByText('LM Studio', { exact: true })).toBeVisible({ timeout: 20000 })
    await expect(row.getByText('Online', { exact: true })).toBeVisible({ timeout: 20000 })
    await row.locator('button[title="Sync Models"]').click()

    // Verify synced models are visible in the endpoint detail dialogs.
    await waitForModelVisibleInDetails(
      page,
      ollamaEndpointName,
      selection.ollamaModel
    )
    await waitForModelVisibleInDetails(
      page,
      lmStudioEndpointName,
      selection.lmStudioModel
    )

    // Create an API key in the UI and verify clipboard readback.
    const createdApiKey = await createApiKeyViaUi(page, apiKeyName)
    createdApiKeyId = createdApiKey.id
    expect(createdApiKey.key).toMatch(/^sk_/)

    // Wait until both synced runtime models are exposed via llmlb.
    const ollamaApiModelId = await waitForApiModelVisible(
      request,
      createdApiKey.key,
      selection.ollamaModel
    )
    const lmStudioApiModelId = await waitForApiModelVisible(
      request,
      createdApiKey.key,
      selection.lmStudioModel
    )

    // Run real inference through llmlb with the UI-created API key.
    await expectChatCompletion(request, createdApiKey.key, ollamaApiModelId)
    await expectChatCompletion(request, createdApiKey.key, lmStudioApiModelId)
  })
})
