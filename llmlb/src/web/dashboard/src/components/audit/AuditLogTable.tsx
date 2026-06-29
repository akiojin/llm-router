import { AuditLogEntry } from '@/lib/api'
import { formatRelativeTime } from '@/lib/utils'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Badge } from '@/components/ui/badge'

interface AuditLogTableProps {
  entries: AuditLogEntry[]
  loading?: boolean
}

function methodBadgeVariant(method: string): 'default' | 'secondary' | 'destructive' | 'outline' {
  switch (method) {
    case 'GET': return 'secondary'
    case 'POST': return 'default'
    case 'PUT': return 'outline'
    case 'DELETE': return 'destructive'
    case 'PATCH': return 'outline'
    default: return 'secondary'
  }
}

function statusColor(code: number): string {
  if (code >= 200 && code < 300) return 'text-success font-medium'
  if (code >= 300 && code < 400) return 'text-warning font-medium'
  if (code >= 400 && code < 500) return 'text-orange-600 dark:text-orange-400 font-medium'
  return 'text-destructive font-medium'
}

export function AuditLogTable({ entries, loading }: AuditLogTableProps) {
  if (loading) {
    return (
      <div className="flex items-center justify-center py-8 text-muted-foreground">
        Loading...
      </div>
    )
  }

  if (entries.length === 0) {
    return (
      <div className="flex items-center justify-center py-8 text-muted-foreground">
        No audit log entries found
      </div>
    )
  }

  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-[140px]">Timestamp</TableHead>
            <TableHead className="w-[80px]">Method</TableHead>
            <TableHead>Path</TableHead>
            <TableHead className="w-[60px]">Status</TableHead>
            <TableHead className="w-[80px]">Actor</TableHead>
            <TableHead className="w-[100px]">Actor ID</TableHead>
            <TableHead className="w-[120px]">Client IP</TableHead>
            <TableHead className="w-[80px]">Duration</TableHead>
            <TableHead className="w-[100px]">Tokens</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {entries.map((entry) => (
            <TableRow key={entry.id}>
              <TableCell className="text-xs text-muted-foreground">
                {formatRelativeTime(entry.timestamp)}
              </TableCell>
              <TableCell>
                <Badge variant={methodBadgeVariant(entry.http_method)}>
                  {entry.http_method}
                </Badge>
              </TableCell>
              <TableCell className="font-mono text-xs max-w-[300px] truncate">
                {entry.request_path}
              </TableCell>
              <TableCell className={statusColor(entry.status_code)}>
                {entry.status_code}
              </TableCell>
              <TableCell>
                <Badge variant="outline" className="text-xs">
                  {entry.actor_type}
                </Badge>
              </TableCell>
              <TableCell className="text-xs truncate max-w-[100px]">
                {entry.actor_username || entry.actor_id || '-'}
              </TableCell>
              <TableCell className="text-xs">
                {entry.client_ip ? (
                  <a
                    href={`?tab=clients&ip=${encodeURIComponent(entry.client_ip)}`}
                    className="text-blue-600 hover:text-blue-800 hover:underline cursor-pointer font-mono"
                  >
                    {entry.client_ip}
                  </a>
                ) : (
                  '-'
                )}
              </TableCell>
              <TableCell className="text-xs text-muted-foreground">
                {entry.duration_ms != null ? `${entry.duration_ms}ms` : '-'}
              </TableCell>
              <TableCell className="text-xs">
                {entry.total_tokens != null ? entry.total_tokens.toLocaleString() : '-'}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
