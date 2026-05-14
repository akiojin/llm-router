// Dashboard API

import { createApiErrorFromResponse, fetchWithAuth, API_BASE } from './client'
import type { DashboardEndpoint } from './endpoints'
import type { ModelStatEntry } from './endpoints'

export interface DashboardStats {
  /**
   * v2 (2026-01+): "runtimes" naming returned by the API
   * - Rust: #[serde(rename = "*_runtimes", alias = "*_nodes")]
   */
  total_runtimes?: number
  online_runtimes?: number
  pending_runtimes?: number
  registering_runtimes?: number
  offline_runtimes?: number

  /**
   * v1 (deprecated): "nodes" naming returned by older servers
   */
  total_nodes?: number
  online_nodes?: number
  pending_nodes?: number
  registering_nodes?: number
  offline_nodes?: number

  total_requests: number
  successful_requests: number
  failed_requests: number
  total_active_requests: number
  queued_requests: number
  average_response_time_ms: number | null
  average_gpu_usage: number | null
  average_gpu_memory_usage: number | null
  // Token statistics
  total_input_tokens: number
  total_output_tokens: number
  total_tokens: number
}

export interface DashboardOperations {
  health: 'healthy' | 'attention' | 'empty' | string
  total_endpoints: number
  online_endpoints: number
  pending_endpoints: number
  registering_endpoints: number
  offline_endpoints: number
  error_endpoints: number
  total_requests: number
  successful_requests: number
  failed_requests: number
  success_rate: number | null
  active_requests: number
  queued_requests: number
  average_response_time_ms: number | null
  output_tps: number | null
  total_input_tokens: number
  total_output_tokens: number
  total_tokens: number
  last_registered_at: string | null
  last_seen_at: string | null
}

export interface DashboardCapacity {
  total_models: number
  gpu_capable_endpoints: number
  gpu_telemetry_endpoints: number
  total_gpu_memory_bytes: number | null
  used_gpu_memory_bytes: number | null
  gpu_memory_usage_percent: number | null
  telemetry_status: 'available' | 'partial' | 'unavailable' | string
}

export interface DashboardActionItem {
  severity: 'critical' | 'warning' | 'info' | string
  title: string
  detail: string
  count: number
}

export type SyncState = 'idle' | 'running' | 'success' | 'failed'

export interface SyncProgress {
  model_id: string
  file: string
  downloaded_bytes: number
  total_bytes: number
}

export interface RequestHistoryItem {
  request_id: string
  timestamp: string
  model: string
  node_id?: string
  node_name?: string
  status: 'success' | 'error'
  duration_ms: number
  total_tokens?: number
  input_tokens?: number
  output_tokens?: number
  error?: string
  request_body?: unknown
  response_body?: unknown
  client_ip?: string
}

// /api/dashboard/request-responses APIのレスポンス型
export interface RequestResponseRecord {
  id: string
  timestamp: string
  request_type: string
  model: string
  node_id?: string
  node_machine_name?: string
  node_ip?: string
  request_body?: unknown
  response_body?: unknown
  duration_ms: number
  status: { type: 'success' } | { type: 'error'; message: string }
  completed_at?: string
  client_ip?: string
}

export interface RequestResponsesPage {
  records: RequestResponseRecord[]
  total_count: number
  page: number
  per_page: number
}

export interface EndpointTpsSummary {
  endpoint_id: string
  model_count: number
  aggregate_tps: number | null
  total_output_tokens: number
  total_requests: number
}

export type TpsApiKind = 'chat_completions' | 'completions' | 'responses'
export type TpsSource = 'production' | 'benchmark'

export interface DashboardOverview {
  endpoints: DashboardEndpoint[]
  operations: DashboardOperations
  capacity: DashboardCapacity
  action_items: DashboardActionItem[]
  history: RequestHistoryItem[]
  endpoint_tps: EndpointTpsSummary[]
  generated_at: string
  generation_time_ms: number
}

export interface LogEntry {
  timestamp: string
  level: string
  message?: string
  target?: string
  fields?: Record<string, unknown>
}

export interface LogResponse {
  source: string
  entries: LogEntry[]
  path?: string
}

// Token Statistics API types
export interface TokenStats {
  total_input_tokens: number
  total_output_tokens: number
  total_tokens: number
  request_count: number
}

export interface DailyTokenStats extends TokenStats {
  date: string
}

export interface MonthlyTokenStats extends TokenStats {
  month: string
}

export const dashboardApi = {
  getOverview: () => fetchWithAuth<DashboardOverview>('/api/dashboard/overview'),

  /** SPEC-e8e9326e: List endpoints */
  getEndpoints: () => fetchWithAuth<DashboardEndpoint[]>('/api/dashboard/endpoints'),

  getStats: () => fetchWithAuth<DashboardStats>('/api/dashboard/stats'),

  // Token statistics endpoints
  getTokenStats: () => fetchWithAuth<TokenStats>('/api/dashboard/stats/tokens'),

  getDailyTokenStats: (days?: number) =>
    fetchWithAuth<DailyTokenStats[]>('/api/dashboard/stats/tokens/daily', {
      params: { days },
    }),

  getMonthlyTokenStats: (months?: number) =>
    fetchWithAuth<MonthlyTokenStats[]>('/api/dashboard/stats/tokens/monthly', {
      params: { months },
    }),

  getRequestHistory: (limit?: number) =>
    fetchWithAuth<RequestHistoryItem[]>('/api/dashboard/request-history', {
      params: { limit },
    }),

  getNodeMetrics: (nodeId: string) =>
    fetchWithAuth<unknown[]>(`/api/dashboard/metrics/${nodeId}`),

  getRequestResponses: (params?: {
    limit?: number
    offset?: number
    model?: string
    status?: string
    client_ip?: string
  }) => fetchWithAuth<RequestResponsesPage>('/api/dashboard/request-responses', { params }),

  getRequestResponseDetail: (id: string) =>
    fetchWithAuth<unknown>(`/api/dashboard/request-responses/${id}`),

  exportRequestResponses: async (format: 'csv' | 'json') => {
    const response = await fetch(
      `${API_BASE}/api/dashboard/request-responses/export?format=${format}`,
      {
        credentials: 'include',
      }
    )
    if (!response.ok) {
      throw await createApiErrorFromResponse(response)
    }
    return response.blob()
  },

  getRouterLogs: (params?: { limit?: number }) =>
    fetchWithAuth<LogResponse>('/api/dashboard/logs/lb', { params }),

  getAllModelStats: () =>
    fetchWithAuth<ModelStatEntry[]>('/api/dashboard/model-stats'),
}
