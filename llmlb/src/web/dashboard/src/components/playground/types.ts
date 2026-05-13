import { ApiError, type ChatMessage } from '@/lib/api'

export const MAX_ATTACHMENT_BYTES = 4 * 1024 * 1024

export interface MessageAttachment {
  type: 'image' | 'audio'
  name: string
  data: string
  mimeType: string
}

export interface Message {
  role: 'user' | 'assistant' | 'system'
  content: string
  attachments?: MessageAttachment[]
}

export function getErrorMessage(error: number | ApiError): string {
  const status = typeof error === 'number' ? error : error.status

  if (error instanceof ApiError) {
    switch (error.errorType) {
      case 'model_loading':
        return (
          error.message ||
          'Model is still loading on the Ollama endpoint. Retry after the initial load finishes or increase the endpoint timeout.'
        )
      case 'timeout':
        return error.message || 'Request timed out.'
      case 'connection_error':
        return error.message || 'Failed to connect to the upstream endpoint.'
      case 'endpoint_request_error':
        return error.message || 'Failed to proxy the request to the upstream endpoint.'
      default:
        if (error.message && error.message !== `${error.status} ${error.statusText}`) {
          return error.message
        }
    }
  }

  switch (status) {
    case 401:
      return 'Session expired. Please sign in again.'
    case 403:
      return 'Access denied to this resource.'
    case 404:
      return 'API endpoint not found.'
    case 429:
      return 'Rate limited. Please reduce request volume.'
    case 503:
      return 'No available endpoints. Please check endpoint status.'
    case 504:
      return 'Request timed out.'
    default:
      return `Server error occurred (HTTP ${status})`
  }
}

export function transformMessage(msg: Message): ChatMessage {
  if (!msg.attachments || msg.attachments.length === 0) {
    return { role: msg.role, content: msg.content }
  }

  const content: Array<unknown> = []
  if (msg.content.trim()) {
    content.push({ type: 'text', text: msg.content })
  }

  msg.attachments.forEach((att) => {
    if (att.type === 'image') {
      content.push({ type: 'image_url', image_url: { url: att.data } })
      return
    }

    const audioData = att.data.startsWith('data:') ? att.data.split(',')[1] : att.data
    content.push({ type: 'input_audio', input_audio: { data: audioData, format: 'wav' } })
  })

  return { role: msg.role, content }
}

export function extractMediaFromContent(content: string) {
  const imageUrlRegex = /(data:image\/[^;]+;base64,[^\s"'<>]+|https?:\/\/[^\s"'<>]+\.(png|jpg|jpeg|gif|webp))/gi
  const audioUrlRegex = /(data:audio\/[^;]+;base64,[^\s"'<>]+|https?:\/\/[^\s"'<>]+\.(mp3|wav|ogg|m4a))/gi

  const imageMatches = content.match(imageUrlRegex) || []
  const audioMatches = content.match(audioUrlRegex) || []

  return { imageMatches, audioMatches }
}
