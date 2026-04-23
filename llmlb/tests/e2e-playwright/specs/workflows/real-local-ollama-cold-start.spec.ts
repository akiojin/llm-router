import { test, expect } from '@playwright/test'
import { createApiKeyWithPermissions } from '../../helpers/api-helpers'
import {
  cleanupRuntimeResources,
  DASHBOARD_ORIGIN,
  ensureOllamaModelUnloaded,
  expectChatCompletion,
  expectChatCompletionError,
  findOllamaColdStartSensitiveModel,
  getEndpointDetails,
  getLocalRuntimeModels,
  gotoEndpointPlayground,
  OLLAMA_BASE,
  openDashboard,
  openEndpointDetails,
  prepareEndpointViaUi,
  retestEndpointConnection,
  resolveApiModelIdForRuntimeModel,
  selectEndpointPlaygroundModel,
  sendPlaygroundMessage,
} from '../../helpers/real-local-runtime'

test.describe.configure({ mode: 'serial' })

test.describe('Real Local Ollama Cold Start @workflows @dashboard @real-runtimes', () => {
  let coldStartModel: string | null = null
  let skipReason = ''
  let endpointNames: string[] = []
  let apiKeyName = ''
  let apiKeyId = ''

  test.beforeAll(async ({ request }) => {
    try {
      const runtimeModels = await getLocalRuntimeModels(request)
      coldStartModel = await findOllamaColdStartSensitiveModel(request, runtimeModels.ollamaModels)
      if (!coldStartModel) {
        skipReason =
          'Ollama preflight failed: no installed model exceeded 1 second on a cold-start probe'
        return
      }
    } catch (error) {
      skipReason = error instanceof Error ? error.message : String(error)
    }
  })

  test.afterEach(async ({ request }) => {
    await cleanupRuntimeResources(request, {
      apiKeyId,
      apiKeyName: apiKeyName || undefined,
      endpointNames,
    })
    endpointNames = []
    apiKeyName = ''
    apiKeyId = ''
  })

  test('cold-start requests honor endpoint inference timeout', async ({ page, request }) => {
    test.setTimeout(20 * 60_000)
    test.skip(!coldStartModel, skipReason)
    const model = coldStartModel

    await page.setViewportSize({ width: 1440, height: 960 })
    await page
      .context()
      .grantPermissions(['clipboard-read', 'clipboard-write'], { origin: DASHBOARD_ORIGIN })

    const endpointName = `e2e-real-cold-start-${Date.now()}`
    apiKeyName = `e2e-real-cold-key-${Date.now()}`
    endpointNames = [endpointName]

    await openDashboard(page)
    const endpoint = await prepareEndpointViaUi(page, request, {
      endpointName,
      baseUrl: OLLAMA_BASE,
      endpointType: 'ollama',
      typeLabel: 'Ollama',
    })

    let detailsDialog = await openEndpointDetails(page, endpointName)
    await detailsDialog.locator('#inferenceTimeout').fill('1')
    await detailsDialog.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByText('Update Complete', { exact: true }).first()).toBeVisible({
      timeout: 10000,
    })
    await detailsDialog.getByRole('button', { name: 'Close', exact: true }).first().click()
    await expect(detailsDialog).toBeHidden({ timeout: 10000 })

    await expect
      .poll(async () => (await getEndpointDetails(request, endpoint.id))?.inference_timeout_secs ?? 0, {
        timeout: 30000,
        intervals: [500, 1000, 2000],
      })
      .toBe(1)

    await ensureOllamaModelUnloaded(request, model)
    await gotoEndpointPlayground(page, endpoint.id)
    await selectEndpointPlaygroundModel(page, model)
    await sendPlaygroundMessage(page, 'Reply with OK only.')
    await expect(page.getByText('Failed to send message', { exact: true }).first()).toBeVisible({
      timeout: 180000,
    })
    await expect(page.getByText(/still loading/i).first()).toBeVisible({ timeout: 180000 })
    await retestEndpointConnection(request, endpoint.id, endpointName, 'ollama')

    const apiKey = await createApiKeyWithPermissions(request, apiKeyName, [
      'openai.inference',
      'openai.models.read',
    ])
    apiKeyId = apiKey.id

    const apiModelId = await resolveApiModelIdForRuntimeModel(
      request,
      apiKey.key,
      model
    )

    await ensureOllamaModelUnloaded(request, model)
    await expectChatCompletionError(
      request,
      apiKey.key,
      apiModelId,
      'Reply with OK only.',
      {
        status: 504,
        errorType: 'model_loading',
        errorMessageIncludes: 'still loading',
        timeout: 180000,
        maxTokens: 4,
      }
    )
    await retestEndpointConnection(request, endpoint.id, endpointName, 'ollama')

    await openDashboard(page)
    detailsDialog = await openEndpointDetails(page, endpointName)
    await detailsDialog.locator('#inferenceTimeout').fill('300')
    await detailsDialog.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByText('Update Complete', { exact: true }).first()).toBeVisible({
      timeout: 10000,
    })
    await detailsDialog.getByRole('button', { name: 'Close', exact: true }).first().click()
    await expect(detailsDialog).toBeHidden({ timeout: 10000 })

    await expect
      .poll(async () => (await getEndpointDetails(request, endpoint.id))?.inference_timeout_secs ?? 0, {
        timeout: 30000,
        intervals: [500, 1000, 2000],
      })
      .toBe(300)

    await retestEndpointConnection(request, endpoint.id, endpointName, 'ollama')
    await ensureOllamaModelUnloaded(request, model)
    await expectChatCompletion(request, apiKey.key, apiModelId, 'Reply with OK only.', {
      timeout: 300000,
      maxTokens: 4,
    })
  })
})
