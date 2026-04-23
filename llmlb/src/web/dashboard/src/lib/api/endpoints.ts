// Endpoints API
// SPEC-e8e9326e: Router-Driven Endpoint Registration System

import { createApiErrorFromResponse, fetchWithAuth, getCsrfToken, API_BASE } from './client'
import type { TpsApiKind, TpsSource } from './dashboard'

/**
 * SPEC-e8e9326e: Router-Driven Endpoint Registration System
 * Dashboard display info for external inference services (Ollama, vLLM, xLLM, etc.)
 */
export type EndpointType =
  | 'xllm'
  | 'ollama'
  | 'vllm'
  | 'lm_studio'
  | 'llamacpp'
  | 'openai_compatible'
  | 'unknown'

export const CREATE_ENDPOINT_TIMEOUT_GUIDANCE = [
  'Local runtimes (xLLM, Ollama, LM Studio) default to 600 seconds.',
  'vLLM, llama.cpp, and OpenAI-compatible endpoints default to 120 seconds.',
] as const

export function getRecommendedInferenceTimeout(endpointType: EndpointType | undefined): number {
  switch (endpointType) {
    case 'xllm':
    case 'ollama':
    case 'lm_studio':
      return 600
    default:
      return 120
  }
}

export function getRecommendedInferenceTimeoutLabel(
  endpointType: EndpointType | undefined
): string {
  switch (endpointType) {
    case 'xllm':
      return 'Recommended for xLLM: 600 seconds'
    case 'ollama':
      return 'Recommended for Ollama: 600 seconds'
    case 'lm_studio':
      return 'Recommended for LM Studio: 600 seconds'
    case 'vllm':
      return 'Recommended for vLLM: 120 seconds'
    case 'llamacpp':
      return 'Recommended for llama.cpp: 120 seconds'
    case 'openai_compatible':
      return 'Recommended for OpenAI-compatible endpoints: 120 seconds'
    default:
      return `Recommended timeout: ${getRecommendedInferenceTimeout(endpointType)} seconds`
  }
}

export interface DashboardEndpoint {
  id: string
  name: string
  base_url: string
  status: 'pending' | 'online' | 'offline' | 'error'
  endpoint_type: EndpointType
  health_check_interval_secs: number
  inference_timeout_secs: number
  latency_ms?: number
  last_seen?: string
  last_error?: string
  error_count: number
  registered_at: string
  notes?: string
  model_count: number
  total_requests: number
  successful_requests: number
  failed_requests: number
}

/**
 * SPEC-e8e9326e: Model download task for xLLM endpoints
 */
export interface DownloadTask {
  task_id: string
  model: string
  status: 'pending' | 'downloading' | 'completed' | 'failed' | 'cancelled'
  progress: number
  speed_mbps?: number
  eta_seconds?: number
  error?: string
  filename?: string
}

/**
 * SPEC-e8e9326e: Model metadata from endpoint
 */
export interface ModelMetadata {
  model: string
  context_length?: number
  size_bytes?: number
  quantization?: string
  family?: string
  parameter_size?: string
}

/**
 * SPEC-8c32349f: Endpoint today stats (daily summary for a single day)
 */
export interface EndpointTodayStats {
  date: string
  total_requests: number
  successful_requests: number
  failed_requests: number
}

/**
 * SPEC-8c32349f: Daily stat entry (used for trend charts)
 */
export interface EndpointDailyStatEntry {
  date: string
  total_requests: number
  successful_requests: number
  failed_requests: number
}

/**
 * SPEC-8c32349f: Model-level request statistics entry
 */
export interface ModelStatEntry {
  model_id: string
  total_requests: number
  successful_requests: number
  failed_requests: number
}

/** SPEC-4bb5b55f: Model-level TPS entry */
export interface ModelTpsEntry {
  model_id: string
  api_kind: TpsApiKind
  source: TpsSource
  tps: number | null
  request_count: number
  total_output_tokens: number
  average_duration_ms: number | null
}

export interface TpsBenchmarkRequest {
  model: string
  api_kind?: TpsApiKind
  total_requests?: number
  concurrency?: number
  max_tokens?: number
  temperature?: number
}

export interface TpsBenchmarkEndpointSummary {
  endpoint_id: string
  endpoint_name: string
  requests: number
  successful_requests: number
  measured_requests: number
  success_rate: number
  mean_tps: number | null
  p50_tps: number | null
  p95_tps: number | null
}

export interface TpsBenchmarkResult {
  api_kind: TpsApiKind
  source: TpsSource
  total_requests: number
  successful_requests: number
  measured_requests: number
  success_rate: number
  mean_tps: number | null
  p50_tps: number | null
  p95_tps: number | null
  per_endpoint: TpsBenchmarkEndpointSummary[]
}

export interface TpsBenchmarkRun {
  run_id: string
  status: 'running' | 'completed' | 'failed'
  requested_at: string
  completed_at: string | null
  request: TpsBenchmarkRequest
  result: TpsBenchmarkResult | null
  error: string | null
}

export interface TpsBenchmarkAccepted {
  run_id: string
  status: 'running'
}

function extractAssistantDeltaText(parsed: {
  choices?: Array<{
    delta?: {
      content?: string
      reasoning?: string
      reasoning_content?: string
    }
  }>
}): string {
  const delta = parsed.choices?.[0]?.delta
  return delta?.content || delta?.reasoning_content || delta?.reasoning || ''
}

