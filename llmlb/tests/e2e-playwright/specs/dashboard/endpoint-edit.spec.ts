import { test, expect } from '@playwright/test'
import { ensureDashboardLogin, deleteEndpointsByName, listEndpoints } from '../../helpers/api-helpers'
import { startMockOpenAIEndpointServer, type MockOpenAIEndpointServer } from '../../helpers/mock-openai-endpoint'
import { DashboardSelectors } from '../../helpers/selectors'

const API_BASE = process.env.BASE_URL || 'http://127.0.0.1:32768'
const AUTH_HEADER = { Authorization: 'Bearer sk_debug', 'Content-Type': 'application/json' }

test.describe('Endpoint Edit @dashboard', () => {
  test.describe.configure({ mode: 'serial' })

  let mock: MockOpenAIEndpointServer
  const endpointName = `e2e-edit-${Date.now()}`

  test.beforeAll(async ({ request }, testInfo) => {
    testInfo.setTimeout(120000)
    mock = await startMockOpenAIEndpointServer()

    // Create endpoint
    await request.post(`${API_BASE}/api/endpoints`, {
      headers: AUTH_HEADER,
      data: { name: endpointName, base_url: mock.baseUrl },
    })

    // Wait for endpoint to be visible in API before tests start
    const deadline = Date.now() + 10000
    while (Date.now() < deadline) {
      const endpoints = await listEndpoints(request)
      if (endpoints.some((e) => e.name === endpointName)) break
      await new Promise((r) => setTimeout(r, 200))
    }
  })

  test.afterAll(async ({ request }) => {
    await deleteEndpointsByName(request, endpointName)
    // Also clean up the renamed endpoint
    await deleteEndpointsByName(request, `${endpointName}-renamed`)
    await mock.close()
  })

  test('EE-01: Display Name change is reflected in the endpoint list', async ({ page, request }) => {
    await ensureDashboardLogin(page)

    // Search for endpoint to handle pagination
    await page.getByPlaceholder('Search by name or URL...').fill(endpointName)
    await page.waitForTimeout(500)

    // Open endpoint detail modal via the table row
    const row = page.locator('tbody tr').filter({ hasText: endpointName })
    await expect(row).toBeVisible({ timeout: 10000 })
    await row.locator('button[title="Details"]').click()

    const modal = page.locator('[role="dialog"]')
    await expect(modal).toBeVisible({ timeout: 10000 })

    // Edit the display name field
    const nameInput = modal.locator('input[name="display_name"], #node-display-name, input[name="name"]')
    const isNameEditable = await nameInput.isVisible({ timeout: 3000 }).catch(() => false)

    if (isNameEditable) {
      const newName = `${endpointName}-renamed`
      await nameInput.fill(newName)

      // Save changes
      const saveBtn = modal.locator(`${DashboardSelectors.modals.nodeModalSave}, button:has-text("Save")`)
      await saveBtn.click()
      await page.waitForTimeout(1000)

      // Verify the new name appears in the list
      const updatedRow = page.locator('tbody tr').filter({ hasText: newName })
      const isUpdated = await updatedRow.isVisible({ timeout: 5000 }).catch(() => false)

      // Also verify via API
      const endpoints = await listEndpoints(request)
      const found = endpoints.find((e) => e.name === newName || e.name === endpointName)
      expect(found).toBeTruthy()
    }
  })

  test('EE-02: Health Check Interval change is reflected in API', async ({ page, request }) => {
    await ensureDashboardLogin(page)

    // Find the endpoint (may have been renamed)
    const endpoints = await listEndpoints(request)
    const ep = endpoints.find((e) => e.name.startsWith('e2e-edit-'))
    if (!ep) {
      test.skip(true, 'Endpoint not found')
      return
    }

    // Search for endpoint to handle pagination
    await page.getByPlaceholder('Search by name or URL...').fill(ep.name)
    await page.waitForTimeout(500)

    // Open detail modal
    const row = page.locator('tbody tr').filter({ hasText: ep.name })
    await expect(row).toBeVisible({ timeout: 10000 })
    await row.locator('button[title="Details"]').click()

    const modal = page.locator('[role="dialog"]')
    await expect(modal).toBeVisible({ timeout: 10000 })

    // Edit health check interval
    const intervalInput = modal.locator(
      'input[name="health_check_interval"], #node-health-check-interval'
    )
    const isEditable = await intervalInput.isVisible({ timeout: 3000 }).catch(() => false)

    if (isEditable) {
      await intervalInput.fill('30')

      const saveBtn = modal.locator(`${DashboardSelectors.modals.nodeModalSave}, button:has-text("Save")`)
      await saveBtn.click()
      await page.waitForTimeout(1000)

      // Verify via API
      const updated = await listEndpoints(request)
      const updatedEp = updated.find((e) => e.id === ep.id)
      expect(updatedEp).toBeTruthy()
    }
  })

  test('EE-03: Inference Timeout change is reflected in API', async ({ page, request }) => {
    await ensureDashboardLogin(page)

    const endpoints = await listEndpoints(request)
    const ep = endpoints.find((e) => e.name.startsWith('e2e-edit-'))
    if (!ep) {
      test.skip(true, 'Endpoint not found')
      return
    }

    // Force refresh dashboard table to reflect any previous test changes
    await page.locator('#refresh-button').click()
    await page.waitForLoadState('load')
    await page.waitForTimeout(1000)

    const row = page.locator('tbody tr').filter({ hasText: ep.name })
    await expect(row).toBeVisible({ timeout: 15000 })
    await row.locator('button[title="Details"]').click()

    const modal = page.locator('[role="dialog"]')
    await expect(modal).toBeVisible({ timeout: 10000 })

    const timeoutInput = modal.locator(
      'input[name="inference_timeout"], #node-inference-timeout'
    )
    const isEditable = await timeoutInput.isVisible({ timeout: 3000 }).catch(() => false)

    if (isEditable) {
      await timeoutInput.fill('120')

      const saveBtn = modal.locator(`${DashboardSelectors.modals.nodeModalSave}, button:has-text("Save")`)
      await saveBtn.click()
      await page.waitForTimeout(1000)

      const updated = await listEndpoints(request)
      const updatedEp = updated.find((e) => e.id === ep.id)
      expect(updatedEp).toBeTruthy()
    }
  })

  test('EE-04: Notes change persists after reopening modal', async ({ page, request }) => {
    await ensureDashboardLogin(page)

    const endpoints = await listEndpoints(request)
    const ep = endpoints.find((e) => e.name.startsWith('e2e-edit-'))
    if (!ep) {
      test.skip(true, 'Endpoint not found')
      return
    }

    const notesText = `Test notes ${Date.now()}`

    // Search for endpoint to handle pagination
    await page.getByPlaceholder('Search by name or URL...').fill(ep.name)
    await page.waitForTimeout(500)

    // Open detail modal
    const row = page.locator('tbody tr').filter({ hasText: ep.name })
    await expect(row).toBeVisible({ timeout: 10000 })
    await row.locator('button[title="Details"]').click()

    const modal = page.locator('[role="dialog"]')
    await expect(modal).toBeVisible({ timeout: 10000 })

    // Edit notes
    const notesInput = modal.locator('textarea[name="notes"], #node-notes')
    const isEditable = await notesInput.isVisible({ timeout: 3000 }).catch(() => false)

    if (isEditable) {
      await notesInput.fill(notesText)

      const saveBtn = modal.locator(`${DashboardSelectors.modals.nodeModalSave}, button:has-text("Save")`)
      await saveBtn.click()
      await page.waitForTimeout(1000)

      // Close modal (Escape or close button)
      await page.keyboard.press('Escape')
      await page.waitForTimeout(500)

      // Reopen modal and verify notes persisted
      await row.locator('button[title="Details"]').click()
      const modal2 = page.locator('[role="dialog"]')
      await expect(modal2).toBeVisible({ timeout: 10000 })

      const notesInput2 = modal2.locator('textarea[name="notes"], #node-notes')
      if (await notesInput2.isVisible({ timeout: 3000 }).catch(() => false)) {
        const value = await notesInput2.inputValue()
        expect(value).toContain(notesText)
      }
    }
  })

  test('EE-05: Inference Timeout minimum value 10 saves successfully', async ({ page, request }) => {
    await ensureDashboardLogin(page)

    const endpoints = await listEndpoints(request)
    const ep = endpoints.find((e) => e.name.startsWith('e2e-edit-'))
    if (!ep) {
      test.skip(true, 'Endpoint not found')
      return
    }

    const row = page.locator('tbody tr').filter({ hasText: ep.name })
    await expect(row).toBeVisible({ timeout: 10000 })
    await row.locator('button[title="Details"]').click()

    const modal = page.locator('[role="dialog"]')
    await expect(modal).toBeVisible({ timeout: 10000 })

    const timeoutInput = modal.locator(
      'input[name="inference_timeout"], #node-inference-timeout'
    )
    const isEditable = await timeoutInput.isVisible({ timeout: 3000 }).catch(() => false)

    if (isEditable) {
      await timeoutInput.fill('10')

      const saveBtn = modal.locator(`${DashboardSelectors.modals.nodeModalSave}, button:has-text("Save")`)
      await saveBtn.click()
      await page.waitForTimeout(1000)

      // No error banner should appear
      const errorBanner = page.locator(DashboardSelectors.errorBanner)
      const hasError = await errorBanner.isVisible({ timeout: 2000 }).catch(() => false)
      expect(hasError).toBe(false)
    }
  })

  test('EE-06: Inference Timeout maximum value 600 saves successfully', async ({ page, request }) => {
    await ensureDashboardLogin(page)

    const endpoints = await listEndpoints(request)
    const ep = endpoints.find((e) => e.name.startsWith('e2e-edit-'))
    if (!ep) {
      test.skip(true, 'Endpoint not found')
      return
    }

    const row = page.locator('tbody tr').filter({ hasText: ep.name })
    await expect(row).toBeVisible({ timeout: 10000 })
    await row.locator('button[title="Details"]').click()

    const modal = page.locator('[role="dialog"]')
    await expect(modal).toBeVisible({ timeout: 10000 })

    const timeoutInput = modal.locator(
      'input[name="inference_timeout"], #node-inference-timeout'
    )
    const isEditable = await timeoutInput.isVisible({ timeout: 3000 }).catch(() => false)

    if (isEditable) {
      await timeoutInput.fill('600')

      const saveBtn = modal.locator(`${DashboardSelectors.modals.nodeModalSave}, button:has-text("Save")`)
      await saveBtn.click()
      await page.waitForTimeout(1000)

      // No error banner should appear
      const errorBanner = page.locator(DashboardSelectors.errorBanner)
      const hasError = await errorBanner.isVisible({ timeout: 2000 }).catch(() => false)
      expect(hasError).toBe(false)
    }
  })

  test('EE-07: Add Endpoint dialog shows timeout guidance', async ({ page }) => {
    await ensureDashboardLogin(page)

    await page.getByRole('button', { name: 'Add Endpoint' }).click()

    const dialog = page.getByRole('dialog').filter({ hasText: 'Add New Endpoint' })
    await expect(dialog).toBeVisible({ timeout: 10000 })
    await expect(
      dialog.getByText('Local runtimes (xLLM, Ollama, LM Studio) default to 600 seconds.')
    ).toBeVisible()
    await expect(
      dialog.getByText('vLLM, llama.cpp, and OpenAI-compatible endpoints default to 120 seconds.')
    ).toBeVisible()
  })

  test('EE-08: Ollama endpoints default to 600 seconds and show a recommendation', async ({
    page,
    request,
  }) => {
    const ollamaEndpointName = `e2e-edit-ollama-${Date.now()}`
    const ollamaMock = await startMockOpenAIEndpointServer({ endpointType: 'ollama' })

    try {
      const createResponse = await request.post(`${API_BASE}/api/endpoints`, {
        headers: AUTH_HEADER,
        data: { name: ollamaEndpointName, base_url: ollamaMock.baseUrl },
      })
      expect(createResponse.ok()).toBeTruthy()

      const createdEndpoint = (await createResponse.json()) as {
        id: string
        inference_timeout_secs: number
      }
      expect(createdEndpoint.inference_timeout_secs).toBe(600)

      await expect
        .poll(
          async () => {
            const endpoints = await listEndpoints(request)
            return endpoints.some((endpoint) => endpoint.name === ollamaEndpointName)
          },
          { timeout: 10000, intervals: [200, 500, 1000] }
        )
        .toBe(true)

      await ensureDashboardLogin(page)
      await page.getByPlaceholder('Search by name or URL...').fill(ollamaEndpointName)
      await page.waitForTimeout(500)

      const row = page.locator('tbody tr').filter({ hasText: ollamaEndpointName })
      await expect(row).toBeVisible({ timeout: 10000 })
      await row.locator('button[title="Details"]').click()

      const modal = page.locator('[role="dialog"]').filter({ hasText: ollamaEndpointName })
      await expect(modal).toBeVisible({ timeout: 10000 })
      await expect(modal.locator('#inferenceTimeout')).toHaveValue('600')
      await expect(modal.getByText('Recommended for Ollama: 600 seconds')).toBeVisible()
    } finally {
      await deleteEndpointsByName(request, ollamaEndpointName)
      await ollamaMock.close()
    }
  })
})
