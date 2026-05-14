import { test, expect } from '@playwright/test'
import {
  cleanupRuntimeResources,
  ensureOllamaModelUnloaded,
  findOllamaColdStartSensitiveModel,
  getEndpointDetailsByName,
  getLocalRuntimeModels,
  gotoEndpointPlayground,
  OLLAMA_BASE,
  openDashboard,
  openEndpointDetails,
  prepareEndpointViaUi,
  probeLocalRuntimes,
  RuntimeSelection,
  searchEndpointRow,
  selectEndpointPlaygroundModel,
  sendPlaygroundMessage,
} from '../../helpers/real-local-runtime'

const API_BASE = process.env.BASE_URL || 'http://127.0.0.1:32768'

function formatFullNumber(value: number | null | undefined): string {
  if (value == null) return '-'
  return value.toLocaleString('ja-JP')
}

function formatTps(value: number | null | undefined): string {
  if (value == null) return '-'
  return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)} tok/s`
}

test.describe.configure({ mode: 'serial' })

test.describe('Real Dashboard Display @dashboard @real-runtimes', () => {
  let runtimeSelection: RuntimeSelection | null = null
  let coldStartModel: string | null = null
  let skipReason = ''
  let endpointNames: string[] = []

  test.beforeAll(async ({ request }) => {
    try {
      const runtimeProbe = await probeLocalRuntimes(request)
      if (!runtimeProbe.ok) {
        skipReason = runtimeProbe.reason
        return
      }
      runtimeSelection = runtimeProbe.selection
      const runtimeModels = await getLocalRuntimeModels(request)
      coldStartModel = await findOllamaColdStartSensitiveModel(request, runtimeModels.ollamaModels)
    } catch (error) {
      skipReason = error instanceof Error ? error.message : String(error)
    }
  })

  test.afterEach(async ({ request }) => {
    await cleanupRuntimeResources(request, { endpointNames })
    endpointNames = []
  })

  test('dashboard header and stats cards match real API data', async ({ page, request }) => {
    test.setTimeout(10 * 60_000)
    test.skip(!runtimeSelection, skipReason)

    await page.setViewportSize({ width: 1440, height: 960 })

    const endpointName = `e2e-real-dashboard-${Date.now()}`
    endpointNames = [endpointName]

    await openDashboard(page)
    await prepareEndpointViaUi(page, request, {
      endpointName,
      baseUrl: OLLAMA_BASE,
      endpointType: 'ollama',
      typeLabel: 'Ollama',
    })

    await page.click('#refresh-button')
    await page.waitForLoadState('load')
    await page.waitForTimeout(1000)

    const systemResponse = await page.request.get(`${API_BASE}/api/system`)
    expect(systemResponse.ok()).toBeTruthy()
    const systemJson = (await systemResponse.json()) as {
      version?: string
      current_version?: string
    }
    const systemVersion = systemJson.version ?? systemJson.current_version ?? ''
    expect(systemVersion.length).toBeGreaterThan(0)

    const endpointsResponse = await page.request.get(`${API_BASE}/api/dashboard/endpoints`)
    expect(endpointsResponse.ok()).toBeTruthy()
    const endpointsJson = (await endpointsResponse.json()) as Array<{ id: string }>

    const overviewResponse = await page.request.get(`${API_BASE}/api/dashboard/overview`)
    expect(overviewResponse.ok()).toBeTruthy()
    const overviewJson = (await overviewResponse.json()) as {
      operations: {
        total_endpoints: number
        total_requests: number
        output_tps: number | null
      }
      capacity: {
        gpu_capable_endpoints: number
      }
    }

    await expect(page.locator('#current-version')).toContainText(`Current v${systemVersion}`)
    await expect(page.locator('#connection-status')).toContainText('Connection: Online')

    await expect(
      page.locator('[data-stat="total-endpoints"]').locator('p.text-3xl')
    ).toHaveText(formatFullNumber(overviewJson.operations.total_endpoints))
    await expect(
      page.locator('[data-stat="total-requests"]').locator('p.text-3xl')
    ).toHaveText(formatFullNumber(overviewJson.operations.total_requests))
    await expect(
      page.locator('[data-stat="output-tps"]').locator('p.text-3xl')
    ).toHaveText(formatTps(overviewJson.operations.output_tps))
    await expect(
      page.locator('[data-stat="gpu-capacity"]').locator('p.text-3xl')
    ).toHaveText(formatFullNumber(overviewJson.capacity.gpu_capable_endpoints))
    expect(endpointsJson.length).toBe(overviewJson.operations.total_endpoints)
  })

  test('dashboard endpoint row and detail show real error classification badges', async ({
    page,
    request,
  }) => {
    test.setTimeout(10 * 60_000)
    test.skip(!runtimeSelection, skipReason)
    test.skip(!coldStartModel, 'No installed Ollama model reproduced a cold-start timeout')
    const model = coldStartModel
    if (!model) return

    await page.setViewportSize({ width: 1440, height: 960 })

    const endpointName = `e2e-real-dashboard-error-${Date.now()}`
    endpointNames = [endpointName]

    await openDashboard(page)
    const endpoint = await prepareEndpointViaUi(page, request, {
      endpointName,
      baseUrl: OLLAMA_BASE,
      endpointType: 'ollama',
      typeLabel: 'Ollama',
    })

    const detailsDialog = await openEndpointDetails(page, endpointName)
    await detailsDialog.locator('#inferenceTimeout').fill('1')
    await detailsDialog.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByText('Update Complete', { exact: true }).first()).toBeVisible({
      timeout: 10000,
    })
    await detailsDialog.getByRole('button', { name: 'Close', exact: true }).first().click()

    await ensureOllamaModelUnloaded(request, model)
    await gotoEndpointPlayground(page, endpoint.id)
    await selectEndpointPlaygroundModel(page, model)
    await sendPlaygroundMessage(page, 'Reply with OK only.')
    await expect(page.getByText('Failed to send message', { exact: true }).first()).toBeVisible({
      timeout: 180000,
    })
    await expect(page.getByText(/still loading/i).first()).toBeVisible({ timeout: 180000 })

    await expect
      .poll(
        async () => (await getEndpointDetailsByName(request, endpointName))?.last_error ?? '',
        { timeout: 30000, intervals: [500, 1000, 2000] }
      )
      .toContain('still loading')

    await openDashboard(page)
    const row = await searchEndpointRow(page, endpointName)
    await expect(row.getByText('Model loading', { exact: true })).toBeVisible({ timeout: 10000 })

    const detailAfterError = await openEndpointDetails(page, endpointName)
    await expect(
      detailAfterError.getByText('Model loading', { exact: true }).first()
    ).toBeVisible({ timeout: 10000 })
    await expect(detailAfterError.getByText(/still loading/i).first()).toBeVisible({
      timeout: 10000,
    })
  })
})
