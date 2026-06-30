import { useState, useEffect, useRef, useCallback } from 'react'
import {
  copyToClipboard,
  selectTextForManualCopy,
  cleanupManualCopyBuffer,
} from '@/lib/utils'
import { toast } from '@/hooks/use-toast'
import type { Message, MessageAttachment } from '@/components/playground/types'
import { MAX_ATTACHMENT_BYTES } from '@/components/playground/types'

export interface UsePlaygroundOptions {
  onResetExtra?: () => void
}

export function usePlayground(options: UsePlaygroundOptions = {}) {
  const [messages, setMessages] = useState<Message[]>([])
  const [input, setInput] = useState('')
  const [isStreaming, setIsStreaming] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [curlOpen, setCurlOpen] = useState(false)
  const [copied, setCopied] = useState(false)
  const [attachments, setAttachments] = useState<MessageAttachment[]>([])

  const [selectedModel, setSelectedModel] = useState('')
  const [systemPrompt, setSystemPrompt] = useState('')
  const [streamEnabled, setStreamEnabled] = useState(true)
  const [temperature, setTemperature] = useState(0.7)
  const [maxTokens, setMaxTokens] = useState(16384)
  const [useMaxContext, setUseMaxContext] = useState(false)

  const messagesEndRef = useRef<HTMLDivElement>(null)
  const abortControllerRef = useRef<AbortController | null>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const imageInputRef = useRef<HTMLInputElement>(null)
  const audioInputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  useEffect(() => {
    if (!curlOpen) {
      cleanupManualCopyBuffer()
    }
  }, [curlOpen])

  const resetChat = useCallback(() => {
    setMessages([])
    setAttachments([])
    options.onResetExtra?.()
    toast({ title: 'Chat cleared' })
  }, [options])

  const handleFileAttachment = useCallback(async (file: File, type: 'image' | 'audio') => {
    if (!file) return

    if (file.size > MAX_ATTACHMENT_BYTES) {
      toast({
        title: 'File too large',
        description: 'Maximum size is 4MB',
        variant: 'destructive',
      })
      return
    }

    try {
      const reader = new FileReader()
      reader.onload = (e) => {
        const data = e.target?.result as string
        const newAttachment: MessageAttachment = {
          type,
          name: file.name,
          data,
          mimeType: file.type,
        }
        setAttachments((prev) => [...prev, newAttachment])
        toast({ title: `${type === 'image' ? 'Image' : 'Audio'} attached` })
      }
      reader.readAsDataURL(file)
    } catch {
      toast({ title: 'Failed to read file', variant: 'destructive' })
    }
  }, [])

  const removeAttachment = useCallback((index: number) => {
    setAttachments((prev) => prev.filter((_, i) => i !== index))
  }, [])

  const stopGeneration = useCallback(() => {
    abortControllerRef.current?.abort()
    setIsStreaming(false)
  }, [])

  const handleCopyCurl = useCallback(async (text: string) => {
    try {
      const { method } = await copyToClipboard(text)
      if (method !== 'manual') {
        setCopied(true)
        setTimeout(() => setCopied(false), 2000)
        toast({ title: 'Copied to clipboard' })
        return
      }

      setCopied(false)
      selectTextForManualCopy(text)
      toast({
        title: 'Auto copy unavailable',
        description: 'Press Ctrl+C to copy the selected value.',
      })
    } catch {
      toast({ title: 'Failed to copy', variant: 'destructive' })
    }
  }, [])

  const handlePaste = useCallback(
    (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
      const files = e.clipboardData?.files
      if (!files) return
      for (const file of Array.from(files)) {
        const type = file.type.startsWith('audio/')
          ? 'audio'
          : file.type.startsWith('image/')
            ? 'image'
            : null
        if (type) {
          void handleFileAttachment(file, type)
        }
      }
    },
    [handleFileAttachment]
  )

  return {
    messages,
    setMessages,
    input,
    setInput,
    isStreaming,
    setIsStreaming,
    settingsOpen,
    setSettingsOpen,
    curlOpen,
    setCurlOpen,
    copied,
    attachments,
    setAttachments,

    selectedModel,
    setSelectedModel,
    systemPrompt,
    setSystemPrompt,
    streamEnabled,
    setStreamEnabled,
    temperature,
    setTemperature,
    maxTokens,
    setMaxTokens,
    useMaxContext,
    setUseMaxContext,

    messagesEndRef,
    abortControllerRef,
    inputRef,
    imageInputRef,
    audioInputRef,

    resetChat,
    handleFileAttachment,
    removeAttachment,
    stopGeneration,
    handleCopyCurl,
    handlePaste,
  }
}
