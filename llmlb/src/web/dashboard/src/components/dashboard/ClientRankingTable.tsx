import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { type ClientIpRanking } from '@/lib/api'
import { ClientDrilldown } from './ClientDrilldown'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { ChevronLeft, ChevronRight, ChevronDown, ChevronUp } from 'lucide-react'

interface ClientRankingTableProps {
  rankings: ClientIpRanking[]
  totalCount: number
  page: number
  perPage: number
  onPageChange: (page: number) => void
}

export function ClientRankingTable({
  rankings,
  totalCount,
  page,
  perPage,
  onPageChange,
}: ClientRankingTableProps) {
  const { t } = useTranslation()
  const totalPages = Math.max(1, Math.ceil(totalCount / perPage))
  const [selectedIp, setSelectedIp] = useState<string | null>(null)

  function formatDate(dateStr: string): string {
    try {
      return new Date(dateStr).toLocaleString()
    } catch {
      return dateStr
    }
  }

  function toggleDrilldown(ip: string) {
    setSelectedIp((prev) => (prev === ip ? null : ip))
  }

  return (
    <div className="space-y-4">
      <div className="rounded-md border">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b bg-muted/50">
              <th className="w-8 px-2 py-3" />
              <th className="px-4 py-3 text-left font-medium">{t('analytics.ipAddress')}</th>
              <th className="px-4 py-3 text-right font-medium">{t('analytics.requests')}</th>
              <th className="px-4 py-3 text-left font-medium">{t('analytics.lastSeen')}</th>
              <th className="px-4 py-3 text-center font-medium">{t('analytics.status')}</th>
            </tr>
          </thead>
          <tbody>
            {rankings.length === 0 ? (
              <tr>
                <td colSpan={5} className="px-4 py-8 text-center text-muted-foreground">
                  {t('analytics.noClientData')}
                </td>
              </tr>
            ) : (
              rankings.map((r) => (
                <>
                  <tr
                    key={r.ip}
                    className={`border-b hover:bg-muted/30 cursor-pointer transition-colors ${
                      selectedIp === r.ip ? 'bg-muted/20' : ''
                    }`}
                    onClick={() => toggleDrilldown(r.ip)}
                  >
                    <td className="px-2 py-3 text-center text-muted-foreground">
                      {selectedIp === r.ip ? (
                        <ChevronUp className="h-4 w-4 inline" />
                      ) : (
                        <ChevronDown className="h-4 w-4 inline" />
                      )}
                    </td>
                    <td className="px-4 py-3 font-mono text-xs">{r.ip}</td>
                    <td className="px-4 py-3 text-right tabular-nums">{r.request_count.toLocaleString()}</td>
                    <td className="px-4 py-3 text-muted-foreground">{formatDate(r.last_seen)}</td>
                    <td className="px-4 py-3 text-center">
                      {r.is_alert && (
                        <Badge variant="destructive" className="text-xs">
                          {t('analytics.alert')}
                        </Badge>
                      )}
                    </td>
                  </tr>
                  {selectedIp === r.ip && (
                    <tr key={`${r.ip}-drilldown`} className="border-b">
                      <td colSpan={5} className="bg-muted/10">
                        <ClientDrilldown ip={r.ip} />
                      </td>
                    </tr>
                  )}
                </>
              ))
            )}
          </tbody>
        </table>
      </div>

      {totalCount > perPage && (
        <div className="flex items-center justify-between">
          <p className="text-sm text-muted-foreground">
            {t('analytics.clientsTotal', { count: totalCount })}
          </p>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => onPageChange(page - 1)}
              disabled={page <= 1}
            >
              <ChevronLeft className="h-4 w-4" />
            </Button>
            <span className="text-sm">
              {page} / {totalPages}
            </span>
            <Button
              variant="outline"
              size="sm"
              onClick={() => onPageChange(page + 1)}
              disabled={page >= totalPages}
            >
              <ChevronRight className="h-4 w-4" />
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}
