// Audit Log API (SPEC-8301d106)

import { fetchWithAuth } from './client'

export interface AuditLogEntry {
  id: number
  timestamp: string
  http_method: string
  request_path: string
  status_code: number
  actor_type: string
  actor_id: string | null
  actor_username: string | null
  api_key_owner_id: string | null
  client_ip: string | null
  duration_ms: number | null
  input_tokens: number | null
  output_tokens: number | null
  total_tokens: number | null
  model_name: string | null
  endpoint_id: string | null
  detail: string | null
  is_migrated: boolean
}

export interface AuditLogListResponse {
  items: AuditLogEntry[]
  total: number
  page: number
  per_page: number
}

export interface HashChainVerifyResult {
  valid: boolean
  batches_checked: number
  tampered_batch: number | null
  message: string | null
}

export interface AuditLogFilters {
  actor_type?: string
  actor_id?: string
  http_method?: string
  request_path?: string
  status_code?: number
  time_from?: string
  time_to?: string
  search?: string
  page?: number
  per_page?: number
  include_archive?: boolean
}

export const auditLogApi = {
  list: (filters: AuditLogFilters = {}): Promise<AuditLogListResponse> =>
    fetchWithAuth('/api/dashboard/audit-logs', {
      params: filters as Record<string, string | number | boolean | undefined>,
    }),

  verify: (): Promise<HashChainVerifyResult> =>
    fetchWithAuth('/api/dashboard/audit-logs/verify', { method: 'POST' }),
}
