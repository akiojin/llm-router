import { useState, useMemo } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { type DashboardEndpoint, type EndpointType, endpointsApi } from '@/lib/api'
import { classifyEndpointLastError } from '@/lib/endpoint-errors'
import { formatRelativeTime, cn } from '@/lib/utils'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Label } from '@/components/ui/label'
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
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { EndpointDetailModal } from './EndpointDetailModal'
import {
  sortEndpoints,
  type EndpointSortDirection as SortDirection,
  type EndpointSortField as SortField,
} from './endpointSorting'
import {
  Search,
  ChevronUp,
  ChevronDown,
  Server,
  Info,
  ChevronLeft,
  ChevronRight,
  Play,
  RefreshCw,
  Trash2,
  Plus,
} from 'lucide-react'

/**
 * SPEC-e8e9326e: Router-Driven Endpoint Registration System
 * Endpoint List Component
 */

interface EndpointTableProps {
  endpoints: DashboardEndpoint[]
  isLoading: boolean
}

const PAGE_SIZE = 10

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

function getStatusLabel(status: DashboardEndpoint['status']): string {
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
      return status
  }
}

/** SPEC-e8e9326e: Get display label for endpoint type */
function getTypeLabel(type: EndpointType): string {
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
      return 'OpenAI Compatible'
    case 'unknown':
      return 'Unknown'
    default:
      return type
  }
}


/** SPEC-e8e9326e: Get badge variant for endpoint type */
function getTypeBadgeVariant(
  type: EndpointType
): 'default' | 'destructive' | 'outline' | 'secondary' {
  switch (type) {
    case 'xllm':
      return 'default'
    case 'ollama':
      return 'secondary'
    case 'vllm':
      return 'secondary'
    case 'lm_studio':
      return 'secondary'
    case 'llamacpp':
      return 'secondary'
    case 'openai_compatible':
      return 'outline'
    case 'unknown':
      return 'outline'
    default:
      return 'outline'
  }
}

