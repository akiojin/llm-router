import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { type RequestHistoryItem } from '@/lib/api'
import {
  copyToClipboard,
  formatDuration,
  formatRelativeTime,
  selectTextForManualCopy,
  cleanupManualCopyBuffer,
} from '@/lib/utils'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { EmptyState } from '@/components/ui/empty-state'
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
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  History,
  ChevronLeft,
  ChevronRight,
  CheckCircle2,
  XCircle,
  Clock,
  Copy,
  Check,
} from 'lucide-react'
import { toast } from '@/hooks/use-toast'

interface RequestHistoryTableProps {
  history: RequestHistoryItem[]
  isLoading: boolean
}

const PAGE_SIZES = [25, 50, 100]

export function RequestHistoryTable({ history, isLoading }: RequestHistoryTableProps) {
  const { t } = useTranslation()
  const [pageSize, setPageSize] = useState(25)
  const [currentPage, setCurrentPage] = useState(1)
  const [statusFilter, setStatusFilter] = useState<'all' | 'success' | 'error'>('all')
  const [selectedRequest, setSelectedRequest] = useState<RequestHistoryItem | null>(null)
  const [copiedField, setCopiedField] = useState<string | null>(null)

  useEffect(() => {
    return () => cleanupManualCopyBuffer()
  }, [])

  const filteredHistory =
    statusFilter === 'all'
      ? history
      : history.filter((item) =>
          statusFilter === 'success' ? item.status === 'success' : item.status !== 'success'
        )
  const totalPages = Math.ceil(filteredHistory.length / pageSize)
  const paginatedHistory = filteredHistory.slice(
    (currentPage - 1) * pageSize,
    currentPage * pageSize
  )

  const handleCopy = async (text: string, field: string) => {
    try {
      const { method } = await copyToClipboard(text)
      if (method !== 'manual') {
        setCopiedField(field)
        setTimeout(() => setCopiedField(null), 2000)
        toast({ title: t('requests.copiedToClipboard') })
        return
      }

      setCopiedField(null)
      selectTextForManualCopy(text)
      toast({
        title: t('requests.autoCopyUnavailable'),
        description: t('requests.pressCtrlCToCopy'),
      })
    } catch {
      toast({ title: t('requests.failedToCopy'), variant: 'destructive' })
    }
  }

  const serializeBody = (body: unknown, kind: 'request' | 'response') => {
    const kindLabel =
      kind === 'request' ? t('requests.bodyKindRequest') : t('requests.bodyKindResponse')
    try {
      const value = JSON.stringify(body, null, 2)
      if (value === undefined) {
        return t('requests.noBody', { kind: kindLabel })
      }
      return value
    } catch {
      return t('requests.unableToDisplayBody', { kind: kindLabel })
    }
  }

  if (isLoading) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <History className="h-5 w-5" />
            {t('requests.requestHistory')}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-4">
            {[...Array(5)].map((_, i) => (
              <div key={i} className="h-12 shimmer rounded" />
            ))}
          </div>
        </CardContent>
      </Card>
    )
  }

  return (
    <>
      <Card>
        <CardHeader className="pb-4">
          <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
            <CardTitle className="flex items-center gap-2">
              <History className="h-5 w-5" />
              {t('requests.requestHistory')}
              <Badge variant="secondary" className="ml-2">
                {history.length}
              </Badge>
            </CardTitle>

            <div className="flex flex-wrap items-center gap-2">
              <Select
                value={statusFilter}
                onValueChange={(value) => {
                  setStatusFilter(value as 'all' | 'success' | 'error')
                  setCurrentPage(1)
                }}
              >
                <SelectTrigger
                  id="history-status-filter"
                  className="w-32"
                  aria-label={t('requests.filterByStatus')}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">{t('requests.allStatus')}</SelectItem>
                  <SelectItem value="success">{t('requests.success')}</SelectItem>
                  <SelectItem value="error">{t('requests.error')}</SelectItem>
                </SelectContent>
              </Select>
              <span className="text-sm text-muted-foreground">{t('requests.show')}</span>
              <Select
                value={pageSize.toString()}
                onValueChange={(value) => {
                  setPageSize(Number(value))
                  setCurrentPage(1)
                }}
              >
                <SelectTrigger id="history-per-page" className="w-20">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PAGE_SIZES.map((size) => (
                    <SelectItem key={size} value={size.toString()}>
                      {size}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <span className="text-sm text-muted-foreground">{t('requests.entries')}</span>
            </div>
          </div>
        </CardHeader>

        <CardContent className="px-0">
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('requests.time')}</TableHead>
                  <TableHead>{t('requests.model')}</TableHead>
                  <TableHead>{t('requests.node')}</TableHead>
                  <TableHead>{t('requests.clientIp')}</TableHead>
                  <TableHead>{t('requests.status')}</TableHead>
                  <TableHead>{t('requests.duration')}</TableHead>
                  <TableHead>{t('requests.tokens')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody id="request-history-tbody">
                {paginatedHistory.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={7} className="p-0">
                      <EmptyState
                        icon={<History className="h-8 w-8" />}
                        title={
                          statusFilter === 'all'
                            ? t('requests.noRequestHistory')
                            : t('requests.noRequestsMatchFilter')
                        }
                        description={
                          statusFilter === 'all'
                            ? t('requests.requestsWillAppearHere')
                            : undefined
                        }
                      />
                    </TableCell>
                  </TableRow>
                ) : (
                  paginatedHistory.map((item) => (
                    <TableRow
                      key={item.request_id}
                      className="cursor-pointer hover:bg-muted/50"
                      onClick={() => setSelectedRequest(item)}
                    >
                      <TableCell className="font-mono text-xs">
                        {formatRelativeTime(item.timestamp)}
                      </TableCell>
                      <TableCell>
                        <Badge variant="secondary">{item.model}</Badge>
                      </TableCell>
                      <TableCell className="text-sm">
                        {item.node_name || item.node_id?.slice(0, 8) || '—'}
                      </TableCell>
                      <TableCell className="font-mono text-xs">
                        {item.client_ip || '—'}
                      </TableCell>
                      <TableCell>
                        <Badge
                          variant={item.status === 'success' ? 'online' : 'destructive'}
                          className="gap-1"
                        >
                          {item.status === 'success' ? (
                            <CheckCircle2 className="h-3 w-3" />
                          ) : (
                            <XCircle className="h-3 w-3" />
                          )}
                          {item.status}
                        </Badge>
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-1 text-sm">
                          <Clock className="h-3 w-3 text-muted-foreground" />
                          {formatDuration(item.duration_ms)}
                        </div>
                      </TableCell>
                      <TableCell className="text-sm">
                        {item.total_tokens?.toLocaleString() || '—'}
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </div>

          {/* Pagination */}
          {totalPages > 1 && (
            <div className="flex items-center justify-between border-t px-6 py-4">
              <p className="text-sm text-muted-foreground">
                {t('requests.showingRange', {
                  from: (currentPage - 1) * pageSize + 1,
                  to: Math.min(currentPage * pageSize, history.length),
                  total: history.length,
                })}
              </p>
              <div className="flex items-center gap-2">
                <Button
                  id="history-page-prev"
                  variant="outline"
                  size="sm"
                  onClick={() => setCurrentPage((p) => Math.max(1, p - 1))}
                  disabled={currentPage === 1}
                >
                  <ChevronLeft className="h-4 w-4" />
                </Button>
                <span id="history-page-info" className="text-sm">
                  {t('requests.pageInfo', { current: currentPage, total: totalPages })}
                </span>
                <Button
                  id="history-page-next"
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

      {/* Request Detail Modal */}
      <Dialog
        open={!!selectedRequest}
        onOpenChange={(open) => {
          if (!open) {
            setSelectedRequest(null)
            cleanupManualCopyBuffer()
          }
        }}
      >
        <DialogContent id="request-modal" className="max-w-2xl max-h-[80vh] overflow-hidden">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <History className="h-5 w-5" />
              {t('requests.requestDetails')}
            </DialogTitle>
            <DialogDescription>
              {t('requests.requestIdLabel')}{' '}
              <code className="text-xs">{selectedRequest?.request_id}</code>
            </DialogDescription>
          </DialogHeader>

          {selectedRequest && (
            <Tabs defaultValue="overview" className="mt-4 min-w-0">
              <TabsList className="grid w-full grid-cols-3">
                <TabsTrigger value="overview">{t('requests.overview')}</TabsTrigger>
                <TabsTrigger value="request">{t('requests.request')}</TabsTrigger>
                <TabsTrigger value="response">{t('requests.response')}</TabsTrigger>
              </TabsList>

              <TabsContent value="overview" className="space-y-4 mt-4">
                <div className="grid grid-cols-2 gap-4">
                  <div className="space-y-1">
                    <p className="text-sm text-muted-foreground">{t('requests.model')}</p>
                    <Badge variant="secondary">{selectedRequest.model}</Badge>
                  </div>
                  <div className="space-y-1">
                    <p className="text-sm text-muted-foreground">{t('requests.status')}</p>
                    <Badge
                      variant={
                        selectedRequest.status === 'success' ? 'online' : 'destructive'
                      }
                    >
                      {selectedRequest.status}
                    </Badge>
                  </div>
                  <div className="space-y-1">
                    <p className="text-sm text-muted-foreground">{t('requests.node')}</p>
                    <p className="text-sm">
                      {selectedRequest.node_name || selectedRequest.node_id || '—'}
                    </p>
                  </div>
                  <div className="space-y-1">
                    <p className="text-sm text-muted-foreground">{t('requests.clientIp')}</p>
                    <p className="font-mono text-sm">
                      {selectedRequest.client_ip || '—'}
                    </p>
                  </div>
                  <div className="space-y-1">
                    <p className="text-sm text-muted-foreground">{t('requests.duration')}</p>
                    <p className="text-sm">{formatDuration(selectedRequest.duration_ms)}</p>
                  </div>
                  <div className="space-y-1">
                    <p className="text-sm text-muted-foreground">{t('requests.timestamp')}</p>
                    <p className="text-sm">
                      {new Date(selectedRequest.timestamp).toLocaleString()}
                    </p>
                  </div>
                  <div className="space-y-1">
                    <p className="text-sm text-muted-foreground">{t('requests.totalTokens')}</p>
                    <p className="text-sm">
                      {selectedRequest.total_tokens?.toLocaleString() || '—'}
                    </p>
                  </div>
                </div>

                {selectedRequest.error && (
                  <div className="rounded-lg border border-destructive/50 bg-destructive/10 p-4">
                    <p className="text-sm font-medium text-destructive">{t('requests.error')}</p>
                    <p className="mt-1 text-sm">{selectedRequest.error}</p>
                  </div>
                )}
              </TabsContent>

              <TabsContent value="request" className="mt-4">
                <div className="relative">
                  <Button
                    variant="outline"
                    size="sm"
                    className="absolute right-2 top-2"
                    onClick={() =>
                      handleCopy(
                        serializeBody(selectedRequest.request_body, 'request'),
                        'request'
                      )
                    }
                  >
                    {copiedField === 'request' ? (
                      <Check className="h-4 w-4" />
                    ) : (
                      <Copy className="h-4 w-4" />
                    )}
                  </Button>
                  <ScrollArea className="h-64 rounded-md border">
                    <pre className="p-4 text-xs whitespace-pre-wrap break-words">
                      {serializeBody(selectedRequest.request_body, 'request')}
                    </pre>
                  </ScrollArea>
                </div>
              </TabsContent>

              <TabsContent value="response" className="mt-4">
                <div className="relative">
                  <Button
                    variant="outline"
                    size="sm"
                    className="absolute right-2 top-2"
                    onClick={() =>
                      handleCopy(
                        serializeBody(selectedRequest.response_body, 'response'),
                        'response'
                      )
                    }
                  >
                    {copiedField === 'response' ? (
                      <Check className="h-4 w-4" />
                    ) : (
                      <Copy className="h-4 w-4" />
                    )}
                  </Button>
                  <ScrollArea className="h-64 rounded-md border">
                    <pre className="p-4 text-xs whitespace-pre-wrap break-words">
                      {serializeBody(selectedRequest.response_body, 'response')}
                    </pre>
                  </ScrollArea>
                </div>
              </TabsContent>
            </Tabs>
          )}
        </DialogContent>
      </Dialog>
    </>
  )
}
