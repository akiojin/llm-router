import { test, expect } from '@playwright/test'
import { ensureDashboardLogin } from '../../helpers/api-helpers'

// メインアプリ(ログイン後)の言語切替が機能することを検証する。
// E2E は locale=en-US 固定なので既定は英語。Header の切替トグルで日本語に切り替わる。
test.describe('Dashboard i18n @dashboard', () => {
  test('DI-01: Header language switcher localizes the main app', async ({ page }) => {
    await ensureDashboardLogin(page)

    // 英語(既定): ツールバーの API Keys ボタン
    await expect(page.getByRole('button', { name: 'API Keys' })).toBeVisible({ timeout: 20000 })

    // 英語のタブ
    await expect(page.getByRole('tab', { name: 'Endpoints' })).toBeVisible()

    // 日本語へ切替: Header とタブの両方が反映される
    await page.getByRole('button', { name: 'Switch to Japanese' }).click()
    await expect(page.getByRole('button', { name: 'APIキー' })).toBeVisible({ timeout: 5000 })
    await expect(page.getByRole('tab', { name: 'エンドポイント' })).toBeVisible({ timeout: 5000 })

    // 英語へ戻す
    await page.getByRole('button', { name: 'Switch to English' }).click()
    await expect(page.getByRole('button', { name: 'API Keys' })).toBeVisible({ timeout: 5000 })
    await expect(page.getByRole('tab', { name: 'Endpoints' })).toBeVisible({ timeout: 5000 })
  })

  test('DI-02: Operations overview card localizes live on language switch', async ({ page }) => {
    await ensureDashboardLogin(page)

    // 既定の Endpoints タブに表示される稼働状況サマリーカード(英語)
    await expect(
      page.getByRole('region', { name: 'Operations overview' }),
    ).toBeVisible({ timeout: 20000 })

    // 日本語へ切替: dashboard.overview 名前空間が即時反映される
    await page.getByRole('button', { name: 'Switch to Japanese' }).click()
    await expect(
      page.getByRole('region', { name: '稼働状況の概要' }),
    ).toBeVisible({ timeout: 5000 })

    // 英語へ戻す
    await page.getByRole('button', { name: 'Switch to English' }).click()
    await expect(
      page.getByRole('region', { name: 'Operations overview' }),
    ).toBeVisible({ timeout: 5000 })
  })
})
