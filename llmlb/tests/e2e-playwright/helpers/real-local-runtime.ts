import { expect, request as playwrightRequest, type APIRequestContext, type Page } from '@playwright/test'
import { spawnSync } from 'node:child_process'
import { DashboardPage } from '../pages/dashboard.page'
import {
  deleteApiKey,
  deleteEndpointsByBaseUrl,
  deleteEndpointsByName,
  listApiKeys,
  listEndpoints,
  type EndpointInfo,
} from './api-helpers'

export const API_BASE = process.env.BASE_URL || 'http://127.0.0.1:32768'
export const DASHBOARD_ORIGIN = new URL(API_BASE).origin
export const OLLAMA_BASE = 'http://127.0.0.1:11434'
export const LM_STUDIO_BASE = 'http://127.0.0.1:1234'
const DEBUG_AUTH_HEADERS = { Authorization: 'Bearer sk_debug' }

export type OllamaTagsResponse = {
  models?: Array<{ name?: string }>
}

export type OllamaPsResponse = {
  models?: Array<{ name?: string; model?: string }>
}

export type LmStudioApiModelsResponse = {
  models?: Array<{ key?: string; type?: string; size_bytes?: number }>
}

export type RuntimeSelection = {
  ollamaModel: string
  lmStudioModel: string
}

export type LmStudioLocalModel = {
  key: string
  type: string
  size_bytes?: number
}

export type LocalRuntimeModels = {
  ollamaModels: string[]
  lmStudioModels: LmStudioLocalModel[]
}

export type SupportedRuntimeModelCase = {
  runtime: 'ollama' | 'lm_studio'
  runtimeModel: string
  canonicalModel: string
  label: string
  requestKind: 'chat' | 'embeddings'
}

export type RuntimeProbeResult =
  | { ok: true; selection: RuntimeSelection }
  | { ok: false; reason: string }

function formatRuntimePreflightError(runtime: string, error: unknown): string {
  const message =
    error instanceof Error ? error.message.replace(/^apiRequestContext\.\w+:\s*/, '') : String(error)
  return `${runtime} preflight failed: ${message}`
}

