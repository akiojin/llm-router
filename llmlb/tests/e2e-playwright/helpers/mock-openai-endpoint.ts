import http, { type IncomingMessage, type ServerResponse } from 'node:http'
import { type AddressInfo } from 'node:net'

export interface MockOpenAIEndpointServer {
  baseUrl: string
  models: string[]
  close: () => Promise<void>
}

function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = []
    req.on('data', (chunk) => chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)))
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')))
    req.on('error', reject)
  })
}

function writeJson(res: ServerResponse, status: number, value: unknown): void {
  const body = JSON.stringify(value)
  res.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Content-Length': Buffer.byteLength(body),
  })
  res.end(body)
}

function sseWrite(res: ServerResponse, data: unknown): void {
  res.write(`data: ${typeof data === 'string' ? data : JSON.stringify(data)}\n\n`)
}

function extractLastUserText(messages: unknown): string {
  if (!Array.isArray(messages)) return ''
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const m = messages[i] as { role?: unknown; content?: unknown } | null
    if (!m || m.role !== 'user') continue
    if (typeof m.content === 'string') return m.content
    try {
      return JSON.stringify(m.content)
    } catch {
      return ''
    }
  }
  return ''
}

export async function startMockOpenAIEndpointServer(options?: {
  models?: string[]
  responseDelayMs?: number
  supportAudio?: boolean
  supportImages?: boolean
  supportResponses?: boolean
  endpointType?: 'xllm' | 'ollama' | 'vllm' | 'openai'
}): Promise<MockOpenAIEndpointServer> {
  const models = options?.models?.length ? options.models : ['mock-model-a', 'mock-model-b']
  const responseDelayMs = Math.max(0, options?.responseDelayMs ?? 0)
  const supportAudio = options?.supportAudio ?? false
  const supportImages = options?.supportImages ?? false
  const supportResponses = options?.supportResponses ?? false
  const endpointType = options?.endpointType

  // reqwest (llmlb) uses keep-alive connections; server.close() waits for them.
  // Track sockets and destroy them on shutdown so afterAll doesn't hang.
  const sockets = new Set<import('node:net').Socket>()

  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url || '/', 'http://127.0.0.1')

    // llmlb health checker prefers /api/health (Phase 1.4). Keep it fast so
    // E2E tests don't depend on /v1/models latency.
    if (req.method === 'GET' && url.pathname === '/api/health') {
      return writeJson(res, 200, {
        gpu: { device_count: 0 },
        load: { active_requests: 0 },
      })
    }

    // OpenAI-compatible models listing.
    if (req.method === 'GET' && url.pathname === '/v1/models') {
      const created = Math.floor(Date.now() / 1000)
      if (responseDelayMs > 0) {
        await new Promise((resolve) => setTimeout(resolve, responseDelayMs))
      }
      const ownedBy = endpointType === 'vllm' ? 'vllm' : 'mock'
      return writeJson(res, 200, {
        object: 'list',
        data: models.map((id) => ({
          id,
          object: 'model',
          created,
          owned_by: ownedBy,
        })),
      })
    }

    // OpenAI-compatible chat completions.
    if (req.method === 'POST' && url.pathname === '/v1/chat/completions') {
      let parsed: any
      try {
        const bodyText = await readBody(req)
        parsed = JSON.parse(bodyText || '{}')
      } catch {
        return writeJson(res, 400, {
          error: { message: 'invalid_json', type: 'invalid_request_error' },
        })
      }

      const model = typeof parsed?.model === 'string' ? parsed.model : 'unknown'
      const lastUser = extractLastUserText(parsed?.messages)
      const reply = `MOCK_OK model=${model} user=${lastUser}`

      // Opt-in: when the user message contains the `[[REASON]]` sentinel, emit a
      // separate `reasoning_content` channel (mimics reasoning models like
      // gemma via LM Studio). Existing specs don't send the sentinel, so they
      // are unaffected.
      const wantsReasoning = lastUser.includes('[[REASON]]')
      const reasoningText = 'MOCK_REASONING internal-thought-step'

      const stream = parsed?.stream === true
      if (responseDelayMs > 0) {
        await new Promise((resolve) => setTimeout(resolve, responseDelayMs))
      }
      if (!stream) {
        const created = Math.floor(Date.now() / 1000)
        return writeJson(res, 200, {
          id: `chatcmpl_mock_${created}`,
          object: 'chat.completion',
          created,
          model,
          choices: [
            {
              index: 0,
              message: {
                role: 'assistant',
                content: reply,
                ...(wantsReasoning ? { reasoning_content: reasoningText } : {}),
              },
              finish_reason: 'stop',
            },
          ],
        })
      }

      res.writeHead(200, {
        'Content-Type': 'text/event-stream; charset=utf-8',
        'Cache-Control': 'no-cache',
        Connection: 'keep-alive',
      })

      // Reasoning phase first (content empty, reasoning_content populated), then
      // the final answer in the content channel.
      if (wantsReasoning) {
        const rparts = [reasoningText.slice(0, 14), reasoningText.slice(14)]
        for (const rpart of rparts) {
          sseWrite(res, { choices: [{ delta: { reasoning_content: rpart } }] })
        }
      }

      // A few chunks so the UI streaming path is exercised.
      const parts = [reply.slice(0, 12), reply.slice(12, 24), reply.slice(24)]
      for (const part of parts) {
        sseWrite(res, { choices: [{ delta: { content: part } }] })
      }
      sseWrite(res, '[DONE]')
      res.end()
      return
    }

    // --- Audio API handlers (when supportAudio is enabled) ---
    if (supportAudio) {
      if (req.method === 'POST' && url.pathname === '/v1/audio/transcriptions') {
        // Consume multipart body (we don't parse it, just drain the stream)
        await readBody(req)
        if (responseDelayMs > 0) {
          await new Promise((resolve) => setTimeout(resolve, responseDelayMs))
        }
        return writeJson(res, 200, { text: `MOCK_TRANSCRIPTION model=${models[0]}` })
      }

      if (req.method === 'POST' && url.pathname === '/v1/audio/speech') {
        await readBody(req)
        if (responseDelayMs > 0) {
          await new Promise((resolve) => setTimeout(resolve, responseDelayMs))
        }
        const audioData = Buffer.from('MOCK_AUDIO_DATA')
        res.writeHead(200, {
          'Content-Type': 'audio/mpeg',
          'Content-Length': audioData.length,
        })
        res.end(audioData)
        return
      }
    }

    // --- Image API handlers (when supportImages is enabled) ---
    if (supportImages) {
      if (req.method === 'POST' && url.pathname === '/v1/images/generations') {
        await readBody(req)
        if (responseDelayMs > 0) {
          await new Promise((resolve) => setTimeout(resolve, responseDelayMs))
        }
        return writeJson(res, 200, {
          created: Math.floor(Date.now() / 1000),
          data: [{ url: 'https://mock.example.com/image.png' }],
        })
      }

      if (req.method === 'POST' && url.pathname === '/v1/images/edits') {
        await readBody(req)
        if (responseDelayMs > 0) {
          await new Promise((resolve) => setTimeout(resolve, responseDelayMs))
        }
        return writeJson(res, 200, {
          created: Math.floor(Date.now() / 1000),
          data: [{ url: 'https://mock.example.com/image.png' }],
        })
      }

      if (req.method === 'POST' && url.pathname === '/v1/images/variations') {
        await readBody(req)
        if (responseDelayMs > 0) {
          await new Promise((resolve) => setTimeout(resolve, responseDelayMs))
        }
        return writeJson(res, 200, {
          created: Math.floor(Date.now() / 1000),
          data: [{ url: 'https://mock.example.com/image.png' }],
        })
      }
    }

    // --- Responses API handler (when supportResponses is enabled) ---
    if (supportResponses) {
      if (req.method === 'POST' && url.pathname === '/v1/responses') {
        let parsed: any
        try {
          const bodyText = await readBody(req)
          parsed = JSON.parse(bodyText || '{}')
        } catch {
          return writeJson(res, 400, {
            error: { message: 'invalid_json', type: 'invalid_request_error' },
          })
        }
        const model = typeof parsed?.model === 'string' ? parsed.model : models[0]
        if (responseDelayMs > 0) {
          await new Promise((resolve) => setTimeout(resolve, responseDelayMs))
        }
        const created = Math.floor(Date.now() / 1000)
        return writeJson(res, 200, {
          id: `resp_mock_${created}`,
          object: 'response',
          created_at: created,
          output: [
            {
              type: 'message',
              role: 'assistant',
              content: [
                {
                  type: 'output_text',
                  text: `MOCK_RESPONSE model=${model}`,
                },
              ],
            },
          ],
        })
      }
    }

    // --- Endpoint type-specific handlers ---
    if (endpointType === 'xllm') {
      // Rust detection queries GET /api/system and expects `xllm_version` field
      if (req.method === 'GET' && url.pathname === '/api/system') {
        return writeJson(res, 200, {
          xllm_version: '0.1.0',
          server_name: 'mock-xllm',
          gpu: { device_count: 1 },
        })
      }

      if (req.method === 'POST' && url.pathname === '/v0/models/download') {
        await readBody(req)
        if (responseDelayMs > 0) {
          await new Promise((resolve) => setTimeout(resolve, responseDelayMs))
        }
        return writeJson(res, 200, { status: 'started', task_id: 'mock-task-1' })
      }
    }

    if (endpointType === 'ollama') {
      if (req.method === 'GET' && url.pathname === '/api/tags') {
        if (responseDelayMs > 0) {
          await new Promise((resolve) => setTimeout(resolve, responseDelayMs))
        }
        return writeJson(res, 200, {
          models: [{ name: 'mock-model', size: 1000000 }],
        })
      }
    }

    // Default: Not found.
    writeJson(res, 404, { error: { message: 'not_found' } })
  })

  server.on('connection', (socket) => {
    sockets.add(socket)
    socket.on('close', () => sockets.delete(socket))
  })

  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve)
  })

  const addr = server.address() as AddressInfo
  const baseUrl = `http://127.0.0.1:${addr.port}`

  return {
    baseUrl,
    models,
    close: () =>
      new Promise<void>((resolve, reject) => {
        for (const s of sockets) s.destroy()
        server.close((err) => {
          if (err) reject(err)
          else resolve()
        })
      }),
  }
}
