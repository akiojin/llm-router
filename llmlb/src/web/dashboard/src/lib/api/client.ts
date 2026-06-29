// Shared API client utilities

const API_BASE = ''

interface FetchOptions extends RequestInit {
  params?: Record<string, string | number | boolean | undefined>
}

interface ApiErrorOptions {
  message?: string
  errorType?: string
  errorCode?: string | number
  rawBody?: string
}

interface ApiErrorBodyShape {
  error?: {
    message?: string
    type?: string
    code?: string | number
  }
  message?: string
}

export class ApiError extends Error {
  public errorType?: string
  public errorCode?: string | number
  public rawBody?: string

  constructor(
    public status: number,
    public statusText: string,
    options: string | ApiErrorOptions = {}
  ) {
    const normalized: ApiErrorOptions =
      typeof options === 'string' ? { message: options } : options

    super(normalized.message || `${status} ${statusText}`)
    this.name = 'ApiError'
    this.errorType = normalized.errorType
    this.errorCode = normalized.errorCode
    this.rawBody = normalized.rawBody
  }
}

function parseApiErrorBody(bodyText: string): ApiErrorOptions {
  if (!bodyText) {
    return {}
  }

  try {
    const parsed = JSON.parse(bodyText) as ApiErrorBodyShape
    if (parsed?.error) {
      return {
        message: parsed.error.message,
        errorType: parsed.error.type,
        errorCode: parsed.error.code,
        rawBody: bodyText,
      }
    }
    if (typeof parsed?.message === 'string') {
      return {
        message: parsed.message,
        rawBody: bodyText,
      }
    }
  } catch {
    // Plain-text error body.
  }

  return {
    message: bodyText,
    rawBody: bodyText,
  }
}

export async function createApiErrorFromResponse(response: Response): Promise<ApiError> {
  const bodyText = await response.text()
  return new ApiError(response.status, response.statusText, parseApiErrorBody(bodyText))
}

// 401 を複数の並行リクエストが同時に受け取っても、ログインへのリダイレクトは一度だけ
// 行う（多重 navigation によるちらつきを防ぐ）。
let redirectingToLogin = false

export async function fetchWithAuth<T>(
  endpoint: string,
  options: FetchOptions = {}
): Promise<T> {
  const { params, ...fetchOptions } = options

  let url = `${API_BASE}${endpoint}`
  if (params) {
    const searchParams = new URLSearchParams()
    Object.entries(params).forEach(([key, value]) => {
      if (value !== undefined) {
        searchParams.append(key, String(value))
      }
    })
    const queryString = searchParams.toString()
    if (queryString) {
      url += `?${queryString}`
    }
  }

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  }

  const method = (fetchOptions.method || 'GET').toUpperCase()
  if (method !== 'GET' && method !== 'HEAD') {
    const csrfToken = getCsrfToken()
    if (csrfToken) {
      headers['X-CSRF-Token'] = csrfToken
    }
  }

  const response = await fetch(url, {
    ...fetchOptions,
    headers,
    credentials: 'include',
  })

  if (response.status === 401) {
    if (!redirectingToLogin) {
      redirectingToLogin = true
      window.location.href = '/dashboard/login.html'
    }
    throw new ApiError(401, 'Unauthorized')
  }

  if (!response.ok) {
    throw await createApiErrorFromResponse(response)
  }

  // Handle empty responses
  const text = await response.text()
  if (!text) {
    return {} as T
  }

  return JSON.parse(text)
}

export function getCsrfToken(): string | null {
  const match = document.cookie.match(/(?:^|; )llmlb_csrf=([^;]*)/)
  return match ? decodeURIComponent(match[1]) : null
}

export { API_BASE }
