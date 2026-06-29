import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  type DashboardEndpoint,
  type EndpointType,
  endpointsApi,
  getRecommendedInferenceTimeout,
  getRecommendedInferenceTimeoutLabel,
} from '@/lib/api'
import { classifyEndpointLastError } from '@/lib/endpoint-errors'
import { formatRelativeTime } from '@/lib/utils'
import { toast } from '@/hooks/use-toast'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  Server,
  Clock,
  AlertCircle,
  Save,
  Play,
  RefreshCw,
  MessageSquare,
  Download,
  Activity,
} from 'lucide-react'
import { ModelDownloadDialog } from './ModelDownloadDialog'
import { EndpointModelsTable } from './EndpointModelsTable'
import { EndpointRequestChart } from './EndpointRequestChart'

/**
 * SPEC-e8e9326e: Router-Driven Endpoint Registration System
 * Endpoint Detail Modal
 */

interface EndpointDetailModalProps {
  endpoint: DashboardEndpoint | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

function getStatusBadgeVariant(
  status: DashboardEndpoint['status']
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

function getStatusLabel(
  status: DashboardEndpoint['status'],
  t: (key: string, opts?: Record<string, unknown>) => string
): string {
  switch (status) {
    case 'online':
      return t('endpointDetail.status.online')
    case 'pending':
      return t('endpointDetail.status.pending')
    case 'offline':
      return t('endpointDetail.status.offline')
    case 'error':
      return t('endpointDetail.status.error')
    default:
      return status
  }
}

function getTypeLabel(
  type: EndpointType | undefined,
  t: (key: string, opts?: Record<string, unknown>) => string
): string {
  switch (type) {
    case 'xllm':
      return 'xLLM'
    case 'ollama':
      return 'Ollama'
    case 'vllm':
      return 'vLLM'
    case 'lm_studio':
      return 'LM Studio'
    case 'llamacpp':
      return 'llama.cpp'
    case 'openai_compatible':
      return t('endpointDetail.type.openaiCompatible')
    case 'unknown':
      return t('endpointDetail.type.unknown')
    default:
      return '-'
  }
}

function getTypeBadgeVariant(
  type: EndpointType | undefined
): 'default' | 'secondary' | 'outline' {
  switch (type) {
    case 'xllm':
      return 'default'
    case 'ollama':
    case 'vllm':
    case 'lm_studio':
    case 'llamacpp':
      return 'secondary'
    default:
      return 'outline'
  }
}

export function EndpointDetailModal({ endpoint, open, onOpenChange }: EndpointDetailModalProps) {
  if (!endpoint) return null

  return (
    <EndpointDetailModalContent
      key={endpoint.id}
      endpoint={endpoint}
      open={open}
      onOpenChange={onOpenChange}
    />
  )
}

interface EndpointDetailModalContentProps {
  endpoint: DashboardEndpoint
  open: boolean
  onOpenChange: (open: boolean) => void
}

function EndpointDetailModalContent({
  endpoint,
  open,
  onOpenChange,
}: EndpointDetailModalContentProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const errorDisplay = classifyEndpointLastError(endpoint?.last_error)
  const [name, setName] = useState(endpoint?.name || '')
  const [notes, setNotes] = useState(endpoint?.notes || '')
  const [healthCheckInterval, setHealthCheckInterval] = useState(
    endpoint?.health_check_interval_secs?.toString() || '30'
  )
  const [inferenceTimeout, setInferenceTimeout] = useState(
    endpoint?.inference_timeout_secs?.toString()
      || getRecommendedInferenceTimeout(endpoint?.endpoint_type).toString()
  )
  const [downloadDialogOpen, setDownloadDialogOpen] = useState(false)
  const recommendedInferenceTimeout = getRecommendedInferenceTimeout(endpoint?.endpoint_type)
  const recommendedInferenceTimeoutLabel = getRecommendedInferenceTimeoutLabel(
    endpoint?.endpoint_type
  )

  // SPEC-8c32349f: Fetch today's request statistics
  const { data: todayStats, isLoading: isLoadingTodayStats } = useQuery({
    queryKey: ['endpoint-today-stats', endpoint?.id],
    queryFn: () => endpointsApi.getTodayStats(endpoint.id),
    enabled: !!endpoint?.id && open,
  })

  const openPlayground = () => {
    if (endpoint) {
      window.location.hash = `playground/${endpoint.id}`
      onOpenChange(false)
    }
  }

  // Update mutation
  const updateMutation = useMutation({
    mutationFn: (data: Parameters<typeof endpointsApi.update>[1]) =>
      endpointsApi.update(endpoint.id, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['dashboard-endpoints'] })
      toast({
        title: t('endpointDetail.toast.updateComplete'),
        description: t('endpointDetail.toast.updateCompleteDescription'),
      })
    },
    onError: (error) => {
      toast({
        title: t('endpointDetail.toast.updateFailed'),
        description: String(error),
        variant: 'destructive',
      })
    },
  })

