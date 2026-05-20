// Dashboard settings API

import { fetchWithAuth } from './client'

export type SettingSource = 'env' | 'database' | 'default'

export interface ApiKeyRequiredSetting {
  key: 'api_key_required'
  value: string
  effective_value: string
  source: SettingSource
  env_override: boolean
}

export const settingsApi = {
  getApiKeyRequired: () =>
    fetchWithAuth<ApiKeyRequiredSetting>('/api/dashboard/settings/api_key_required'),
  updateApiKeyRequired: (required: boolean) =>
    fetchWithAuth<ApiKeyRequiredSetting>('/api/dashboard/settings/api_key_required', {
      method: 'PUT',
      body: JSON.stringify({ value: String(required) }),
    }),
}
