import { useState, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  type RegisteredModelView,
  type DashboardEndpoint,
  type ModelsView,
  type LifecycleStatus,
  type ModelCapabilities,
  type ModelStatEntry,
  type ModelTpsEntry,
  endpointsApi,
  dashboardApi,
} from '@/lib/api'
import { formatBytes } from '@/lib/utils'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import {
  Package,
  Search,
  RefreshCw,
  ChevronRight,
  ChevronDown,
  ChevronUp,
  MessageSquare,
  FileText,
  Layers,
  Settings,
  Cpu,
  Volume2,
  Mic,
  Image,
  Settings2,
  Play,
  Filter,
  Plus,
  Trash2,
  Server,
} from 'lucide-react'
import { ModelAddWizard } from './ModelAddWizard'
import { ModelDeleteDialog } from './ModelDeleteDialog'

/**
 * SPEC-8795f98f: Models Tab
 */

interface ModelsTableProps {
  models: RegisteredModelView[]
  endpoints: DashboardEndpoint[]
  isLoading: boolean
  onRefresh?: () => void
  viewerMode?: boolean
  /** US-029: 表示モード（canonical 集約 / detail 全 variant） */
  view?: ModelsView
  /** US-029: 表示モード切替コールバック */
  onViewChange?: (view: ModelsView) => void
}

type SortField =
  | 'id'
  | 'bestStatus'
  | 'endpointCount'
  | 'totalRequests'
type SortDirection = 'asc' | 'desc'
type SupportedApi =
  | 'chat_completions'
  | 'completions'
  | 'responses'
  | 'embeddings'
  | 'fine_tune'
  | 'inference'
  | 'audio_speech'
  | 'audio_transcription'
  | 'image_input'
  | 'image_generation'

interface AggregatedModel {
  id: string
  bestStatus: LifecycleStatus
  ready: boolean
  supportedApis: SupportedApi[]
  maxTokens?: number | null
  source?: string
  tags: string[]
  description?: string
  repo?: string
  filename?: string
  requiredMemoryBytes?: number
  chatTemplate?: string
  endpointIds: string[]
  endpointCount: number
}

function emptyCapabilities(): ModelCapabilities {
  return {
    chat_completion: false,
    completion: false,
    embeddings: false,
    fine_tune: false,
    inference: false,
    text_to_speech: false,
    speech_to_text: false,
    image_input: false,
    image_generation: false,
  }
}

function normalizeSupportedApi(api: string): SupportedApi | null {
  switch (api) {
    case 'chat':
    case 'chat_completion':
    case 'chat_completions':
      return 'chat_completions'
    case 'completion':
    case 'completions':
      return 'completions'
    case 'response':
    case 'responses':
      return 'responses'
    case 'embedding':
    case 'embeddings':
      return 'embeddings'
    case 'fine_tune':
    case 'fine_tuning':
      return 'fine_tune'
    case 'inference':
      return 'inference'
    case 'text_to_speech':
    case 'tts':
    case 'audio_speech':
      return 'audio_speech'
    case 'speech_to_text':
    case 'asr':
    case 'audio_transcription':
    case 'audio_transcriptions':
      return 'audio_transcription'
    case 'image':
    case 'images':
    case 'image_input':
    case 'vision':
    case 'visual':
    case 'multimodal':
      return 'image_input'
    case 'image_generation':
    case 'images_generations':
      return 'image_generation'
    default:
      return null
  }
}

function uniqueApis(apis: SupportedApi[]): SupportedApi[] {
  return Array.from(new Set(apis))
}