  // Test connection mutation
  const testMutation = useMutation({
    mutationFn: () => endpointsApi.test(endpoint.id),
    onSuccess: (result) => {
      queryClient.invalidateQueries({ queryKey: ['dashboard-endpoints'] })
      toast({
        title: result.success
          ? t('endpointDetail.toast.connectionSuccessful')
          : t('endpointDetail.toast.connectionFailed'),
        description:
          result.message
          || (result.latency_ms
            ? t('endpointDetail.toast.latency', { value: result.latency_ms })
            : ''),
        variant: result.success ? 'default' : 'destructive',
      })
    },
    onError: (error) => {
      toast({
        title: t('endpointDetail.toast.connectionTestFailed'),
        description: String(error),
        variant: 'destructive',
      })
    },
  })

  // Sync models mutation
  const syncMutation = useMutation({
    mutationFn: () => endpointsApi.sync(endpoint.id),
    onSuccess: (result) => {
      queryClient.invalidateQueries({ queryKey: ['dashboard-endpoints'] })
      toast({
        title: t('endpointDetail.toast.syncComplete'),
        description: t('endpointDetail.toast.syncedModels', { value: result.synced_models }),
      })
    },
    onError: (error) => {
      toast({
        title: t('endpointDetail.toast.syncFailed'),
        description: String(error),
        variant: 'destructive',
      })
    },
  })

  const handleSave = () => {
    updateMutation.mutate({
      name: name !== endpoint?.name ? name : undefined,
      notes: notes !== endpoint?.notes ? notes : undefined,
      health_check_interval_secs:
        parseInt(healthCheckInterval) !== endpoint?.health_check_interval_secs
          ? parseInt(healthCheckInterval)
          : undefined,
      inference_timeout_secs:
        parseInt(inferenceTimeout) !== endpoint?.inference_timeout_secs
          ? parseInt(inferenceTimeout)
          : undefined,
    })
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Server className="h-5 w-5" />
            {endpoint.name}
          </DialogTitle>
          <DialogDescription>{endpoint.base_url}</DialogDescription>
        </DialogHeader>

        <ScrollArea className="max-h-[calc(100vh-12rem)]">
        <div className="space-y-6 py-4">
          {/* Status Section */}
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-4">
              <Badge variant={getStatusBadgeVariant(endpoint.status)}>
                {getStatusLabel(endpoint.status, t)}
              </Badge>
              <Badge variant={getTypeBadgeVariant(endpoint.endpoint_type)}>
                {getTypeLabel(endpoint.endpoint_type, t)}
              </Badge>
              <span className="text-xs text-muted-foreground">
                {t('endpointDetail.typeAutoDetected')}
              </span>
              <span className="text-sm text-muted-foreground">
                {t('endpointDetail.modelsCount', { value: endpoint.model_count })}
              </span>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={() => testMutation.mutate()}
                disabled={testMutation.isPending}
              >
                <Play className="h-4 w-4 mr-1" />
                {t('endpointDetail.testConnection')}
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => syncMutation.mutate()}
                disabled={syncMutation.isPending || endpoint.status !== 'online'}
              >
                <RefreshCw className={`h-4 w-4 mr-1 ${syncMutation.isPending ? 'animate-spin' : ''}`} />
                {t('endpointDetail.syncModels')}
              </Button>
            </div>
          </div>

