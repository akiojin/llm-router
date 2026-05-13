import { test, expect } from '@playwright/test';
import { DashboardPage } from '../../pages/dashboard.page';

test.describe('Model canonical tags @dashboard', () => {
  test('MCT-01: canonical model IDs are tagged and runtime aliases remain visible', async ({
    page,
  }) => {
    await page.route('**/api/dashboard/models', async (route) => {
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          object: 'list',
          data: [
            {
              id: 'Qwen/Qwen3.6-35B-A3B',
              object: 'model',
              created: 0,
              owned_by: 'load balancer',
              lifecycle_status: 'registered',
              ready: true,
              supported_apis: ['chat_completions', 'responses'],
              max_tokens: null,
              endpoint_ids: ['endpoint-qwen'],
              canonical_name: 'Qwen/Qwen3.6-35B-A3B',
              is_canonical: true,
              aliases: ['qwen/qwen3.6-35b-a3b'],
            },
          ],
        }),
      });
    });

    const dashboard = new DashboardPage(page);
    await dashboard.gotoModels();

    const row = page.getByRole('row').filter({ hasText: 'Qwen/Qwen3.6-35B-A3B' });
    await expect(row.getByText('canonical', { exact: true })).toBeVisible();
    await expect(row.getByText('qwen/qwen3.6-35b-a3b', { exact: true })).toBeVisible();
  });
});
