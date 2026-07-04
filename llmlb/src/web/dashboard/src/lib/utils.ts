import { type ClassValue, clsx } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export function formatBytes(bytes: number | null | undefined, decimals = 2): string {
  if (bytes == null || bytes === 0) return '0 Bytes'

  const k = 1024
  const dm = decimals < 0 ? 0 : decimals
  const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB', 'PB']

  const i = Math.floor(Math.log(bytes) / Math.log(k))

  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dm))} ${sizes[i]}`
}

export function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return '-'
  if (ms < 1000) return `${ms.toFixed(0)}ms`
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`
  if (ms < 3600000) return `${(ms / 60000).toFixed(1)}m`
  return `${(ms / 3600000).toFixed(1)}h`
}

export function formatNumber(num: number | null | undefined): string {
  if (num == null) return '-'
  if (num >= 1000000) return `${(num / 1000000).toFixed(1)}M`
  if (num >= 1000) return `${(num / 1000).toFixed(1)}K`
  return num.toLocaleString()
}

export function formatFullNumber(num: number | null | undefined): string {
  if (num == null) return '-'
  return num.toLocaleString('ja-JP')
}

export function formatPercentage(value: number | null | undefined, decimals = 1): string {
  if (value == null) return '-'
  return `${value.toFixed(decimals)}%`
}

