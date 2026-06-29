import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import {
  clientsApi,
  type ClientDetailResponse,
  type ClientApiKeyUsage,
  type ModelDistribution,
} from '@/lib/api'
import { ModelDistributionPie } from './ModelDistributionPie'
import { BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer } from 'recharts'
import { Loader2 } from 'lucide-react'

interface ClientDrilldownProps {
  ip: string
}

export function ClientDrilldown({ ip }: ClientDrilldownProps) {
  const { t } = useTranslation()
  const { data, isLoading } = useQuery<ClientDetailResponse>({
    queryKey: ['client-detail', ip],
    queryFn: () => clientsApi.getClientDetail(ip),
  })

  const { data: apiKeysData } = useQuery<ClientApiKeyUsage[]>({
    queryKey: ['client-api-keys', ip],
    queryFn: () => clientsApi.getClientApiKeys(ip),
  })

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-8">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    )
  }

  if (!data || data.total_requests === 0) {
    return (
      <div className="py-4 text-center text-sm text-muted-foreground">
        {t('analytics.noDataForThisIp')}
      </div>
    )
  }

  const apiKeys = apiKeysData ?? []

  return (
    <div className="space-y-4 p-4">
      {/* Summary */}
      <div className="flex flex-wrap gap-4 text-sm">
        <div>
          <span className="text-muted-foreground">{t('analytics.totalLabel')}</span>
          <span className="font-medium">{t('analytics.requestsCount', { count: data.total_requests.toLocaleString() })}</span>
        </div>
        {data.first_seen && (
          <div>
            <span className="text-muted-foreground">{t('analytics.firstLabel')}</span>
            <span>{formatDate(data.first_seen)}</span>
          </div>
        )}
        {data.last_seen && (
          <div>
            <span className="text-muted-foreground">{t('analytics.lastLabel')}</span>
            <span>{formatDate(data.last_seen)}</span>
          </div>
        )}
      </div>

      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        {/* Recent requests */}
        <div className="rounded-md border">
          <div className="border-b bg-muted/50 px-3 py-2 text-xs font-medium">
            {t('analytics.recentRequests')}
          </div>
          <div className="max-h-48 overflow-y-auto">
            <table className="w-full text-xs">
              <tbody>
                {data.recent_requests.map((r) => (
                  <tr key={r.id} className="border-b last:border-0">
                    <td className="px-3 py-1.5 text-muted-foreground">
                      {formatTime(r.timestamp)}
                    </td>
                    <td className="px-3 py-1.5 font-mono">{r.model}</td>
                    <td className="px-3 py-1.5 text-right tabular-nums">
                      {r.duration_ms != null ? `${r.duration_ms}ms` : '-'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>

        {/* Model distribution mini pie */}
        <div className="rounded-md border">
          <div className="border-b bg-muted/50 px-3 py-2 text-xs font-medium">
            {t('analytics.modelDistribution')}
          </div>
          <div className="p-2">
            <ModelDistributionPie data={data.model_distribution as ModelDistribution[]} />
          </div>
        </div>

        {/* Hourly pattern mini bar chart */}
        <div className="rounded-md border">
          <div className="border-b bg-muted/50 px-3 py-2 text-xs font-medium">
            {t('analytics.hourlyPattern')}
          </div>
          <div className="p-2">
            {data.hourly_pattern.every((p) => p.count === 0) ? (
              <div className="flex items-center justify-center py-8 text-xs text-muted-foreground">
                {t('analytics.noHourlyData')}
              </div>
            ) : (
              <ResponsiveContainer width="100%" height={200}>
                <BarChart data={data.hourly_pattern} margin={{ top: 4, right: 4, left: -20, bottom: 0 }}>
                  <XAxis
                    dataKey="hour"
                    tick={{ fontSize: 9 }}
                    className="fill-muted-foreground"
                    tickLine={false}
                    axisLine={false}
                  />
                  <YAxis
                    tick={{ fontSize: 9 }}
                    className="fill-muted-foreground"
                    tickLine={false}
                    axisLine={false}
                    allowDecimals={false}
                  />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: 'hsl(var(--popover))',
                      border: '1px solid hsl(var(--border))',
                      borderRadius: '6px',
                      fontSize: '11px',
                    }}
                    formatter={(value) => [Number(value ?? 0), t('analytics.requests')]}
                    labelFormatter={(label) => `${String(label ?? '')}:00`}
                  />
                  <Bar dataKey="count" fill="hsl(var(--chart-3))" radius={[2, 2, 0, 0]} />
                </BarChart>
              </ResponsiveContainer>
            )}
          </div>
        </div>

        {/* API Keys */}
        <div className="rounded-md border">
          <div className="border-b bg-muted/50 px-3 py-2 text-xs font-medium">
            {t('analytics.apiKeys')}
          </div>
          <div className="max-h-48 overflow-y-auto">
            {apiKeys.length === 0 ? (
              <div className="flex items-center justify-center py-8 text-xs text-muted-foreground">
                {t('analytics.noApiKeyData')}
              </div>
            ) : (
              <table className="w-full text-xs">
                <tbody>
                  {apiKeys.map((k) => (
                    <tr key={k.api_key_id} className="border-b last:border-0">
                      <td className="px-3 py-1.5">
                        {k.name ?? <span className="text-muted-foreground italic">{t('analytics.deleted')}</span>}
                      </td>
                      <td className="px-3 py-1.5 text-right tabular-nums">
                        {k.request_count.toLocaleString()}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

function formatDate(dateStr: string): string {
  try {
    return new Date(dateStr).toLocaleString()
  } catch {
    return dateStr
  }
}

function formatTime(dateStr: string): string {
  try {
    return new Date(dateStr).toLocaleTimeString()
  } catch {
    return dateStr
  }
}
