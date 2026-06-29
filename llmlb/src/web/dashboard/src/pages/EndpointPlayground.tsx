import { useEffect, useRef } from 'react'
import { useQuery } from '@tanstack/react-query'
import { endpointsApi, ApiError, type DashboardEndpoint } from '@/lib/api'
import { cn, isAbortError } from '@/lib/utils'
import { toast } from '@/hooks/use-toast'
import { usePlayground } from '@/hooks/usePlayground'
import { splitAssistantMessage } from '@/lib/reasoning'
import { PlaygroundBase, getErrorMessage, transformMessage, MAX_INPUT_CHARS, type Message } from '@/components/playground'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Cpu, CircleDot, Loader2, Code } from 'lucide-react'

interface EndpointPlaygroundProps {
  endpointId: string
  onBack: () => void
}

function getStatusBadgeVariant(
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

function getStatusIndicatorColor(status: DashboardEndpoint['status'] | undefined): string {
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

function getStatusLabel(
  status: DashboardEndpoint['status'] | undefined
): string {
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
      return 'Unknown'
  }
}

export default function EndpointPlayground({ endpointId, onBack }: EndpointPlaygroundProps) {
  const pg = usePlayground()
  const isMountedRef = useRef(true)

  useEffect(() => {
    return () => {
      isMountedRef.current = false
      pg.abortControllerRef.current?.abort()
    }
  }, [])

  const { data: endpoint, isLoading: isLoadingEndpoint } = useQuery({
    queryKey: ['endpoint', endpointId],
    queryFn: () => endpointsApi.get(endpointId),
  })

  const { data: endpointModels, isLoading: isLoadingModels, error: modelsError } = useQuery({
    queryKey: ['endpoint-models', endpointId],
    queryFn: () => endpointsApi.getModels(endpointId),
    retry: false,
  })

  useEffect(() => {
    if (modelsError) {
      let description = 'Failed to fetch model list'
      if (modelsError instanceof ApiError) {
        description = getErrorMessage(modelsError)
      }
      toast({ title: 'Error', description, variant: 'destructive' })
    }
  }, [modelsError])

  useEffect(() => {
    if (endpointModels?.models && !pg.selectedModel && endpointModels.models.length > 0) {
      pg.setSelectedModel(endpointModels.models[0].model_id)
    }
  }, [endpointModels, pg.selectedModel, pg.setSelectedModel])

  const models = endpointModels?.models || []
  const selectedModelMaxTokens = models.find(m => m.model_id === pg.selectedModel)?.max_tokens
  const effectiveMaxTokens = pg.useMaxContext && selectedModelMaxTokens != null ? selectedModelMaxTokens : pg.maxTokens
  const baseUrl = endpoint?.base_url?.replace(/\/$/, '') || ''
  const hasBaseUrl = baseUrl.length > 0

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
        ? [{ role: 'system' as const, content: pg.systemPrompt }, ...newMessages.map(transformMessage)]
        : newMessages.map(transformMessage)

      if (pg.streamEnabled) {
        let assistantContent = ''
        let assistantReasoning = ''
        pg.setMessages((prev) => [...prev, { role: 'assistant', content: '' }])

        await endpointsApi.chatCompletions(
          endpointId,
          {
            model: pg.selectedModel,
            messages: requestMessages,
            stream: true,
            temperature: pg.temperature,
            max_tokens: effectiveMaxTokens,
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
        const data = await endpointsApi.chatCompletions(
          endpointId,
          {
            model: pg.selectedModel,
            messages: requestMessages,
            stream: false,
            temperature: pg.temperature,
            max_tokens: effectiveMaxTokens,
          },
          undefined,
          pg.abortControllerRef.current.signal
        )

        const { content, reasoning } = splitAssistantMessage(data)
        pg.setMessages((prev) => [...prev, {
          role: 'assistant',
          content,
          reasoning: reasoning || undefined,
        }])
      }
    } catch (error) {
      if (!isAbortError(error)) {
        toast({
          title: 'Failed to send message',
          description:
            error instanceof ApiError
              ? getErrorMessage(error)
              : error instanceof Error
                ? error.message
                : 'Unknown error',
          variant: 'destructive',
        })
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

  const generateCurl = () => {
    const requestMessages = pg.systemPrompt
      ? [{ role: 'system', content: pg.systemPrompt }, ...pg.messages]
      : pg.messages

    if (!hasBaseUrl) {
      return '# Error: endpoint base_url is not configured. Please set it in the dashboard.'
    }

    return `curl -X POST '${baseUrl}/v1/chat/completions' \\
  -H 'Content-Type: application/json' \\
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

  if (isLoadingEndpoint) {
    return (
      <div className="flex h-screen w-full items-center justify-center bg-background">
        <div className="flex flex-col items-center gap-4">
          <Loader2 className="h-8 w-8 animate-spin text-primary" />
          <p className="text-sm text-muted-foreground">Loading endpoint...</p>
        </div>
      </div>
    )
  }

  return (
    <PlaygroundBase
      onBack={onBack}
      sidebarWidth="w-64"
      sidebarHeader={
        <div className="flex items-center gap-2">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-primary/10">
            <Cpu className="h-4 w-4 text-primary" />
          </div>
          <div>
            <h1 className="font-semibold text-sm truncate" title={endpoint?.name}>
              {endpoint?.name || 'Endpoint'}
            </h1>
            <p className="text-xs text-muted-foreground">Playground</p>
          </div>
        </div>
      }
      sidebarInfo={
        <div className="p-3 space-y-2">
          <div className="text-xs text-muted-foreground">
            <span className="font-medium">URL:</span>{' '}
            <span className="truncate block" title={endpoint?.base_url}>
              {hasBaseUrl ? endpoint?.base_url : 'Not set'}
            </span>
          </div>
          {!hasBaseUrl && (
            <div className="text-xs text-destructive">
              Base URL is not configured. Please check the endpoint settings.
            </div>
          )}
          <div className="text-xs text-muted-foreground">
            <span className="font-medium">Status:</span>{' '}
            <Badge variant={getStatusBadgeVariant(endpoint?.status)} className="text-xs">
              {getStatusLabel(endpoint?.status)}
            </Badge>
          </div>
          <div className="text-xs text-muted-foreground">
            <span className="font-medium">Models:</span> {models.length}
          </div>
        </div>
      }
      headerContent={
        <div className="flex items-center gap-3">
          <Select value={pg.selectedModel} onValueChange={pg.setSelectedModel}>
            <SelectTrigger className="w-64">
              <SelectValue placeholder="Select a model" />
            </SelectTrigger>
            <SelectContent>
              {isLoadingModels ? (
                <SelectItem value="__loading__" disabled>Loading models...</SelectItem>
              ) : models.length === 0 ? (
                <SelectItem value="__no_models__" disabled>No models available</SelectItem>
              ) : (
                models.map((model) => (
                  <SelectItem key={model.model_id} value={model.model_id}>{model.model_id}</SelectItem>
                ))
              )}
            </SelectContent>
          </Select>

          <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <CircleDot className={cn("h-3 w-3", getStatusIndicatorColor(endpoint?.status))} />
            {getStatusLabel(endpoint?.status)}
          </span>

          {pg.streamEnabled && (
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
      messages={pg.messages}
      messagesEndRef={pg.messagesEndRef}
      emptyTitle="Start a conversation"
      emptyDescription="Select a model and send a message to get started."
      messageMaxWidth="max-w-3xl"
      input={pg.input}
      onInputChange={pg.setInput}
      onSend={() => void sendMessage()}
      onStop={pg.stopGeneration}
      isStreaming={pg.isStreaming}
      attachments={pg.attachments}
      onRemoveAttachment={pg.removeAttachment}
      onPaste={pg.handlePaste}
      inputRef={pg.inputRef}
      imageInputRef={pg.imageInputRef}
      audioInputRef={pg.audioInputRef}
      onImageAttach={(file) => void pg.handleFileAttachment(file, 'image')}
      onAudioAttach={(file) => void pg.handleFileAttachment(file, 'audio')}
      sendDisabled={(!pg.input.trim() && pg.attachments.length === 0) || !pg.selectedModel}
      formMaxWidth="max-w-3xl"
      settingsOpen={pg.settingsOpen}
      onSettingsOpenChange={pg.setSettingsOpen}
      systemPrompt={pg.systemPrompt}
      onSystemPromptChange={pg.setSystemPrompt}
      streamEnabled={pg.streamEnabled}
      onStreamEnabledChange={pg.setStreamEnabled}
      temperature={pg.temperature}
      onTemperatureChange={pg.setTemperature}
      maxTokens={pg.maxTokens}
      onMaxTokensChange={pg.setMaxTokens}
      useMaxContext={pg.useMaxContext}
      onUseMaxContextChange={pg.setUseMaxContext}
      selectedModelMaxTokens={selectedModelMaxTokens}
      settingsDescription="Configure your chat preferences."
      curlOpen={pg.curlOpen}
      onCurlOpenChange={pg.setCurlOpen}
      curlCommand={generateCurl()}
      copied={pg.copied}
      onCopyCurl={pg.handleCopyCurl}
      curlCopyDisabled={!hasBaseUrl}
      curlDescription="Copy this command to replicate the API call."
      resetChat={pg.resetChat}
    />
  )
}
