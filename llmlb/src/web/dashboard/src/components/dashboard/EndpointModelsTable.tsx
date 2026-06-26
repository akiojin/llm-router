import { useQuery } from '@tanstack/react-query'
import { type ModelTpsEntry, endpointsApi } from '@/lib/api'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Label } from '@/components/ui/label'
import { Loader2, Grid3X3 } from 'lucide-react'
import { useMemo } from 'react'

/**
 * SPEC-8c32349f: Unified endpoint models table
 * Consolidates Models, Production Throughput by Model/API (TPS), and Requests by Model
 * into a single integrated display showing all relevant metrics per model.
 */

interface EndpointModelsTableProps {
  endpointId: string
  enabled?: boolean
  headerActions?: React.ReactNode
}

interface EndpointModel {
  model_id: string
  capabilities?: string[]
  max_tokens?: number | null
  last_checked?: string
}

interface ModelRow {
  model_id: string
  max_tokens?: number | null
  tps: number | null
  request_count: number
  total_output_tokens: number
  average_duration_ms: number | null
  successful_requests: number
  total_requests: number
}

function formatTps(tps: number | null): string {
  if (tps === null || tps === undefined) return '—'
  return `${tps.toFixed(1)} tok/s`
}

function formatDuration(ms: number | null): string {
  if (ms === null || ms === undefined) return '—'
  if (ms < 1000) return `${ms.toFixed(0)} ms`
  return `${(ms / 1000).toFixed(1)} s`
}

function formatSuccessRate(entry: ModelRow): string {
  if (entry.total_requests === 0) return '-'
  const rate = (entry.successful_requests / entry.total_requests) * 100
  return `${rate.toFixed(1)}%`
}

function getSuccessRateColor(entry: ModelRow): string {
  if (entry.total_requests === 0) return ''
  const rate = (entry.successful_requests / entry.total_requests) * 100
  if (rate < 80) return 'bg-red-100 text-red-900'
  if (rate < 95) return 'bg-yellow-100 text-yellow-900'
  return ''
}

export function EndpointModelsTable({
  endpointId,
  enabled = true,
  headerActions,
}: EndpointModelsTableProps) {
  const { data: modelsData, isLoading: modelsLoading } = useQuery({
    queryKey: ['endpoint-models', endpointId],
    queryFn: () => endpointsApi.getModels(endpointId),
    enabled,
  })

  const { data: modelTps, isLoading: tpsLoading } = useQuery({
    queryKey: ['endpoint-model-tps', endpointId],
    queryFn: () => endpointsApi.getModelTps(endpointId),
    enabled,
    refetchInterval: 10000,
  })

  const { data: modelStats, isLoading: statsLoading } = useQuery({
    queryKey: ['endpoint-model-stats', endpointId],
    queryFn: () => endpointsApi.getModelStats(endpointId),
    enabled,
  })

  // Consolidate data from three sources using useMemo
  const consolidatedRows = useMemo(() => {
    const rows: Map<string, ModelRow> = new Map()

    // Start with models from getModels as the base
    const models: EndpointModel[] = modelsData?.models || []
    for (const model of models) {
      rows.set(model.model_id, {
        model_id: model.model_id,
        max_tokens: model.max_tokens,
        tps: null,
        request_count: 0,
        total_output_tokens: 0,
        average_duration_ms: null,
        successful_requests: 0,
        total_requests: 0,
      })
    }

    // Merge TPS data: find max TPS per model when multiple api_kind entries exist
    const tpsMap = new Map<string, ModelTpsEntry>()
    if (modelTps) {
      for (const entry of modelTps) {
        const existing = tpsMap.get(entry.model_id)
        if (!existing || (entry.tps ?? -1) > (existing.tps ?? -1)) {
          tpsMap.set(entry.model_id, entry)
        }
      }
    }

    for (const [modelId, tpsEntry] of tpsMap) {
      const row = rows.get(modelId) || {
        model_id: modelId,
        max_tokens: undefined,
        tps: null,
        request_count: 0,
        total_output_tokens: 0,
        average_duration_ms: null,
        successful_requests: 0,
        total_requests: 0,
      }
      row.tps = tpsEntry.tps
      row.request_count = tpsEntry.request_count
      row.total_output_tokens = tpsEntry.total_output_tokens
      row.average_duration_ms = tpsEntry.average_duration_ms
      rows.set(modelId, row)
    }

    // Merge stats data
    if (modelStats) {
      for (const stat of modelStats) {
        const row = rows.get(stat.model_id) || {
          model_id: stat.model_id,
          max_tokens: undefined,
          tps: null,
          request_count: 0,
          total_output_tokens: 0,
          average_duration_ms: null,
          successful_requests: 0,
          total_requests: 0,
        }
        row.successful_requests = stat.successful_requests
        row.total_requests = stat.total_requests
        rows.set(stat.model_id, row)
      }
    }

    return Array.from(rows.values())
  }, [modelsData, modelTps, modelStats])

  const isLoading = modelsLoading || tpsLoading || statsLoading

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <Label className="flex items-center gap-2">
          <Grid3X3 className="h-4 w-4" />
          Models ({consolidatedRows.length})
        </Label>
        {headerActions && <div className="flex gap-2">{headerActions}</div>}
      </div>

      {isLoading ? (
        <div className="flex items-center justify-center py-4">
          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
          <span className="ml-2 text-sm text-muted-foreground">Loading models...</span>
        </div>
      ) : consolidatedRows.length === 0 ? (
        <p className="text-sm text-muted-foreground text-center py-4">
          No models available
        </p>
      ) : (
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Model</TableHead>
                <TableHead className="text-right">ctx</TableHead>
                <TableHead className="text-right">TPS</TableHead>
                <TableHead className="text-right">Requests</TableHead>
                <TableHead className="text-right">Success%</TableHead>
                <TableHead className="text-right">Avg Duration</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {consolidatedRows.map((entry) => (
                <TableRow key={entry.model_id}>
                  <TableCell className="font-mono text-xs">{entry.model_id}</TableCell>
                  <TableCell className="text-right">
                    {entry.max_tokens ? `${(entry.max_tokens / 1024).toFixed(0)}K` : '—'}
                  </TableCell>
                  <TableCell className="text-right font-medium">
                    {formatTps(entry.tps)}
                  </TableCell>
                  <TableCell className="text-right">
                    {entry.request_count.toLocaleString()}
                  </TableCell>
                  <TableCell className={`text-right ${getSuccessRateColor(entry)}`}>
                    {formatSuccessRate(entry)}
                  </TableCell>
                  <TableCell className="text-right">
                    {formatDuration(entry.average_duration_ms)}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  )
}