export function formatDate(date: Date | string): string {
  const d = typeof date === 'string' ? new Date(date) : date
  return d.toLocaleDateString('ja-JP', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function formatRelativeTime(date: Date | string | null | undefined): string {
  if (date == null) return '-'
  const d = typeof date === 'string' ? new Date(date) : date
  if (isNaN(d.getTime())) return '-'
  const now = new Date()
  const diff = now.getTime() - d.getTime()
  const seconds = Math.floor(diff / 1000)

  if (seconds < 60) return 'just now'
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`
  if (seconds < 604800) return `${Math.floor(seconds / 86400)}d ago`
  return formatDate(d)
}

export function truncate(str: string, length: number): string {
  if (str.length <= length) return str
  return `${str.slice(0, length)}...`
}

/**
 * fetch の中断（AbortController.abort()）由来のエラーかを安全に判定する。
 * 中断は「失敗」ではなくユーザー操作・unmount による正常な打ち切りなので、
 * エラートースト等の対象から除外するために使う。
 */
export function isAbortError(error: unknown): boolean {
  if (error instanceof DOMException) {
    return error.name === 'AbortError'
  }
  if (!error || typeof error !== 'object') {
    return false
  }
  return (error as { name?: unknown }).name === 'AbortError'
}

export type ClipboardCopyMethod = 'exec-command' | 'clipboard' | 'manual'

export interface ClipboardCopyResult {
  method: ClipboardCopyMethod
}

let manualSelectionElement: HTMLTextAreaElement | null = null
let manualCleanupTimer: ReturnType<typeof setTimeout> | null = null
const MANUAL_COPY_BUFFER_TTL_MS = 15_000

function clearManualCleanupTimer() {
  if (manualCleanupTimer != null) {
    clearTimeout(manualCleanupTimer)
    manualCleanupTimer = null
  }
}

function createHiddenCopyTextarea(text: string): HTMLTextAreaElement | null {
  if (typeof document === 'undefined') {
    return null
  }

  const textarea = document.createElement('textarea')
  textarea.value = text
  textarea.setAttribute('readonly', '')
  textarea.setAttribute('aria-hidden', 'true')
  textarea.style.position = 'fixed'
  textarea.style.top = '0'
  textarea.style.left = '0'
  textarea.style.width = '1px'
  textarea.style.height = '1px'
  textarea.style.opacity = '0'
  textarea.style.pointerEvents = 'none'
  textarea.style.zIndex = '-1'

  document.body.appendChild(textarea)
  return textarea
}

function applyTextareaSelection(textarea: HTMLTextAreaElement) {
  textarea.focus()
  textarea.select()
  textarea.setSelectionRange(0, textarea.value.length)
}

function restoreFocusedElement(element: HTMLElement | null) {
  if (!element || typeof element.focus !== 'function') {
    return
  }

  try {
    element.focus()
  } catch {
    // Ignore focus restoration failures for detached or unfocusable elements.
  }
}

function attemptExecCommandCopy(text: string): boolean {
  if (typeof document === 'undefined' || typeof document.execCommand !== 'function') {
    return false
  }

  const textarea = createHiddenCopyTextarea(text)
  if (!textarea) {
    return false
  }

  const activeElement = document.activeElement instanceof HTMLElement ? document.activeElement : null

  try {
    applyTextareaSelection(textarea)
    return document.execCommand('copy')
  } catch {
    return false
  } finally {
    textarea.remove()
    restoreFocusedElement(activeElement)
  }
}

function handleManualCopyKeydown(event: KeyboardEvent) {
  const key = event.key.toLowerCase()
  if ((event.ctrlKey || event.metaKey) && key === 'c') {
    if (typeof window !== 'undefined') {
      window.setTimeout(() => cleanupManualCopyBuffer(), 0)
      return
    }
    cleanupManualCopyBuffer()
  }
}

function handleManualCopyVisibilityChange() {
  if (typeof document !== 'undefined' && document.visibilityState !== 'visible') {
    cleanupManualCopyBuffer()
  }
}

function handleManualCopyPageHide() {
  cleanupManualCopyBuffer()
}

function handleManualCopyBlur() {
  cleanupManualCopyBuffer()
}

export function cleanupManualCopyBuffer() {
  clearManualCleanupTimer()

  if (typeof document !== 'undefined') {
    document.removeEventListener('keydown', handleManualCopyKeydown, true)
    document.removeEventListener('visibilitychange', handleManualCopyVisibilityChange)
  }
  if (typeof window !== 'undefined') {
    window.removeEventListener('pagehide', handleManualCopyPageHide)
  }

  if (manualSelectionElement) {
    manualSelectionElement.removeEventListener('blur', handleManualCopyBlur)
    if (manualSelectionElement.isConnected) {
      manualSelectionElement.remove()
    }
    manualSelectionElement = null
  }
}

export async function copyToClipboard(text: string): Promise<ClipboardCopyResult> {
  if (typeof text !== 'string' || text.length === 0) {
    throw new Error('Clipboard value is empty')
  }

  cleanupManualCopyBuffer()

  const isClipboardApiAvailable =
    typeof navigator !== 'undefined' && typeof navigator.clipboard?.writeText === 'function'
  const isSecureContext = typeof window !== 'undefined' ? window.isSecureContext : true

  if (isClipboardApiAvailable && isSecureContext) {
    try {
      await navigator.clipboard.writeText(text)
      return { method: 'clipboard' }
    } catch {
      // Fall through to manual selection when automatic clipboard writes are unavailable.
    }
  }

  if (attemptExecCommandCopy(text)) {
    return { method: 'exec-command' }
  }

  return { method: 'manual' }
}

export function selectTextForManualCopy(text: string): boolean {
  if (typeof text !== 'string' || text.length === 0 || typeof document === 'undefined') {
    return false
  }

  cleanupManualCopyBuffer()

  const textarea = createHiddenCopyTextarea(text)
  if (!textarea) {
    return false
  }

  const applySelection = () => applyTextareaSelection(textarea)

  applySelection()
  if (typeof window !== 'undefined') {
    if (typeof window.requestAnimationFrame === 'function') {
      window.requestAnimationFrame(applySelection)
    }
    window.setTimeout(applySelection, 0)
  }

  manualSelectionElement = textarea
  textarea.addEventListener('blur', handleManualCopyBlur)
  document.addEventListener('keydown', handleManualCopyKeydown, true)
  document.addEventListener('visibilitychange', handleManualCopyVisibilityChange)
  if (typeof window !== 'undefined') {
    window.addEventListener('pagehide', handleManualCopyPageHide)
  }
  manualCleanupTimer = setTimeout(() => cleanupManualCopyBuffer(), MANUAL_COPY_BUFFER_TTL_MS)
  return true
}
