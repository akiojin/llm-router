import { useEffect, type RefObject } from 'react'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { Image as ImageIcon, Mic, X, Volume2, Send, Loader2 } from 'lucide-react'
import type { MessageAttachment } from './types'

interface ChatFormProps {
  input: string
  onInputChange: (value: string) => void
  onSend: () => void
  onStop: () => void
  isStreaming: boolean
  disabled?: boolean
  attachments: MessageAttachment[]
  onRemoveAttachment: (index: number) => void
  onPaste: (e: React.ClipboardEvent<HTMLTextAreaElement>) => void
  inputRef: RefObject<HTMLTextAreaElement | null>
  imageInputRef: RefObject<HTMLInputElement | null>
  audioInputRef: RefObject<HTMLInputElement | null>
  onImageAttach: (file: File) => void
  onAudioAttach: (file: File) => void
  sendDisabled?: boolean
  placeholder?: string
  maxWidth?: string
  showAttachButtons?: boolean
  extraContent?: React.ReactNode
  sendButton?: React.ReactNode
  inputId?: string
}

export function ChatForm({
  input,
  onInputChange,
  onSend,
  onStop,
  isStreaming,
  disabled = false,
  attachments,
  onRemoveAttachment,
  onPaste,
  inputRef,
  imageInputRef,
  audioInputRef,
  onImageAttach,
  onAudioAttach,
  sendDisabled = false,
  placeholder = 'Type a message or attach files...',
  maxWidth = 'max-w-4xl',
  showAttachButtons = true,
  extraContent,
  sendButton,
  inputId,
}: ChatFormProps) {
  // テキストエリアの高さを内容に応じて自動調整（最大 200px）
  useEffect(() => {
    const el = inputRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`
  }, [input, inputRef])

  return (
    <div className="border-t p-4 bg-gradient-to-b from-background via-background to-muted/5">
      <div className={`${maxWidth} mx-auto space-y-3`}>
        {extraContent}

        {showAttachButtons && attachments.length > 0 && (
          <div className="flex flex-wrap gap-2 p-3 rounded-lg bg-muted/40 border border-muted-foreground/10">
            {attachments.map((att, i) => (
              <div key={`${att.name}-${i}`} className="relative group inline-block rounded-md overflow-hidden bg-background border border-border">
                {att.type === 'image' ? (
                  <>
                    <img src={att.data} alt={att.name} className="h-16 w-16 object-cover" />
                    <div className="absolute inset-0 bg-black/0 group-hover:bg-black/40 transition-colors flex items-center justify-center opacity-0 group-hover:opacity-100">
                      <Button
                        variant="destructive"
                        size="icon"
                        onClick={() => onRemoveAttachment(i)}
                        className="rounded-full p-1 h-auto w-auto"
                      >
                        <X className="h-3 w-3" />
                      </Button>
                    </div>
                  </>
                ) : (
                  <div className="h-16 w-16 flex items-center justify-center bg-muted">
                    <Volume2 className="h-4 w-4" />
                  </div>
                )}
              </div>
            ))}
          </div>
        )}

        <div className="flex items-end gap-2">
          {showAttachButtons && (
            <>
              <input
                ref={imageInputRef}
                type="file"
                accept="image/*"
                className="hidden"
                onChange={(e) => {
                  const file = e.target.files?.[0]
                  if (file) onImageAttach(file)
                  e.currentTarget.value = ''
                }}
              />
              <input
                ref={audioInputRef}
                type="file"
                accept="audio/*"
                className="hidden"
                onChange={(e) => {
                  const file = e.target.files?.[0]
                  if (file) onAudioAttach(file)
                  e.currentTarget.value = ''
                }}
              />

              <Button
                variant="outline"
                size="icon"
                onClick={() => imageInputRef.current?.click()}
                title="Attach image"
                className="shrink-0"
              >
                <ImageIcon className="h-4 w-4" />
              </Button>
              <Button
                variant="outline"
                size="icon"
                onClick={() => audioInputRef.current?.click()}
                title="Attach audio"
                className="shrink-0"
              >
                <Mic className="h-4 w-4" />
              </Button>
            </>
          )}

          <Textarea
            ref={inputRef}
            rows={1}
            placeholder={placeholder}
            value={input}
            onChange={(e) => onInputChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault()
                onSend()
              } else if (e.key === 'Escape' && isStreaming) {
                e.preventDefault()
                onStop()
              }
            }}
            onPaste={onPaste}
            disabled={disabled}
            id={inputId}
            className="max-h-[200px] min-h-[2.5rem] resize-none"
          />

          {sendButton ? (
            sendButton
          ) : isStreaming ? (
            <Button
              variant="destructive"
              onClick={onStop}
              title="Stop generating (Esc)"
              className="shrink-0 animate-pulse ring-2 ring-destructive/40"
            >
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              Stop
            </Button>
          ) : (
            <Button
              onClick={onSend}
              disabled={sendDisabled}
              className="shrink-0"
            >
              <Send className="mr-2 h-4 w-4" />
              Send
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}
