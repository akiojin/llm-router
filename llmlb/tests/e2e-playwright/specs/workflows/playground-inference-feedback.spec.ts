import { test, expect } from '@playwright/test'
import { deleteEndpointsByName, ensureDashboardLogin } from '../../helpers/api-helpers'
import { startMockOpenAIEndpointServer, type MockOpenAIEndpointServer } from '../../helpers/mock-openai-endpoint'

test.describe.configure({ mode: 'serial' })

test.describe('Playground inference feedback @playground', () => {
  let mock: MockOpenAIEndpointServer
  let endpointName = ''

  test.beforeAll(async () => {
    mock = await startMockOpenAIEndpointServer({ responseDelayMs: 2500 })
  })

  test.afterAll(async () => {
    await mock.close()
  })

  test.afterEach(async ({ request }) => {
    if (endpointName) {
      await deleteEndpointsByName(request, endpointName)
      endpointName = ''
    }
  })

  test('shows generation feedback while waiting for the first response chunk', async ({ page }) => {
    endpointName = `e2e-playground-feedback-${Date.now()}-${Math.random().toString(16).slice(2)}`

    await ensureDashboardLogin(page)

    await page.getByRole('button', { name: 'Add Endpoint' }).click()
    await page.fill('#endpoint-name', endpointName)
    await page.fill('#endpoint-url', mock.baseUrl)
    await page.getByRole('button', { name: 'Create Endpoint' }).click()

    const searchInput = page.getByPlaceholder('Search by name or URL...')
    await expect(searchInput).toBeVisible({ timeout: 20000 })
    await searchInput.fill(endpointName)

    const row = page.getByRole('row').filter({ hasText: endpointName })
    await expect(row).toBeVisible({ timeout: 20000 })

    await row.locator('button[title="Test Connection"]').click()
    await expect(row.getByText('Online')).toBeVisible({ timeout: 20000 })

    await row.locator('button[title="Sync Models"]').click()
    await page.waitForTimeout(3000)

    const rowAfterSync = page.getByRole('row').filter({ hasText: endpointName })
    await expect(rowAfterSync).toBeVisible({ timeout: 10000 })
    await rowAfterSync.locator('button[title="Details"]').click()

    const detailsDialog = page.getByRole('dialog').filter({ hasText: endpointName })
    await expect(detailsDialog).toBeVisible({ timeout: 20000 })
    await expect(detailsDialog.getByText(mock.models[0]).first()).toBeVisible({ timeout: 20000 })

    await detailsDialog.getByRole('button', { name: 'Open Playground' }).click()
    await expect(page.getByText('Start a conversation')).toBeVisible({ timeout: 20000 })

    const modelSelect = page.getByRole('combobox').first()
    await modelSelect.click()
    await page.getByRole('option', { name: mock.models[0] }).click()

    await page.getByPlaceholder('Type a message or attach files...').fill('Show me that generation is still running')
    await page.getByRole('button', { name: 'Send' }).click()

    const generatingIndicator = page.getByTestId('playground-generating-indicator')
    await expect(generatingIndicator).toBeVisible({ timeout: 1000 })
    await expect(generatingIndicator).toContainText('Generating response')
    await expect(page.getByRole('button', { name: /Stop/i })).toBeVisible()

    await expect(page.getByText('MOCK_OK')).toBeVisible({ timeout: 20000 })
    await expect(generatingIndicator).toHaveCount(0)
  })
})
