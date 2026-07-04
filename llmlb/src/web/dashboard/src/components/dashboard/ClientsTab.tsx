import { useState, useMemo } from 'react'
import { queryKeys } from '@/lib/query-keys';
import { useQuery } from '@tanstack/react-query'
import {
  clientsApi,
  type ClientRankingResponse,
  type UniqueIpTimelinePoint,
  type ModelDistribution,
  type HeatmapCell,
} from '@/lib/api'
import { ClientBarChart } from './ClientBarChart'
import { ClientRankingTable } from './ClientRankingTable'
import { UniqueIpTimeline } from './UniqueIpTimeline'
import { ModelDistributionPie } from './ModelDistributionPie'
import { RequestHeatmap } from './RequestHeatmap'
import { AlertThresholdSettings } from './AlertThresholdSettings'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Users, TrendingUp, PieChart, Grid3X3, Loader2, X } from 'lucide-react'

export function ClientsTab() {
  const [page, setPage] = useState(1)
  const perPage = 20

  // URLのクエリパラメーターからIPフィルタを取得
  const ipFilter = useMemo(() => {
    const params = new URLSearchParams(window.location.search)
    return params.get('ip') || undefined
  }, [])

  const { data, isLoading } = useQuery<ClientRankingResponse>({
    queryKey: queryKeys.clientRanking(page, perPage, ipFilter),
    queryFn: () => clientsApi.getClientRanking({ page, per_page: perPage, ip: ipFilter }),
  })

  const { data: timelineData } = useQuery<UniqueIpTimelinePoint[]>({
    queryKey: queryKeys.clientTimeline,
    queryFn: () => clientsApi.getTimeline(),
  })

  const { data: modelsData } = useQuery<ModelDistribution[]>({
    queryKey: queryKeys.clientModels,
    queryFn: () => clientsApi.getModels(),
  })

  const { data: heatmapData } = useQuery<HeatmapCell[]>({
    queryKey: queryKeys.clientHeatmap(ipFilter),
    queryFn: () => clientsApi.getHeatmap({ ip: ipFilter }),
  })

  const handleClearFilter = () => {
    const params = new URLSearchParams(window.location.search)
    params.delete('ip')
    const newSearch = params.toString()
    window.location.search = newSearch
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-16">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
        <span className="ml-2 text-muted-foreground">Loading client data...</span>
      </div>
    )
  }

  const rankings = data?.rankings ?? []
  const totalCount = data?.total_count ?? 0

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Users className="h-4 w-4" />
            Top Clients by Request Count
          </CardTitle>
        </CardHeader>
        <CardContent>
          <ClientBarChart rankings={rankings} />
        </CardContent>
      </Card>

      <div className="grid gap-6 md:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <TrendingUp className="h-4 w-4" />
              Unique IPs (24h)
            </CardTitle>
          </CardHeader>
          <CardContent>
            <UniqueIpTimeline data={timelineData ?? []} />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <PieChart className="h-4 w-4" />
              Model Distribution
            </CardTitle>
          </CardHeader>
          <CardContent>
            <ModelDistributionPie data={modelsData ?? []} />
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle className="flex items-center gap-2 text-base">
            <Grid3X3 className="h-4 w-4" />
            Request Heatmap (Hour x Day)
          </CardTitle>
          {ipFilter && (
            <div className="flex items-center gap-2">
              <Badge variant="secondary" className="text-xs">
                {`Filtered: ${ipFilter}`}
              </Badge>
              <button
                onClick={handleClearFilter}
                className="text-muted-foreground hover:text-foreground transition-colors"
                title="Clear IP filter"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
          )}
        </CardHeader>
        <CardContent>
          <RequestHeatmap data={heatmapData ?? []} />
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex flex-col items-start gap-3">
          <div className="flex w-full items-center justify-between">
            <CardTitle className="flex items-center gap-2 text-base">
              <Users className="h-4 w-4" />
              Client IP Ranking
            </CardTitle>
            <AlertThresholdSettings />
          </div>
          {ipFilter && (
            <div className="flex items-center gap-2">
              <Badge variant="secondary" className="text-xs">
                {`Filtered: ${ipFilter}`}
              </Badge>
              <button
                onClick={handleClearFilter}
                className="text-muted-foreground hover:text-foreground transition-colors"
                title="Clear IP filter"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
          )}
        </CardHeader>
        <CardContent>
          <ClientRankingTable
            rankings={rankings}
            totalCount={totalCount}
            page={page}
            perPage={perPage}
            onPageChange={setPage}
          />
        </CardContent>
      </Card>
    </div>
  )
}