          <Separator />

          {/* SPEC-8c32349f: Request Statistics Cards */}
          <div className="grid grid-cols-2 gap-4">
            {/* Total Requests */}
            <div className="rounded-lg border p-3">
              <div className="flex items-center gap-1.5 mb-1">
                <Activity className="h-3.5 w-3.5 text-muted-foreground" />
                <span className="text-xs text-muted-foreground">{t('endpointDetail.stats.totalRequests')}</span>
              </div>
              <span className="text-xl font-bold">
                {endpoint.total_requests > 0
                  ? endpoint.total_requests.toLocaleString()
                  : '-'}
              </span>
            </div>

            {/* Today's Requests */}
            <div className="rounded-lg border p-3">
              <div className="flex items-center gap-1.5 mb-1">
                <Activity className="h-3.5 w-3.5 text-muted-foreground" />
                <span className="text-xs text-muted-foreground">{t('endpointDetail.stats.today')}</span>
              </div>
              {isLoadingTodayStats ? (
                <div className="h-7 w-16 rounded bg-muted animate-pulse" />
              ) : (
                <span className="text-xl font-bold">
                  {todayStats && todayStats.total_requests > 0
                    ? todayStats.total_requests.toLocaleString()
                    : '-'}
                </span>
              )}
            </div>

            {/* Success Rate */}
            <div className="rounded-lg border p-3">
              <div className="flex items-center gap-1.5 mb-1">
                <Activity className="h-3.5 w-3.5 text-muted-foreground" />
                <span className="text-xs text-muted-foreground">{t('endpointDetail.stats.successRate')}</span>
              </div>
              {(() => {
                const total = endpoint.total_requests
                if (total === 0) {
                  return <span className="text-xl font-bold">-</span>
                }
                const successRate = (endpoint.successful_requests / total) * 100
                const errorRate = 100 - successRate
                let colorClass = ''
                if (errorRate >= 20) {
                  colorClass = 'text-red-600'
                } else if (errorRate >= 5) {
                  colorClass = 'text-yellow-600'
                }
                return (
                  <span className={`text-xl font-bold ${colorClass}`}>
                    {successRate.toFixed(1)}%
                  </span>
                )
              })()}
            </div>

            {/* Average Response Time */}
            <div className="rounded-lg border p-3">
              <div className="flex items-center gap-1.5 mb-1">
                <Clock className="h-3.5 w-3.5 text-muted-foreground" />
                <span className="text-xs text-muted-foreground">{t('endpointDetail.stats.avgResponse')}</span>
              </div>
              <span className="text-xl font-bold">
                {endpoint.latency_ms != null ? `${endpoint.latency_ms}ms` : '-'}
              </span>
            </div>
          </div>

          <Separator />

          {/* SPEC-8c32349f: Daily Request Trend Chart (Phase 6) */}
          <EndpointRequestChart endpointId={endpoint.id} />

          <Separator />

          {/* Info Section */}
          <div className="grid grid-cols-2 gap-4 text-sm">
            <div>
              <span className="text-muted-foreground">{t('endpointDetail.info.latency')}</span>
              <span className="ml-2">{endpoint.latency_ms != null ? `${endpoint.latency_ms}ms` : '-'}</span>
            </div>
            <div>
              <span className="text-muted-foreground">{t('endpointDetail.info.registered')}</span>
              <span className="ml-2">{formatRelativeTime(endpoint.registered_at)}</span>
            </div>
            <div>
              <span className="text-muted-foreground">{t('endpointDetail.info.lastSeen')}</span>
              <span className="ml-2">
                {endpoint.last_seen ? formatRelativeTime(endpoint.last_seen) : '-'}
              </span>
            </div>
            <div>
              <span className="text-muted-foreground">{t('endpointDetail.info.errorCount')}</span>
              <span className="ml-2">{endpoint.error_count}</span>
            </div>
          </div>