const SUPPORTED_RUNTIME_MODEL_CASES: Array<{
  runtime: 'ollama' | 'lm_studio'
  candidateRuntimeModels: string[]
  canonicalModel: string
  label: string
  requestKind: 'chat' | 'embeddings'
}> = [
  {
    runtime: 'ollama',
    candidateRuntimeModels: ['gpt-oss:20b'],
    canonicalModel: 'openai/gpt-oss-20b',
    label: 'Ollama gpt-oss:20b',
    requestKind: 'chat',
  },
  {
    runtime: 'lm_studio',
    candidateRuntimeModels: ['openai/gpt-oss-20b'],
    canonicalModel: 'openai/gpt-oss-20b',
    label: 'LM Studio openai/gpt-oss-20b',
    requestKind: 'chat',
  },
  {
    runtime: 'ollama',
    candidateRuntimeModels: ['gpt-oss:120b'],
    canonicalModel: 'openai/gpt-oss-120b',
    label: 'Ollama gpt-oss:120b',
    requestKind: 'chat',
  },
  {
    runtime: 'lm_studio',
    candidateRuntimeModels: ['openai/gpt-oss-120b'],
    canonicalModel: 'openai/gpt-oss-120b',
    label: 'LM Studio openai/gpt-oss-120b',
    requestKind: 'chat',
  },
  {
    runtime: 'ollama',
    candidateRuntimeModels: ['qwen3-coder:30b', 'qwen3-coder:latest'],
    canonicalModel: 'Qwen/Qwen3-Coder-30B-A3B-Instruct',
    label: 'Ollama qwen3-coder:30b',
    requestKind: 'chat',
  },
  {
    runtime: 'lm_studio',
    candidateRuntimeModels: ['Qwen/qwen3-coder-30b', 'qwen/qwen3-coder-30b'],
    canonicalModel: 'Qwen/Qwen3-Coder-30B-A3B-Instruct',
    label: 'LM Studio qwen3-coder-30b',
    requestKind: 'chat',
  },
  {
    runtime: 'ollama',
    candidateRuntimeModels: ['qwen3:30b'],
    canonicalModel: 'Qwen/Qwen3-30B',
    label: 'Ollama qwen3:30b',
    requestKind: 'chat',
  },
  {
    runtime: 'ollama',
    candidateRuntimeModels: ['llama3.3:70b'],
    canonicalModel: 'meta-llama/Llama-3.3-70B-Instruct',
    label: 'Ollama llama3.3:70b',
    requestKind: 'chat',
  },
  {
    runtime: 'ollama',
    candidateRuntimeModels: ['gemma3:27b'],
    canonicalModel: 'google/gemma-3-27b-it',
    label: 'Ollama gemma3:27b',
    requestKind: 'chat',
  },
  {
    runtime: 'ollama',
    candidateRuntimeModels: ['qwen3.5:latest', 'qwen3.5:35b-a3b', 'qwen3.5-35b-a3b'],
    canonicalModel: 'Qwen/Qwen3.5-35B-A3B',
    label: 'Ollama qwen3.5',
    requestKind: 'chat',
  },
  {
    runtime: 'lm_studio',
    candidateRuntimeModels: ['qwen3.5-35b-a3b', 'qwen/qwen3.5-35b-a3b', 'qwen/qwen3.5-35b-a3b:2'],
    canonicalModel: 'Qwen/Qwen3.5-35B-A3B',
    label: 'LM Studio qwen3.5',
    requestKind: 'chat',
  },
  {
    runtime: 'ollama',
    candidateRuntimeModels: ['glm-4.7-flash:latest', 'glm-4.7-flash'],
    canonicalModel: 'THUDM/glm-4.7-flash',
    label: 'Ollama glm-4.7-flash',
    requestKind: 'chat',
  },
  {
    runtime: 'lm_studio',
    candidateRuntimeModels: ['zai-org/glm-4.7-flash'],
    canonicalModel: 'THUDM/glm-4.7-flash',
    label: 'LM Studio glm-4.7-flash',
    requestKind: 'chat',
  },
  {
    runtime: 'lm_studio',
    candidateRuntimeModels: [
      'nvidia-nemotron-3-super-120b-a12b',
      'nvidia/nemotron-3-super',
      'unsloth/nvidia-nemotron-3-super-120b-a12b',
    ],
    canonicalModel: 'nvidia/nemotron-3-super-120b-a12b',
    label: 'LM Studio nemotron-3-super',
    requestKind: 'chat',
  },
  {
    runtime: 'lm_studio',
    candidateRuntimeModels: ['nvidia/nemotron-3-nano', 'nvidia/nemotron-3-nano-4b'],
    canonicalModel: 'nvidia/Nemotron-3-Nano',
    label: 'LM Studio nemotron-3-nano',
    requestKind: 'chat',
  },
  {
    runtime: 'lm_studio',
    candidateRuntimeModels: ['text-embedding-nomic-embed-text-v1.5', 'nomic-ai/nomic-embed-text-v1.5'],
    canonicalModel: 'nomic-ai/nomic-embed-text-v1.5',
    label: 'LM Studio nomic embedding',
    requestKind: 'embeddings',
  },
]

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

