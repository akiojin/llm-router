import { test, expect, type APIRequestContext } from '@playwright/test'
import {
  API_BASE,
  DASHBOARD_ORIGIN,
  LM_STUDIO_BASE,
  findOpenAiModelEntry,
  getEndpointByName,
  openDashboard,
  registerEndpointViaUi,
  searchEndpointRow,
  waitForApiModelVisible,
  waitForEndpointRegistered,
  waitForEndpointTypeAndStatus,
  waitForModelVisibleInDetails,
} from '../../helpers/real-local-runtime'
import { listEndpoints } from '../../helpers/api-helpers'

const DEBUG_API_KEY = 'sk_debug'
const TARGET_MODEL = 'google/gemma-4-26b-a4b'
const RUN_LMSTUDIO_GEMMA4_E2E = process.env.RUN_LMSTUDIO_GEMMA4_E2E === '1'
const TINY_PNG_DATA_URL =
  'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAAAAXNSR0IArs4c6QAAAARnQU1BAACxjwv8YQUAAAAJcEhZcwAADsMAAA7DAcdvqGQAAACHSURBVHhe7dAhAQAADITA719681QAcQbJbjuzMdg0gMGmAQw2DWCwaQCDTQMYbBrAYNMABpsGMNg0gMGmAQw2DWCwaQCDTQMYbBrAYNMABpsGMNg0gMGmAQw2DWCwaQCDTQMYbBrAYNMABpsGMNg0gMGmAQw2DWCwaQCDTQMYbBrAYNMABpsHQ4jh0hEeUY0AAAAASUVORK5CYII='

type LmStudioModel = {
  key?: string
  type?: string
  capabilities?: {
    vision?: boolean
  }
  loaded_instances?: Array<{
    id?: string
    identifier?: string
    model_key?: string
  }>
}

type LmStudioModelsResponse = {
  models?: LmStudioModel[]
}

type OpenAiVisionModelEntry = {
  id?: string
  aliases?: string[]
  canonical_name?: string | null
  supported_apis?: string[]
}

type ChatCompletionJson = {
  choices?: Array<{
    message?: {
      role?: string
      content?: string
      reasoning?: string
      reasoning_content?: string
    }
  }>
}

