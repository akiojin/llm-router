import { useState, useRef, useCallback, useMemo, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  dashboardApi,
  modelsApi,
  systemApi,
  type SystemInfo,
  type UpdateState,
  type ScheduleInfo,
  type DashboardOverview,
  type DashboardEndpoint,
  type RequestHistoryItem,
  type RequestResponsesPage,
  type RegisteredModelView,
  type ModelsView,
  type VersionResponse,
} from '@/lib/api'
import { useAuth } from '@/hooks/useAuth'
import { useDashboardWebSocket } from '@/hooks/useWebSocket'
import { toast } from '@/hooks/use-toast'
import { Header } from '@/components/dashboard/Header'
import { OperationsOverview } from '@/components/dashboard/OperationsOverview'
import { EndpointTable } from '@/components/dashboard/EndpointTable'
import { ModelsTable } from '@/components/dashboard/ModelsTable'
import { RequestHistoryTable } from '@/components/dashboard/RequestHistoryTable'
import { LogViewer } from '@/components/dashboard/LogViewer'
import { TokenStatsSection } from '@/components/dashboard/TokenStatsSection'
import { ClientsTab } from '@/components/dashboard/ClientsTab'
import { Button } from '@/components/ui/button'
import { Progress } from '@/components/ui/progress'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  DialogDescription,
} from '@/components/ui/dialog'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import {
  AlertCircle,
  AlertTriangle,
  Globe,
  History,
  FileText,
  BarChart3,
  ArrowUpCircle,
  ExternalLink,
  Loader2,
  RefreshCcw,
  Users,
  Settings,
  Undo2,
  Calendar,
  Clock,
  Zap,
  Package,
  ShieldCheck,
} from 'lucide-react'