export async function probeLocalRuntimes(
  request: APIRequestContext
): Promise<RuntimeProbeResult> {
  let runtimeModels: LocalRuntimeModels
  try {
    runtimeModels = await getLocalRuntimeModels(request)
  } catch (error) {
    return { ok: false, reason: formatRuntimePreflightError('Local runtime', error) }
  }
  const ollamaModels = runtimeModels.ollamaModels
  const ollamaModel = chooseOllamaModel(ollamaModels)
  if (!ollamaModel) {
    return { ok: false, reason: 'Ollama preflight failed: no local models found' }
  }

  const lmStudioModel = chooseLmStudioModel(
    runtimeModels.lmStudioModels
  )
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

export async function getLocalRuntimeModels(
  request: APIRequestContext
): Promise<LocalRuntimeModels> {
  let ollamaResponse
  try {
    ollamaResponse = await request.get(`${OLLAMA_BASE}/api/tags`, { timeout: 5000 })
  } catch (error) {
    throw new Error(formatRuntimePreflightError('Ollama', error), { cause: error })
  }
  if (!ollamaResponse.ok()) {
    throw new Error(`Ollama preflight failed: HTTP ${ollamaResponse.status()}`)
  }
  const ollamaJson = (await ollamaResponse.json()) as OllamaTagsResponse
  const ollamaModels = (ollamaJson.models ?? [])
    .map((model) => model.name?.trim() ?? '')
    .filter((name) => name.length > 0)

  let lmStudioResponse
  try {
    lmStudioResponse = await request.get(`${LM_STUDIO_BASE}/api/v1/models`, {
      timeout: 5000,
    })
  } catch (error) {
    throw new Error(formatRuntimePreflightError('LM Studio', error), { cause: error })
  }
  if (!lmStudioResponse.ok()) {
    throw new Error(`LM Studio preflight failed: HTTP ${lmStudioResponse.status()}`)
  }
  const lmStudioJson = (await lmStudioResponse.json()) as LmStudioApiModelsResponse
  const lmStudioModels = (lmStudioJson.models ?? [])
    .filter(
      (model): model is LmStudioLocalModel =>
        typeof model.type === 'string' &&
        typeof model.key === 'string' &&
        model.key.length > 0
    )
    .map((model) => ({
      key: model.key,
      type: model.type,
      size_bytes: model.size_bytes,
    }))

  return {
    ollamaModels,
    lmStudioModels,
  }
}

export async function getLoadedOllamaModels(request: APIRequestContext): Promise<string[]> {
  const response = await request.get(`${OLLAMA_BASE}/api/ps`, { timeout: 5000 })
  if (!response.ok()) {
    throw new Error(`Ollama process list failed: HTTP ${response.status()}`)
  }
  const json = (await response.json()) as OllamaPsResponse
  return (json.models ?? [])
    .map((model) => model.name?.trim() || model.model?.trim() || '')
    .filter((model) => model.length > 0)
}

export async function ensureOllamaModelUnloaded(
  request: APIRequestContext,
  model: string
): Promise<void> {
  const loadedBefore = await getLoadedOllamaModels(request)
  if (!loadedBefore.includes(model)) {
    return
  }

  let stopError: string | null = null
  const result = spawnSync('ollama', ['stop', model], {
    encoding: 'utf8',
    windowsHide: true,
  })
  if (result.status !== 0) {
    stopError = (result.stderr || result.stdout || '').trim() || `exit ${result.status}`
  }

  try {
    await expect
      .poll(async () => (await getLoadedOllamaModels(request)).includes(model), {
        timeout: 30000,
        intervals: [500, 1000, 2000],
      })
      .toBe(false)
  } catch (error) {
    const loadedNow = await getLoadedOllamaModels(request).catch(() => loadedBefore)
    const extra = stopError ? `; ollama stop error: ${stopError}` : ''
    throw new Error(
      `Failed to unload Ollama model ${model}; still loaded models: ${loadedNow.join(', ') || '(none)'}${extra}`,
      { cause: error }
    )
  }
}

export async function findOllamaColdStartSensitiveModel(
  request: APIRequestContext,
  availableModels: string[]
): Promise<string | null> {
  const preferredCandidates = [
    'qwen3:30b',
    'qwen3-coder:30b',
    'qwen3-coder:latest',
    'qwen3.5:latest',
    'gpt-oss:20b',
    'glm-4.7-flash:latest',
  ]
  const candidates = preferredCandidates.filter((candidate) => availableModels.includes(candidate))

  for (const model of candidates) {
    await ensureOllamaModelUnloaded(request, model)

    const api = await playwrightRequest.newContext()
    let timedOut = false
    try {
      await api.post(`${OLLAMA_BASE}/v1/chat/completions`, {
        data: {
          model,
          messages: [{ role: 'user', content: 'Reply with OK only.' }],
          max_tokens: 4,
          temperature: 0,
          stream: false,
        },
        headers: {
          'Content-Type': 'application/json',
        },
        timeout: 1000,
      })
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      timedOut =
        message.includes('Timeout') ||
        message.includes('timeout') ||
        message.includes('timed out')
    } finally {
      await api.dispose()
    }

    await ensureOllamaModelUnloaded(request, model)
    if (timedOut) {
      return model
    }
  }

  return null
}

export function findSupportedLocalRuntimeModels(
  runtimeModels: LocalRuntimeModels
): SupportedRuntimeModelCase[] {
  return SUPPORTED_RUNTIME_MODEL_CASES.flatMap((entry) => {
    const availableModels =
      entry.runtime === 'ollama'
        ? runtimeModels.ollamaModels
        : runtimeModels.lmStudioModels.map((model) => model.key)
    const runtimeModel = entry.candidateRuntimeModels.find((candidate) =>
      availableModels.includes(candidate)
    )
    if (!runtimeModel) return []
    return [
      {
        runtime: entry.runtime,
        runtimeModel,
        canonicalModel: entry.canonicalModel,
        label: entry.label,
        requestKind: entry.requestKind,
      },
    ]
  })
}

export async function openDashboard(page: Page) {
  const dashboard = new DashboardPage(page)
  await dashboard.goto()
  return dashboard
}

export async function clearEndpointSearch(page: Page) {
  const searchInput = page.getByPlaceholder('Search by name or URL...')
  if (await searchInput.isVisible().catch(() => false)) {
    await searchInput.fill('')
  }
}

export async function searchEndpointRow(page: Page, endpointName: string) {
  const searchInput = page.getByPlaceholder('Search by name or URL...')
  await expect(searchInput).toBeVisible({ timeout: 20000 })
  await searchInput.fill(endpointName)
  const row = page.getByRole('row').filter({ hasText: endpointName })
  await expect(row).toBeVisible({ timeout: 20000 })
  return row
}

export async function registerEndpointViaUi(
  page: Page,
  endpointName: string,
  baseUrl: string
) {
  await clearEndpointSearch(page)
  await page.getByRole('button', { name: 'Add Endpoint' }).click()
  const dialog = page.getByRole('dialog').filter({ hasText: 'Add New Endpoint' })
  await expect(dialog).toBeVisible({ timeout: 10000 })
  await dialog.locator('#endpoint-name').fill(endpointName)
  await dialog.locator('#endpoint-url').fill(baseUrl)
  const createResponse = page.waitForResponse(
    (response) =>
      response.url().includes('/api/endpoints') &&
      response.request().method() === 'POST' &&
      response.status() >= 200 &&
      response.status() < 300
  )
  await dialog.getByRole('button', { name: 'Create Endpoint' }).click()
  await createResponse
  await expect(dialog).toBeHidden({ timeout: 20000 })
  return searchEndpointRow(page, endpointName)
}

export async function waitForEndpointStatus(
  request: APIRequestContext,
  endpointName: string,
  expectedStatus: string | string[]
) {
  const expected = Array.isArray(expectedStatus) ? expectedStatus : [expectedStatus]
  await expect
    .poll(
      async () => {
        const endpoint = (await listEndpoints(request)).find((entry) => entry.name === endpointName)
        return expected.includes(endpoint?.status ?? '')
      },
      { timeout: 120000, intervals: [1000, 2000, 5000] }
    )
    .toBeTruthy()
}

export async function waitForEndpointTypeAndStatus(
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
      { timeout: 120000, intervals: [1000, 2000, 5000] }
    )
    .toBe(`online|${endpointType}`)
}

