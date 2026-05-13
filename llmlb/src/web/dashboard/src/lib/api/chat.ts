// Chat API (OpenAI compatible)

import { API_BASE, createApiErrorFromResponse, getCsrfToken } from './client'
import type { OpenAIModelsResponse } from './models'

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant'
  content: string | Array<unknown>
}

export interface ChatSession {
  id: string
  title: string
  messages: ChatMessage[]
  model?: string
  created_at: string
  updated_at: string
}

export interface ChatCompletionRequest {
  model: string
  messages: ChatMessage[]
  stream?: boolean
  temperature?: number
  max_tokens?: number
  user?: string
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

export const chatApi = {
  complete: async (
    request: ChatCompletionRequest,
    onChunk?: (chunk: string) => void,
    signal?: AbortSignal
  ) => {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
    }

    const csrfToken = getCsrfToken()
    if (csrfToken) {
      headers['X-CSRF-Token'] = csrfToken
    }

    const response = await fetch(`${API_BASE}/api/dashboard/playground/chat/completions`, {
      method: 'POST',
      headers,
      body: JSON.stringify(request),
      credentials: 'include',
      signal,
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

  getModels: async (): Promise<OpenAIModelsResponse> => {
    const response = await fetch(`${API_BASE}/api/dashboard/playground/models`, {
      credentials: 'include',
    })
    if (!response.ok) {
      throw await createApiErrorFromResponse(response)
    }
    return response.json()
  },

  // Session management (local storage based for now)
  getSessions: async (): Promise<ChatSession[]> => {
    const sessions = localStorage.getItem('chat_sessions')
    return sessions ? JSON.parse(sessions) : []
  },

  saveSessions: async (sessions: ChatSession[]): Promise<void> => {
    localStorage.setItem('chat_sessions', JSON.stringify(sessions))
  },
}
