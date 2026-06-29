import { useState, useEffect, useRef } from 'react'
import { useQuery } from '@tanstack/react-query'
import { type LogEntry, dashboardApi } from '@/lib/api'
import { cn } from '@/lib/utils'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Switch } from '@/components/ui/switch'
import { Label } from '@/components/ui/label'
import { FileText, RefreshCw, Trash2, Server, Download } from 'lucide-react'
import { toast } from '@/hooks/use-toast'

/**
 * SPEC-e8e9326e: Router-Driven Endpoint Registration System
 * Log Viewer Component
 *
 * Note: Endpoints are external services (Ollama, vLLM, etc.) that do not push logs to the load balancer.
 * Currently displays load balancer logs only. Can be extended when endpoints provide log APIs.
 */

type LogLevel = 'all' | 'error' | 'warn' | 'info' | 'debug'

export function LogViewer() {
  const [levelFilter, setLevelFilter] = useState<LogLevel>('all')
  const [autoScroll, setAutoScroll] = useState(true)
  const scrollRef = useRef<HTMLDivElement>(null)

  // Fetch load balancer logs only (endpoints are external services without log API)
  const {
    data: routerLogs,
    refetch: refetchRouter,
    isRefetching,
  } = useQuery({
    queryKey: ['router-logs'],
    queryFn: () => dashboardApi.getRouterLogs({ limit: 200 }),
    refetchInterval: 5000,
  })

  const logs = routerLogs?.entries as LogEntry[] | undefined

  const filteredLogs = logs?.filter((log) => {
    if (levelFilter === 'all') return true
    return log.level.toLowerCase() === levelFilter
  })

  // Auto-scroll to bottom
  useEffect(() => {
    if (autoScroll && scrollRef.current) {
      const scrollContainer = scrollRef.current.querySelector('[data-radix-scroll-area-viewport]')
      if (scrollContainer) {
        scrollContainer.scrollTop = scrollContainer.scrollHeight
      }
    }
  }, [filteredLogs, autoScroll])

  const handleRefresh = () => {
    refetchRouter()
  }

  const handleClear = () => {
    toast({ title: 'Log clearing is handled on the server side' })
  }

  const handleDownload = () => {
    if (!filteredLogs || filteredLogs.length === 0) {
      toast({ title: 'No logs to download', variant: 'destructive' })
      return
    }

    const logText = filteredLogs
      .map((log) => `[${log.timestamp}] [${log.level}] ${log.target ? `[${log.target}] ` : ''}${log.message || ''}`)
      .join('\n')

    const blob = new Blob([logText], { type: 'text/plain' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `logs-llmlb-${new Date().toISOString().slice(0, 10)}.txt`
    a.click()
    URL.revokeObjectURL(url)
    toast({ title: 'Logs downloaded' })
  }

  const getLevelColor = (level: string) => {
    const l = level.toLowerCase()
    if (l === 'error') return 'text-destructive'
    if (l === 'warn' || l === 'warning') return 'text-warning'
    if (l === 'info') return 'text-primary'
    if (l === 'debug') return 'text-muted-foreground'
    return 'text-foreground'
  }

  return (
    <Card>
      <CardHeader className="pb-4">
        <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
          <CardTitle className="flex items-center gap-2">
            <FileText className="h-5 w-5" />
            Log Viewer
            {filteredLogs && (
              <Badge variant="secondary" className="ml-2">
                {filteredLogs.length}
              </Badge>
            )}
          </CardTitle>

          <div className="flex flex-wrap items-center gap-2">
            {/* Source Label (router only) */}
            <div className="flex items-center gap-2 px-3 py-2 border rounded-md bg-muted/30">
              <Server className="h-4 w-4" />
              <span className="text-sm font-medium">LLM Load Balancer</span>
            </div>

            {/* Level Filter */}
            <Select value={levelFilter} onValueChange={(v) => setLevelFilter(v as LogLevel)}>
              <SelectTrigger className="w-28">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All</SelectItem>
                <SelectItem value="error">Error</SelectItem>
                <SelectItem value="warn">Warn</SelectItem>
                <SelectItem value="info">Info</SelectItem>
                <SelectItem value="debug">Debug</SelectItem>
              </SelectContent>
            </Select>

            {/* Auto-scroll toggle */}
            <div className="flex items-center gap-2">
              <Switch
                id="auto-scroll"
                checked={autoScroll}
                onCheckedChange={setAutoScroll}
              />
              <Label htmlFor="auto-scroll" className="text-sm">
                Auto-scroll
              </Label>
            </div>
          </div>
        </div>
      </CardHeader>

      <CardContent>
        {/* Actions */}
        <div className="mb-4 flex gap-2">
          <Button
            id="logs-router-refresh"
            variant="outline"
            size="sm"
            onClick={handleRefresh}
            disabled={isRefetching}
          >
            <RefreshCw className={cn('mr-2 h-4 w-4', isRefetching && 'animate-spin')} />
            Refresh
          </Button>
          <Button variant="outline" size="sm" onClick={handleDownload}>
            <Download className="mr-2 h-4 w-4" />
            Download
          </Button>
          <Button variant="outline" size="sm" onClick={handleClear}>
            <Trash2 className="mr-2 h-4 w-4" />
            Clear
          </Button>
        </div>

        {/* Log Content */}
        <ScrollArea className="h-96 rounded-md border bg-muted/30" ref={scrollRef}>
          <div
            id="logs-router-list"
            className="p-4 font-mono text-xs space-y-0.5"
          >
            {!filteredLogs || filteredLogs.length === 0 ? (
              <div className="flex h-32 items-center justify-center text-muted-foreground">
                No logs available
              </div>
            ) : (
              filteredLogs.map((log, i) => (
                <div
                  key={i}
                  className="flex gap-2 py-0.5 hover:bg-muted/50 rounded px-1 -mx-1"
                >
                  <span className="text-muted-foreground shrink-0">
                    {new Date(log.timestamp).toLocaleTimeString()}
                  </span>
                  <span
                    className={cn(
                      'uppercase w-12 shrink-0 font-semibold',
                      getLevelColor(log.level)
                    )}
                  >
                    {log.level}
                  </span>
                  {log.target && (
                    <span className="text-muted-foreground shrink-0">
                      [{log.target}]
                    </span>
                  )}
                  <span className="break-all">{log.message || ''}</span>
                </div>
              ))
            )}
          </div>
        </ScrollArea>
      </CardContent>
    </Card>
  )
}