export async function waitForEndpointRegistered(
  request: APIRequestContext,
  endpointName: string
) {
  await expect
    .poll(async () => Boolean(await getEndpointByName(request, endpointName)), {
      timeout: 30000,
      intervals: [500, 1000, 2000],
    })
    .toBe(true)
}

export async function getEndpointByName(
  request: APIRequestContext,
  endpointName: string
): Promise<EndpointInfo | undefined> {
  return (await listEndpoints(request)).find((entry) => entry.name === endpointName)
}

export interface EndpointDetail extends EndpointInfo {
  health_check_interval_secs: number
  inference_timeout_secs: number
  last_error?: string | null
  notes?: string
}

export async function getEndpointDetails(
  request: APIRequestContext,
  endpointId: string
): Promise<EndpointDetail | null> {
  const response = await request.get(`${API_BASE}/api/endpoints/${endpointId}`, {
    headers: DEBUG_AUTH_HEADERS,
  })
  if (!response.ok()) {
    return null
  }
  return (await response.json()) as EndpointDetail
}

export async function getEndpointDetailsByName(
  request: APIRequestContext,
  endpointName: string
): Promise<EndpointDetail | null> {
  const endpoint = await getEndpointByName(request, endpointName)
  if (!endpoint) return null
  return getEndpointDetails(request, endpoint.id)
}

