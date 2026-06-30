// Chat API (OpenAI compatible)

import { API_BASE, createApiErrorFromResponse, getCsrfToken } from './client'
import type { OpenAIModelsResponse } from './models'
import { splitAssistantDelta, type AssistantTextParts } from '../reasoning'

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant'
  content: string | Array<unknown>
}

export interface ChatCompletionRequest {
  model: string
  messages: ChatMessage[]
  stream?: boolean
  temperature?: number
  max_tokens?: number
  user?: string
}

// チャット補完 POST の共通実装。path で通常 Chat と Load Test 用エンドポイントを切り替える。
const postChatCompletion = async (
  path: string,
  request: ChatCompletionRequest,
  onDelta?: (delta: AssistantTextParts) => void,
  signal?: AbortSignal
) => {
  const headers: HeadersInit = {
    'Content-Type': 'application/json',
  }

  const csrfToken = getCsrfToken()
  if (csrfToken) {
    headers['X-CSRF-Token'] = csrfToken
  }

  const response = await fetch(`${API_BASE}${path}`, {
    method: 'POST',
    headers,
    body: JSON.stringify(request),
    credentials: 'include',
    signal,
  })

  if (!response.ok) {
    throw await createApiErrorFromResponse(response)
  }

    if (request.stream && onDelta) {
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
              const delta = splitAssistantDelta(parsed)
              if (delta.content || delta.reasoning) {
                onDelta(delta)
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
}

export const chatApi = {
  // 通常の Chat プレイグラウンド（全認証ユーザー）
  complete: (
    request: ChatCompletionRequest,
    onDelta?: (delta: AssistantTextParts) => void,
    signal?: AbortSignal
  ) => postChatCompletion('/api/dashboard/playground/chat/completions', request, onDelta, signal),

  // Load Test 用（admin ロール限定エンドポイント）。非ストリーミングのみ使用。
  completeLoadTest: (request: ChatCompletionRequest, signal?: AbortSignal) =>
    postChatCompletion('/api/dashboard/playground/load-test/chat/completions', request, undefined, signal),

  getModels: async (): Promise<OpenAIModelsResponse> => {
    const response = await fetch(`${API_BASE}/api/dashboard/playground/models`, {
      credentials: 'include',
    })
    if (!response.ok) {
      throw await createApiErrorFromResponse(response)
    }
    return response.json()
  },
}
