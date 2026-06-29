import { useState, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { type DashboardEndpoint, endpointsApi } from '@/lib/api'
import { toast } from '@/hooks/use-toast'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Progress } from '@/components/ui/progress'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Download, Loader2, CheckCircle, XCircle } from 'lucide-react'

/**
 * SPEC-e8e9326e: xLLM Model Download Dialog
 * Allows downloading models to xLLM endpoints
 */

interface ModelDownloadDialogProps {
  endpoint: DashboardEndpoint | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

type DownloadStatus = 'idle' | 'downloading' | 'completed' | 'error'

export function ModelDownloadDialog({
  endpoint,
  open,
  onOpenChange,
}: ModelDownloadDialogProps) {
  return (
    <ModelDownloadDialogContent
      key={`${endpoint?.id ?? 'empty'}:${open ? 'open' : 'closed'}`}
      endpoint={endpoint}
      open={open}
      onOpenChange={onOpenChange}
    />
  )
}

function ModelDownloadDialogContent({
  endpoint,
  open,
  onOpenChange,
}: ModelDownloadDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [modelName, setModelName] = useState('')
  const [status, setStatus] = useState<DownloadStatus>('idle')
  const [progress, setProgress] = useState(0)
  const [progressMessage, setProgressMessage] = useState('')
  const [errorMessage, setErrorMessage] = useState('')
  const pollingRef = useRef<number | null>(null)

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (pollingRef.current) {
        clearInterval(pollingRef.current)
      }
    }
  }, [])

  // Download mutation
  const downloadMutation = useMutation({
    mutationFn: (data: { model: string }) =>
      endpointsApi.downloadModel(endpoint!.id, data),
    onSuccess: (data) => {
      setStatus('downloading')
      setProgress(0)
      setProgressMessage(t('modelDialogs.startingDownload'))
      // Start polling for progress
      startProgressPolling(data.task_id)
    },
    onError: (error) => {
      setStatus('error')
      setErrorMessage(error instanceof Error ? error.message : t('modelDialogs.downloadFailed'))
      toast({
        title: t('modelDialogs.downloadFailedTitle'),
        description: error instanceof Error ? error.message : t('modelDialogs.unknownError'),
        variant: 'destructive',
      })
    },
  })

  const startProgressPolling = (downloadTaskId: string) => {
    if (pollingRef.current) {
      clearInterval(pollingRef.current)
    }

    const pollProgress = async () => {
      if (!endpoint) return

      try {
        const result = await endpointsApi.getDownloadProgress(endpoint.id)
        const task =
          result.tasks.find((t) => t.task_id === downloadTaskId) ||
          result.tasks.find((t) => t.model === modelName)

        if (!task) {
          setProgressMessage(t('modelDialogs.waitingForDownload'))
          return
        }

        if (task.status === 'completed') {
          setStatus('completed')
          setProgress(100)
          setProgressMessage(t('modelDialogs.downloadCompletedMessage'))
          if (pollingRef.current) {
            clearInterval(pollingRef.current)
            pollingRef.current = null
          }
          queryClient.invalidateQueries({ queryKey: ['endpoint-models', endpoint.id] })
          queryClient.invalidateQueries({ queryKey: ['dashboard-endpoints'] })
          toast({
            title: t('modelDialogs.downloadCompletedTitle'),
            description: t('modelDialogs.downloadCompletedDesc', { model: modelName }),
          })
        } else if (task.status === 'failed' || task.status === 'cancelled') {
          setStatus('error')
          setErrorMessage(task.error || t('modelDialogs.downloadFailed'))
          setProgressMessage('')
          if (pollingRef.current) {
            clearInterval(pollingRef.current)
            pollingRef.current = null
          }
        } else if (task.status === 'downloading' || task.status === 'pending') {
          setProgress(task.progress || 0)
          setProgressMessage(buildProgressMessage(task, t))
        }
      } catch {
        // Polling error - might be temporary, keep trying
      }
    }

    // Poll immediately, then every 2 seconds
    pollProgress()
    pollingRef.current = window.setInterval(pollProgress, 2000)
  }

  const handleDownload = () => {
    if (!modelName.trim()) {
      toast({
        title: t('modelDialogs.modelNameRequired'),
        description: t('modelDialogs.pleaseEnterModelName'),
        variant: 'destructive',
      })
      return
    }

    setErrorMessage('')
    downloadMutation.mutate({ model: modelName.trim() })
  }

  const handleClose = () => {
    if (status === 'downloading') {
      // Don't close while downloading - warn user
      toast({
        title: t('modelDialogs.downloadInProgressTitle'),
        description: t('modelDialogs.pleaseWaitDownload'),
      })
      return
    }
    onOpenChange(false)
  }

  if (!endpoint || endpoint.endpoint_type !== 'xllm') return null

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (nextOpen) {
          onOpenChange(true)
          return
        }
        handleClose()
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Download className="h-5 w-5" />
            {t('modelDialogs.downloadModelTitle')}
          </DialogTitle>
          <DialogDescription>
            {t('modelDialogs.downloadModelToDesc', { name: endpoint.name })}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {status === 'idle' && (
            <div className="space-y-2">
              <Label htmlFor="model-name">{t('modelDialogs.modelNameLabel')}</Label>
              <Input
                id="model-name"
                placeholder={t('modelDialogs.modelNamePlaceholder')}
                value={modelName}
                onChange={(e) => setModelName(e.target.value)}
                disabled={downloadMutation.isPending}
              />
              <p className="text-xs text-muted-foreground">
                {t('modelDialogs.modelNameHelp')}
              </p>
            </div>
          )}

          {status === 'downloading' && (
            <div className="space-y-3">
              <div className="flex items-center gap-2">
                <Loader2 className="h-4 w-4 animate-spin" />
                <span className="text-sm font-medium">{t('modelDialogs.downloadingModel', { model: modelName })}</span>
              </div>
              <Progress value={progress} className="h-2" />
              <p className="text-xs text-muted-foreground">{progressMessage}</p>
            </div>
          )}

          {status === 'completed' && (
            <div className="flex flex-col items-center gap-2 py-4">
              <CheckCircle className="h-12 w-12 text-green-500" />
              <p className="text-sm font-medium">{t('modelDialogs.downloadCompletedTitle')}</p>
              <p className="text-xs text-muted-foreground">
                {t('modelDialogs.modelNowAvailable', { model: modelName })}
              </p>
            </div>
          )}

          {status === 'error' && (
            <div className="flex flex-col items-center gap-2 py-4">
              <XCircle className="h-12 w-12 text-destructive" />
              <p className="text-sm font-medium text-destructive">{t('modelDialogs.downloadFailedTitle')}</p>
              <p className="text-xs text-muted-foreground">{errorMessage}</p>
            </div>
          )}
        </div>

        <DialogFooter>
          {(status === 'idle' || status === 'error') && (
            <>
              <Button variant="outline" onClick={handleClose}>
                {t('modelDialogs.cancel')}
              </Button>
              <Button
                onClick={handleDownload}
                disabled={!modelName.trim() || downloadMutation.isPending}
              >
                {downloadMutation.isPending && (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                )}
                {t('modelDialogs.download')}
              </Button>
            </>
          )}
          {status === 'completed' && (
            <Button onClick={handleClose}>{t('modelDialogs.close')}</Button>
          )}
          {status === 'downloading' && (
            <p className="text-xs text-muted-foreground">
              {t('modelDialogs.downloadInProgressShort')}
            </p>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function buildProgressMessage(
  task: { speed_mbps?: number; eta_seconds?: number },
  t: (key: string, opts?: Record<string, unknown>) => string
): string {
  const parts: string[] = []
  if (task.speed_mbps != null) parts.push(`${task.speed_mbps.toFixed(1)} Mbps`)
  if (task.eta_seconds != null)
    parts.push(t('modelDialogs.etaFormat', { value: formatEta(task.eta_seconds) }))
  return parts.join(' / ') || t('modelDialogs.downloadingFallback')
}

function formatEta(seconds: number): string {
  if (!Number.isFinite(seconds)) return '-'
  const s = Math.max(0, Math.floor(seconds))
  const m = Math.floor(s / 60)
  const r = s % 60
  if (m <= 0) return `${r}s`
  return `${m}m ${r}s`
}