export async function prepareEndpointViaUi(
  page: Page,
  request: APIRequestContext,
  options: {
    endpointName: string
    baseUrl: string
    endpointType: 'ollama' | 'lm_studio'
    typeLabel: 'Ollama' | 'LM Studio'
  }
) {
  await deleteEndpointsByBaseUrl(request, options.baseUrl)
  let row = await registerEndpointViaUi(page, options.endpointName, options.baseUrl)
  await waitForEndpointRegistered(request, options.endpointName)
  await row.locator('button[title="Test Connection"]').click()
  await waitForEndpointTypeAndStatus(request, options.endpointName, options.endpointType)
  row = await searchEndpointRow(page, options.endpointName)
  await expect(row.getByText(options.typeLabel, { exact: true })).toBeVisible({ timeout: 20000 })
  await expect(row.getByText('Online', { exact: true })).toBeVisible({ timeout: 20000 })
  await row.locator('button[title="Sync Models"]').click()

  await expect
    .poll(async () => {
      const endpoint = await getEndpointByName(request, options.endpointName)
      return endpoint?.model_count ?? 0
    }, { timeout: 120000, intervals: [1000, 2000, 5000] })
    .toBeGreaterThan(0)

  const endpoint = await getEndpointByName(request, options.endpointName)
  expect(endpoint).toBeTruthy()
  return endpoint!
}

export async function retestEndpointConnection(
  request: APIRequestContext,
  endpointId: string,
  endpointName: string,
  endpointType: string
) {
  const response = await request.post(`${API_BASE}/api/endpoints/${endpointId}/test`, {
    headers: DEBUG_AUTH_HEADERS,
  })
  expect(response.ok()).toBeTruthy()
  await waitForEndpointTypeAndStatus(request, endpointName, endpointType)
}

export async function waitForModelVisibleInDetails(
  page: Page,
  endpointName: string,
  expectedModel: string
) {
  const row = await searchEndpointRow(page, endpointName)
  await row.locator('button[title="Details"]').click()
  const detailsDialog = page.getByRole('dialog').filter({ hasText: endpointName })
  await expect(detailsDialog).toBeVisible({ timeout: 20000 })
  await expect(detailsDialog.getByText(expectedModel, { exact: true })).toBeVisible({
    timeout: 120000,
  })
  await page.keyboard.press('Escape')
  await expect(detailsDialog).toBeHidden({ timeout: 20000 })
}

export async function openEndpointDetails(page: Page, endpointName: string) {
  const row = await searchEndpointRow(page, endpointName)
  await row.locator('button[title="Details"]').click()
  const detailsDialog = page.getByRole('dialog').filter({ hasText: endpointName })
  await expect(detailsDialog).toBeVisible({ timeout: 20000 })
  return detailsDialog
}

export async function waitForApiModelVisible(
  request: APIRequestContext,
  apiKey: string,
  modelId: string
) {
  let resolvedModelId = ''
  await expect
    .poll(
      async () => {
        const models = await getOpenAiModels(request, apiKey).catch(() => [])
        const match = findOpenAiModelEntry(models, modelId)
        resolvedModelId = match?.id?.trim() ?? ''
        return resolvedModelId
      },
      { timeout: 120000, intervals: [1000, 2000, 5000] }
    )
    .not.toBe('')

  return resolvedModelId
}