export function EndpointTable({ endpoints, isLoading }: EndpointTableProps) {
  const queryClient = useQueryClient()
  const [search, setSearch] = useState('')
  const [statusFilter, setStatusFilter] = useState<
    'all' | 'online' | 'pending' | 'offline' | 'error'
  >('all')
  const [typeFilter, setTypeFilter] = useState<'all' | EndpointType>('all')
  const [sortField, setSortField] = useState<SortField>('status')
  const [sortDirection, setSortDirection] = useState<SortDirection>('desc')
  const [currentPage, setCurrentPage] = useState(1)
  const [selectedEndpoint, setSelectedEndpoint] = useState<DashboardEndpoint | null>(null)
  const [deletingEndpoint, setDeletingEndpoint] = useState<DashboardEndpoint | null>(null)
  const [isDeleting, setIsDeleting] = useState(false)
  const [isTesting, setIsTesting] = useState<string | null>(null)
  const [isSyncing, setIsSyncing] = useState<string | null>(null)
  // Create endpoint state
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false)
  const [isCreating, setIsCreating] = useState(false)
  const [createError, setCreateError] = useState<string | null>(null)
  const [createForm, setCreateForm] = useState({
    name: '',
    base_url: '',
    api_key: '',
    notes: '',
  })

  const handleCreate = async () => {
    if (!createForm.name || !createForm.base_url) return
    setIsCreating(true)
    setCreateError(null)
    try {
      await endpointsApi.create({
        name: createForm.name,
        base_url: createForm.base_url,
        api_key: createForm.api_key || undefined,
        notes: createForm.notes || undefined,
      })
      await queryClient.invalidateQueries({ queryKey: ['dashboard-endpoints'] })
      setIsCreateDialogOpen(false)
      setCreateForm({ name: '', base_url: '', api_key: '', notes: '' })
    } catch (error) {
      console.error('Failed to create endpoint:', error)
      setCreateError(error instanceof Error ? error.message : 'Failed to create endpoint')
    } finally {
      setIsCreating(false)
    }
  }

  const handleDelete = async () => {
    if (!deletingEndpoint) return
    setIsDeleting(true)
    try {
      await endpointsApi.delete(deletingEndpoint.id)
      await queryClient.invalidateQueries({ queryKey: ['dashboard-endpoints'] })
    } catch (error) {
      console.error('Failed to delete endpoint:', error)
    } finally {
      setIsDeleting(false)
      setDeletingEndpoint(null)
    }
  }

  const handleTest = async (endpoint: DashboardEndpoint) => {
    setIsTesting(endpoint.id)
    try {
      await endpointsApi.test(endpoint.id)
      await queryClient.invalidateQueries({ queryKey: ['dashboard-endpoints'] })
    } catch (error) {
      console.error('Failed to test endpoint:', error)
    } finally {
      setIsTesting(null)
    }
  }

  const handleSync = async (endpoint: DashboardEndpoint) => {
    setIsSyncing(endpoint.id)
    try {
      await endpointsApi.sync(endpoint.id)
      await queryClient.invalidateQueries({ queryKey: ['dashboard-endpoints'] })
    } catch (error) {
      console.error('Failed to sync endpoint:', error)
    } finally {
      setIsSyncing(null)
    }
  }

  const filteredEndpoints = useMemo(() => {
    return endpoints.filter((endpoint) => {
      const matchesSearch =
        endpoint.name.toLowerCase().includes(search.toLowerCase()) ||
        endpoint.base_url.toLowerCase().includes(search.toLowerCase())
      const matchesStatus = statusFilter === 'all' || endpoint.status === statusFilter
      const matchesType = typeFilter === 'all' || endpoint.endpoint_type === typeFilter
      return matchesSearch && matchesStatus && matchesType
    })
  }, [endpoints, search, statusFilter, typeFilter])

  const sortedEndpoints = useMemo(() => {
    return sortEndpoints(
      filteredEndpoints,
      sortField,
      sortDirection
    )
  }, [filteredEndpoints, sortField, sortDirection])

  const paginatedEndpoints = useMemo(() => {
    const start = (currentPage - 1) * PAGE_SIZE
    return sortedEndpoints.slice(start, start + PAGE_SIZE)
  }, [sortedEndpoints, currentPage])

  const totalPages = Math.ceil(sortedEndpoints.length / PAGE_SIZE)

  const handleSort = (field: SortField) => {
    if (sortField === field) {
      setSortDirection(sortDirection === 'asc' ? 'desc' : 'asc')
    } else {
      setSortField(field)
      setSortDirection('desc')
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

  if (isLoading && endpoints.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Server className="h-5 w-5" />
            Endpoints
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-center h-32">
            <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
          </div>
        </CardContent>
      </Card>
    )
  }

  return (
    <>
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle className="flex items-center gap-2">
              <Server className="h-5 w-5" />
              Endpoints
              <Badge variant="secondary" className="ml-2">
                {filteredEndpoints.length}
              </Badge>
            </CardTitle>
            <Button onClick={() => setIsCreateDialogOpen(true)}>
              <Plus className="h-4 w-4 mr-2" />
              Add Endpoint
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          {/* Filters */}
          <div className="flex flex-col sm:flex-row gap-4 mb-4">
            <div className="relative flex-1">
              <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-muted-foreground h-4 w-4" />
              <Input
                placeholder="Search by name or URL..."
                value={search}
                onChange={(e) => {
                  setSearch(e.target.value)
                  setCurrentPage(1)
                }}
                className="pl-10"
              />
            </div>
            <Select
              value={statusFilter}
              onValueChange={(value: typeof statusFilter) => {
                setStatusFilter(value)
                setCurrentPage(1)
              }}
            >
              <SelectTrigger className="w-[140px]">
                <SelectValue placeholder="Status" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All Status</SelectItem>
                <SelectItem value="online">Online</SelectItem>
                <SelectItem value="pending">Pending</SelectItem>
                <SelectItem value="offline">Offline</SelectItem>
                <SelectItem value="error">Error</SelectItem>
              </SelectContent>
            </Select>
            {/* SPEC-e8e9326e: Type filter */}
            <Select
              value={typeFilter}
              onValueChange={(value: typeof typeFilter) => {
                setTypeFilter(value)
                setCurrentPage(1)
              }}
            >
              <SelectTrigger className="w-[140px]">
                <SelectValue placeholder="Type" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All Types</SelectItem>
                <SelectItem value="xllm">xLLM</SelectItem>
                <SelectItem value="ollama">Ollama</SelectItem>
                <SelectItem value="vllm">vLLM</SelectItem>
                <SelectItem value="lm_studio">LM Studio</SelectItem>
                <SelectItem value="llamacpp">llama.cpp</SelectItem>
                <SelectItem value="openai_compatible">OpenAI Compatible</SelectItem>
                <SelectItem value="unknown">Unknown</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* Table */}
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead
                    className="cursor-pointer hover:bg-muted/50"
                    onClick={() => handleSort('name')}
                  >
                    Name
                    <SortIcon field="name" />
                  </TableHead>
                  <TableHead>URL</TableHead>
                  <TableHead>Type</TableHead>
                  <TableHead
                    className="cursor-pointer hover:bg-muted/50"
                    onClick={() => handleSort('status')}
                  >
                    Status
                    <SortIcon field="status" />
                  </TableHead>
                  <TableHead
                    className="cursor-pointer hover:bg-muted/50 text-right"
                    onClick={() => handleSort('total_requests')}
                  >
                    Requests
                    <SortIcon field="total_requests" />
                  </TableHead>
                  <TableHead
                    className="cursor-pointer hover:bg-muted/50 text-right"
                    onClick={() => handleSort('latency_ms')}
                  >
                    Latency
                    <SortIcon field="latency_ms" />
                  </TableHead>
                  <TableHead
                    className="cursor-pointer hover:bg-muted/50 text-right"
                    onClick={() => handleSort('model_count')}
                  >
                    Models
                    <SortIcon field="model_count" />
                  </TableHead>
                  <TableHead>Last Seen</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {paginatedEndpoints.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={9} className="text-center py-8 text-muted-foreground">
                      {search || statusFilter !== 'all'
                        ? 'No endpoints match the filter criteria'
                        : 'No endpoints registered'}
                    </TableCell>
                  </TableRow>
                ) : (
                  paginatedEndpoints.map((endpoint) => {
                    const errorDisplay = classifyEndpointLastError(endpoint.last_error)
                    return (
                      <TableRow key={endpoint.id}>
                        <TableCell className="font-medium">
                          <div className="flex items-center gap-2">
                            <span>{endpoint.name}</span>
                          </div>
                        </TableCell>
                        <TableCell>
                          <span className="text-muted-foreground font-mono text-sm">
                            {endpoint.base_url}
                          </span>
                        </TableCell>
                        <TableCell>
                          <Badge
                            variant={getTypeBadgeVariant(endpoint.endpoint_type)}
                          >
                            {getTypeLabel(endpoint.endpoint_type)}
                          </Badge>
                        </TableCell>
                        <TableCell>
                          <Badge variant={getStatusBadgeVariant(endpoint.status)}>
                            {getStatusLabel(endpoint.status)}
                          </Badge>
                          {endpoint.last_error && (
                            <>
                              <span className="ml-2 text-xs text-destructive">
                                ({endpoint.error_count} errors)
                              </span>
                              {errorDisplay && (
                                <Badge
                                  variant="outline"
                                  className="ml-2 border-destructive/40 text-destructive"
                                >
                                  {errorDisplay.label}
                                </Badge>
                              )}
                            </>
                          )}
                        </TableCell>
                      <TableCell className="text-right">
                        {endpoint.total_requests > 0 ? (
                          <span
                            className={cn(
                              endpoint.total_requests > 0 &&
                                endpoint.failed_requests / endpoint.total_requests >= 0.2
                                ? 'text-destructive font-medium'
                                : endpoint.total_requests > 0 &&
                                    endpoint.failed_requests / endpoint.total_requests >= 0.05
                                  ? 'text-yellow-600 dark:text-yellow-500 font-medium'
                                  : ''
                            )}
                          >
                            {endpoint.total_requests.toLocaleString()}
                            {' '}
                            ({(
                              (endpoint.successful_requests / endpoint.total_requests) *
                              100
                            ).toFixed(1)}%)
                          </span>
                        ) : (
                          '-'
                        )}
                      </TableCell>
                      <TableCell className="text-right">
                        {endpoint.latency_ms != null ? `${endpoint.latency_ms}ms` : '-'}
                      </TableCell>
                      <TableCell className="text-right">{endpoint.model_count}</TableCell>
                      <TableCell>
                        {endpoint.last_seen ? formatRelativeTime(endpoint.last_seen) : '-'}
                      </TableCell>
                      <TableCell className="text-right">
                        <div className="flex items-center justify-end gap-1">
                          <Button
                            variant="outline"
                            size="icon"
                            onClick={() => setSelectedEndpoint(endpoint)}
                            title="Details"
                          >
                            <Info className="h-4 w-4" />
                          </Button>
                          <Button
                            variant="outline"
                            size="icon"
                            onClick={() => handleTest(endpoint)}
                            disabled={isTesting === endpoint.id}
                            title="Test Connection"
                          >
                            <Play
                              className={cn('h-4 w-4', isTesting === endpoint.id && 'animate-pulse')}
                            />
                          </Button>
                          <Button
                            variant="outline"
                            size="icon"
                            onClick={() => handleSync(endpoint)}
                            disabled={isSyncing === endpoint.id || endpoint.status !== 'online'}
                            title="Sync Models"
                          >
                            <RefreshCw
                              className={cn('h-4 w-4', isSyncing === endpoint.id && 'animate-spin')}
                            />
                          </Button>
                          <Button
                            variant="outline"
                            size="icon"
                            onClick={() => setDeletingEndpoint(endpoint)}
                            title="Delete"
                          >
                            <Trash2 className="h-4 w-4 text-destructive" />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                    )
                  })
                )}
              </TableBody>
            </Table>
          </div>

          {/* Pagination */}
          {totalPages > 1 && (
            <div className="flex items-center justify-between mt-4">
              <div className="text-sm text-muted-foreground">
                Showing {(currentPage - 1) * PAGE_SIZE + 1} -{' '}
                {Math.min(currentPage * PAGE_SIZE, sortedEndpoints.length)} of {sortedEndpoints.length}
              </div>
              <div className="flex items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setCurrentPage((p) => Math.max(1, p - 1))}
                  disabled={currentPage === 1}
                >
                  <ChevronLeft className="h-4 w-4" />
                </Button>
                <span className="text-sm">
                  {currentPage} / {totalPages}
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setCurrentPage((p) => Math.min(totalPages, p + 1))}
                  disabled={currentPage === totalPages}
                >
                  <ChevronRight className="h-4 w-4" />
                </Button>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Detail Modal */}
      {selectedEndpoint && (
        <EndpointDetailModal
          endpoint={selectedEndpoint}
          open={!!selectedEndpoint}
          onOpenChange={(open) => !open && setSelectedEndpoint(null)}
        />
      )}

      {/* Delete Confirmation Dialog */}
      <AlertDialog open={!!deletingEndpoint} onOpenChange={(open) => !open && setDeletingEndpoint(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete Endpoint?</AlertDialogTitle>
            <AlertDialogDescription>
              This will delete &quot;{deletingEndpoint?.name}&quot;. This action cannot be undone.
              Models associated with this endpoint will no longer be available.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isDeleting}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleDelete}
              disabled={isDeleting}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {isDeleting ? 'Deleting...' : 'Delete'}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Create Endpoint Dialog */}
      <Dialog open={isCreateDialogOpen} onOpenChange={(open) => {
        if (!open) {
          setCreateError(null)
          setCreateForm({ name: '', base_url: '', api_key: '', notes: '' })
        }
        setIsCreateDialogOpen(open)
      }}>
        <DialogContent className="sm:max-w-[500px]">
          <DialogHeader>
            <DialogTitle>Add New Endpoint</DialogTitle>
            <DialogDescription>
              Register a new inference service endpoint (Ollama, vLLM, etc.)
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-4 py-4">
            <div className="grid gap-2">
              <Label htmlFor="endpoint-name">Name *</Label>
              <Input
                id="endpoint-name"
                placeholder="e.g., Production Ollama"
                value={createForm.name}
                onChange={(e) => setCreateForm({ ...createForm, name: e.target.value })}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="endpoint-url">Base URL *</Label>
              <Input
                id="endpoint-url"
                placeholder="e.g., http://localhost:11434"
                value={createForm.base_url}
                onChange={(e) => setCreateForm({ ...createForm, base_url: e.target.value })}
              />
              <p className="text-xs text-muted-foreground">
                The base URL of the OpenAI-compatible API endpoint
              </p>
              <p className="text-xs text-muted-foreground">
                Type is auto-detected
              </p>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="endpoint-api-key">API Key (optional)</Label>
              <Input
                id="endpoint-api-key"
                type="password"
                placeholder="sk-..."
                value={createForm.api_key}
                onChange={(e) => setCreateForm({ ...createForm, api_key: e.target.value })}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="endpoint-notes">Notes (optional)</Label>
              <Input
                id="endpoint-notes"
                placeholder="Description or notes about this endpoint"
                value={createForm.notes}
                onChange={(e) => setCreateForm({ ...createForm, notes: e.target.value })}
              />
            </div>
            {createError && (
              <p className="text-sm text-destructive">{createError}</p>
            )}
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setIsCreateDialogOpen(false)}
              disabled={isCreating}
            >
              Cancel
            </Button>
            <Button
              onClick={handleCreate}
              disabled={isCreating || !createForm.name || !createForm.base_url}
            >
              {isCreating ? 'Creating...' : 'Create Endpoint'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
