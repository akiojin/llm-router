import { useState, useEffect, useRef, useCallback, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  chatApi,
  dashboardApi,
  ApiError,
  type ChatMessage,
  type OpenAIModel,
  type OpenAIModelsResponse,
  type RequestResponseRecord,
} from '@/lib/api'
import { cn, isAbortError } from '@/lib/utils'
import { toast } from '@/hooks/use-toast'
import { usePlayground } from '@/hooks/usePlayground'
import { splitAssistantMessage } from '@/lib/reasoning'
import {
  PlaygroundBase,
  getErrorMessage,
  transformMessage,
  MAX_INPUT_CHARS,
  type Message,
} from '@/components/playground'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Network,
  MessageSquare,
  Send,
  AlertCircle,
  RefreshCw,
  Gauge,
  Play,
  Square,
  Loader2,
  Code,
  ShieldCheck,
} from 'lucide-react'

const DEFAULT_LOAD_TEST_SETTINGS = {
  totalRequests: 200,
  concurrency: 10,
  intervalMs: 0,
}

type PlaygroundMode = 'chat' | 'load_test'

interface DistributionRow {
  endpoint: string
  count: number
  success: number
  error: number
  averageDurationMs: number
}

interface LoadTestProgress {
  total: number
  completed: number
  success: number
  error: number
}

interface DistributionSummary {
  runId: string
  expectedCount: number
  matchedCount: number
  rows: DistributionRow[]
  updatedAt: string
}

interface LoadBalancerPlaygroundProps {
  onBack: () => void
  initialModel?: string
}

function generateRunId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return `${Date.now()}-${Math.random().toString(16).slice(2)}`
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function toNumberValue(value: string, fallback: number): number {
  const parsed = Number.parseInt(value, 10)
  return Number.isFinite(parsed) ? parsed : fallback
}

function extractUserTag(requestBody: unknown): string | null {
  if (!requestBody || typeof requestBody !== 'object') {
    return null
  }
  const record = requestBody as Record<string, unknown>
  return typeof record.user === 'string' ? record.user : null
}

function buildDistributionRows(records: RequestResponseRecord[]): DistributionRow[] {
  const map = new Map<
    string,
    { count: number; success: number; error: number; totalDuration: number }
  >()

  records.forEach((record) => {
    const endpoint = record.endpoint_name || record.endpoint_id || 'unknown'
    const current = map.get(endpoint) ?? { count: 0, success: 0, error: 0, totalDuration: 0 }
    current.count += 1
    current.totalDuration += record.duration_ms ?? 0
    if (record.status.type === 'success') {
      current.success += 1
    } else {
      current.error += 1
    }
    map.set(endpoint, current)
  })

  return Array.from(map.entries())
    .map(([endpoint, data]) => ({
      endpoint,
      count: data.count,
      success: data.success,
      error: data.error,
      averageDurationMs: data.count > 0 ? Math.round(data.totalDuration / data.count) : 0,
    }))
    .sort((a, b) => b.count - a.count)
}

