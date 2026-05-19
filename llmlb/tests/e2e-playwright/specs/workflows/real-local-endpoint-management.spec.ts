import { test, expect } from '@playwright/test'
import {
  cleanupRuntimeResources,
  getEndpointDetailsByName,
  OLLAMA_BASE,
  openDashboard,
  openEndpointDetails,
  prepareEndpointViaUi,
  probeLocalRuntimes,
  REAL_LOCAL_RUNTIME_SAVE_TIMEOUT_MS,
  RuntimeSelection,
  searchEndpointRow,
  waitForEndpointTypeAndStatus,
} from '../../helpers/real-local-runtime'

test.describe.configure({ mode: 'serial' })

test.describe('Real Local Endpoint Management @workflows @dashboard @real-runtimes', () => {
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

  test('edit endpoint settings, persist after reload, reconnect, and delete', async ({ page, request }) => {
    test.setTimeout(15 * 60_000)
    test.skip(!runtimeSelection, skipReason)

    await page.setViewportSize({ width: 1440, height: 960 })

    const originalName = `e2e-real-manage-${Date.now()}`
    const notes = `managed-by-playwright-${Date.now()}`
    endpointNames = [originalName]

    await openDashboard(page)
    await prepareEndpointViaUi(page, request, {
      endpointName: originalName,
      baseUrl: OLLAMA_BASE,
      endpointType: 'ollama',
      typeLabel: 'Ollama',
    })

    let detailsDialog = await openEndpointDetails(page, originalName)
    await detailsDialog.locator('#healthCheckInterval').fill('45')
    await detailsDialog.locator('#inferenceTimeout').fill('180')
    await detailsDialog.locator('#notes').fill(notes)
    await detailsDialog.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByText('Update Complete', { exact: true }).first()).toBeVisible({
      timeout: REAL_LOCAL_RUNTIME_SAVE_TIMEOUT_MS,
    })
    await detailsDialog.getByRole('button', { name: 'Close', exact: true }).first().click()
    await expect(detailsDialog).toBeHidden({ timeout: 10000 })

    await expect
      .poll(async () => {
        const endpoint = await getEndpointDetailsByName(request, originalName)
        return endpoint
          ? {
              status: endpoint.status,
              health: endpoint.health_check_interval_secs,
              timeout: endpoint.inference_timeout_secs,
              notes: endpoint.notes ?? '',
            }
          : null
      }, { timeout: 30000, intervals: [500, 1000, 2000] })
      .toEqual({
        status: 'online',
        health: 45,
        timeout: 180,
        notes,
      })

    await page.reload()
    await openDashboard(page)

    let row = await searchEndpointRow(page, originalName)
    await expect(row.getByText('Ollama', { exact: true })).toBeVisible({ timeout: 20000 })
    await expect(row.getByText('Online', { exact: true })).toBeVisible({ timeout: 20000 })

    row = await searchEndpointRow(page, originalName)
    await row.locator('button[title="Test Connection"]').click()
    await waitForEndpointTypeAndStatus(request, originalName, 'ollama')
    row = await searchEndpointRow(page, originalName)
    await expect(row.getByText('Online', { exact: true })).toBeVisible({ timeout: 20000 })

    await row.locator('button[title="Delete"]').click()
    const deleteDialog = page.getByRole('alertdialog').filter({ hasText: 'Delete Endpoint?' })
    await expect(deleteDialog).toBeVisible({ timeout: 10000 })
    await deleteDialog.getByRole('button', { name: 'Delete' }).click()

    await expect
      .poll(async () => Boolean(await getEndpointDetailsByName(request, originalName)), {
        timeout: 30000,
        intervals: [500, 1000, 2000],
      })
      .toBe(false)

    endpointNames = []
  })
})