export type OpenAiModelEntry = {
  id?: string
  aliases?: string[]
  canonical_name?: string | null
}

function normalizeModelReference(reference: string | null | undefined): string {
  return reference?.trim() ?? ''
}

export function findOpenAiModelEntry(
  models: OpenAiModelEntry[],
  reference: string
): OpenAiModelEntry | undefined {
  const expected = normalizeModelReference(reference)
  if (!expected) return undefined

  return models.find((model) => {
    const candidates = [
      normalizeModelReference(model.id),
      normalizeModelReference(model.canonical_name),
      ...(model.aliases ?? []).map((alias) => normalizeModelReference(alias)),
    ].filter((candidate) => candidate.length > 0)
    return candidates.includes(expected)
  })
}

export async function getOpenAiModels(
  request: APIRequestContext,
  apiKey: string
): Promise<OpenAiModelEntry[]> {
  const response = await request.get(`${API_BASE}/v1/models`, {
    headers: { Authorization: `Bearer ${apiKey}` },
    timeout: 10000,
  })
  expect(response.ok()).toBeTruthy()
  const json = (await response.json()) as { data?: OpenAiModelEntry[] }
  return json.data ?? []
}

export async function resolveApiModelIdForRuntimeModel(
  request: APIRequestContext,
  apiKey: string,
  runtimeModel: string
): Promise<string> {
  let resolvedModelId = ''
  await expect
    .poll(
      async () => {
        const models = await getOpenAiModels(request, apiKey)
        const match = findOpenAiModelEntry(models, runtimeModel)
        resolvedModelId = match?.id?.trim() ?? ''
        return resolvedModelId
      },
      { timeout: 120000, intervals: [1000, 2000, 5000] }
    )
    .not.toBe('')

  return resolvedModelId
}

export async function createApiKeyViaUi(
  page: Page,
  keyName: string
): Promise<{ id: string; key: string }> {
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

export async function deleteApiKeysByName(
  request: APIRequestContext,
  keyName: string
): Promise<void> {
  const apiKeys = await listApiKeys(request)
  const targets = apiKeys.filter((key) => key.name === keyName)
  for (const target of targets) {
    await deleteApiKey(request, target.id).catch(() => {})
  }
}

export async function cleanupRuntimeResources(
  request: APIRequestContext,
  resources: {
    apiKeyId?: string
    apiKeyName?: string
    endpointNames?: string[]
  }
) {
  if (resources.apiKeyId) {
    await deleteApiKey(request, resources.apiKeyId).catch(() => {})
  }
  if (resources.apiKeyName) {
    await deleteApiKeysByName(request, resources.apiKeyName)
  }
  for (const endpointName of resources.endpointNames ?? []) {
    await deleteEndpointsByName(request, endpointName)
  }
}

export async function expectChatCompletion(
  _request: APIRequestContext,
  apiKey: string,
  model: string,
  prompt = 'Hi.',
  options?: {
    timeout?: number
    maxTokens?: number
  }
) {
  const api = await playwrightRequest.newContext()
  try {
    const response = await api.post(`${API_BASE}/v1/chat/completions`, {
      data: {
        model,
        messages: [{ role: 'user', content: prompt }],
        max_tokens: options?.maxTokens ?? 8,
        temperature: 0,
        stream: false,
      },
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${apiKey}`,
      },
      timeout: options?.timeout ?? 120000,
    })
    if (!response.ok()) {
      const bodyText = await response.text()
      throw new Error(
        `Chat completion failed with HTTP ${response.status()}: ${bodyText || '(empty body)'}`
      )
    }

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
  } finally {
    await api.dispose()
  }
}

export async function expectChatCompletionError(
  _request: APIRequestContext,
  apiKey: string,
  model: string,
  prompt: string,
  options: {
    status: number
    errorType?: string
    errorMessageIncludes?: string
    timeout?: number
    maxTokens?: number
  }
) {
  const api = await playwrightRequest.newContext()
  try {
    const response = await api.post(`${API_BASE}/v1/chat/completions`, {
      data: {
        model,
        messages: [{ role: 'user', content: prompt }],
        max_tokens: options.maxTokens ?? 8,
        temperature: 0,
        stream: false,
      },
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${apiKey}`,
      },
      timeout: options.timeout ?? 120000,
    })
    expect(response.status()).toBe(options.status)

    const json = (await response.json()) as {
      error?: {
        message?: string
        type?: string
      }
    }
    if (options.errorType) {
      expect(json.error?.type).toBe(options.errorType)
    }
    if (options.errorMessageIncludes) {
      expect(json.error?.message ?? '').toContain(options.errorMessageIncludes)
    }
  } finally {
    await api.dispose()
  }
}