async function postVisionChatWithModelReloadRetry(
  request: APIRequestContext,
  model: string
): Promise<ChatCompletionJson> {
  let lastFailure = ''
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    const chatResponse = await request.post(`${API_BASE}/v1/chat/completions`, {
      data: {
        model,
        messages: [
          {
            role: 'user',
            content: [
              { type: 'text', text: 'Describe this tiny image in one short sentence.' },
              { type: 'image_url', image_url: { url: TINY_PNG_DATA_URL } },
            ],
          },
        ],
        max_tokens: 64,
        temperature: 0,
        stream: false,
      },
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${DEBUG_API_KEY}`,
      },
      timeout: 600000,
    })

    if (chatResponse.ok()) {
      return (await chatResponse.json()) as ChatCompletionJson
    }

    const bodyText = await chatResponse.text()
    lastFailure = `HTTP ${chatResponse.status()}: ${bodyText}`
    if (chatResponse.status() === 502 && bodyText.includes('Model reloaded.')) {
      await new Promise((resolve) => setTimeout(resolve, 5000))
      continue
    }

    break
  }

  throw new Error(`Gemma4 vision chat failed with ${lastFailure}`)
}

test.describe.configure({ mode: 'serial' })

test.describe('LM Studio Gemma4 vision chat @workflows @real-runtimes', () => {
  test.skip(
    !RUN_LMSTUDIO_GEMMA4_E2E,
    'Set RUN_LMSTUDIO_GEMMA4_E2E=1 to run the external LM Studio Gemma4 vision E2E'
  )

  test('register LM Studio endpoint and run image_url chat completion through llmlb', async ({
    page,
    request,
  }) => {
    test.setTimeout(20 * 60_000)

    const lmStudioResponse = await request.get(`${LM_STUDIO_BASE}/api/v1/models`, {
      timeout: 10000,
    })
    expect(lmStudioResponse.ok(), `LM Studio preflight HTTP ${lmStudioResponse.status()}`).toBe(
      true
    )
    const lmStudioJson = (await lmStudioResponse.json()) as LmStudioModelsResponse
    const targetLmStudioModel = (lmStudioJson.models ?? []).find(
      (model) => model.key === TARGET_MODEL
    )
    expect(targetLmStudioModel, `${TARGET_MODEL} must be available in LM Studio`).toBeTruthy()
    expect(targetLmStudioModel?.type).toBe('llm')
    expect(targetLmStudioModel?.capabilities?.vision).toBe(true)
    const isLoaded =
      targetLmStudioModel?.loaded_instances?.some(
        (instance) =>
          instance.id === TARGET_MODEL ||
          instance.identifier === TARGET_MODEL ||
          instance.model_key === TARGET_MODEL
      ) ?? false
    console.log(`LM Studio ${TARGET_MODEL} loaded_instances present: ${isLoaded}`)

    await page.setViewportSize({ width: 1440, height: 960 })
    await page
      .context()
      .grantPermissions(['clipboard-read', 'clipboard-write'], { origin: DASHBOARD_ORIGIN })

    await openDashboard(page)

    const existingEndpoint = (await listEndpoints(request)).find(
      (endpoint) => endpoint.base_url === LM_STUDIO_BASE
    )
    const endpointName = existingEndpoint?.name ?? `e2e-gemma4-lmstudio-${Date.now()}`
    console.log(`Leaving LM Studio endpoint registered for manual check: ${endpointName}`)

    let row = existingEndpoint
      ? await searchEndpointRow(page, endpointName)
      : await registerEndpointViaUi(page, endpointName, LM_STUDIO_BASE)
    await waitForEndpointRegistered(request, endpointName)

    await row.locator('button[title="Test Connection"]').click()
    await waitForEndpointTypeAndStatus(request, endpointName, 'lm_studio')

    row = await searchEndpointRow(page, endpointName)
    await expect(row.getByText('LM Studio', { exact: true })).toBeVisible({ timeout: 20000 })
    await expect(row.getByText('Online', { exact: true })).toBeVisible({ timeout: 20000 })

    await row.locator('button[title="Sync Models"]').click()
    await expect
      .poll(
        async () => {
          const endpoint = await getEndpointByName(request, endpointName)
          return endpoint?.model_count ?? 0
        },
        { timeout: 120000, intervals: [1000, 2000, 5000] }
      )
      .toBeGreaterThan(0)

    await waitForModelVisibleInDetails(page, endpointName, TARGET_MODEL)

    const resolvedModelId = await waitForApiModelVisible(request, DEBUG_API_KEY, TARGET_MODEL)
    const modelsResponse = await request.get(`${API_BASE}/v1/models`, {
      headers: { Authorization: `Bearer ${DEBUG_API_KEY}` },
      timeout: 10000,
    })
    expect(modelsResponse.ok()).toBeTruthy()
    const modelsJson = (await modelsResponse.json()) as { data?: OpenAiVisionModelEntry[] }
    const apiModel = findOpenAiModelEntry(modelsJson.data ?? [], TARGET_MODEL) as
      | OpenAiVisionModelEntry
      | undefined
    expect(apiModel, `${TARGET_MODEL} should be exposed via /v1/models`).toBeTruthy()
    expect(apiModel?.supported_apis ?? []).toContain('image_input')

    const chatJson = await postVisionChatWithModelReloadRetry(request, resolvedModelId)
    const message = chatJson.choices?.[0]?.message
    expect(message?.role).toBe('assistant')
    expect(
      Boolean(
        message?.content?.trim() ||
          message?.reasoning?.trim() ||
          message?.reasoning_content?.trim()
      )
    ).toBe(true)
  })
})