export default function LoadBalancerPlayground({ onBack, initialModel }: LoadBalancerPlaygroundProps) {
  const [mode, setMode] = useState<PlaygroundMode>('chat')

  const [loadTestTotalRequests, setLoadTestTotalRequests] = useState(
    String(DEFAULT_LOAD_TEST_SETTINGS.totalRequests)
  )
  const [loadTestConcurrency, setLoadTestConcurrency] = useState(
    String(DEFAULT_LOAD_TEST_SETTINGS.concurrency)
  )
  const [loadTestIntervalMs, setLoadTestIntervalMs] = useState(
    String(DEFAULT_LOAD_TEST_SETTINGS.intervalMs)
  )
  const [isLoadTesting, setIsLoadTesting] = useState(false)
  const [isStoppingLoadTest, setIsStoppingLoadTest] = useState(false)
  const [loadTestProgress, setLoadTestProgress] = useState<LoadTestProgress | null>(null)
  const [distributionSummary, setDistributionSummary] = useState<DistributionSummary | null>(null)
  const [distributionError, setDistributionError] = useState<string | null>(null)
  const [isRefreshingDistribution, setIsRefreshingDistribution] = useState(false)

  const isMountedRef = useRef(true)
  const loadTestStopRef = useRef(false)
  const loadTestAbortControllersRef = useRef<Set<AbortController>>(new Set())
  const appliedInitialModelRef = useRef<string | null>(null)

  const pg = usePlayground({
    onResetExtra: () => {
      setDistributionSummary(null)
      setDistributionError(null)
    },
  })

  const {
    data: modelsData,
    isLoading: isLoadingModels,
    error: modelsError,
    refetch: refetchModels,
  } = useQuery<OpenAIModelsResponse>({
    queryKey: ['lb-playground-models'],
    queryFn: () => chatApi.getModels(),
    retry: false,
    staleTime: 5000,
  })

  useEffect(() => {
    if (!modelsError) return
    let description = 'Failed to fetch model list'
    if (modelsError instanceof ApiError) {
      description = getErrorMessage(modelsError)
    } else if (modelsError instanceof Error) {
      description = modelsError.message
    }
    toast({ title: 'Error', description, variant: 'destructive' })
  }, [modelsError])

  useEffect(() => {
    if (!initialModel) {
      appliedInitialModelRef.current = null
      return
    }
    if (!modelsData?.data) return
    const exists = modelsData.data.some((m) => m.id === initialModel)
    if (!exists) return
    if (appliedInitialModelRef.current === initialModel) return
    pg.setSelectedModel(initialModel)
    appliedInitialModelRef.current = initialModel
  }, [initialModel, modelsData?.data, pg.setSelectedModel])

  useEffect(() => {
    const models = Array.isArray(modelsData?.data) ? modelsData.data : []
    const hasInitialModel = initialModel ? models.some((model) => model.id === initialModel) : false
    if (hasInitialModel) return
    if (models.length === 0) {
      if (pg.selectedModel) pg.setSelectedModel('')
      return
    }
    const hasSelectedModel = models.some((model) => model.id === pg.selectedModel)
    if (!pg.selectedModel || !hasSelectedModel) {
      pg.setSelectedModel(models[0].id)
    }
  }, [modelsData, pg.selectedModel, initialModel, pg.setSelectedModel])

  const selectedModelMaxTokens = modelsData?.data?.find(m => m.id === pg.selectedModel)?.max_tokens
  const effectiveMaxTokens = pg.useMaxContext && selectedModelMaxTokens != null ? selectedModelMaxTokens : pg.maxTokens

  useEffect(() => {
    return () => {
      isMountedRef.current = false
      pg.abortControllerRef.current?.abort()
      loadTestStopRef.current = true
      loadTestAbortControllersRef.current.forEach((controller) => controller.abort())
      loadTestAbortControllersRef.current.clear()
    }
  }, [])

  const fetchDistributionForRun = useCallback(async (runId: string, expectedCount: number) => {
    setIsRefreshingDistribution(true)
    setDistributionError(null)

    try {
      const matched: RequestResponseRecord[] = []
      const pageSize = 100
      const maxPages = 30

      for (let page = 0; page < maxPages; page += 1) {
        const response = await dashboardApi.getRequestResponses({
          limit: pageSize,
          offset: page * pageSize,
        })

        if (response.records.length === 0) break

        for (const record of response.records) {
          const tag = extractUserTag(record.request_body)
          if (tag?.startsWith(`lbpg:${runId}:`)) {
            matched.push(record)
          }
        }

        if (matched.length >= expectedCount || (page + 1) * pageSize >= response.total_count) break
      }

      setDistributionSummary({
        runId,
        expectedCount,
        matchedCount: matched.length,
        rows: buildDistributionRows(matched),
        updatedAt: new Date().toISOString(),
      })
    } catch (error) {
      setDistributionError(error instanceof Error ? error.message : 'Failed to load distribution')
    } finally {
      setIsRefreshingDistribution(false)
    }
  }, [])

  const sendMessage = async () => {
    if ((!pg.input.trim() && pg.attachments.length === 0) || !pg.selectedModel || pg.isStreaming) return

    if (pg.input.length > MAX_INPUT_CHARS) {
      toast({
        title: 'Message too long',
        description: `Keep your message under ${MAX_INPUT_CHARS.toLocaleString()} characters.`,
        variant: 'destructive',
      })
      return
    }

    const runId = generateRunId()
    const runTag = `lbpg:${runId}:1`

    const userMessage: Message = {
      role: 'user',
      content: pg.input.trim(),
      attachments: pg.attachments.length > 0 ? pg.attachments : undefined,
    }
    const newMessages = [...pg.messages, userMessage]

    pg.setMessages(newMessages)
    pg.setInput('')
    pg.setAttachments([])
    pg.setIsStreaming(true)

    pg.abortControllerRef.current = new AbortController()

    try {
      const requestMessages = pg.systemPrompt
        ? ([{ role: 'system', content: pg.systemPrompt } as ChatMessage].concat(
            newMessages.map(transformMessage)
          ))
        : newMessages.map(transformMessage)

      if (pg.streamEnabled) {
        let assistantContent = ''
        let assistantReasoning = ''
        pg.setMessages((prev) => [...prev, { role: 'assistant', content: '' }])

        await chatApi.complete(
          {
            model: pg.selectedModel,
            messages: requestMessages,
            stream: true,
            temperature: pg.temperature,
            max_tokens: effectiveMaxTokens,
            user: runTag,
          },
          (delta) => {
            assistantContent += delta.content
            assistantReasoning += delta.reasoning
            if (!isMountedRef.current) return
            pg.setMessages((prev) => {
              const updated = [...prev]
              updated[updated.length - 1] = {
                role: 'assistant',
                content: assistantContent,
                reasoning: assistantReasoning || undefined,
              }
              return updated
            })
          },
          pg.abortControllerRef.current.signal
        )
      } else {
        const response = await chatApi.complete(
          {
            model: pg.selectedModel,
            messages: requestMessages,
            stream: false,
            temperature: pg.temperature,
            max_tokens: effectiveMaxTokens,
            user: runTag,
          },
          undefined,
          pg.abortControllerRef.current.signal
        )

        const { content, reasoning } = splitAssistantMessage(response)
        pg.setMessages((prev) => [...prev, {
          role: 'assistant',
          content,
          reasoning: reasoning || undefined,
        }])
      }

      await fetchDistributionForRun(runId, 1)
    } catch (error) {
      if (!isAbortError(error)) {
        const description =
          error instanceof ApiError
            ? getErrorMessage(error)
            : error instanceof Error
              ? error.message
              : 'Unknown error'

        toast({ title: 'Failed to send message', description, variant: 'destructive' })
        // Drop only the optimistic empty assistant placeholder (streaming),
        // keep the user's message, and restore the input so it can be resent.
        pg.setMessages((prev) => {
          const last = prev[prev.length - 1]
          if (last && last.role === 'assistant' && last.content === '') {
            return prev.slice(0, -1)
          }
          return prev
        })
        pg.setInput(userMessage.content)
      }
    } finally {
      if (isMountedRef.current) {
        pg.setIsStreaming(false)
      }
      pg.abortControllerRef.current = null
      if (isMountedRef.current) {
        pg.inputRef.current?.focus()
      }
    }
  }

  const startLoadTest = async () => {
    if (!pg.selectedModel || isLoadTesting) return

    const totalRequests = Math.max(1, toNumberValue(loadTestTotalRequests, DEFAULT_LOAD_TEST_SETTINGS.totalRequests))
    const concurrency = Math.max(1, toNumberValue(loadTestConcurrency, DEFAULT_LOAD_TEST_SETTINGS.concurrency))
    const intervalMs = Math.max(0, toNumberValue(loadTestIntervalMs, DEFAULT_LOAD_TEST_SETTINGS.intervalMs))

    const prompt = pg.input.trim() || 'Load balancing validation request'
    const runId = generateRunId()

    setIsLoadTesting(true)
    setIsStoppingLoadTest(false)
    setLoadTestProgress({ total: totalRequests, completed: 0, success: 0, error: 0 })
    setDistributionSummary(null)
    setDistributionError(null)

    loadTestStopRef.current = false

    let nextIndex = 0
    let completed = 0
    let success = 0
    let error = 0

    const worker = async () => {
      while (true) {
        if (loadTestStopRef.current) return

        const index = nextIndex
        if (index >= totalRequests) return
        nextIndex += 1

        const runTag = `lbpg:${runId}:${index + 1}`
        const requestMessages: ChatMessage[] = pg.systemPrompt
          ? [
              { role: 'system', content: pg.systemPrompt },
              { role: 'user', content: prompt },
            ]
          : [{ role: 'user', content: prompt }]
        const requestAbortController = new AbortController()
        loadTestAbortControllersRef.current.add(requestAbortController)

        try {
          await chatApi.complete(
            {
              model: pg.selectedModel,
              messages: requestMessages,
              stream: false,
              temperature: pg.temperature,
              max_tokens: effectiveMaxTokens,
              user: runTag,
            },
            undefined,
            requestAbortController.signal
          )
          success += 1
        } catch (requestError) {
          if (!isAbortError(requestError)) {
            error += 1
          }
        } finally {
          loadTestAbortControllersRef.current.delete(requestAbortController)
          completed += 1
          if (isMountedRef.current) {
            setLoadTestProgress({ total: totalRequests, completed, success, error })
          }
        }

        if (intervalMs > 0) {
          await delay(intervalMs)
        }
      }
    }

    try {
      const workers = Array.from({ length: concurrency }, () => worker())
      await Promise.all(workers)

      if (!isMountedRef.current) return

      const finalCount = completed
      await fetchDistributionForRun(runId, finalCount)

      pg.setMessages((prev) => [
        ...prev,
        {
          role: 'assistant',
          content: `Load test finished. requests=${finalCount}, success=${success}, error=${error}`,
        },
      ])
    } finally {
      loadTestAbortControllersRef.current.clear()
      if (isMountedRef.current) {
        setIsLoadTesting(false)
        setIsStoppingLoadTest(false)
      }
      loadTestStopRef.current = false
    }
  }

  const stopLoadTest = () => {
    if (!isLoadTesting) return
    loadTestStopRef.current = true
    loadTestAbortControllersRef.current.forEach((controller) => controller.abort())
    loadTestAbortControllersRef.current.clear()
    setIsStoppingLoadTest(true)
  }

  const generateCurl = () => {
    const requestMessages = pg.systemPrompt
      ? ([{ role: 'system', content: pg.systemPrompt } as ChatMessage].concat(pg.messages.map(transformMessage)))
      : pg.messages.map(transformMessage)

    return `curl -X POST '/api/dashboard/playground/chat/completions' \\
  -H 'Content-Type: application/json' \\
  -H 'X-CSRF-Token: <llmlb_csrf cookie value>' \\
  -b 'llmlb_jwt=<dashboard cookie>; llmlb_csrf=<csrf cookie>' \\
  -d '${JSON.stringify(
    {
      model: pg.selectedModel,
      messages: requestMessages,
      stream: pg.streamEnabled,
      temperature: pg.temperature,
      max_tokens: effectiveMaxTokens,
    },
    null,
    2
  )}'`
  }

  const modelOptions = useMemo<OpenAIModel[]>(
    () => (Array.isArray(modelsData?.data) ? modelsData.data : []),
    [modelsData]
  )

  const canSendChat = (pg.input.trim().length > 0 || pg.attachments.length > 0) && !!pg.selectedModel
  const progressRate =
    loadTestProgress && loadTestProgress.total > 0
      ? Math.round((loadTestProgress.completed / loadTestProgress.total) * 100)
      : 0

  const loadTestSettingsPanel = mode === 'load_test' ? (
    <div className="rounded-lg border bg-muted/20 p-3 space-y-3" id="lb-load-test-settings">
      <div className="flex items-center justify-between">
        <p className="text-sm font-medium">Load test settings</p>
        <Badge variant="secondary">Default: High</Badge>
      </div>
      <div className="grid grid-cols-3 gap-3">
        <div className="space-y-1">
          <Label htmlFor="lb-total-requests" className="text-xs">Requests</Label>
          <Input
            id="lb-total-requests"
            type="number"
            min={1}
            value={loadTestTotalRequests}
            onChange={(e) => setLoadTestTotalRequests(e.target.value)}
            disabled={isLoadTesting}
          />
        </div>
        <div className="space-y-1">
          <Label htmlFor="lb-concurrency" className="text-xs">Concurrency</Label>
          <Input
            id="lb-concurrency"
            type="number"
            min={1}
            value={loadTestConcurrency}
            onChange={(e) => setLoadTestConcurrency(e.target.value)}
            disabled={isLoadTesting}
          />
        </div>
        <div className="space-y-1">
          <Label htmlFor="lb-interval-ms" className="text-xs">Interval (ms)</Label>
          <Input
            id="lb-interval-ms"
            type="number"
            min={0}
            value={loadTestIntervalMs}
            onChange={(e) => setLoadTestIntervalMs(e.target.value)}
            disabled={isLoadTesting}
          />
        </div>
      </div>

      {loadTestProgress && (
        <div className="space-y-2" id="lb-load-test-progress">
          <div className="flex items-center justify-between text-xs text-muted-foreground">
            <span>{`${loadTestProgress.completed}/${loadTestProgress.total} completed`}</span>
            <span>{`success=${loadTestProgress.success}, error=${loadTestProgress.error}`}</span>
          </div>
          <div className="h-2 rounded-full bg-muted">
            <div
              className="h-2 rounded-full bg-primary transition-all"
              style={{ width: `${progressRate}%` }}
            />
          </div>
        </div>
      )}
    </div>
  ) : null

  const sendButtonElement = mode === 'chat' ? (
    pg.isStreaming ? (
      <Button variant="destructive" onClick={pg.stopGeneration} className="shrink-0" id="lb-stop-chat">
        <Loader2 className="mr-2 h-4 w-4 animate-spin" />
        Stop
      </Button>
    ) : (
      <Button
        onClick={() => void sendMessage()}
        disabled={!canSendChat}
        className="shrink-0"
        id="lb-send-chat"
      >
        <Send className="mr-2 h-4 w-4" />
        Send
      </Button>
    )
  ) : isLoadTesting ? (
    <Button
      variant="destructive"
      onClick={stopLoadTest}
      className="shrink-0"
      id="lb-stop-load-test"
    >
      <Square className="mr-2 h-4 w-4" />
      {isStoppingLoadTest ? 'Stopping...' : 'Stop'}
    </Button>
  ) : (
    <Button
      onClick={() => void startLoadTest()}
      disabled={!pg.selectedModel}
      className="shrink-0"
      id="lb-start-load-test"
    >
      <Play className="mr-2 h-4 w-4" />
      Start Load Test
    </Button>
  )

  const distributionPanel = (distributionSummary || distributionError || isRefreshingDistribution) ? (
    <div className="border-b bg-muted/20 px-4 py-3" id="lb-distribution-panel">
      <div className="mb-2 flex items-center justify-between">
        <div className="flex items-center gap-2 text-sm font-medium">
          <Network className="h-4 w-4 text-primary" />
          Request Distribution
        </div>
        {distributionSummary && (
          <Button
            variant="outline"
            size="sm"
            onClick={() =>
              fetchDistributionForRun(distributionSummary.runId, distributionSummary.expectedCount)
            }
            disabled={isRefreshingDistribution}
          >
            <RefreshCw className={cn('mr-2 h-3.5 w-3.5', isRefreshingDistribution && 'animate-spin')} />
            Refresh
          </Button>
        )}
      </div>

      {distributionError && (
        <div className="flex items-center gap-2 text-sm text-destructive">
          <AlertCircle className="h-4 w-4" />
          {distributionError}
        </div>
      )}

      {distributionSummary && (
        <div className="space-y-2">
          <p className="text-xs text-muted-foreground" id="lb-distribution-summary">
            {`Run: ${distributionSummary.runId.slice(0, 8)} | Matched: ${distributionSummary.matchedCount}/${distributionSummary.expectedCount} | Updated: ${new Date(distributionSummary.updatedAt).toLocaleTimeString()}`}
          </p>
          <div className="rounded-md border bg-background">
            <div className="grid grid-cols-[1fr_80px_80px_80px_120px] border-b px-3 py-2 text-xs font-medium text-muted-foreground">
              <span>Endpoint</span>
              <span className="text-right">Count</span>
              <span className="text-right">Success</span>
              <span className="text-right">Error</span>
              <span className="text-right">Avg ms</span>
            </div>
            {distributionSummary.rows.length === 0 ? (
              <p className="px-3 py-3 text-xs text-muted-foreground">
                No records found for this run yet.
              </p>
            ) : (
              distributionSummary.rows.map((row) => (
                <div
                  key={row.endpoint}
                  className="grid grid-cols-[1fr_80px_80px_80px_120px] px-3 py-2 text-xs"
                  data-testid="lb-distribution-row"
                >
                  <span className="truncate" title={row.endpoint}>{row.endpoint}</span>
                  <span className="text-right">{row.count}</span>
                  <span className="text-right text-green-600">{row.success}</span>
                  <span className="text-right text-destructive">{row.error}</span>
                  <span className="text-right">{row.averageDurationMs}</span>
                </div>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  ) : null

  return (
    <PlaygroundBase
      onBack={onBack}
      sidebarWidth="w-72"
      sidebarId="lb-playground-sidebar"
      sidebarHeader={
        <div className="flex items-center gap-2">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-primary/10">
            <Network className="h-4 w-4 text-primary" />
          </div>
          <div>
            <h1 className="font-semibold text-sm">Load Balancer</h1>
            <p className="text-xs text-muted-foreground">Playground</p>
          </div>
        </div>
      }
      sidebarInfo={
        <div className="p-3 space-y-2">
          <div
            id="lb-session-auth"
            className="text-xs text-muted-foreground flex items-center gap-2"
          >
            <ShieldCheck className="h-3 w-3" />
            <span className="font-medium">Dashboard session:</span>
            <Badge variant="default">Active</Badge>
          </div>
          <div className="text-xs text-muted-foreground">
            <span className="font-medium">Models:</span> {modelOptions.length}
          </div>
          <div className="text-xs text-muted-foreground">
            <span className="font-medium">Mode:</span>{' '}
            {mode === 'chat' ? 'Interactive chat' : 'Load test'}
          </div>
        </div>
      }
      sidebarExtra={
        <>
          <Button
            variant="outline"
            className="w-full justify-start"
            onClick={() => refetchModels()}
            disabled={isLoadingModels}
          >
            <RefreshCw className={cn('mr-2 h-4 w-4', isLoadingModels && 'animate-spin')} />
            Refresh Models
          </Button>
        </>
      }
      headerContent={
        <div className="flex items-center gap-3">
          <div className="inline-flex rounded-md border border-border p-1">
            <Button
              size="sm"
              variant={mode === 'chat' ? 'default' : 'outline'}
              onClick={() => setMode('chat')}
              className="h-7"
              id="lb-mode-chat"
            >
              <MessageSquare className="mr-1.5 h-3.5 w-3.5" />
              Chat
            </Button>
            <Button
              size="sm"
              variant={mode === 'load_test' ? 'default' : 'outline'}
              onClick={() => setMode('load_test')}
              className="h-7"
              id="lb-mode-load-test"
            >
              <Gauge className="mr-1.5 h-3.5 w-3.5" />
              Load Test
            </Button>
          </div>

          <Select value={pg.selectedModel} onValueChange={pg.setSelectedModel}>
            <SelectTrigger className="w-80" id="lb-model-select">
              <SelectValue placeholder="Select a model" />
            </SelectTrigger>
            <SelectContent>
              {isLoadingModels ? (
                <SelectItem value="__loading__" disabled>Loading models...</SelectItem>
              ) : modelOptions.length === 0 ? (
                <SelectItem value="__empty__" disabled>No models available</SelectItem>
              ) : (
                modelOptions.map((model) => (
                  <SelectItem key={model.id} value={model.id}>{model.id}</SelectItem>
                ))
              )}
            </SelectContent>
          </Select>

          {mode === 'chat' && pg.streamEnabled && (
            <Badge variant="secondary" className="text-xs">Streaming</Badge>
          )}
        </div>
      }
      headerRight={
        <Button variant="outline" size="sm" onClick={() => pg.setCurlOpen(true)}>
          <Code className="mr-2 h-4 w-4" />
          cURL
        </Button>
      }
      aboveMessages={distributionPanel}
      messages={pg.messages}
      messagesEndRef={pg.messagesEndRef}
      emptyTitle="Start a load balancer conversation"
      emptyDescription="Choose a model and send requests through the load balancer."
      input={pg.input}
      onInputChange={pg.setInput}
      onSend={() => {
        if (mode === 'chat') void sendMessage()
      }}
      onStop={pg.stopGeneration}
      isStreaming={pg.isStreaming}
      inputDisabled={pg.isStreaming || isLoadTesting}
      attachments={pg.attachments}
      onRemoveAttachment={pg.removeAttachment}
      onPaste={(e) => {
        if (mode !== 'chat') return
        pg.handlePaste(e)
      }}
      inputRef={pg.inputRef}
      imageInputRef={pg.imageInputRef}
      audioInputRef={pg.audioInputRef}
      onImageAttach={(file) => void pg.handleFileAttachment(file, 'image')}
      onAudioAttach={(file) => void pg.handleFileAttachment(file, 'audio')}
      sendDisabled={!canSendChat}
      inputPlaceholder={
        mode === 'chat'
          ? 'Type a message or attach files...'
          : 'Prompt used for each load test request...'
      }
      showAttachButtons={mode === 'chat'}
      formExtraContent={loadTestSettingsPanel}
      sendButton={sendButtonElement}
      inputId="lb-chat-input"
      settingsOpen={pg.settingsOpen}
      onSettingsOpenChange={pg.setSettingsOpen}
      systemPrompt={pg.systemPrompt}
      onSystemPromptChange={pg.setSystemPrompt}
      streamEnabled={pg.streamEnabled}
      onStreamEnabledChange={pg.setStreamEnabled}
      streamDisabled={mode === 'load_test'}
      temperature={pg.temperature}
      onTemperatureChange={pg.setTemperature}
      maxTokens={pg.maxTokens}
      onMaxTokensChange={pg.setMaxTokens}
      useMaxContext={pg.useMaxContext}
      onUseMaxContextChange={pg.setUseMaxContext}
      selectedModelMaxTokens={selectedModelMaxTokens}
      maxContextCheckboxId="lb-use-max-context"
      curlOpen={pg.curlOpen}
      onCurlOpenChange={pg.setCurlOpen}
      curlCommand={generateCurl()}
      copied={pg.copied}
      onCopyCurl={pg.handleCopyCurl}
      curlDescription="Copy this command to replay the current request through the load balancer."
      resetChat={pg.resetChat}
    />
  )
}
