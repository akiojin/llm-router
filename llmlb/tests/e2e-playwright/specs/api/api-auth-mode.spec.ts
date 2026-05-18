import {
  test,
  expect,
  request as playwrightRequest,
  type APIRequestContext,
} from '@playwright/test';

const API_BASE = process.env.BASE_URL || 'http://127.0.0.1:32768';

type ApiKeyRequiredSetting = {
  key: 'api_key_required';
  value: string;
  effective_value: string;
  source: 'env' | 'database' | 'default';
  env_override: boolean;
};

async function getDashboardToken(request: APIRequestContext): Promise<string> {
  const response = await request.post(`${API_BASE}/api/auth/login`, {
    headers: { 'Content-Type': 'application/json' },
    data: { username: 'admin', password: 'test' },
  });
  expect(response.status()).toBe(200);
  const body = (await response.json()) as { token?: string };
  expect(body.token).toBeTruthy();
  return body.token as string;
}

async function getApiKeyRequired(
  request: APIRequestContext,
  token: string
): Promise<ApiKeyRequiredSetting> {
  const response = await request.get(`${API_BASE}/api/dashboard/settings/api_key_required`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  expect(response.status()).toBe(200);
  return response.json();
}

async function updateApiKeyRequired(
  request: APIRequestContext,
  token: string,
  required: boolean
): Promise<ApiKeyRequiredSetting> {
  const response = await request.put(`${API_BASE}/api/dashboard/settings/api_key_required`, {
    headers: {
      Authorization: `Bearer ${token}`,
      'Content-Type': 'application/json',
    },
    data: { value: String(required) },
  });
  expect(response.status()).toBe(200);
  return response.json();
}

async function anonymousGetStatus(path: string): Promise<number> {
  const anonymousContext = await playwrightRequest.newContext();
  try {
    const response = await anonymousContext.get(`${API_BASE}${path}`);
    return response.status();
  } finally {
    await anonymousContext.dispose();
  }
}

test.describe.configure({ mode: 'serial' });

test.describe('API Authentication Mode @api', () => {
  test('AM-01: api_key_required setting controls effective auth mode', async ({ request }) => {
    const token = await getDashboardToken(request);
    const initial = await getApiKeyRequired(request, token);

    try {
      const optional = await updateApiKeyRequired(request, token, false);

      expect(optional.key).toBe('api_key_required');
      expect(optional.value).toBe('false');

      const anonymousUsersStatus = await anonymousGetStatus('/api/users');

      if (optional.env_override) {
        expect(optional.source).toBe('env');
        expect(optional.effective_value).toBe('true');
        expect(anonymousUsersStatus).toBe(401);
        return;
      }

      expect(optional.source).toBe('database');
      expect(optional.effective_value).toBe('false');
      expect(anonymousUsersStatus).toBe(200);

      const required = await updateApiKeyRequired(request, token, true);
      expect(required.source).toBe('database');
      expect(required.effective_value).toBe('true');

      expect(await anonymousGetStatus('/api/users')).toBe(401);
    } finally {
      await updateApiKeyRequired(request, token, initial.value === 'true');
    }
  });
});