function supportedApisFromCapabilities(capabilities?: ModelCapabilities): SupportedApi[] {
  const caps = capabilities ?? emptyCapabilities()
  return uniqueApis([
    ...(caps.chat_completion ? ['chat_completions' as const] : []),
    ...(caps.completion ? ['completions' as const] : []),
    ...(caps.embeddings ? ['embeddings' as const] : []),
    ...(caps.fine_tune ? ['fine_tune' as const] : []),
    ...(caps.inference ? ['inference' as const] : []),
    ...(caps.text_to_speech ? ['audio_speech' as const] : []),
    ...(caps.speech_to_text ? ['audio_transcription' as const] : []),
    ...(caps.image_input ? ['image_input' as const] : []),
    ...(caps.image_generation ? ['image_generation' as const] : []),
  ])
}

function normalizeSupportedApis(
  supportedApis?: string[],
  capabilities?: ModelCapabilities
): SupportedApi[] {
  const apis = uniqueApis(
    (supportedApis ?? [])
      .map(normalizeSupportedApi)
      .filter((api): api is SupportedApi => api != null)
  )
  return apis.length > 0 ? apis : supportedApisFromCapabilities(capabilities)
}

function aggregateModels(models: RegisteredModelView[]): AggregatedModel[] {
  return models.map((model) => {
    const endpointIds = model.endpoint_ids ?? []
    return {
      id: model.name,
      bestStatus: model.lifecycle_status,
      ready: model.ready,
      supportedApis: normalizeSupportedApis(model.supported_apis, model.capabilities),
      maxTokens: undefined,
      source: model.source,
      tags: model.tags ?? [],
      description: model.description,
      repo: model.repo,
      filename: model.filename,
      requiredMemoryBytes:
        typeof model.required_memory_gb === 'number'
          ? Math.round(model.required_memory_gb * 1024 * 1024 * 1024)
          : undefined,
      chatTemplate: model.chat_template,
      endpointIds,
      endpointCount: endpointIds.length,
    }
  })
}

const LIFECYCLE_PRIORITY: Record<LifecycleStatus, number> = {
  registered: 4,
  caching: 3,
  pending: 2,
  error: 1,
}

function getLifecycleBadgeVariant(
  status: LifecycleStatus
): 'online' | 'pending' | 'destructive' {
  switch (status) {
    case 'registered':
      return 'online'
    case 'caching':
    case 'pending':
      return 'pending'
    case 'error':
      return 'destructive'
  }
}

function getLifecycleLabel(status: LifecycleStatus): string {
  return status.charAt(0).toUpperCase() + status.slice(1)
}

const SUPPORTED_API_BADGES: {
  key: SupportedApi
  icon: typeof MessageSquare
  label: string
}[] = [
  { key: 'chat_completions', icon: MessageSquare, label: 'Chat' },
  { key: 'completions', icon: FileText, label: 'Completion' },
  { key: 'responses', icon: MessageSquare, label: 'Responses' },
  { key: 'embeddings', icon: Layers, label: 'Embed' },
  { key: 'fine_tune', icon: Settings, label: 'Tune' },
  { key: 'inference', icon: Cpu, label: 'Infer' },
  { key: 'audio_speech', icon: Volume2, label: 'TTS' },
  { key: 'audio_transcription', icon: Mic, label: 'STT' },
  { key: 'image_input', icon: Image, label: 'Image' },
  { key: 'image_generation', icon: Image, label: 'Image Gen' },
]

interface ColumnDef {
  key: string
  label: string
  defaultVisible: boolean
  render: (model: AggregatedModel) => React.ReactNode
}

function SupportedApiBadges({ apis }: { apis: SupportedApi[] }) {
  const active = SUPPORTED_API_BADGES.filter((api) => apis.includes(api.key))
  if (active.length === 0) {
    return <span className="text-xs text-muted-foreground">Not reported</span>
  }
  return (
    <TooltipProvider>
      <div className="flex gap-1 flex-wrap">
        {active.map(({ key, icon: Icon, label }) => (
          <Tooltip key={key}>
            <TooltipTrigger asChild>
              <Badge variant="outline" className="gap-1 px-2 py-0.5">
                <Icon className="h-3 w-3" />
                <span>{label}</span>
              </Badge>
            </TooltipTrigger>
            <TooltipContent>{label}</TooltipContent>
          </Tooltip>
        ))}
      </div>
    </TooltipProvider>
  )
}