          {/* Error Message */}
          {endpoint.last_error && (
            <div className="bg-destructive/10 border border-destructive/20 rounded-md p-3">
              <div className="flex items-center gap-2 text-destructive">
                <AlertCircle className="h-4 w-4" />
                <span className="font-medium">{t('endpointDetail.lastError')}</span>
                {errorDisplay && (
                  <Badge variant="outline" className="border-destructive/40 text-destructive">
                    {errorDisplay.label}
                  </Badge>
                )}
              </div>
              <p className="text-sm text-destructive/80 mt-1">{endpoint.last_error}</p>
            </div>
          )}

          <Separator />

          {/* SPEC-8c32349f + SPEC-4bb5b55f: Unified Models Table with TPS and Stats */}
          <EndpointModelsTable
            endpointId={endpoint.id}
            enabled={open}
            headerActions={
              <div className="flex items-center gap-2">
                {endpoint.endpoint_type === 'xllm' && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setDownloadDialogOpen(true)}
                    disabled={endpoint.status !== 'online'}
                  >
                    <Download className="h-4 w-4 mr-1" />
                    {t('endpointDetail.downloadModel')}
                  </Button>
                )}
                <Button
                  variant="default"
                  size="sm"
                  onClick={openPlayground}
                  disabled={endpoint.status !== 'online'}
                >
                  <MessageSquare className="h-4 w-4 mr-1" />
                  {t('endpointDetail.openPlayground')}
                </Button>
              </div>
            }
          />

          <Separator />

          {/* Edit Section */}
          <div className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="name">{t('endpointDetail.displayName')}</Label>
              <Input
                id="name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={t('endpointDetail.endpointNamePlaceholder')}
              />
            </div>

            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label htmlFor="healthCheckInterval">
                  <Clock className="h-4 w-4 inline mr-1" />
                  {t('endpointDetail.healthCheckInterval')}
                </Label>
                <Input
                  id="healthCheckInterval"
                  type="number"
                  min="5"
                  max="3600"
                  value={healthCheckInterval}
                  onChange={(e) => setHealthCheckInterval(e.target.value)}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="inferenceTimeout">
                  <Clock className="h-4 w-4 inline mr-1" />
                  {t('endpointDetail.inferenceTimeout')}
                </Label>
                <Input
                  id="inferenceTimeout"
                  type="number"
                  min="10"
                  max="600"
                  value={inferenceTimeout}
                  onChange={(e) => setInferenceTimeout(e.target.value)}
                />
                <p className="text-xs text-muted-foreground">
                  {recommendedInferenceTimeoutLabel}
                </p>
                {inferenceTimeout !== recommendedInferenceTimeout.toString() && (
                  <p className="text-xs text-muted-foreground">
                    {t('endpointDetail.inferenceTimeoutDiffers')}
                  </p>
                )}
              </div>
            </div>

            <div className="space-y-2">
              <Label htmlFor="notes">{t('endpointDetail.notes')}</Label>
              <Textarea
                id="notes"
                value={notes}
                onChange={(e) => setNotes(e.target.value)}
                placeholder={t('endpointDetail.notesPlaceholder')}
                rows={3}
              />
            </div>
          </div>
        </div>
        </ScrollArea>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('endpointDetail.close')}
          </Button>
          <Button onClick={handleSave} disabled={updateMutation.isPending}>
            <Save className="h-4 w-4 mr-1" />
            {updateMutation.isPending ? t('endpointDetail.saving') : t('endpointDetail.save')}
          </Button>
        </DialogFooter>
      </DialogContent>

      {/* xLLM Model Download Dialog */}
      <ModelDownloadDialog
        endpoint={endpoint}
        open={downloadDialogOpen}
        onOpenChange={setDownloadDialogOpen}
      />
    </Dialog>
  )
}