export async function expectEmbeddings(
  _request: APIRequestContext,
  apiKey: string,
  model: string,
  input = 'embedding smoke test'
) {
  const api = await playwrightRequest.newContext()
  try {
    const response = await api.post(`${API_BASE}/v1/embeddings`, {
      data: {
        model,
        input,
      },
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${apiKey}`,
      },
      timeout: 120000,
    })
    expect(response.ok()).toBeTruthy()

    const json = (await response.json()) as {
      data?: Array<{ embedding?: number[] }>
      model?: string
    }
    expect(json.model).toBe(model)
    expect(Array.isArray(json.data)).toBeTruthy()
    expect((json.data?.[0]?.embedding?.length ?? 0) > 0).toBeTruthy()
  } finally {
    await api.dispose()
  }
}

export function getSseDataLines(body: string): string[] {
  return body
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.startsWith('data: '))
}

export async function postStreamingChatCompletion(
  request: APIRequestContext,
  apiKey: string,
  model: string,
  prompt: string
) {
  const response = await request.post(`${API_BASE}/v1/chat/completions`, {
    data: {
      model,
      messages: [{ role: 'user', content: prompt }],
      stream: true,
      temperature: 0,
      max_tokens: 64,
    },
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${apiKey}`,
    },
    timeout: 300000,
  })
  expect(response.ok()).toBeTruthy()
  const text = await response.text()
  return {
    response,
    text,
    dataLines: getSseDataLines(text),
  }
}

export async function gotoEndpointPlayground(page: Page, endpointId: string) {
  await page.goto(`${API_BASE}/dashboard/#playground/${endpointId}`)
  await expect(page.getByText('Start a conversation')).toBeVisible({ timeout: 20000 })
}

export async function openPlaygroundSettings(page: Page) {
  await page.getByRole('button', { name: 'Settings' }).click()
  const dialog = page.getByRole('dialog').filter({ hasText: 'Settings' })
  await expect(dialog).toBeVisible({ timeout: 10000 })
  return dialog
}

export async function selectEndpointPlaygroundModel(page: Page, modelName: string) {
  const modelSelect = page.getByRole('combobox').first()
  await modelSelect.click()
  await page.getByRole('option', { name: modelName }).click()
}

export async function sendPlaygroundMessage(page: Page, message: string) {
  const input = page.getByPlaceholder('Type a message or attach files...')
  await input.fill(message)
  await page.getByRole('button', { name: 'Send' }).click()
}

export async function waitForAssistantBubbleCount(page: Page, expectedCount: number) {
  const assistantIcons = page.locator('svg.lucide-bot')
  await expect(assistantIcons).toHaveCount(expectedCount, { timeout: 120000 })
}

export async function waitForAssistantText(page: Page) {
  const assistantRows = page.locator('div.flex.gap-3').filter({
    has: page.locator('svg.lucide-bot'),
  })
  await expect
    .poll(
      async () => {
        const messages = await assistantRows
          .locator('p.text-sm.whitespace-pre-wrap')
          .allTextContents()
        return messages.map((message) => message.trim()).filter((message) => message.length > 0)
          .length
      },
      { timeout: 120000, intervals: [1000, 2000, 5000] }
    )
    .toBeGreaterThan(0)
}
