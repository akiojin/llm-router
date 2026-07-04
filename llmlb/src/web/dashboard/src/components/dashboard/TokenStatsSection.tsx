import { useQuery } from '@tanstack/react-query'
import { queryKeys } from '@/lib/query-keys';
import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts'
import { dashboardApi, type DailyTokenStats, type MonthlyTokenStats } from '@/lib/api'
import { formatNumber } from '@/lib/utils'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Skeleton } from '@/components/ui/skeleton'
import { EmptyState } from '@/components/ui/empty-state'
import { MessageSquare, TrendingUp, Calendar } from 'lucide-react'

interface TokenChartDatum {
  label: string
  input: number
  output: number
}

/** 入力/出力トークンを期間ごとに積み上げ表示するバーチャート。 */
function TokenBarChart({ data }: { data: TokenChartDatum[] }) {
  return (
    <div
      role="img"
      aria-label="Token Statistics"
      className="mb-4 h-64 w-full"
    >
      <ResponsiveContainer width="100%" height="100%">
        <BarChart data={data} margin={{ top: 4, right: 8, left: -8, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" className="stroke-border" vertical={false} />
          <XAxis
            dataKey="label"
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
            width={48}
            tickFormatter={(v) => formatNumber(Number(v))}
          />
          <Tooltip
            contentStyle={{
              backgroundColor: 'hsl(var(--popover))',
              border: '1px solid hsl(var(--border))',
              borderRadius: '6px',
              fontSize: '12px',
            }}
            labelStyle={{ color: 'hsl(var(--popover-foreground))' }}
            formatter={(value, name) => [
              formatNumber(Number(value ?? 0)),
              name === 'input' ? 'Input' : 'Output',
            ]}
          />
          <Legend
            formatter={(value) => (value === 'input' ? 'Input' : 'Output')}
            wrapperStyle={{ fontSize: '12px' }}
          />
          <Bar dataKey="input" stackId="tokens" fill="hsl(var(--chart-1))" radius={[0, 0, 0, 0]} />
          <Bar dataKey="output" stackId="tokens" fill="hsl(var(--chart-2))" radius={[4, 4, 0, 0]} />
        </BarChart>
      </ResponsiveContainer>
    </div>
  )
}

/** ローディング中のアクセシブルなスケルトン表示（スクリーンリーダーへ通知）。 */
function StatsLoading({ rows }: { rows: number }) {
  return (
    <div role="status" aria-label="Loading" aria-busy="true" className="space-y-2">
      {[...Array(rows)].map((_, i) => (
        <Skeleton key={i} className="h-10" />
      ))}
    </div>
  )
}

export function TokenStatsSection() {
  const { data: dailyStats, isLoading: loadingDaily } = useQuery<DailyTokenStats[]>({
    queryKey: queryKeys.tokenStatsDaily,
    queryFn: () => dashboardApi.getDailyTokenStats(7),
  })

  const { data: monthlyStats, isLoading: loadingMonthly } = useQuery<MonthlyTokenStats[]>({
    queryKey: queryKeys.tokenStatsMonthly,
    queryFn: () => dashboardApi.getMonthlyTokenStats(6),
  })

  const dailyChart: TokenChartDatum[] = (dailyStats ?? []).map((s) => ({
    label: s.date,
    input: s.total_input_tokens,
    output: s.total_output_tokens,
  }))
  const monthlyChart: TokenChartDatum[] = (monthlyStats ?? []).map((s) => ({
    label: s.month,
    input: s.total_input_tokens,
    output: s.total_output_tokens,
  }))

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <MessageSquare className="h-5 w-5" />
          Token Statistics
        </CardTitle>
      </CardHeader>
      <CardContent>
        <Tabs defaultValue="daily" className="space-y-4">
          <TabsList>
            <TabsTrigger value="daily" className="gap-2">
              <TrendingUp className="h-4 w-4" />
              Daily
            </TabsTrigger>
            <TabsTrigger value="monthly" className="gap-2">
              <Calendar className="h-4 w-4" />
              Monthly
            </TabsTrigger>
          </TabsList>

          <TabsContent value="daily">
            {loadingDaily ? (
              <StatsLoading rows={5} />
            ) : dailyStats && dailyStats.length > 0 ? (
              <div className="space-y-2">
                <TokenBarChart data={dailyChart} />
                <div className="grid grid-cols-5 gap-2 text-sm font-medium text-muted-foreground border-b pb-2">
                  <div>Date</div>
                  <div className="text-right">Requests</div>
                  <div className="text-right">Input</div>
                  <div className="text-right">Output</div>
                  <div className="text-right">Total</div>
                </div>
                {dailyStats.map((stat) => (
                  <div key={stat.date} className="grid grid-cols-5 gap-2 text-sm py-2 border-b border-border/50">
                    <div className="font-medium">{stat.date}</div>
                    <div className="text-right">{formatNumber(stat.request_count)}</div>
                    <div className="text-right text-muted-foreground">{formatNumber(stat.total_input_tokens)}</div>
                    <div className="text-right text-muted-foreground">{formatNumber(stat.total_output_tokens)}</div>
                    <div className="text-right font-medium">{formatNumber(stat.total_tokens)}</div>
                  </div>
                ))}
              </div>
            ) : (
              <EmptyState
                icon={<MessageSquare className="h-10 w-10" />}
                title="No daily statistics available"
              />
            )}
          </TabsContent>

          <TabsContent value="monthly">
            {loadingMonthly ? (
              <StatsLoading rows={3} />
            ) : monthlyStats && monthlyStats.length > 0 ? (
              <div className="space-y-2">
                <TokenBarChart data={monthlyChart} />
                <div className="grid grid-cols-5 gap-2 text-sm font-medium text-muted-foreground border-b pb-2">
                  <div>Month</div>
                  <div className="text-right">Requests</div>
                  <div className="text-right">Input</div>
                  <div className="text-right">Output</div>
                  <div className="text-right">Total</div>
                </div>
                {monthlyStats.map((stat) => (
                  <div key={stat.month} className="grid grid-cols-5 gap-2 text-sm py-2 border-b border-border/50">
                    <div className="font-medium">{stat.month}</div>
                    <div className="text-right">{formatNumber(stat.request_count)}</div>
                    <div className="text-right text-muted-foreground">{formatNumber(stat.total_input_tokens)}</div>
                    <div className="text-right text-muted-foreground">{formatNumber(stat.total_output_tokens)}</div>
                    <div className="text-right font-medium">{formatNumber(stat.total_tokens)}</div>
                  </div>
                ))}
              </div>
            ) : (
              <EmptyState
                icon={<MessageSquare className="h-10 w-10" />}
                title="No monthly statistics available"
              />
            )}
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  )
}
