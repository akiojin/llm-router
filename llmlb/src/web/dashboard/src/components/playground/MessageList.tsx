import { useState, type RefObject } from 'react'
import { cn } from '@/lib/utils'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Bot, Check, ChevronRight, Copy, Loader2, User, Volume2, MessageSquare } from 'lucide-react'
import type { Message } from './types'
import { extractMediaFromContent } from './types'
import { MarkdownMessage } from './MarkdownMessage'

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)
  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      // clipboard 不可環境では無視
    }
  }
  return (
    <button
      type="button"
      onClick={handleCopy}
      aria-label="Copy message"
      className="absolute right-1 top-1 rounded p-1 text-muted-foreground opacity-0 transition-opacity hover:bg-background hover:text-foreground focus-visible:opacity-100 group-hover/msg:opacity-100"
    >
      {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
    </button>
  )
}

interface MessageListProps {
  messages: Message[]
  messagesEndRef: RefObject<HTMLDivElement | null>
  emptyTitle: string
  emptyDescription: string
  isGenerating?: boolean
  maxWidth?: string
}

function GeneratingIndicator() {
  return (
    <div
      data-testid="playground-generating-indicator"
      aria-live="polite"
      className="flex items-center gap-2 text-sm text-muted-foreground"
    >
      <Loader2 className="h-4 w-4 animate-spin" />
      <span>Generating response...</span>
    </div>
  )
}

export function MessageList({
  messages,
  messagesEndRef,
  emptyTitle,
  emptyDescription,
  isGenerating = false,
  maxWidth = 'max-w-4xl',
}: MessageListProps) {
  const lastMessage = messages[messages.length - 1]
  const showPendingMessage =
    isGenerating &&
    (messages.length === 0 || lastMessage.role === 'user')

  return (
    <ScrollArea className="flex-1 p-4">
      <div className={cn(maxWidth, 'mx-auto space-y-4')}>
        {messages.length === 0 && !isGenerating ? (
          <div className="flex flex-col items-center justify-center h-64 text-center">
            <MessageSquare className="h-12 w-12 text-muted-foreground/50 mb-4" />
            <h2 className="text-lg font-medium">{emptyTitle}</h2>
            <p className="text-sm text-muted-foreground mt-1">{emptyDescription}</p>
          </div>
        ) : (
          messages.map((message, index) => (
            <div key={index} className={cn('flex gap-3', message.role === 'user' ? 'justify-end' : '')}>
              {message.role === 'assistant' && (
                <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-primary/10">
                  <Bot className="h-4 w-4 text-primary" />
                </div>
              )}
              <div
                className={cn(
                  'group/msg relative rounded-lg px-4 py-3 max-w-[80%] space-y-2',
                  message.role === 'user' ? 'bg-primary text-primary-foreground' : 'bg-muted'
                )}
              >
                {message.role === 'assistant' && message.content && (
                  <CopyButton text={message.content} />
                )}

                {message.role === 'assistant' && message.reasoning && (
                  <details
                    data-testid="reasoning-disclosure"
                    className="group rounded-md border border-border/50 bg-background/40 text-xs"
                  >
                    <summary className="flex cursor-pointer select-none items-center gap-1 px-2 py-1 text-muted-foreground transition-colors hover:text-foreground [&::-webkit-details-marker]:hidden">
                      <ChevronRight className="h-3 w-3 shrink-0 transition-transform group-open:rotate-90" />
                      Show reasoning
                    </summary>
                    <div className="whitespace-pre-wrap border-t border-border/50 px-2 py-1.5 text-muted-foreground">
                      {message.reasoning}
                    </div>
                  </details>
                )}

                {message.content ? (
                  message.role === 'assistant' ? (
                    <MarkdownMessage content={message.content} />
                  ) : (
                    <p className="text-sm whitespace-pre-wrap">{message.content}</p>
                  )
                ) : (
                  message.role === 'assistant' &&
                  isGenerating &&
                  index === messages.length - 1 && <GeneratingIndicator />
                )}

                {message.attachments && message.attachments.length > 0 && (
                  <div className="grid grid-cols-2 gap-2 mt-2">
                    {message.attachments.map((attachment, aIdx) => (
                      <div key={aIdx} className="rounded-md overflow-hidden bg-black/20 p-1">
                        {attachment.type === 'image' && (
                          <img
                            src={attachment.data}
                            alt={attachment.name}
                            className="w-full h-32 object-cover rounded-sm"
                          />
                        )}
                        {attachment.type === 'audio' && (
                          <div className="flex flex-col items-center justify-center h-32 gap-2">
                            <Volume2 className="h-6 w-6" />
                            <audio src={attachment.data} controls className="w-full max-w-[120px]" />
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                )}

                {message.role === 'assistant' && (() => {
                  const { imageMatches, audioMatches } = extractMediaFromContent(message.content)
                  return (
                    <>
                      {imageMatches.length > 0 && (
                        <div className="grid grid-cols-2 gap-2 mt-2">
                          {imageMatches.map((url, i) => (
                            <div key={`${url}-${i}`} className="rounded-md overflow-hidden bg-black/20 p-1">
                              <img src={url} alt={`assistant-image-${i}`} className="w-full h-32 object-cover rounded-sm" />
                            </div>
                          ))}
                        </div>
                      )}
                      {audioMatches.length > 0 && (
                        <div className="space-y-2 mt-2">
                          {audioMatches.map((url, i) => (
                            <div key={`${url}-${i}`} className="rounded-md bg-black/20 p-2">
                              <audio src={url} controls className="w-full" />
                            </div>
                          ))}
                        </div>
                      )}
                    </>
                  )
                })()}
              </div>
              {message.role === 'user' && (
                <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-muted">
                  <User className="h-4 w-4" />
                </div>
              )}
            </div>
          ))
        )}
        {showPendingMessage && (
          <div className="flex gap-3">
            <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-primary/10">
              <Bot className="h-4 w-4 text-primary" />
            </div>
            <div className="rounded-lg px-4 py-3 max-w-[80%] space-y-2 bg-muted">
              <GeneratingIndicator />
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>
    </ScrollArea>
  )
}
