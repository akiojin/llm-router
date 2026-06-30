import { test, expect } from '@playwright/test'
import { ensureDashboardLogin, deleteEndpointsByName } from '../../helpers/api-helpers'
import {
  startMockOpenAIEndpointServer,
  type MockOpenAIEndpointServer,
} from '../../helpers/mock-openai-endpoint'

test.describe.configure({ mode: 'serial' })

// 推論モデル（gemma via LM Studio 等）は思考過程を `reasoning_content` として
// content とは別チャネルで返す。Playground はこれを最終回答に混ぜず、「思考過程」
// 折りたたみ（既定で閉じる）として分離表示する。モックは `[[REASON]]` センチネルで
// reasoning_content を emit する。
test.describe('Playground reasoning separation @playground', () => {
  let mock: MockOpenAIEndpointServer
  let endpointName = ''

  test.beforeAll(async () => {
    mock = await startMockOpenAIEndpointServer()
  })

  test.afterAll(async () => {
    await mock.close()
  })

  test.afterEach(async ({ request }) => {
    if (endpointName) {
      await deleteEndpointsByName(request, endpointName)
    }
  })

  test('reasoning is shown in a collapsed disclosure, not merged into the answer', async ({
    page,
  }) => {
    endpointName = `e2e-reasoning-${Date.now()}-${Math.random().toString(16).slice(2)}`

    await ensureDashboardLogin(page)

    // エンドポイントを UI から登録（JWT + CSRF）。
    await page.getByRole('button', { name: 'Add Endpoint' }).click()
    await page.fill('#endpoint-name', endpointName)
    await page.fill('#endpoint-url', mock.baseUrl)
    await page.getByRole('button', { name: 'Create Endpoint' }).click()

    const searchInput = page.getByPlaceholder('Search by name or URL...')
    await expect(searchInput).toBeVisible({ timeout: 20000 })
    await searchInput.fill(endpointName)

    const row = page.getByRole('row').filter({ hasText: endpointName })
    await expect(row).toBeVisible({ timeout: 20000 })

    // 接続テストで Online にし、モデルを同期。
    await row.locator('button[title="Test Connection"]').click()
    await expect(row.getByText('Online')).toBeVisible({ timeout: 20000 })
    await row.locator('button[title="Sync Models"]').click()
    await page.waitForTimeout(2000)

    const rowAfterSync = page.getByRole('row').filter({ hasText: endpointName })
    await rowAfterSync.locator('button[title="Details"]').click()
    const detailsDialog = page.getByRole('dialog').filter({ hasText: endpointName })
    await expect(detailsDialog).toBeVisible({ timeout: 20000 })
    await expect(detailsDialog.getByText(mock.models[0]).first()).toBeVisible({ timeout: 20000 })
    await detailsDialog.getByRole('button', { name: 'Open Playground' }).click()

    await expect(page.getByText('Start a conversation')).toBeVisible({ timeout: 20000 })

    const modelSelect = page.getByRole('combobox').first()
    await modelSelect.click()
    await page.getByRole('option', { name: mock.models[0] }).click()

    // `[[REASON]]` センチネルで reasoning_content を誘発。
    const input = page.getByPlaceholder('Type a message or attach files...')
    await input.fill('[[REASON]] hello')
    await page.getByRole('button', { name: 'Send' }).click()

    // 最終回答は主表示される。
    await expect(page.getByText('MOCK_OK')).toBeVisible({ timeout: 20000 })

    // 「思考過程」折りたたみが存在し、既定では閉じている（reasoning は非表示）。
    const disclosure = page.getByTestId('reasoning-disclosure')
    await expect(disclosure).toBeVisible({ timeout: 20000 })
    await expect(page.getByText('MOCK_REASONING')).toBeHidden()

    // 展開すると思考過程が見える（= content とは別チャネルで保持されている）。
    await disclosure.locator('summary').click()
    await expect(page.getByText('MOCK_REASONING')).toBeVisible({ timeout: 10000 })

    // 最終回答の吹き出しに思考過程が混入していない。
    await expect(page.getByText('MOCK_OK')).not.toContainText('MOCK_REASONING')
  })
})
