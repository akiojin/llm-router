import { useState, type RefObject, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import { Settings, Trash2, ArrowLeft, Menu } from 'lucide-react'
import type { Message, MessageAttachment } from './types'
import { MessageList } from './MessageList'
import { ChatForm } from './ChatForm'
import { SettingsDialog } from './SettingsDialog'
import { CurlDialog } from './CurlDialog'

interface PlaygroundBaseProps {
  onBack: () => void
  sidebarHeader: ReactNode
  sidebarInfo: ReactNode
  sidebarExtra?: ReactNode
  headerContent: ReactNode
  headerRight?: ReactNode
  aboveMessages?: ReactNode

  messages: Message[]
  messagesEndRef: RefObject<HTMLDivElement | null>
  emptyTitle: string
  emptyDescription: string
  messageMaxWidth?: string

  input: string
  onInputChange: (value: string) => void
  onSend: () => void
  onStop: () => void
  isStreaming: boolean
  inputDisabled?: boolean
  attachments: MessageAttachment[]
  onRemoveAttachment: (index: number) => void
  onPaste: (e: React.ClipboardEvent<HTMLTextAreaElement>) => void
  inputRef: RefObject<HTMLTextAreaElement | null>
  imageInputRef: RefObject<HTMLInputElement | null>
  audioInputRef: RefObject<HTMLInputElement | null>
  onImageAttach: (file: File) => void
  onAudioAttach: (file: File) => void
  sendDisabled?: boolean
  inputPlaceholder?: string
  formMaxWidth?: string
  showAttachButtons?: boolean
  formExtraContent?: ReactNode
  sendButton?: ReactNode
  inputId?: string

  settingsOpen: boolean
  onSettingsOpenChange: (open: boolean) => void
  systemPrompt: string
  onSystemPromptChange: (value: string) => void
  streamEnabled: boolean
  onStreamEnabledChange: (value: boolean) => void
  streamDisabled?: boolean
  temperature: number
  onTemperatureChange: (value: number) => void
  maxTokens: number
  onMaxTokensChange: (value: number) => void
  useMaxContext: boolean
  onUseMaxContextChange: (value: boolean) => void
  selectedModelMaxTokens?: number | null
  settingsDescription?: string
  maxContextCheckboxId?: string

  curlOpen: boolean
  onCurlOpenChange: (open: boolean) => void
  curlCommand: string
  copied: boolean
  onCopyCurl: (text: string) => void
  curlCopyDisabled?: boolean
  curlDescription?: string

  resetChat: () => void
  sidebarWidth?: string
  sidebarId?: string
}

export function PlaygroundBase({
  onBack,
  sidebarHeader,
  sidebarInfo,
  sidebarExtra,
  headerContent,
  headerRight,
  aboveMessages,

  messages,
  messagesEndRef,
  emptyTitle,
  emptyDescription,
  messageMaxWidth,

  input,
  onInputChange,
  onSend,
  onStop,
  isStreaming,
  inputDisabled = false,
  attachments,
  onRemoveAttachment,
  onPaste,
  inputRef,
  imageInputRef,
  audioInputRef,
  onImageAttach,
  onAudioAttach,
  sendDisabled = false,
  inputPlaceholder,
  formMaxWidth,
  showAttachButtons = true,
  formExtraContent,
  sendButton,
  inputId,

  settingsOpen,
  onSettingsOpenChange,
  systemPrompt,
  onSystemPromptChange,
  streamEnabled,
  onStreamEnabledChange,
  streamDisabled,
  temperature,
  onTemperatureChange,
  maxTokens,
  onMaxTokensChange,
  useMaxContext,
  onUseMaxContextChange,
  selectedModelMaxTokens,
  settingsDescription,
  maxContextCheckboxId,

  curlOpen,
  onCurlOpenChange,
  curlCommand,
  copied,
  onCopyCurl,
  curlCopyDisabled,
  curlDescription,

  resetChat,
  sidebarWidth = 'w-72',
  sidebarId,
}: PlaygroundBaseProps) {
  const { t } = useTranslation()
  // モバイルではサイドバーをドロワー化（既定で隠し、ハンバーガーで開く）。md 以上は常時表示。
  const [sidebarOpen, setSidebarOpen] = useState(false)

  return (
    <div className="flex h-screen bg-background">
      {/* モバイル用バックドロップ */}
      {sidebarOpen && (
        <div
          className="fixed inset-0 z-30 bg-black/50 md:hidden"
          onClick={() => setSidebarOpen(false)}
          aria-hidden="true"
        />
      )}

      <div
        id={sidebarId}
        className={cn(
          // デスクトップ(md+)は static で従来どおり常時表示。モバイル(max-md)のみ
          // 固定オーバーレイのドロワーになる（fixed と md:static の優先順位問題を回避）。
          'flex flex-col border-r bg-background shrink-0 max-md:fixed max-md:inset-y-0 max-md:left-0 max-md:z-40 max-md:transition-transform',
          sidebarWidth,
          sidebarOpen ? 'max-md:translate-x-0' : 'max-md:-translate-x-full'
        )}
      >
        <div className="p-4 border-b">{sidebarHeader}</div>

        {sidebarInfo}

        <Separator />

        <div className="p-3 space-y-2">
          <Button
            variant="outline"
            className="w-full justify-start"
            onClick={() => {
              setSidebarOpen(false)
              onBack()
            }}
          >
            <ArrowLeft className="mr-2 h-4 w-4" />
            {t('playground.backToDashboard')}
          </Button>
          <Button
            variant="outline"
            className="w-full justify-start"
            onClick={() => {
              setSidebarOpen(false)
              onSettingsOpenChange(true)
            }}
          >
            <Settings className="mr-2 h-4 w-4" />
            {t('playground.settings')}
          </Button>
          {sidebarExtra}
        </div>

        <div className="flex-1" />

        <div className="p-3 border-t">
          <Button
            variant="outline"
            size="sm"
            className="w-full"
            onClick={() => {
              setSidebarOpen(false)
              resetChat()
            }}
          >
            <Trash2 className="mr-2 h-4 w-4" />
            {t('playground.clearChat')}
          </Button>
        </div>
      </div>

      <div className="flex-1 flex flex-col min-w-0">
        <div className="h-14 border-b flex items-center justify-between gap-2 px-4">
          <div className="flex min-w-0 items-center gap-2">
            <Button
              variant="outline"
              size="icon"
              className="shrink-0 md:hidden"
              aria-label="Open menu"
              onClick={() => setSidebarOpen(true)}
            >
              <Menu className="h-4 w-4" />
            </Button>
            {headerContent}
          </div>
          {headerRight}
        </div>

        {aboveMessages}

        <MessageList
          messages={messages}
          messagesEndRef={messagesEndRef}
          emptyTitle={emptyTitle}
          emptyDescription={emptyDescription}
          isGenerating={isStreaming}
          maxWidth={messageMaxWidth}
        />

        <ChatForm
          input={input}
          onInputChange={onInputChange}
          onSend={onSend}
          onStop={onStop}
          isStreaming={isStreaming}
          disabled={inputDisabled}
          attachments={attachments}
          onRemoveAttachment={onRemoveAttachment}
          onPaste={onPaste}
          inputRef={inputRef}
          imageInputRef={imageInputRef}
          audioInputRef={audioInputRef}
          onImageAttach={onImageAttach}
          onAudioAttach={onAudioAttach}
          sendDisabled={sendDisabled}
          placeholder={inputPlaceholder}
          maxWidth={formMaxWidth}
          showAttachButtons={showAttachButtons}
          extraContent={formExtraContent}
          sendButton={sendButton}
          inputId={inputId}
        />
      </div>

      <SettingsDialog
        open={settingsOpen}
        onOpenChange={onSettingsOpenChange}
        systemPrompt={systemPrompt}
        onSystemPromptChange={onSystemPromptChange}
        streamEnabled={streamEnabled}
        onStreamEnabledChange={onStreamEnabledChange}
        streamDisabled={streamDisabled}
        temperature={temperature}
        onTemperatureChange={onTemperatureChange}
        maxTokens={maxTokens}
        onMaxTokensChange={onMaxTokensChange}
        useMaxContext={useMaxContext}
        onUseMaxContextChange={onUseMaxContextChange}
        selectedModelMaxTokens={selectedModelMaxTokens}
        description={settingsDescription}
        maxContextCheckboxId={maxContextCheckboxId}
      />

      <CurlDialog
        open={curlOpen}
        onOpenChange={onCurlOpenChange}
        curlCommand={curlCommand}
        copied={copied}
        onCopy={onCopyCurl}
        copyDisabled={curlCopyDisabled}
        description={curlDescription}
      />
    </div>
  )
}
