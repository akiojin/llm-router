import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import { dashboardApi, type DailyTokenStats, type MonthlyTokenStats } from '@/lib/api'
import { formatNumber } from '@/lib/utils'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { MessageSquare, TrendingUp, Calendar } from 'lucide-react'

export function TokenStatsSection() {
  const { t } = useTranslation()
  const { data: dailyStats, isLoading: loadingDaily } = useQuery<DailyTokenStats[]>({
    queryKey: ['token-stats-daily'],
    queryFn: () => dashboardApi.getDailyTokenStats(7),
  })

  const { data: monthlyStats, isLoading: loadingMonthly } = useQuery<MonthlyTokenStats[]>({
    queryKey: ['token-stats-monthly'],
    queryFn: () => dashboardApi.getMonthlyTokenStats(6),
  })

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <MessageSquare className="h-5 w-5" />
          {t('analytics.tokenStatistics')}
        </CardTitle>
      </CardHeader>
      <CardContent>
        <Tabs defaultValue="daily" className="space-y-4">
          <TabsList>
            <TabsTrigger value="daily" className="gap-2">
              <TrendingUp className="h-4 w-4" />
              {t('analytics.daily')}
            </TabsTrigger>
            <TabsTrigger value="monthly" className="gap-2">
              <Calendar className="h-4 w-4" />
              {t('analytics.monthly')}
            </TabsTrigger>
          </TabsList>

          <TabsContent value="daily">
            {loadingDaily ? (
              <div className="space-y-2">
                {[...Array(5)].map((_, i) => (
                  <div key={i} className="h-10 shimmer rounded" />
                ))}
              </div>
            ) : dailyStats && dailyStats.length > 0 ? (
              <div className="space-y-2">
                <div className="grid grid-cols-5 gap-2 text-sm font-medium text-muted-foreground border-b pb-2">
                  <div>{t('analytics.date')}</div>
                  <div className="text-right">{t('analytics.requests')}</div>
                  <div className="text-right">{t('analytics.input')}</div>
                  <div className="text-right">{t('analytics.output')}</div>
                  <div className="text-right">{t('analytics.total')}</div>
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
              <p className="text-muted-foreground text-center py-8">{t('analytics.noDailyStatistics')}</p>
            )}
          </TabsContent>

          <TabsContent value="monthly">
            {loadingMonthly ? (
              <div className="space-y-2">
                {[...Array(3)].map((_, i) => (
                  <div key={i} className="h-10 shimmer rounded" />
                ))}
              </div>
            ) : monthlyStats && monthlyStats.length > 0 ? (
              <div className="space-y-2">
                <div className="grid grid-cols-5 gap-2 text-sm font-medium text-muted-foreground border-b pb-2">
                  <div>{t('analytics.month')}</div>
                  <div className="text-right">{t('analytics.requests')}</div>
                  <div className="text-right">{t('analytics.input')}</div>
                  <div className="text-right">{t('analytics.output')}</div>
                  <div className="text-right">{t('analytics.total')}</div>
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
              <p className="text-muted-foreground text-center py-8">{t('analytics.noMonthlyStatistics')}</p>
            )}
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  )
}
