import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { endpointsApi, type EndpointDailyStatEntry } from '@/lib/api'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Label } from '@/components/ui/label'
import { BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, Legend } from 'recharts'
import { BarChart3, Loader2 } from 'lucide-react'

/**
 * SPEC-8c32349f: Endpoint Request Chart (Phase 6)
 *
 * Recharts stacked bar chart showing daily request counts
 * (successful / failed) with 7/30/90 day period switching.
 */

interface EndpointRequestChartProps {
  endpointId: string
}

type DaysPeriod = '7' | '30' | '90'

function formatDateLabel(dateStr: string): string {
  // YYYY-MM-DD -> MM/DD
  const parts = dateStr.split('-')
  if (parts.length === 3) {
    return `${parts[1]}/${parts[2]}`
  }
  return dateStr
}

export function EndpointRequestChart({ endpointId }: EndpointRequestChartProps) {
  const [days, setDays] = useState<DaysPeriod>('7')

  const { data, isLoading } = useQuery<EndpointDailyStatEntry[]>({
    queryKey: ['endpoint-daily-stats', endpointId, days],
    queryFn: () => endpointsApi.getDailyStats(endpointId, Number(days)),
    enabled: !!endpointId,
  })

  const chartData = (data ?? []).map((entry) => ({
    date: formatDateLabel(entry.date),
    successful: entry.successful_requests,
    failed: entry.failed_requests,
  }))

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <Label className="flex items-center gap-2">
          <BarChart3 className="h-4 w-4" />
          Daily Requests
        </Label>
        <Tabs value={days} onValueChange={(v) => setDays(v as DaysPeriod)}>
          <TabsList className="h-8">
            <TabsTrigger value="7" className="px-2.5 py-1 text-xs">
              7D
            </TabsTrigger>
            <TabsTrigger value="30" className="px-2.5 py-1 text-xs">
              30D
            </TabsTrigger>
            <TabsTrigger value="90" className="px-2.5 py-1 text-xs">
              90D
            </TabsTrigger>
          </TabsList>
        </Tabs>
      </div>

      {isLoading ? (
        <div className="flex items-center justify-center py-8">
          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
          <span className="ml-2 text-sm text-muted-foreground">Loading chart...</span>
        </div>
      ) : chartData.length === 0 ? (
        <div className="flex items-center justify-center py-8 text-sm text-muted-foreground">
          No request data available
        </div>
      ) : (
        <ResponsiveContainer width="100%" height={220}>
          <BarChart data={chartData} margin={{ top: 4, right: 4, left: -12, bottom: 0 }}>
            <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
            <XAxis
              dataKey="date"
              tick={{ fontSize: 11 }}
              className="fill-muted-foreground"
              tickLine={false}
              axisLine={false}
            />
            <YAxis
              tick={{ fontSize: 11 }}
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
                fontSize: '12px',
              }}
              labelStyle={{ color: 'hsl(var(--popover-foreground))' }}
            />
            <Legend
              wrapperStyle={{ fontSize: '12px' }}
            />
            <Bar
              dataKey="successful"
              name="Successful"
              stackId="requests"
              fill="#22c55e"
              radius={[0, 0, 0, 0]}
            />
            <Bar
              dataKey="failed"
              name="Failed"
              stackId="requests"
              fill="#ef4444"
              radius={[2, 2, 0, 0]}
            />
          </BarChart>
        </ResponsiveContainer>
      )}
    </div>
  )
}
