// Models API

import { fetchWithAuth } from './client'

export type LifecycleStatus = 'pending' | 'caching' | 'registered' | 'error'

export interface DownloadProgress {
  percent: number
  bytes_downloaded?: number
  bytes_total?: number
  error?: string
}

// Azure OpenAI 形式の capabilities (boolean object)
export interface ModelCapabilities {
  chat_completion: boolean
  completion: boolean
  embeddings: boolean
  fine_tune: boolean
  inference: boolean
  text_to_speech: boolean
  speech_to_text: boolean
  image_generation: boolean
  image_input?: boolean
}

// /v1/models レスポンスの model object
export interface OpenAIModel {
  id: string
  object: 'model'
  created: number
  owned_by: string
  capabilities?: ModelCapabilities
  supported_apis?: string[]
  // Dashboard extended fields
  lifecycle_status: LifecycleStatus
  download_progress?: DownloadProgress | null
  ready: boolean
  repo?: string | null
  filename?: string | null
  size_bytes?: number
  required_memory_bytes?: number
  source?: string
  tags?: string[]
  description?: string
  chat_template?: string
  max_tokens?: number | null
  endpoint_ids?: string[]
  // US-029: Canonical 統一情報
  canonical_name?: string
  aliases?: string[]
  quantization?: string | null
}

// US-029: モデル一覧の表示モード
export type ModelsView = 'canonical' | 'detail'

// /api/models/discover-gguf response types
export interface GgufFileInfo {
  filename: string
  size_bytes: number
  quantization?: string | null
}

export interface GgufDiscoveryResult {
  repo: string
  provider: string
  trusted: boolean
  files: GgufFileInfo[]
}

export interface DiscoverGgufResponse {
  base_model: string
  gguf_alternatives: GgufDiscoveryResult[]
  cached: boolean
}

// /v1/models レスポンス
export interface OpenAIModelsResponse {
  object: 'list'
  data: OpenAIModel[]
}

// 後方互換用: RegisteredModelView は OpenAIModel にマッピング
export interface RegisteredModelView {
  owned_by?: string // "router" | "openai" | "google" | "anthropic"
  name: string
  source?: string
  description?: string
  status?: string
  lifecycle_status: LifecycleStatus
  download_progress?: DownloadProgress
  ready: boolean
  repo?: string
  filename?: string
  size_gb?: number
  required_memory_gb?: number
  tags: string[]
  capabilities?: ModelCapabilities
  supported_apis?: string[]
  chat_template?: string
  endpoint_ids: string[]
  // US-029: Canonical 統一情報
  canonical_name?: string
  aliases?: string[]
  quantization?: string | null
}

// OpenAIModel を RegisteredModelView に変換
function toRegisteredModelView(model: OpenAIModel): RegisteredModelView {
  const sizeGb =
    typeof model.size_bytes === 'number' ? model.size_bytes / (1024 * 1024 * 1024) : undefined
  const requiredGb =
    typeof model.required_memory_bytes === 'number'
      ? model.required_memory_bytes / (1024 * 1024 * 1024)
      : undefined
  return {
    name: model.id,
    owned_by: model.owned_by,
    lifecycle_status: model.lifecycle_status,
    download_progress: model.download_progress ?? undefined,
    ready: model.ready,
    source: model.source,
    description: model.description,
    repo: model.repo ?? undefined,
    filename: model.filename ?? undefined,
    size_gb: sizeGb,
    required_memory_gb: requiredGb,
    capabilities: model.capabilities,
    supported_apis: model.supported_apis,
    tags: model.tags ?? [],
    chat_template: model.chat_template,
    endpoint_ids: model.endpoint_ids ?? [],
    canonical_name: model.canonical_name,
    aliases: model.aliases,
    quantization: model.quantization,
  }
}

// NOTE: Model Hub機能は廃止されました
// モデル管理はエンドポイント側の責任（ゲートウェイ設計方針）
// ダウンロード状態は /v1/models の lifecycle_status で確認

export const modelsApi = {
  /**
   * OpenAI互換の登録済みモデル一覧を取得
   *
   * US-029: `view='canonical'`（既定）は論理モデル単位に集約、`view='detail'` は全 variant を列挙。
   */
  getRegistered: async (view: ModelsView = 'canonical'): Promise<RegisteredModelView[]> => {
    // /api/dashboard/models - JWT認証で取得
    const query = view === 'detail' ? '?view=detail' : '?view=canonical'
    const json = await fetchWithAuth<OpenAIModelsResponse>(`/api/dashboard/models${query}`)
    // Convert from OpenAI format to RegisteredModelView format
    return json.data.map(toRegisteredModelView)
  },
}
