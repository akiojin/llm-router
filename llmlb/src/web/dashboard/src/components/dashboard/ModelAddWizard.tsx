import { useState, useEffect, useCallback, useRef } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  type CatalogSearchResult,
  type RecommendedEndpoint,
  catalogApi,
  endpointsApi,
} from '@/lib/api'
import { toast } from '@/hooks/use-toast'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Checkbox } from '@/components/ui/checkbox'
import { Progress } from '@/components/ui/progress'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Search, Download, Check, AlertCircle, Loader2, Plus, ArrowLeft } from 'lucide-react'

interface ModelAddWizardProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

type WizardStep = 'search' | 'detail' | 'endpoints' | 'download'

export function ModelAddWizard({ open, onOpenChange }: ModelAddWizardProps) {
  return (
    <ModelAddWizardContent
      key={open ? 'open' : 'closed'}
      open={open}
      onOpenChange={onOpenChange}
    />
  )
}

function ModelAddWizardContent({ open, onOpenChange }: ModelAddWizardProps) {
  const queryClient = useQueryClient()
  const [step, setStep] = useState<WizardStep>('search')
  const [searchQuery, setSearchQuery] = useState('')
  const [debouncedQuery, setDebouncedQuery] = useState('')
  const [selectedModel, setSelectedModel] = useState<CatalogSearchResult | null>(null)
  const [selectedEndpointIds, setSelectedEndpointIds] = useState<Set<string>>(new Set())
  const [downloadStatuses, setDownloadStatuses] = useState<
    Record<string, 'pending' | 'downloading' | 'completed' | 'failed'>
  >({})
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Cancel pending search debounce when this dialog session is discarded.
  useEffect(() => {
    return () => {
      if (debounceRef.current) {
        clearTimeout(debounceRef.current)
      }
    }
  }, [])

  // Debounce search input
  const handleSearchChange = useCallback((value: string) => {
    setSearchQuery(value)
    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(() => {
      setDebouncedQuery(value)
    }, 300)
  }, [])

  // Search query
  const { data: searchResults, isLoading: isSearching } = useQuery({
    queryKey: ['catalog-search', debouncedQuery],
    queryFn: () => catalogApi.search(debouncedQuery, 20),
    enabled: debouncedQuery.length >= 2,
  })

  // Model detail query
  const { data: modelDetail, isLoading: isLoadingDetail } = useQuery({
    queryKey: ['catalog-model', selectedModel?.repo_id],
    queryFn: () => catalogApi.getModel(selectedModel!.repo_id),
    enabled: !!selectedModel && (step === 'detail' || step === 'endpoints'),
  })

  // Endpoint recommendations
  const { data: recommendations, isLoading: isLoadingEndpoints } = useQuery({
    queryKey: ['catalog-recommend', selectedModel?.repo_id],
    queryFn: () => catalogApi.recommendEndpoints(selectedModel!.repo_id),
    enabled: !!selectedModel && step === 'endpoints',
  })

  // Download mutation
  const downloadMutation = useMutation({
    mutationFn: async ({
      endpointId,
      model,
    }: {
      endpointId: string
      model: string
    }) => {
      return endpointsApi.downloadModel(endpointId, {
        model,
        hf_repo: selectedModel?.repo_id,
      })
    },
  })

  const handleSelectModel = (model: CatalogSearchResult) => {
    setSelectedModel(model)
    setStep('detail')
  }

  const handleProceedToEndpoints = () => {
    setSelectedEndpointIds(new Set())
    setStep('endpoints')
  }

  const toggleEndpoint = (id: string) => {
    setSelectedEndpointIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  const handleStartDownload = async () => {
    if (!selectedModel || selectedEndpointIds.size === 0) return

    setStep('download')
    const initialStatuses: Record<string, 'pending' | 'downloading' | 'completed' | 'failed'> = {}
    for (const id of selectedEndpointIds) {
      initialStatuses[id] = 'pending'
    }
    setDownloadStatuses(initialStatuses)

    for (const endpointId of selectedEndpointIds) {
      setDownloadStatuses((prev) => ({ ...prev, [endpointId]: 'downloading' }))
      try {
        await downloadMutation.mutateAsync({
          endpointId,
          model: selectedModel.repo_id,
        })
        setDownloadStatuses((prev) => ({ ...prev, [endpointId]: 'completed' }))
      } catch {
        setDownloadStatuses((prev) => ({ ...prev, [endpointId]: 'failed' }))
      }
    }

    queryClient.invalidateQueries({ queryKey: ['dashboard-endpoints'] })
    queryClient.invalidateQueries({ queryKey: ['models'] })
    toast({
      title: 'Download requests sent',
      description: `Initiated download of ${selectedModel.repo_id} to ${selectedEndpointIds.size} endpoint(s)`,
    })
  }

  const handleBack = () => {
    switch (step) {
      case 'detail':
        setStep('search')
        break
      case 'endpoints':
        setStep('detail')
        break
      default:
        break
    }
  }

  const allDownloadsFinished =
    step === 'download' &&
    Object.values(downloadStatuses).every((s) => s === 'completed' || s === 'failed')

  const stepTitle: Record<WizardStep, string> = {
    search: 'Search HuggingFace Models',
    detail: 'Model Details',
    endpoints: 'Select Endpoints',
    download: 'Download Progress',
  }

  const downloadableEndpoints = recommendations?.endpoints.filter((ep) => ep.can_download) ?? []
  const compatibleEngineEntries = modelDetail
    ? Object.entries(modelDetail.engine_names).filter(
        (entry): entry is [string, string] => entry[1] != null && entry[1] !== ''
      )
    : []

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Plus className="h-5 w-5" />
            {stepTitle[step]}
          </DialogTitle>
          <DialogDescription>
            {step === 'search' && 'Search for models on HuggingFace to add to your endpoints'}
            {step === 'detail' && `Details for ${selectedModel?.repo_id ?? ''}`}
            {step === 'endpoints' && 'Choose which endpoints should download this model'}
            {step === 'download' && 'Sending download requests to selected endpoints'}
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 overflow-y-auto py-4">
          {/* Step 1: Search */}
          {step === 'search' && (
            <div className="space-y-4">
              <div className="relative">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground h-4 w-4" />
                <Input
                  placeholder="Search models (e.g., llama, mistral, phi)..."
                  value={searchQuery}
                  onChange={(e) => handleSearchChange(e.target.value)}
                  className="pl-10"
                  autoFocus
                />
              </div>

              {isSearching && (
                <div className="flex items-center justify-center py-8">
                  <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
                </div>
              )}

              {searchResults && searchResults.models.length > 0 && (
                <div className="rounded-md border max-h-[400px] overflow-y-auto">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Repository</TableHead>
                        <TableHead>Description</TableHead>
                        <TableHead className="text-right">Downloads</TableHead>
                        <TableHead>Tags</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {searchResults.models.map((model) => (
                        <TableRow
                          key={model.repo_id}
                          className="cursor-pointer hover:bg-muted/50"
                          onClick={() => handleSelectModel(model)}
                        >
                          <TableCell className="font-mono text-sm">
                            {model.repo_id}
                          </TableCell>
                          <TableCell className="text-sm text-muted-foreground max-w-[200px] truncate">
                            {model.description ?? '-'}
                          </TableCell>
                          <TableCell className="text-sm text-right tabular-nums">
                            {model.downloads != null
                              ? model.downloads.toLocaleString()
                              : '-'}
                          </TableCell>
                          <TableCell>
                            <div className="flex gap-1 flex-wrap max-w-[150px]">
                              {(model.tags ?? []).slice(0, 3).map((tag) => (
                                <Badge key={tag} variant="secondary" className="text-xs">
                                  {tag}
                                </Badge>
                              ))}
                              {(model.tags ?? []).length > 3 && (
                                <Badge variant="outline" className="text-xs">
                                  +{(model.tags ?? []).length - 3}
                                </Badge>
                              )}
                            </div>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              )}

              {searchResults && searchResults.models.length === 0 && debouncedQuery.length >= 2 && (
                <div className="text-center py-8 text-muted-foreground text-sm">
                  No models found for &quot;{debouncedQuery}&quot;
                </div>
              )}

              {!searchResults && !isSearching && debouncedQuery.length < 2 && (
                <div className="text-center py-8 text-muted-foreground text-sm">
                  Enter at least 2 characters to search
                </div>
              )}
            </div>
          )}

          {/* Step 2: Model Detail */}
          {step === 'detail' && (
            <div className="space-y-4">
              {isLoadingDetail && (
                <div className="flex items-center justify-center py-8">
                  <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
                </div>
              )}

              {modelDetail && (
                <>
                  <div className="space-y-2">
                    <div className="flex items-center gap-2">
                      <span className="font-mono font-medium">{modelDetail.repo_id}</span>
                      {modelDetail.pipeline_tag && (
                        <Badge variant="secondary">{modelDetail.pipeline_tag}</Badge>
                      )}
                    </div>
                    {modelDetail.description && (
                      <p className="text-sm text-muted-foreground">{modelDetail.description}</p>
                    )}
                    {modelDetail.downloads != null && (
                      <p className="text-xs text-muted-foreground">
                        Downloads: {modelDetail.downloads.toLocaleString()}
                      </p>
                    )}
                  </div>

                  {/* Tags */}
                  {modelDetail.tags && modelDetail.tags.length > 0 && (
                    <div className="space-y-1">
                      <p className="text-xs font-medium text-muted-foreground">Tags</p>
                      <div className="flex gap-1 flex-wrap">
                        {modelDetail.tags.map((tag) => (
                          <Badge key={tag} variant="outline" className="text-xs">
                            {tag}
                          </Badge>
                        ))}
                      </div>
                    </div>
                  )}

                  {/* Engine compatibility */}
                  {compatibleEngineEntries.length > 0 && (
                    <div className="space-y-1">
                      <p className="text-xs font-medium text-muted-foreground">Mapped Engine Names</p>
                      <div className="flex gap-1 flex-wrap">
                        {compatibleEngineEntries.map(([engine, name]) => (
                          <Badge key={engine} variant="secondary" className="text-xs">
                            {engine}: {name}
                          </Badge>
                        ))}
                      </div>
                    </div>
                  )}

                  {/* Files */}
                  {modelDetail.siblings && modelDetail.siblings.length > 0 && (
                    <div className="space-y-1">
                      <p className="text-xs font-medium text-muted-foreground">
                        Files ({modelDetail.siblings.length})
                      </p>
                      <div className="rounded-md border max-h-[200px] overflow-y-auto">
                        <div className="p-2 space-y-0.5">
                          {modelDetail.siblings.map((file) => (
                            <div
                              key={file.rfilename}
                              className="text-xs font-mono text-muted-foreground"
                            >
                              {file.rfilename}
                            </div>
                          ))}
                        </div>
                      </div>
                    </div>
                  )}

                  {/* Supported downloads */}
                  {modelDetail.supports_download.length > 0 && (
                    <div className="space-y-1">
                      <p className="text-xs font-medium text-muted-foreground">
                        Supports Download Via
                      </p>
                      <div className="flex gap-1 flex-wrap">
                        {modelDetail.supports_download.map((engine) => (
                          <Badge key={engine} variant="online" className="text-xs">
                            {engine}
                          </Badge>
                        ))}
                      </div>
                    </div>
                  )}
                </>
              )}
            </div>
          )}

          {/* Step 3: Select Endpoints */}
          {step === 'endpoints' && (
            <div className="space-y-4">
              {isLoadingEndpoints && (
                <div className="flex items-center justify-center py-8">
                  <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
                </div>
              )}

              {recommendations && (
                <>
                  {downloadableEndpoints.length === 0 ? (
                    <div className="flex flex-col items-center gap-2 py-8 text-muted-foreground">
                      <AlertCircle className="h-8 w-8" />
                      <p className="text-sm">No endpoints available that can download this model</p>
                    </div>
                  ) : (
                    <div className="space-y-2">
                      {downloadableEndpoints.map((ep) => (
                        <EndpointCheckRow
                          key={ep.id}
                          endpoint={ep}
                          checked={selectedEndpointIds.has(ep.id)}
                          onCheckedChange={() => toggleEndpoint(ep.id)}
                        />
                      ))}
                    </div>
                  )}
                </>
              )}
            </div>
          )}

          {/* Step 4: Download */}
          {step === 'download' && (
            <div className="space-y-3">
              {recommendations?.endpoints
                .filter((ep) => selectedEndpointIds.has(ep.id))
                .map((ep) => {
                  const status = downloadStatuses[ep.id] ?? 'pending'
                  return (
                    <div key={ep.id} className="flex items-center gap-3 p-3 rounded-md border">
                      <div className="flex-1 min-w-0">
                        <p className="text-sm font-medium truncate">{ep.name}</p>
                        <p className="text-xs text-muted-foreground">{ep.endpoint_type}</p>
                      </div>
                      <DownloadStatusIcon status={status} />
                    </div>
                  )
                })}

              {!allDownloadsFinished && (
                <div className="space-y-1">
                  <Progress value={undefined} className="h-1" />
                  <p className="text-xs text-muted-foreground text-center">
                    Sending download requests...
                  </p>
                </div>
              )}

              {allDownloadsFinished && (
                <div className="text-center py-2">
                  <p className="text-sm text-muted-foreground">
                    All download requests have been sent. Check endpoint details for progress.
                  </p>
                </div>
              )}
            </div>
          )}
        </div>

        <DialogFooter>
          {step !== 'search' && step !== 'download' && (
            <Button variant="outline" onClick={handleBack}>
              <ArrowLeft className="h-4 w-4 mr-1" />
              Back
            </Button>
          )}
          {step === 'detail' && (
            <Button onClick={handleProceedToEndpoints}>
              Select Endpoints
            </Button>
          )}
          {step === 'endpoints' && (
            <Button
              onClick={handleStartDownload}
              disabled={selectedEndpointIds.size === 0}
            >
              <Download className="h-4 w-4 mr-1" />
              Download to {selectedEndpointIds.size} Endpoint
              {selectedEndpointIds.size !== 1 ? 's' : ''}
            </Button>
          )}
          {step === 'download' && allDownloadsFinished && (
            <Button onClick={() => onOpenChange(false)}>
              <Check className="h-4 w-4 mr-1" />
              Done
            </Button>
          )}
          {step === 'search' && (
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function EndpointCheckRow({
  endpoint,
  checked,
  onCheckedChange,
}: {
  endpoint: RecommendedEndpoint
  checked: boolean
  onCheckedChange: () => void
}) {
  return (
    <div
      className="flex items-center gap-3 p-3 rounded-md border cursor-pointer hover:bg-muted/50"
      onClick={onCheckedChange}
    >
      <Checkbox checked={checked} onCheckedChange={() => onCheckedChange()} />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium truncate">{endpoint.name}</span>
          <Badge variant="outline" className="text-xs">
            {endpoint.endpoint_type}
          </Badge>
          {endpoint.has_model && (
            <Badge variant="secondary" className="text-xs">
              Already has model
            </Badge>
          )}
        </div>
      </div>
    </div>
  )
}

function DownloadStatusIcon({
  status,
}: {
  status: 'pending' | 'downloading' | 'completed' | 'failed'
}) {
  switch (status) {
    case 'pending':
      return <Loader2 className="h-4 w-4 text-muted-foreground" />
    case 'downloading':
      return <Loader2 className="h-4 w-4 animate-spin text-primary" />
    case 'completed':
      return <Check className="h-4 w-4 text-green-500" />
    case 'failed':
      return <AlertCircle className="h-4 w-4 text-destructive" />
  }
}