const SYSTEM_INFO_QUERY_KEY = ['system-info'] as const
const CHECK_COOLDOWN_MS = 30_000

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)}KB`
  return `${(bytes / (1024 * 1024)).toFixed(0)}MB`
}

function formatCountdown(targetIso: string): string | null {
  const target = new Date(targetIso).getTime()
  const now = Date.now()
  const diffSec = Math.max(0, Math.floor((target - now) / 1000))
  if (diffSec <= 0) return '0:00'
  const min = Math.floor(diffSec / 60)
  const sec = diffSec % 60
  return `${min}:${sec.toString().padStart(2, '0')}`
}

export default function Dashboard() {
  const { t } = useTranslation()
  const { user } = useAuth()
  const isViewer = user?.role === 'viewer'
  const isAdmin = user?.role === 'admin'
  const { isConnected: wsConnected } = useDashboardWebSocket({ enabled: !isViewer })
  const queryClient = useQueryClient()
  const [lastRefreshed, setLastRefreshed] = useState<Date | null>(null)
  const [fetchTimeMs, setFetchTimeMs] = useState<number | null>(null)
  const fetchStartRef = useRef<number | null>(null)
  const [isApplyingUpdate, setIsApplyingUpdate] = useState(false)
  const [isApplyingForceUpdate, setIsApplyingForceUpdate] = useState(false)
  const [isForceUpdateDialogOpen, setIsForceUpdateDialogOpen] = useState(false)
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false)
  const [lastCheckTimestamp, setLastCheckTimestamp] = useState(0)
  const [isRollbackDialogOpen, setIsRollbackDialogOpen] = useState(false)
  const [isRollingBack, setIsRollingBack] = useState(false)
  const [isSettingsOpen, setIsSettingsOpen] = useState(false)
  const [scheduleMode, setScheduleMode] = useState<'immediate' | 'idle' | 'scheduled'>('immediate')
  const [scheduledAt, setScheduledAt] = useState('')
  const [isScheduling, setIsScheduling] = useState(false)
  const [drainCountdown, setDrainCountdown] = useState<string | null>(null)
  const [applyTimeoutCountdown, setApplyTimeoutCountdown] = useState<string | null>(null)
  const [activeTab, setActiveTab] = useState('endpoints')

  // Read tab parameter from URL search params and set activeTab
  useEffect(() => {
    const searchParams = new URLSearchParams(window.location.search)
    const tabParam = searchParams.get('tab')
    if (tabParam && ['endpoints', 'models', 'statistics', 'history', 'clients', 'logs'].includes(tabParam)) {
      setActiveTab(tabParam)
    }
  }, [])

  // When WebSocket is connected, reduce polling frequency
  const pollingInterval = wsConnected ? 10000 : 5000

  const fetchWithTiming = useCallback(async () => {
    fetchStartRef.current = performance.now()
    const result = await dashboardApi.getOverview()
    const endTime = performance.now()
    setFetchTimeMs(Math.round(endTime - (fetchStartRef.current || endTime)))
    setLastRefreshed(new Date())
    return result
  }, [])

  const { data, isLoading, error, refetch } = useQuery<DashboardOverview>({
    queryKey: ['dashboard-overview'],
    queryFn: fetchWithTiming,
    refetchInterval: pollingInterval,
  })

  const {
    data: systemInfo,
  } = useQuery<SystemInfo>({
    queryKey: SYSTEM_INFO_QUERY_KEY,
    queryFn: () => systemApi.getSystem(),
    refetchInterval: pollingInterval,
    enabled: !isViewer,
  })

  // Bug 3: /api/version は認証不要・軽量なので全ロールで常時取得し、
  // systemInfo 未取得時のフォールバックにする
  const { data: versionData } = useQuery<VersionResponse>({
    queryKey: ['version'],
    queryFn: () => systemApi.getVersion(),
    refetchInterval: pollingInterval,
  })

  // admin: systemInfo.version 優先, フォールバック: versionData.version
  const systemVersion = systemInfo?.version ?? versionData?.version ?? null

  // Drain timeout countdown timer
  useEffect(() => {
    const update = systemInfo?.update
    if (update?.state !== 'draining' || !update.timeout_at) {
      setDrainCountdown(null)
      return
    }
    const tick = () => setDrainCountdown(formatCountdown(update.timeout_at))
    tick()
    const timer = setInterval(tick, 1000)
    return () => clearInterval(timer)
  }, [systemInfo?.update])

  useEffect(() => {
    const update = systemInfo?.update
    if (update?.state !== 'applying' || !update.timeout_at) {
      setApplyTimeoutCountdown(null)
      return
    }
    const timeoutAt = update.timeout_at
    const tick = () => setApplyTimeoutCountdown(formatCountdown(timeoutAt))
    tick()
    const timer = setInterval(tick, 1000)
    return () => clearInterval(timer)
  }, [systemInfo?.update])

  // Fetch request history (individual request details)
  const { data: requestResponsesData, isLoading: isLoadingHistory } =
    useQuery<RequestResponsesPage>({
      queryKey: ['request-responses'],
      queryFn: () => dashboardApi.getRequestResponses({ limit: 100 }),
      refetchInterval: pollingInterval,
      enabled: !isViewer,
    })

  // SPEC-e8e9326e: Fetch endpoints list
  const { data: endpointsData, isLoading: isLoadingEndpoints } = useQuery<DashboardEndpoint[]>({
    queryKey: ['dashboard-endpoints'],
    queryFn: () => dashboardApi.getEndpoints(),
    refetchInterval: pollingInterval,
    enabled: !isViewer,
  })

  // US-029: モデル一覧の表示モード（canonical 集約 / detail 全 variant）
  const [modelsView, setModelsView] = useState<ModelsView>('canonical')

  const {
    data: viewerModels,
    isLoading: isLoadingViewerModels,
    refetch: refetchViewerModels,
  } = useQuery<RegisteredModelView[]>({
    queryKey: ['viewer-models', modelsView],
    queryFn: () => modelsApi.getRegistered(modelsView),
    refetchInterval: pollingInterval,
  })

  // Map RequestResponseRecord to RequestHistoryItem
  const historyItems: RequestHistoryItem[] = useMemo(() => {
    if (!requestResponsesData?.records) return []
    return requestResponsesData.records.map((record) => ({
      request_id: record.id,
      timestamp: record.timestamp,
      model: record.model,
      node_id: record.node_id,
      node_name: record.node_machine_name,
      status: record.status.type,
      duration_ms: record.duration_ms,
      error: record.status.type === 'error' ? record.status.message : undefined,
      request_body: record.request_body,
      response_body: record.response_body,
      client_ip: record.client_ip,
    }))
  }, [requestResponsesData])

  const updateBanner = useMemo(() => {
    const update = systemInfo?.update as UpdateState | undefined
    const updateState = update?.state
    const hasAvailableUpdate = updateState === 'available'
    const isPayloadReady =
      hasAvailableUpdate && update?.payload?.payload === 'ready'
    const failedHasUpdateCandidate = updateState === 'failed' && Boolean(update?.latest)
    const canApply = isAdmin && (updateState === 'available' || failedHasUpdateCandidate)
    const applying = updateState === 'draining' || updateState === 'applying'
    const showRestartButton = updateState === 'available' || failedHasUpdateCandidate || applying
    const showForceButton = hasAvailableUpdate
    const canForceApply = isAdmin && isPayloadReady && !applying
    const cooldownRemaining = Math.max(0, CHECK_COOLDOWN_MS - (Date.now() - lastCheckTimestamp))
    const isCooldown = cooldownRemaining > 0
    const canCheck = isAdmin && !applying && !isCooldown
    const forceUpdateTitle = !isAdmin
      ? 'Admin role is required'
      : applying
        ? 'Update is in progress'
        : !hasAvailableUpdate
          ? 'No update is available'
        : isPayloadReady
          ? undefined
          : 'Update payload is still preparing'

    const rollbackAvailable = systemInfo?.rollback_available === true
    const scheduleInfo = systemInfo?.schedule as ScheduleInfo | null | undefined

    let title = 'Update'
    let description = 'Update status unavailable'
    let link: string | null = null
    let payloadHint: string | null = null
    let downloadProgress: { downloaded_bytes: number; total_bytes: number } | null = null

    if (updateState === 'available' && update) {
      title = `Update available: v${update.latest}`
      description = `Current: v${update.current}`
      link = update.release_url
      if (update.payload?.payload === 'downloading') {
        const dl = update.payload
        if (dl.downloaded_bytes != null && dl.total_bytes != null && dl.total_bytes > 0) {
          downloadProgress = {
            downloaded_bytes: dl.downloaded_bytes,
            total_bytes: dl.total_bytes,
          }
          const pct = Math.round((dl.downloaded_bytes / dl.total_bytes) * 100)
          payloadHint = `Downloading: ${formatBytes(dl.downloaded_bytes)} / ${formatBytes(dl.total_bytes)} (${pct}%)`
        } else {
          payloadHint = 'Downloading...'
        }
      } else if (update.payload?.payload === 'ready') {
        payloadHint = 'Ready'
      } else if (update.payload?.payload === 'error') {
        payloadHint = 'Download failed'
      } else {
        payloadHint = 'Preparing...'
      }
    } else if (updateState === 'up_to_date' && update) {
      title = 'Up to date'
      const checkedAt = update.checked_at ?? null
      if (checkedAt) {
        const asDate = new Date(checkedAt)
        description = `Last checked: ${Number.isNaN(asDate.valueOf()) ? checkedAt : asDate.toLocaleString()}`
      } else {
        description = 'Last checked: unknown'
      }
    } else if (updateState === 'draining' && update) {
      title = `Updating to v${update.latest}`
      description = `Waiting for in-flight requests: ${update.in_flight}`
    } else if (updateState === 'applying' && update) {
      title = `Applying update: v${update.latest}`
      description = update.phase_message ?? 'Restarting...'
    } else if (updateState === 'failed' && update) {
      title = 'Update failed'
      description = update.message
      link = update.release_url || null
    }

    const onCheck = async () => {
      setIsCheckingUpdate(true)
      setLastCheckTimestamp(Date.now())
      try {
        const { update } = await systemApi.checkUpdate()
        const currentSystemInfo = queryClient.getQueryData<SystemInfo>(SYSTEM_INFO_QUERY_KEY)
        if (currentSystemInfo) {
          queryClient.setQueryData<SystemInfo>(
            SYSTEM_INFO_QUERY_KEY,
            {
              ...currentSystemInfo,
              update,
            }
          )
        } else {
          const freshSystemInfo = await systemApi.getSystem()
          queryClient.setQueryData<SystemInfo>(
            SYSTEM_INFO_QUERY_KEY,
            {
              ...freshSystemInfo,
              update,
            }
          )
        }
        toast({
          title: 'Checked for updates',
        })
      } catch (e) {
        toast({
          title: 'Update check failed',
          description: e instanceof Error ? e.message : String(e),
          variant: 'destructive',
        })
      } finally {
        setIsCheckingUpdate(false)
        await queryClient.invalidateQueries({ queryKey: SYSTEM_INFO_QUERY_KEY })
      }
    }

    const onApply = async () => {
      setIsApplyingUpdate(true)
      try {
        const result = await systemApi.applyUpdate()
        if (result.queued) {
          toast({
            title: 'Update queued',
            description:
              'llmlb will restart after in-flight requests complete.',
          })
        } else {
          toast({
            title: 'Applying update',
            description: 'llmlb is restarting now.',
          })
        }
        await queryClient.invalidateQueries({ queryKey: SYSTEM_INFO_QUERY_KEY })
      } catch (e) {
        toast({
          title: 'Failed to apply update',
          description: e instanceof Error ? e.message : String(e),
          variant: 'destructive',
        })
      } finally {
        setIsApplyingUpdate(false)
      }
    }

    const onForceApply = async () => {
      setIsApplyingForceUpdate(true)
      try {
        const result = await systemApi.applyForceUpdate()
        toast({
          title: 'Force update started',
          description:
            result.dropped_in_flight > 0
              ? `${result.dropped_in_flight} in-flight request(s) were terminated.`
              : 'No in-flight requests were active.',
        })
        setIsForceUpdateDialogOpen(false)
        await queryClient.invalidateQueries({ queryKey: SYSTEM_INFO_QUERY_KEY })
      } catch (e) {
        toast({
          title: 'Failed to force update',
          description: e instanceof Error ? e.message : String(e),
          variant: 'destructive',
        })
      } finally {
        setIsApplyingForceUpdate(false)
      }
    }

    const onRollback = async () => {
      setIsRollingBack(true)
      try {
        await systemApi.rollback()
        toast({
          title: 'Rolling back',
          description: 'Restoring previous version and restarting...',
        })
        setIsRollbackDialogOpen(false)
        await queryClient.invalidateQueries({ queryKey: SYSTEM_INFO_QUERY_KEY })
      } catch (e) {
        toast({
          title: 'Rollback failed',
          description: e instanceof Error ? e.message : String(e),
          variant: 'destructive',
        })
      } finally {
        setIsRollingBack(false)
      }
    }

    const onSchedule = async () => {
      setIsScheduling(true)
      try {
        if (scheduleMode === 'immediate') {
          await onApply()
        } else {
          await systemApi.createSchedule({
            mode: scheduleMode,
            scheduled_at: scheduleMode === 'scheduled' ? scheduledAt : undefined,
          })
          toast({ title: 'Schedule created' })
        }
        setIsSettingsOpen(false)
        await queryClient.invalidateQueries({ queryKey: SYSTEM_INFO_QUERY_KEY })
      } catch (e) {
        toast({
          title: 'Failed to create schedule',
          description: e instanceof Error ? e.message : String(e),
          variant: 'destructive',
        })
      } finally {
        setIsScheduling(false)
      }
    }

    const onCancelSchedule = async () => {
      try {
        await systemApi.cancelSchedule()
        toast({ title: 'Schedule cancelled' })
        await queryClient.invalidateQueries({ queryKey: SYSTEM_INFO_QUERY_KEY })
      } catch (e) {
        toast({
          title: 'Failed to cancel schedule',
          description: e instanceof Error ? e.message : String(e),
          variant: 'destructive',
        })
      }
    }

    return (
      <section className="mb-6">
        <div className="rounded-2xl border border-border/60 bg-card/60 backdrop-blur-xl px-5 py-4 shadow-sm">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div className="flex items-start gap-3">
              <div className="mt-0.5 flex h-9 w-9 items-center justify-center rounded-xl bg-primary/10">
                {applying ? (
                  <Loader2 className="h-5 w-5 animate-spin text-primary" />
                ) : (
                  <ArrowUpCircle className="h-5 w-5 text-primary" />
                )}
              </div>
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <p className="font-medium leading-6">{title}</p>
                  {payloadHint && (
                    <span className="rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">
                      {payloadHint}
                    </span>
                  )}
                </div>
                {description && (
                  <p className="mt-0.5 text-sm text-muted-foreground">
                    {description}
                  </p>
                )}
                {downloadProgress && (
                  <div className="mt-2 w-64">
                    <Progress
                      value={Math.round(
                        (downloadProgress.downloaded_bytes / downloadProgress.total_bytes) * 100
                      )}
                    />
                  </div>
                )}
                {updateState === 'draining' && drainCountdown != null && (
                  <p className="mt-1 text-xs text-muted-foreground">
                    Drain timeout in {drainCountdown}
                  </p>
                )}
                {updateState === 'applying' && applyTimeoutCountdown != null && (
                  <p className="mt-1 text-xs text-muted-foreground">
                    Apply timeout in {applyTimeoutCountdown}
                  </p>
                )}
                {scheduleInfo && (
                  <p className="mt-1 text-xs text-muted-foreground">
                    Scheduled by {scheduleInfo.scheduled_by} ({scheduleInfo.mode})
                    {scheduleInfo.scheduled_at && (
                      <> at {new Date(scheduleInfo.scheduled_at).toLocaleString()}</>
                    )}
                    <button
                      type="button"
                      className="ml-2 text-destructive hover:underline"
                      onClick={() => void onCancelSchedule()}
                    >
                      Cancel
                    </button>
                  </p>
                )}
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-2">
              {link && (
                <a
                  href={link}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center gap-1 rounded-lg border border-border/60 bg-background/60 px-3 py-2 text-sm hover:bg-background"
                >
                  <ExternalLink className="h-4 w-4" />
                  Release
                </a>
              )}

              {/* Settings button */}
              {isAdmin && (
                <Dialog open={isSettingsOpen} onOpenChange={setIsSettingsOpen}>
                  <DialogTrigger asChild>
                    <Button variant="outline" size="icon" title="Settings">
                      <Settings className="h-4 w-4" />
                    </Button>
                  </DialogTrigger>
                  <DialogContent className="sm:max-w-lg">
                    <DialogHeader>
                      <DialogTitle>Settings</DialogTitle>
                      <DialogDescription>
                        Configure access control, update scheduling, and history.
                      </DialogDescription>
                    </DialogHeader>
                    <Tabs defaultValue="authentication">
                      <TabsList className="grid w-full grid-cols-3">
                        <TabsTrigger value="authentication">
                          <ShieldCheck className="mr-1.5 h-3.5 w-3.5" />
                          Auth
                        </TabsTrigger>
                        <TabsTrigger value="schedule">
                          <Calendar className="mr-1.5 h-3.5 w-3.5" />
                          Schedule
                        </TabsTrigger>
                        <TabsTrigger value="history">
                          <History className="mr-1.5 h-3.5 w-3.5" />
                          History
                        </TabsTrigger>
                      </TabsList>

                      <TabsContent value="authentication" className="space-y-4 pt-4">
                        <div className="rounded-lg border border-border p-4">
                          <div className="space-y-1">
                            <Label className="text-sm font-medium">Authentication required</Label>
                            <p className="text-sm text-muted-foreground">
                              All external and management APIs always require a valid API key or an
                              authenticated dashboard session. Unauthenticated requests receive 401.
                            </p>
                          </div>
                        </div>
                      </TabsContent>

                      <TabsContent value="schedule" className="space-y-4 pt-4">
                        <div className="space-y-3">
                          <Label
                            className={`flex items-center gap-2 cursor-pointer rounded-lg border p-3 ${scheduleMode === 'immediate' ? 'border-primary bg-primary/5' : 'border-border'}`}
                          >
                            <input
                              type="radio"
                              name="scheduleMode"
                              value="immediate"
                              checked={scheduleMode === 'immediate'}
                              onChange={() => setScheduleMode('immediate')}
                              className="accent-primary"
                            />
                            <Zap className="h-4 w-4" />
                            <span>Immediate</span>
                          </Label>
                          <Label
                            className={`flex items-center gap-2 cursor-pointer rounded-lg border p-3 ${scheduleMode === 'idle' ? 'border-primary bg-primary/5' : 'border-border'}`}
                          >
                            <input
                              type="radio"
                              name="scheduleMode"
                              value="idle"
                              checked={scheduleMode === 'idle'}
                              onChange={() => setScheduleMode('idle')}
                              className="accent-primary"
                            />
                            <Clock className="h-4 w-4" />
                            <span>When idle (in_flight = 0)</span>
                          </Label>
                          <Label
                            className={`flex items-center gap-2 cursor-pointer rounded-lg border p-3 ${scheduleMode === 'scheduled' ? 'border-primary bg-primary/5' : 'border-border'}`}
                          >
                            <input
                              type="radio"
                              name="scheduleMode"
                              value="scheduled"
                              checked={scheduleMode === 'scheduled'}
                              onChange={() => setScheduleMode('scheduled')}
                              className="accent-primary"
                            />
                            <Calendar className="h-4 w-4" />
                            <span>Scheduled time</span>
                          </Label>
                          {scheduleMode === 'scheduled' && (
                            <Input
                              type="datetime-local"
                              value={scheduledAt}
                              onChange={(e) => setScheduledAt(e.target.value)}
                              className="mt-2"
                            />
                          )}
                        </div>
                        <Button
                          className="w-full"
                          onClick={() => void onSchedule()}
                          disabled={isScheduling || (scheduleMode === 'scheduled' && !scheduledAt)}
                        >
                          {isScheduling && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                          {scheduleMode === 'immediate' ? 'Apply now' : 'Schedule'}
                        </Button>
                      </TabsContent>

                      <TabsContent value="history" className="pt-4">
                        <p className="text-sm text-muted-foreground">
                          Update history will be available after the first update completes.
                        </p>
                      </TabsContent>
                    </Tabs>
                  </DialogContent>
                </Dialog>
              )}

              <Button
                variant="outline"
                onClick={onCheck}
                disabled={!canCheck || isCheckingUpdate || isApplyingUpdate || isApplyingForceUpdate}
                title={
                  !isAdmin
                    ? 'Admin role is required'
                    : applying
                      ? 'Update is in progress'
                      : isCooldown
                        ? 'Please wait before checking again'
                        : undefined
                }
              >
                {isCheckingUpdate ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <RefreshCcw className="h-4 w-4" />
                )}
                Check for updates
              </Button>
              {showRestartButton && (
                <Button
                  onClick={onApply}
                  disabled={!canApply || isApplyingUpdate || isApplyingForceUpdate || applying}
                  title={
                    !isAdmin
                      ? 'Admin role is required'
                      : applying
                        ? 'Update is in progress'
                        : undefined
                  }
                >
                  {isApplyingUpdate || applying ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <ArrowUpCircle className="h-4 w-4" />
                  )}
                  {update?.state === 'draining'
                    ? `Waiting to update... (${update.in_flight})`
                    : update?.state === 'applying'
                      ? 'Applying update...'
                      : 'Restart to update'}
                </Button>
              )}
              {showForceButton && (
                <AlertDialog
                  open={isForceUpdateDialogOpen}
                  onOpenChange={setIsForceUpdateDialogOpen}
                >
                  <AlertDialogTrigger asChild>
                    <Button
                      variant="destructive"
                      disabled={!canForceApply || isApplyingUpdate || isApplyingForceUpdate}
                      title={forceUpdateTitle}
                    >
                      {isApplyingForceUpdate ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <AlertTriangle className="h-4 w-4" />
                      )}
                      Force update now
                    </Button>
                  </AlertDialogTrigger>
                  <AlertDialogContent>
                    <AlertDialogHeader>
                      <AlertDialogTitle>Force update now?</AlertDialogTitle>
                      <AlertDialogDescription>
                        In-flight inference requests will be terminated immediately and llmlb will restart.
                        Use this only for urgent maintenance.
                      </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                      <AlertDialogCancel disabled={isApplyingForceUpdate}>Cancel</AlertDialogCancel>
                      <AlertDialogAction
                        disabled={isApplyingForceUpdate}
                        onClick={(event) => {
                          event.preventDefault()
                          void onForceApply()
                        }}
                      >
                        {isApplyingForceUpdate ? (
                          <>
                            <Loader2 className="h-4 w-4 animate-spin" />
                            Applying...
                          </>
                        ) : (
                          'Force update'
                        )}
                      </AlertDialogAction>
                    </AlertDialogFooter>
                  </AlertDialogContent>
                </AlertDialog>
              )}

              {/* Rollback button */}
              {isAdmin && rollbackAvailable && (
                <AlertDialog
                  open={isRollbackDialogOpen}
                  onOpenChange={setIsRollbackDialogOpen}
                >
                  <AlertDialogTrigger asChild>
                    <Button
                      variant="outline"
                      disabled={!rollbackAvailable || applying}
                      title={
                        !rollbackAvailable
                          ? 'No previous version available'
                          : applying
                            ? 'Update is in progress'
                            : undefined
                      }
                    >
                      <Undo2 className="h-4 w-4" />
                      Rollback to previous version
                    </Button>
                  </AlertDialogTrigger>
                  <AlertDialogContent>
                    <AlertDialogHeader>
                      <AlertDialogTitle>Rollback to previous version?</AlertDialogTitle>
                      <AlertDialogDescription>
                        This will restore the previous binary and restart llmlb.
                        In-flight requests will complete before the rollback is applied.
                      </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                      <AlertDialogCancel disabled={isRollingBack}>Cancel</AlertDialogCancel>
                      <AlertDialogAction
                        disabled={isRollingBack}
                        onClick={(event) => {
                          event.preventDefault()
                          void onRollback()
                        }}
                      >
                        {isRollingBack ? (
                          <>
                            <Loader2 className="h-4 w-4 animate-spin" />
                            Rolling back...
                          </>
                        ) : (
                          'Rollback'
                        )}
                      </AlertDialogAction>
                    </AlertDialogFooter>
                  </AlertDialogContent>
                </AlertDialog>
              )}
            </div>
          </div>
        </div>
      </section>
    )
  }, [
    systemInfo?.update,
    systemInfo?.rollback_available,
    systemInfo?.schedule,
    user?.role,
    isApplyingUpdate,
    isApplyingForceUpdate,
    isForceUpdateDialogOpen,
    isCheckingUpdate,
    lastCheckTimestamp,
    isRollbackDialogOpen,
    isRollingBack,
    isSettingsOpen,
    scheduleMode,
    scheduledAt,
    isScheduling,
    drainCountdown,
    applyTimeoutCountdown,
    queryClient,
  ])

  if (error) {
    return (
      <div className="flex h-screen w-full items-center justify-center bg-background">
        <div className="flex flex-col items-center gap-4 text-center">
          <div className="flex h-16 w-16 items-center justify-center rounded-full bg-destructive/10">
            <AlertCircle className="h-8 w-8 text-destructive" />
          </div>
          <div>
            <h2 className="text-lg font-semibold">Failed to load dashboard</h2>
            <p className="mt-1 text-sm text-muted-foreground">
              {error instanceof Error ? error.message : 'An error occurred'}
            </p>
          </div>
          <Button variant="link" onClick={() => refetch()}>
            Try again
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className="min-h-screen bg-background">
      {/* Background Grid */}
      <div className="fixed inset-0 bg-grid opacity-20 pointer-events-none" />

      {/* Header */}
      <Header
        user={user}
        isConnected={!error}
        lastRefreshed={lastRefreshed}
        fetchTimeMs={fetchTimeMs}
        systemVersion={systemVersion}
        updateState={(systemInfo?.update as UpdateState | undefined)?.state}
        updateLatest={(() => {
          const u = systemInfo?.update as UpdateState | undefined
          if (!u) return null
          if ('latest' in u) return u.latest ?? null
          return null
        })()}
        minimalViewer={isViewer}
      />

      {/* Main Content */}
      <main className="relative mx-auto max-w-[1600px] px-4 py-6 sm:px-6 lg:px-8">
        {!isViewer && updateBanner}
        <section className="mb-8">
          <OperationsOverview overview={data} isLoading={isLoading} />
        </section>

        {isViewer ? (
          <section className="mb-8">
            <ModelsTable
              models={viewerModels || []}
              endpoints={endpointsData || []}
              isLoading={isLoadingViewerModels}
              onRefresh={() => {
                void refetchViewerModels()
              }}
              view={modelsView}
              onViewChange={setModelsView}
              viewerMode
            />
          </section>
        ) : (
          <Tabs value={activeTab} onValueChange={setActiveTab} className="space-y-6">
            <TabsList className="grid w-full grid-cols-6 lg:w-auto lg:inline-grid">
              <TabsTrigger value="endpoints" className="gap-2">
                <Globe className="h-4 w-4" />
                <span className="hidden sm:inline">{t('dashboard.tabs.endpoints')}</span>
              </TabsTrigger>
              <TabsTrigger value="models" className="gap-2">
                <Package className="h-4 w-4" />
                <span className="hidden sm:inline">{t('dashboard.tabs.models')}</span>
              </TabsTrigger>
              <TabsTrigger value="statistics" className="gap-2">
                <BarChart3 className="h-4 w-4" />
                <span className="hidden sm:inline">{t('dashboard.tabs.usage')}</span>
              </TabsTrigger>
              <TabsTrigger value="history" className="gap-2">
                <History className="h-4 w-4" />
                <span className="hidden sm:inline">{t('dashboard.tabs.requests')}</span>
              </TabsTrigger>
              <TabsTrigger value="clients" className="gap-2">
                <Users className="h-4 w-4" />
                <span className="hidden sm:inline">{t('dashboard.tabs.traffic')}</span>
              </TabsTrigger>
              <TabsTrigger value="logs" className="gap-2">
                <FileText className="h-4 w-4" />
                <span className="hidden sm:inline">{t('dashboard.tabs.system')}</span>
              </TabsTrigger>
            </TabsList>

            <TabsContent value="endpoints" className="animate-fade-in">
              <EndpointTable
                endpoints={endpointsData || []}
                isLoading={isLoadingEndpoints}
              />
            </TabsContent>

            <TabsContent value="models" className="animate-fade-in">
              <ModelsTable
                models={viewerModels || []}
                endpoints={endpointsData || []}
                isLoading={isLoadingViewerModels}
                onRefresh={() => { void refetchViewerModels() }}
                view={modelsView}
                onViewChange={setModelsView}
              />
            </TabsContent>

            <TabsContent value="statistics" className="animate-fade-in">
              <TokenStatsSection />
            </TabsContent>

            <TabsContent value="history" className="animate-fade-in">
              <RequestHistoryTable
                history={historyItems}
                isLoading={isLoadingHistory}
              />
            </TabsContent>

            <TabsContent value="clients" className="animate-fade-in">
              <ClientsTab />
            </TabsContent>

            <TabsContent value="logs" className="animate-fade-in">
              <LogViewer />
            </TabsContent>
          </Tabs>
        )}
      </main>
    </div>
  )
}
