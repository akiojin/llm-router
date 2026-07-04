import type { DashboardEndpoint, EndpointType } from '@/lib/api'

/**
 * エンドポイントの status / type を UI 表示（バッジ variant・ラベル・色）へ変換する関数群。
 *
 * arch-review [L20]: これらの変換が EndpointTable / EndpointDetailModal /
 * EndpointPlayground に逐語コピーされていたため、単一の真実源として集約した。
 * 各引数は `undefined` を許容し、デフォルト分岐は元の 3 実装の挙動を包含する。
 */

export function getStatusBadgeVariant(
  status: DashboardEndpoint['status'] | undefined
): 'online' | 'pending' | 'offline' | 'destructive' | 'outline' {
  switch (status) {
    case 'online':
      return 'online'
    case 'pending':
      return 'pending'
    case 'offline':
      return 'offline'
    case 'error':
      return 'destructive'
    default:
      return 'outline'
  }
}

export function getStatusLabel(status: DashboardEndpoint['status'] | undefined): string {
  switch (status) {
    case 'online':
      return 'Online'
    case 'pending':
      return 'Pending'
    case 'offline':
      return 'Offline'
    case 'error':
      return 'Error'
    default:
      return status ?? 'Unknown'
  }
}

/** SPEC-e8e9326e: status に応じた表示色（インジケータ用）。 */
export function getStatusIndicatorColor(
  status: DashboardEndpoint['status'] | undefined
): string {
  switch (status) {
    case 'online':
      return 'text-success'
    case 'pending':
      return 'text-warning'
    case 'offline':
      return 'text-destructive/70'
    case 'error':
      return 'text-destructive'
    default:
      return 'text-muted-foreground'
  }
}

/** SPEC-e8e9326e: Get display label for endpoint type */
export function getTypeLabel(type: EndpointType | undefined): string {
  switch (type) {
    case 'xllm':
      return 'xLLM'
    case 'ollama':
      return 'Ollama'
    case 'vllm':
      return 'vLLM'
    case 'lm_studio':
      return 'LM Studio'
    case 'llamacpp':
      return 'llama.cpp'
    case 'openai_compatible':
      return 'OpenAI Compatible'
    case 'unknown':
      return 'Unknown'
    default:
      return type ?? '-'
  }
}

/** SPEC-e8e9326e: Get badge variant for endpoint type */
export function getTypeBadgeVariant(
  type: EndpointType | undefined
): 'default' | 'secondary' | 'outline' {
  switch (type) {
    case 'xllm':
      return 'default'
    case 'ollama':
    case 'vllm':
    case 'lm_studio':
    case 'llamacpp':
      return 'secondary'
    default:
      return 'outline'
  }
}