export const endpointsApi = {
  /** List endpoints for dashboard */
  list: () => fetchWithAuth<DashboardEndpoint[]>('/api/dashboard/endpoints'),

  /** SPEC-e8e9326e: List endpoints by type */
  listByType: (type: EndpointType) =>
    fetchWithAuth<DashboardEndpoint[]>('/api/endpoints', {
      params: { type },
    }),

  /** Create endpoint */
  create: (data: {
    name: string
    base_url: string
    api_key?: string
    health_check_interval_secs?: number
    inference_timeout_secs?: number
    notes?: string
  }) =>
    fetchWithAuth<DashboardEndpoint>('/api/endpoints', {
      method: 'POST',
      body: JSON.stringify(data),
    }),

  /** Get endpoint details */
  get: (id: string) => fetchWithAuth<DashboardEndpoint>(`/api/endpoints/${id}`),

  /** Update endpoint */
  update: (
    id: string,
    data: {
      name?: string
      base_url?: string
      api_key?: string
      health_check_interval_secs?: number
      inference_timeout_secs?: number
      notes?: string
    }
  ) =>
    fetchWithAuth<DashboardEndpoint>(`/api/endpoints/${id}`, {
      method: 'PUT',
      body: JSON.stringify(data),
    }),

  /** Delete endpoint */
  delete: (id: string) =>
    fetchWithAuth<void>(`/api/endpoints/${id}`, { method: 'DELETE' }),

  /** Test connection */
  test: (id: string) =>
    fetchWithAuth<{ success: boolean; message?: string; latency_ms?: number }>(
      `/api/endpoints/${id}/test`,
      { method: 'POST' }
    ),

  /** Sync models */
  sync: (id: string) =>
    fetchWithAuth<{ synced_models: number }>(`/api/endpoints/${id}/sync`, {
      method: 'POST',
    }),

  /** Get endpoint models */
  getModels: (id: string) =>
    fetchWithAuth<{
      endpoint_id: string
      models: Array<{
        model_id: string
        capabilities?: string[]
        max_tokens?: number | null
        last_checked?: string
      }>
    }>(`/api/endpoints/${id}/models`),

  /** SPEC-e8e9326e: Download model to endpoint (xLLM, Ollama, LM Studio) */
  downloadModel: (
    id: string,
    data: { model: string; filename?: string; hf_repo?: string; quantization?: string }
  ) =>
    fetchWithAuth<{ task_id: string }>(`/api/endpoints/${id}/download`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),

  /** Delete model from endpoint */
  deleteModel: (endpointId: string, model: string) =>
    fetchWithAuth<void>(`/api/endpoints/${endpointId}/models/delete`, {
      method: 'POST',
      body: JSON.stringify({ model }),
    }),

  /** SPEC-e8e9326e: Get download progress (xLLM only) */
  getDownloadProgress: (id: string) =>
    fetchWithAuth<{ tasks: DownloadTask[] }>(
      `/api/endpoints/${id}/download/progress`
    ),

  /** SPEC-e8e9326e: Get model metadata */
  getModelInfo: (id: string, model: string) =>
    fetchWithAuth<ModelMetadata>(
      `/api/endpoints/${id}/models/${encodeURIComponent(model)}/info`
    ),

  /** SPEC-8c32349f: Get today's request statistics for an endpoint */
  getTodayStats: (id: string) =>
    fetchWithAuth<EndpointTodayStats>(`/api/endpoints/${id}/today-stats`),

  /** SPEC-8c32349f: Get daily request statistics for an endpoint */
  getDailyStats: (id: string, days?: number) =>
    fetchWithAuth<EndpointDailyStatEntry[]>(`/api/endpoints/${id}/daily-stats`, {
      params: { days },
    }),

  /** SPEC-8c32349f: Get model-level request statistics */
  getModelStats: (id: string) =>
    fetchWithAuth<ModelStatEntry[]>(`/api/endpoints/${id}/model-stats`),

  /** SPEC-4bb5b55f: Get model-level TPS statistics */
  getModelTps: (id: string) =>
    fetchWithAuth<ModelTpsEntry[]>(`/api/endpoints/${id}/model-tps`),

  /** Proxy chat completions to endpoint (JWT authenticated) */
  chatCompletions: async (
    id: string,
    request: {
      model: string
      messages: Array<{ role: string; content: string | Array<unknown> }>
      stream?: boolean
      temperature?: number
      max_tokens?: number
    },
    onChunk?: (chunk: string) => void
  ) => {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
    }
    const csrfToken = getCsrfToken()
    if (csrfToken) {
      headers['X-CSRF-Token'] = csrfToken
    }

    const response = await fetch(`${API_BASE}/api/endpoints/${id}/chat/completions`, {
      method: 'POST',
      headers,
      body: JSON.stringify(request),
      credentials: 'include',
    })

    if (!response.ok) {
      throw await createApiErrorFromResponse(response)
    }

    if (request.stream && onChunk) {
      const reader = response.body?.getReader()
      if (!reader) throw new Error('No response body')

      const decoder = new TextDecoder()
      let buffer = ''

      while (true) {
        const { done, value } = await reader.read()
        if (done) break

        buffer += decoder.decode(value, { stream: true })
        const lines = buffer.split('\n')
        buffer = lines.pop() || ''

        for (const line of lines) {
          if (line.startsWith('data: ')) {
            const data = line.slice(6)
            if (data === '[DONE]') continue
            try {
              const parsed = JSON.parse(data)
              const content = extractAssistantDeltaText(parsed)
              if (content) {
                onChunk(content)
              }
            } catch {
              // Ignore parse errors
            }
          }
        }
      }

      return null
    }

    return response.json()
  },
}

export const benchmarkApi = {
  startTpsBenchmark: (request: TpsBenchmarkRequest) =>
    fetchWithAuth<TpsBenchmarkAccepted>('/api/benchmarks/tps', {
      method: 'POST',
      body: JSON.stringify(request),
    }),

  getTpsBenchmark: (runId: string) =>
    fetchWithAuth<TpsBenchmarkRun>(`/api/benchmarks/tps/${runId}`),
}