function TrafficCell({ stat }: { stat?: ModelStatEntry }) {
  const total = stat?.total_requests ?? 0
  const successful = stat?.successful_requests ?? 0
  const failed = stat?.failed_requests ?? 0
  if (total === 0) {
    return <span className="text-sm tabular-nums text-muted-foreground">0</span>
  }
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="text-sm tabular-nums">{total.toLocaleString()}</span>
        </TooltipTrigger>
        <TooltipContent>
          <div className="text-xs space-y-0.5">
            <div className="text-green-400">OK: {successful.toLocaleString()}</div>
            <div className="text-red-400">Fail: {failed.toLocaleString()}</div>
          </div>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

function EndpointCountCell({ count }: { count: number }) {
  return (
    <div className="flex items-center gap-2">
      <Server className="h-3.5 w-3.5 text-muted-foreground" />
      <span className={count === 0 ? 'text-sm tabular-nums text-muted-foreground' : 'text-sm tabular-nums'}>
        {count.toLocaleString()}
      </span>
    </div>
  )
}

function formatApiKindLabel(apiKind: ModelTpsEntry['api_kind']): string {
  switch (apiKind) {
    case 'chat_completions':
      return 'chat'
    case 'completions':
      return 'completion'
    case 'responses':
      return 'responses'
    default:
      return apiKind
  }
}

function formatTps(tps: number | null): string {
  if (tps == null) return '-'
  return `${tps.toFixed(1)} tok/s`
}

function EndpointStatsRow({
  endpoint,
  modelId,
  onDelete,
}: {
  endpoint: DashboardEndpoint
  modelId: string
  onDelete?: () => void
}) {
  const { data: stats } = useQuery({
    queryKey: ['endpoint-model-stats', endpoint.id],
    queryFn: () => endpointsApi.getModelStats(endpoint.id),
  })
  const { data: tpsEntries } = useQuery({
    queryKey: ['endpoint-model-tps', endpoint.id],
    queryFn: () => endpointsApi.getModelTps(endpoint.id),
  })

  const modelStat = stats?.find((s) => s.model_id === modelId)
  const totalRequests = modelStat?.total_requests ?? 0
  const successfulRequests = modelStat?.successful_requests ?? 0
  const failedRequests = modelStat?.failed_requests ?? 0
  const modelTps = (tpsEntries ?? [])
    .filter((entry) => entry.model_id === modelId && entry.source === 'production')
    .sort((a, b) => a.api_kind.localeCompare(b.api_kind))
  const modelTpsSummary =
    modelTps.length > 0
      ? modelTps
          .map((entry) => `${formatApiKindLabel(entry.api_kind)} ${formatTps(entry.tps)}`)
          .join(' | ')
      : '-'

  return (
    <div className="flex items-center justify-between py-1.5 px-3 text-sm">
      <div className="flex items-center gap-2 min-w-0">
        <Badge
          variant={endpoint.status === 'online' ? 'online' : endpoint.status === 'error' ? 'destructive' : 'pending'}
          className="text-xs"
        >
          {endpoint.status}
        </Badge>
        <span className="truncate font-medium">{endpoint.name}</span>
      </div>
      <div className="flex items-center gap-4 text-xs text-muted-foreground shrink-0">
        <span>Total: {totalRequests.toLocaleString()}</span>
        <span className="text-green-600">
          OK: {successfulRequests.toLocaleString()}
        </span>
        <span className="text-red-600">
          Fail: {failedRequests.toLocaleString()}
        </span>
        <span>TPS: {modelTpsSummary}</span>
        <a
          href={`#playground/${endpoint.id}`}
          className="text-primary hover:underline"
        >
          <Play className="h-3 w-3" />
        </a>
        {onDelete && (
          <button
            onClick={(e) => {
              e.stopPropagation()
              onDelete()
            }}
            className="text-muted-foreground hover:text-destructive transition-colors"
            title="Delete model from endpoint"
          >
            <Trash2 className="h-3 w-3" />
          </button>
        )}
      </div>
    </div>
  )
}

export function ModelsTable({
  models,
  endpoints,
  isLoading,
  onRefresh,
  viewerMode = false,
  view,
  onViewChange,
}: ModelsTableProps) {
  // US-029: Canonical 表示 ⇔ 詳細表示のトグル（view/onViewChange 両指定時のみ表示）
  const viewToggle =
    view && onViewChange ? (
      <div className="inline-flex overflow-hidden rounded-md border" role="group" aria-label="Model view mode">
        <Button
          variant={view === 'canonical' ? 'secondary' : 'ghost'}
          size="sm"
          className="rounded-none"
          aria-pressed={view === 'canonical'}
          onClick={() => onViewChange('canonical')}
        >
          Canonical
        </Button>
        <Button
          variant={view === 'detail' ? 'secondary' : 'ghost'}
          size="sm"
          className="rounded-none"
          aria-pressed={view === 'detail'}
          onClick={() => onViewChange('detail')}
        >
          Detail
        </Button>
      </div>
    ) : null

  const [search, setSearch] = useState('')
  const [statusFilter, setStatusFilter] = useState<LifecycleStatus | 'all'>('all')
  const [capabilityFilters, setCapabilityFilters] = useState<Record<string, boolean>>({})
  const [sortField, setSortField] = useState<SortField>('id')
  const [sortDirection, setSortDirection] = useState<SortDirection>('asc')
  const [expandedModels, setExpandedModels] = useState<Set<string>>(new Set())
  const [addWizardOpen, setAddWizardOpen] = useState(false)
  const [deleteDialog, setDeleteDialog] = useState<{
    open: boolean
    modelId: string
    endpointId: string
    endpointName: string
    endpointType: string
  }>({ open: false, modelId: '', endpointId: '', endpointName: '', endpointType: '' })
  const [columnVisibility, setColumnVisibility] = useState<Record<string, boolean>>({
    id: true,
    bestStatus: true,
    endpointCount: true,
    totalRequests: true,
    supportedApis: true,
    maxTokens: false,
    source: false,
    tags: false,
    description: false,
    repo: false,
    filename: false,
    requiredMemoryBytes: false,
    chatTemplate: false,
  })

  const aggregated = useMemo(() => aggregateModels(models), [models])

  const { data: allModelStats } = useQuery({
    queryKey: ['all-model-stats'],
    queryFn: () => dashboardApi.getAllModelStats(),
    enabled: !viewerMode,
  })

  const modelStatsMap = useMemo(() => {
    const map = new Map<string, ModelStatEntry>()
    if (allModelStats) {
      for (const stat of allModelStats) {
        map.set(stat.model_id, stat)
      }
    }
    return map
  }, [allModelStats])

  const aggregatedWithStatsFallback = useMemo(() => {
    if (!allModelStats) return aggregated

    const existingIds = new Set(aggregated.map((m) => m.id))
    const statsOnlyModels: AggregatedModel[] = allModelStats
      .filter((stat) => !existingIds.has(stat.model_id))
      .map((stat) => ({
        id: stat.model_id,
        bestStatus: 'registered',
        ready: false,
        supportedApis: [],
        tags: [],
        endpointIds: [],
        endpointCount: 0,
      }))

    return [...aggregated, ...statsOnlyModels]
  }, [aggregated, allModelStats])

  const columns: ColumnDef[] = useMemo(
    () => [
      {
        key: 'id',
        label: 'Model ID',
        defaultVisible: true,
        render: (m) => (
          <span className="font-mono text-sm truncate" title={m.id}>
            {m.id}
          </span>
        ),
      },
      {
        key: 'bestStatus',
        label: 'Status',
        defaultVisible: true,
        render: (m) => (
          <div className="flex items-center gap-1.5">
            <span
              className={`inline-block h-2 w-2 rounded-full shrink-0 ${m.ready ? 'bg-green-500' : 'bg-gray-300'}`}
              title={m.ready ? 'Ready' : 'Not Ready'}
            />
            <Badge variant={getLifecycleBadgeVariant(m.bestStatus)}>
              {getLifecycleLabel(m.bestStatus)}
            </Badge>
          </div>
        ),
      },
      {
        key: 'endpointCount',
        label: 'Endpoints',
        defaultVisible: true,
        render: (m) => <EndpointCountCell count={m.endpointCount} />,
      },
      {
        key: 'totalRequests',
        label: 'Routed Requests',
        defaultVisible: true,
        render: (m) => <TrafficCell stat={modelStatsMap.get(m.id)} />,
      },
      {
        key: 'supportedApis',
        label: 'APIs',
        defaultVisible: true,
        render: (m) => <SupportedApiBadges apis={m.supportedApis} />,
      },
      {
        key: 'maxTokens',
        label: 'Max Tokens',
        defaultVisible: false,
        render: (m) => (
          <span className="text-sm">
            {m.maxTokens != null ? m.maxTokens.toLocaleString() : '-'}
          </span>
        ),
      },
      {
        key: 'source',
        label: 'Source',
        defaultVisible: false,
        render: (m) => <span className="text-sm">{m.source ?? '-'}</span>,
      },
      {
        key: 'tags',
        label: 'Tags',
        defaultVisible: false,
        render: (m) =>
          m.tags.length > 0 ? (
            <div className="flex gap-1 flex-wrap">
              {m.tags.map((tag) => (
                <Badge key={tag} variant="secondary" className="text-xs">
                  {tag}
                </Badge>
              ))}
            </div>
          ) : (
            <span className="text-sm text-muted-foreground">-</span>
          ),
      },
      {
        key: 'description',
        label: 'Description',
        defaultVisible: false,
        render: (m) => (
          <span className="text-sm truncate max-w-[200px] inline-block" title={m.description}>
            {m.description ?? '-'}
          </span>
        ),
      },
      {
        key: 'repo',
        label: 'Repo',
        defaultVisible: false,
        render: (m) => <span className="text-sm">{m.repo ?? '-'}</span>,
      },
      {
        key: 'filename',
        label: 'Filename',
        defaultVisible: false,
        render: (m) => (
          <span className="text-sm font-mono">{m.filename ?? '-'}</span>
        ),
      },
      {
        key: 'requiredMemoryBytes',
        label: 'Required Memory',
        defaultVisible: false,
        render: (m) => (
          <span className="text-sm">
            {m.requiredMemoryBytes ? formatBytes(m.requiredMemoryBytes) : '-'}
          </span>
        ),
      },
      {
        key: 'chatTemplate',
        label: 'Chat Template',
        defaultVisible: false,
        render: (m) => (
          <span className="text-sm truncate max-w-[200px] inline-block" title={m.chatTemplate}>
            {m.chatTemplate ?? '-'}
          </span>
        ),
      },
    ],
    [modelStatsMap]
  )

  const visibleColumns = useMemo(
    () => columns.filter((col) => columnVisibility[col.key]),
    [columns, columnVisibility]
  )

  const activeApiFilters = useMemo(
    () => Object.entries(capabilityFilters).filter(([, v]) => v).map(([k]) => k),
    [capabilityFilters]
  )

  const filtered = useMemo(() => {
    return aggregatedWithStatsFallback.filter((m) => {
      if (search && !m.id.toLowerCase().includes(search.toLowerCase())) return false
      if (statusFilter !== 'all' && m.bestStatus !== statusFilter) return false
      if (activeApiFilters.length > 0) {
        for (const api of activeApiFilters) {
          if (!m.supportedApis.includes(api as SupportedApi)) return false
        }
      }
      return true
    })
  }, [aggregatedWithStatsFallback, search, statusFilter, activeApiFilters])

  const sorted = useMemo(() => {
    return [...filtered].sort((a, b) => {
      let cmp = 0
      switch (sortField) {
        case 'id':
          cmp = a.id.localeCompare(b.id)
          break
        case 'bestStatus':
          cmp = LIFECYCLE_PRIORITY[a.bestStatus] - LIFECYCLE_PRIORITY[b.bestStatus]
          break
        case 'endpointCount':
          cmp = a.endpointCount - b.endpointCount
          break
        case 'totalRequests':
          cmp = (modelStatsMap.get(a.id)?.total_requests ?? 0) - (modelStatsMap.get(b.id)?.total_requests ?? 0)
          break
      }
      return sortDirection === 'asc' ? cmp : -cmp
    })
  }, [filtered, sortField, sortDirection, modelStatsMap])

  const handleSort = (field: SortField) => {
    if (sortField === field) {
      setSortDirection(sortDirection === 'asc' ? 'desc' : 'asc')
    } else {
      setSortField(field)
      setSortDirection('asc')
    }
  }

  const SortIcon = ({ field }: { field: SortField }) => {
    if (sortField !== field) return null
    return sortDirection === 'asc' ? (
      <ChevronUp className="ml-1 h-4 w-4 inline" />
    ) : (
      <ChevronDown className="ml-1 h-4 w-4 inline" />
    )
  }

  const toggleExpand = (id: string) => {
    setExpandedModels((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  if (isLoading && models.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Package className="h-5 w-5" />
            Models
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-center h-32">
            <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-primary" />
          </div>
        </CardContent>
      </Card>
    )
  }

  if (viewerMode) {
    const viewerFiltered = aggregatedWithStatsFallback.filter((m) =>
      m.id.toLowerCase().includes(search.toLowerCase())
    )
    return (
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle className="flex items-center gap-2">
              <Package className="h-5 w-5" />
              Models
              <Badge variant="secondary" className="ml-2">
                {aggregatedWithStatsFallback.length}
              </Badge>
            </CardTitle>
            <div className="flex items-center gap-2">
              {viewToggle}
              {onRefresh && (
                <Button variant="outline" size="sm" onClick={onRefresh}>
                  <RefreshCw className="h-4 w-4 mr-1" />
                  Refresh
                </Button>
              )}
            </div>
          </div>
        </CardHeader>
        <CardContent>
          <div className="relative mb-4">
            <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-muted-foreground h-4 w-4" />
            <Input
              placeholder="Search by model ID..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-10"
            />
          </div>
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Model ID</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Description</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {viewerFiltered.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={3} className="text-center py-8 text-muted-foreground">
                      {search ? 'No models match your search' : 'No models registered'}
                    </TableCell>
                  </TableRow>
                ) : (
                  viewerFiltered.map((model) => (
                    <TableRow key={model.id}>
                      <TableCell>
                        <span className="font-mono text-sm truncate" title={model.id}>
                          {model.id}
                        </span>
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-1.5">
                          <span
                            className={`inline-block h-2 w-2 rounded-full shrink-0 ${
                              model.ready ? 'bg-green-500' : 'bg-gray-300'
                            }`}
                            title={model.ready ? 'Ready' : 'Not Ready'}
                          />
                          <Badge variant={getLifecycleBadgeVariant(model.bestStatus)}>
                            {getLifecycleLabel(model.bestStatus)}
                          </Badge>
                        </div>
                      </TableCell>
                      <TableCell className="text-sm text-muted-foreground">
                        <span
                          className="truncate inline-block max-w-[640px] align-bottom"
                          title={model.description ?? ''}
                        >
                          {model.description ?? '-'}
                        </span>
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </div>
        </CardContent>
      </Card>
    )
  }

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2">
            <Package className="h-5 w-5" />
            Models
            <Badge variant="secondary" className="ml-2">
              {aggregatedWithStatsFallback.length}
            </Badge>
          </CardTitle>
          <div className="flex items-center gap-2">
            {viewToggle}
            <Button variant="outline" size="sm" onClick={() => setAddWizardOpen(true)}>
              <Plus className="h-4 w-4 mr-1" />
              Add Model
            </Button>
            {onRefresh && (
              <Button variant="outline" size="sm" onClick={onRefresh}>
                <RefreshCw className="h-4 w-4 mr-1" />
                Refresh
              </Button>
            )}
          </div>
        </div>
      </CardHeader>
      <CardContent>
        {/* Filters */}
        <div className="flex flex-col sm:flex-row gap-4 mb-4">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-muted-foreground h-4 w-4" />
            <Input
              placeholder="Search by model ID..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-10"
            />
          </div>
          <Select
            value={statusFilter}
            onValueChange={(v) => setStatusFilter(v as LifecycleStatus | 'all')}
          >
            <SelectTrigger className="w-[140px]">
              <SelectValue placeholder="Status" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All Status</SelectItem>
              <SelectItem value="registered">Registered</SelectItem>
              <SelectItem value="caching">Caching</SelectItem>
              <SelectItem value="pending">Pending</SelectItem>
              <SelectItem value="error">Error</SelectItem>
            </SelectContent>
          </Select>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="sm">
                <Filter className="h-4 w-4 mr-1" />
                APIs
                {activeApiFilters.length > 0 && (
                  <Badge variant="secondary" className="ml-1 text-xs">
                    {activeApiFilters.length}
                  </Badge>
                )}
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {SUPPORTED_API_BADGES.map(({ key, label }) => (
                <DropdownMenuCheckboxItem
                  key={key}
                  checked={!!capabilityFilters[key]}
                  onCheckedChange={(checked) =>
                    setCapabilityFilters((prev) => ({ ...prev, [key]: !!checked }))
                  }
                >
                  {label}
                </DropdownMenuCheckboxItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="sm">
                <Settings2 className="h-4 w-4 mr-1" />
                Columns
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {columns.map((col) => (
                <DropdownMenuCheckboxItem
                  key={col.key}
                  checked={!!columnVisibility[col.key]}
                  onCheckedChange={(checked) =>
                    setColumnVisibility((prev) => ({ ...prev, [col.key]: !!checked }))
                  }
                  disabled={col.key === 'id'}
                >
                  {col.label}
                </DropdownMenuCheckboxItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {/* Table */}
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-8" />
                {visibleColumns.map((col) => {
                  const sortable: SortField[] = [
                    'id',
                    'bestStatus',
                    'endpointCount',
                    'totalRequests',
                  ]
                  const isSortable = sortable.includes(col.key as SortField)
                  return (
                    <TableHead
                      key={col.key}
                      className={isSortable ? 'cursor-pointer hover:bg-muted/50' : ''}
                      onClick={isSortable ? () => handleSort(col.key as SortField) : undefined}
                    >
                      {col.label}
                      {isSortable && <SortIcon field={col.key as SortField} />}
                    </TableHead>
                  )
                })}
                <TableHead className="w-10" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {sorted.length === 0 ? (
                <TableRow>
                  <TableCell
                    colSpan={visibleColumns.length + 2}
                    className="text-center py-8 text-muted-foreground"
                  >
                    {search || statusFilter !== 'all' || activeApiFilters.length > 0
                      ? 'No models match the filter criteria'
                      : 'No models registered'}
                  </TableCell>
                </TableRow>
              ) : (
                sorted.map((model) => {
                  const isExpanded = expandedModels.has(model.id)
                  return (
                    <ModelRow
                      key={model.id}
                      model={model}
                      visibleColumns={visibleColumns}
                      isExpanded={isExpanded}
                      onToggleExpand={() => toggleExpand(model.id)}
                      endpoints={endpoints}
                      onDeleteModel={(endpointId, endpointName, endpointType) =>
                        setDeleteDialog({
                          open: true,
                          modelId: model.id,
                          endpointId,
                          endpointName,
                          endpointType,
                        })
                      }
                    />
                  )
                })
              )}
            </TableBody>
          </Table>
        </div>

        <ModelAddWizard open={addWizardOpen} onOpenChange={setAddWizardOpen} />
        <ModelDeleteDialog
          open={deleteDialog.open}
          onOpenChange={(open) => setDeleteDialog((prev) => ({ ...prev, open }))}
          modelId={deleteDialog.modelId}
          endpointId={deleteDialog.endpointId}
          endpointName={deleteDialog.endpointName}
          endpointType={deleteDialog.endpointType}
        />
      </CardContent>
    </Card>
  )
}

const DELETABLE_ENDPOINT_TYPES = new Set(['xllm', 'ollama'])

function ModelRow({
  model,
  visibleColumns,
  isExpanded,
  onToggleExpand,
  endpoints,
  onDeleteModel,
}: {
  model: AggregatedModel
  visibleColumns: ColumnDef[]
  isExpanded: boolean
  onToggleExpand: () => void
  endpoints: DashboardEndpoint[]
  onDeleteModel: (endpointId: string, endpointName: string, endpointType: string) => void
}) {
  const modelEndpointIdSet = new Set(model.endpointIds)
  const modelEndpoints = endpoints.filter((ep) => modelEndpointIdSet.has(ep.id))

  return (
    <>
      <TableRow className="cursor-pointer hover:bg-muted/50" onClick={onToggleExpand}>
        <TableCell className="w-8 px-2">
          <Button
            variant="secondary"
            size="icon"
            aria-label={isExpanded ? 'Collapse row' : 'Expand row'}
            aria-expanded={isExpanded}
            className="h-6 w-6 bg-transparent shadow-none hover:bg-muted/70"
          >
            {isExpanded ? (
              <ChevronDown className="h-4 w-4" />
            ) : (
              <ChevronRight className="h-4 w-4" />
            )}
          </Button>
        </TableCell>
        {visibleColumns.map((col) => (
          <TableCell key={col.key}>{col.render(model)}</TableCell>
        ))}
        <TableCell className="w-10">
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="secondary"
                  size="icon"
                  aria-label="Open in playground"
                  className="h-7 w-7 bg-transparent shadow-none hover:bg-muted/70"
                  disabled={!model.ready}
                  onClick={(e) => {
                    e.stopPropagation()
                    window.location.hash = 'lb-playground?model=' + encodeURIComponent(model.id)
                  }}
                >
                  <Play className="h-4 w-4" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Open in Playground</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        </TableCell>
      </TableRow>
      {isExpanded && (
        <TableRow>
          <TableCell colSpan={visibleColumns.length + 2} className="bg-muted/30 p-0">
            <div className="py-2 px-4">
              <div className="text-xs font-medium text-muted-foreground mb-2">
                Endpoints ({model.endpointCount} source{model.endpointCount !== 1 ? 's' : ''})
              </div>
              <div className="space-y-1 rounded-md border bg-background">
                {modelEndpoints.length > 0 ? (
                  modelEndpoints.map((ep) => (
                    <EndpointStatsRow
                      key={ep.id}
                      endpoint={ep}
                      modelId={model.id}
                      onDelete={
                        DELETABLE_ENDPOINT_TYPES.has(ep.endpoint_type)
                          ? () => onDeleteModel(ep.id, ep.name, ep.endpoint_type)
                          : undefined
                      }
                    />
                  ))
                ) : (
                  <div className="py-2 px-3 text-xs text-muted-foreground">
                    No endpoints currently serve this model
                  </div>
                )}
              </div>
            </div>
          </TableCell>
        </TableRow>
      )}
    </>
  )
}
