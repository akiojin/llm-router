import { test, expect } from '@playwright/test'
import {
  DASHBOARD_ORIGIN,
  OLLAMA_BASE,
  LM_STUDIO_BASE,
  cleanupRuntimeResources,
  createApiKeyViaUi,
  expectChatCompletion,
  expectEmbeddings,
  findOpenAiModelEntry,
  findSupportedLocalRuntimeModels,
  getLocalRuntimeModels,
  getOpenAiModels,
  openDashboard,
  prepareEndpointViaUi,
  waitForModelVisibleInDetails,
  waitForApiModelVisible,
  type SupportedRuntimeModelCase,
} from '../../helpers/real-local-runtime'

test.describe.configure({ mode: 'serial' })

test.describe('Real Local Supported Models @workflows @real-runtimes', () => {
  let supportedCases: SupportedRuntimeModelCase[] = []
  let skipReason = ''
  let createdApiKeyId = ''
  let apiKeyName = ''
  let endpointNames: string[] = []

  test.beforeAll(async ({ request }) => {
    try {
      const runtimeModels = await getLocalRuntimeModels(request)
      supportedCases = findSupportedLocalRuntimeModels(runtimeModels)
      if (supportedCases.length === 0) {
        skipReason = 'No #481-built-in llmlb-supported local models are currently installed'
      }
    } catch (error) {
      skipReason = error instanceof Error ? error.message : String(error)
    }
  })

  test.afterEach(async ({ request }) => {
    await cleanupRuntimeResources(request, {
      apiKeyId: createdApiKeyId || undefined,
      apiKeyName: apiKeyName || undefined,
      endpointNames,
    })
    createdApiKeyId = ''
    apiKeyName = ''
    endpointNames = []
  })

  test('exercise every locally available #481-built-in llmlb-supported model through /v1/models and real API calls', async ({
    page,
    request,
  }) => {
    test.setTimeout(60 * 60_000)
    test.skip(supportedCases.length === 0, skipReason)

    await page.setViewportSize({ width: 1440, height: 960 })
    await page
      .context()
      .grantPermissions(['clipboard-read', 'clipboard-write'], { origin: DASHBOARD_ORIGIN })

    const ollamaCases = supportedCases.filter((entry) => entry.runtime === 'ollama')
    const lmStudioCases = supportedCases.filter((entry) => entry.runtime === 'lm_studio')

    const ollamaEndpointName = `e2e-supported-ollama-${Date.now()}`
    const lmStudioEndpointName = `e2e-supported-lmstudio-${Date.now()}`
    apiKeyName = `e2e-supported-key-${Date.now()}`

    const dashboard = await openDashboard(page)
    void dashboard

    if (ollamaCases.length > 0) {
      endpointNames.push(ollamaEndpointName)
      await prepareEndpointViaUi(page, request, {
        endpointName: ollamaEndpointName,
        baseUrl: OLLAMA_BASE,
        endpointType: 'ollama',
        typeLabel: 'Ollama',
      })
      for (const modelCase of ollamaCases) {
        await waitForModelVisibleInDetails(page, ollamaEndpointName, modelCase.runtimeModel)
      }
    }

    if (lmStudioCases.length > 0) {
      endpointNames.push(lmStudioEndpointName)
      await prepareEndpointViaUi(page, request, {
        endpointName: lmStudioEndpointName,
        baseUrl: LM_STUDIO_BASE,
        endpointType: 'lm_studio',
        typeLabel: 'LM Studio',
      })
      for (const modelCase of lmStudioCases) {
        await waitForModelVisibleInDetails(page, lmStudioEndpointName, modelCase.runtimeModel)
      }
    }

    const createdApiKey = await createApiKeyViaUi(page, apiKeyName)
    createdApiKeyId = createdApiKey.id
    await page.close()

    const resolvedApiModelIds = new Map<string, string>()
    const canonicalModels = [...new Set(supportedCases.map((entry) => entry.canonicalModel))]
    for (const canonicalModel of canonicalModels) {
      const resolvedApiModelId = await waitForApiModelVisible(
        request,
        createdApiKey.key,
        canonicalModel
      )
      resolvedApiModelIds.set(canonicalModel, resolvedApiModelId)
    }

    const apiModels = await getOpenAiModels(request, createdApiKey.key)
    const failures: string[] = []

    for (const modelCase of supportedCases) {
      await test.step(`model exposure: ${modelCase.label}`, async () => {
        try {
          const apiModel =
            findOpenAiModelEntry(apiModels, modelCase.canonicalModel) ??
            findOpenAiModelEntry(apiModels, modelCase.runtimeModel)
          expect(
            apiModel,
            `${modelCase.label} should be exposed as ${modelCase.canonicalModel}`
          ).toBeTruthy()
          const identifiers = [
            apiModel?.id ?? '',
            apiModel?.canonical_name ?? '',
            ...(apiModel?.aliases ?? []),
          ].filter((value) => value.length > 0)
          expect(identifiers).toContain(modelCase.canonicalModel)
          expect(identifiers).toContain(modelCase.runtimeModel)
        } catch (error) {
          failures.push(
            `[exposure] ${modelCase.label}: ${error instanceof Error ? error.message : String(error)}`
          )
        }
      })
    }

    const chatCanonicalModels = [
      ...new Set(
        supportedCases
          .filter((entry) => entry.requestKind === 'chat')
          .map((entry) => entry.canonicalModel)
      ),
    ]
    for (const canonicalModel of chatCanonicalModels) {
      await test.step(`chat completion: ${canonicalModel}`, async () => {
        try {
          await expectChatCompletion(
            request,
            createdApiKey.key,
            resolvedApiModelIds.get(canonicalModel) ?? canonicalModel,
            'Hi.'
          )
        } catch (error) {
          failures.push(
            `[chat] ${canonicalModel}: ${error instanceof Error ? error.message : String(error)}`
          )
        }
      })
    }

    const embeddingCanonicalModels = [
      ...new Set(
        supportedCases
          .filter((entry) => entry.requestKind === 'embeddings')
          .map((entry) => entry.canonicalModel)
      ),
    ]
    for (const canonicalModel of embeddingCanonicalModels) {
      await test.step(`embeddings: ${canonicalModel}`, async () => {
        try {
          await expectEmbeddings(
            request,
            createdApiKey.key,
            resolvedApiModelIds.get(canonicalModel) ?? canonicalModel
          )
        } catch (error) {
          failures.push(
            `[embeddings] ${canonicalModel}: ${error instanceof Error ? error.message : String(error)}`
          )
        }
      })
    }

    expect(
      failures,
      failures.length === 0 ? undefined : `Failing models:\n${failures.join('\n')}`
    ).toEqual([])
  })
})
