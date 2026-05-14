import { test, expect, type Locator } from '@playwright/test'
import {
  cleanupRuntimeResources,
  gotoEndpointPlayground,
  LM_STUDIO_BASE,
  OLLAMA_BASE,
  openDashboard,
  openPlaygroundSettings,
  prepareEndpointViaUi,
  probeLocalRuntimes,
  RuntimeSelection,
  searchEndpointRow,
  selectEndpointPlaygroundModel,
  sendPlaygroundMessage,
  waitForAssistantBubbleCount,
  waitForAssistantText,
  waitForModelVisibleInDetails,
} from '../../helpers/real-local-runtime'

async function setStreaming(dialog: Locator, enabled: boolean) {
  const streamingSwitch = dialog.getByRole('switch')
  const current = await streamingSwitch.getAttribute('aria-checked')
  if ((current === 'true') !== enabled) {
    await streamingSwitch.click()
  }
  await expect(streamingSwitch).toHaveAttribute('aria-checked', enabled ? 'true' : 'false')
}

test.describe.configure({ mode: 'serial' })

test.describe('Real Local Playground UI @workflows @playground @real-runtimes', () => {
  let runtimeSelection: RuntimeSelection | null = null
  let skipReason = ''
  let endpointNames: string[] = []

  test.beforeAll(async ({ request }) => {
    const runtimeProbe = await probeLocalRuntimes(request)
    if (!runtimeProbe.ok) {
      skipReason = runtimeProbe.reason
      return
    }
    runtimeSelection = runtimeProbe.selection
  })

  test.afterEach(async ({ request }) => {
    await cleanupRuntimeResources(request, { endpointNames })
    endpointNames = []
  })

  test('exercise endpoint playground for Ollama and LM Studio', async ({ page, request }) => {
    test.setTimeout(20 * 60_000)
    test.skip(!runtimeSelection, skipReason)
    const selection = runtimeSelection
    if (!selection) return

    await page.setViewportSize({ width: 1440, height: 960 })

    const ollamaEndpointName = `e2e-real-pg-ollama-${Date.now()}`
    const lmStudioEndpointName = `e2e-real-pg-lmstudio-${Date.now()}`
    endpointNames = [ollamaEndpointName, lmStudioEndpointName]

    await openDashboard(page)

    const ollamaEndpoint = await prepareEndpointViaUi(page, request, {
      endpointName: ollamaEndpointName,
      baseUrl: OLLAMA_BASE,
      endpointType: 'ollama',
      typeLabel: 'Ollama',
    })
    const lmStudioEndpoint = await prepareEndpointViaUi(page, request, {
      endpointName: lmStudioEndpointName,
      baseUrl: LM_STUDIO_BASE,
      endpointType: 'lm_studio',
      typeLabel: 'LM Studio',
    })

    await waitForModelVisibleInDetails(page, ollamaEndpointName, selection.ollamaModel)
    await waitForModelVisibleInDetails(page, lmStudioEndpointName, selection.lmStudioModel)

    await gotoEndpointPlayground(page, ollamaEndpoint.id)
    let settingsDialog = await openPlaygroundSettings(page)
    await settingsDialog.getByPlaceholder('You are a helpful assistant...').fill(
      'Reply briefly and follow the user instruction.'
    )
    await setStreaming(settingsDialog, true)
    await settingsDialog.getByRole('button', { name: 'Done' }).click()
    await selectEndpointPlaygroundModel(page, selection.ollamaModel)
    await sendPlaygroundMessage(page, 'Respond with a short confirmation for the endpoint playground test.')
    await waitForAssistantBubbleCount(page, 1)
    await waitForAssistantText(page)
    await expect(page.getByText('Failed to send message')).toHaveCount(0)

    await page.getByRole('button', { name: 'cURL' }).click()
    let curlDialog = page.getByRole('dialog').filter({ hasText: 'cURL Command' })
    await expect(curlDialog).toBeVisible({ timeout: 10000 })
    await expect(curlDialog.locator('pre')).toContainText(OLLAMA_BASE)
    await expect(curlDialog.locator('pre')).toContainText(selection.ollamaModel)
    await expect(curlDialog.locator('pre')).toContainText('"stream": true')
    await curlDialog.getByRole('button', { name: 'Close', exact: true }).first().click()

    await page.getByRole('button', { name: 'Back to Dashboard' }).click()
    await searchEndpointRow(page, ollamaEndpointName)

    await gotoEndpointPlayground(page, lmStudioEndpoint.id)
    settingsDialog = await openPlaygroundSettings(page)
    await settingsDialog.getByPlaceholder('You are a helpful assistant...').fill(
      'Reply briefly and follow the user instruction.'
    )
    await setStreaming(settingsDialog, false)
    await settingsDialog.getByRole('button', { name: 'Done' }).click()
    await selectEndpointPlaygroundModel(page, selection.lmStudioModel)
    await sendPlaygroundMessage(page, 'Respond with a short confirmation for the endpoint playground test.')
    await waitForAssistantBubbleCount(page, 1)
    await expect(page.getByText('Failed to send message')).toHaveCount(0)

    await page.getByRole('button', { name: 'cURL' }).click()
    curlDialog = page.getByRole('dialog').filter({ hasText: 'cURL Command' })
    await expect(curlDialog).toBeVisible({ timeout: 10000 })
    await expect(curlDialog.locator('pre')).toContainText(LM_STUDIO_BASE)
    await expect(curlDialog.locator('pre')).toContainText(selection.lmStudioModel)
    await expect(curlDialog.locator('pre')).toContainText('"stream": false')
    await curlDialog.getByRole('button', { name: 'Close', exact: true }).first().click()
  })
})
