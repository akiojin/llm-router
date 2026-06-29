import { useCallback, useState } from 'react'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { AuditLogFilters as FilterType } from '@/lib/api'

interface AuditLogFiltersProps {
  filters: FilterType
  onFiltersChange: (filters: FilterType) => void
}

export function AuditLogFilters({ filters, onFiltersChange }: AuditLogFiltersProps) {
  const [searchText, setSearchText] = useState(filters.search || '')
  const [debounceTimer, setDebounceTimer] = useState<ReturnType<typeof setTimeout> | null>(null)

  const handleSearchChange = useCallback((value: string) => {
    setSearchText(value)
    if (debounceTimer) clearTimeout(debounceTimer)
    const timer = setTimeout(() => {
      onFiltersChange({ ...filters, search: value || undefined, page: 1 })
    }, 300)
    setDebounceTimer(timer)
  }, [filters, onFiltersChange, debounceTimer])

  const handleSelectChange = useCallback((key: keyof FilterType, value: string) => {
    onFiltersChange({
      ...filters,
      [key]: value === 'all' ? undefined : value,
      page: 1,
    })
  }, [filters, onFiltersChange])

  return (
    <div className="flex flex-wrap gap-3">
      <Input
        placeholder="Search..."
        value={searchText}
        onChange={(e) => handleSearchChange(e.target.value)}
        className="w-[200px]"
      />
      <Select
        value={filters.actor_type || 'all'}
        onValueChange={(v) => handleSelectChange('actor_type', v)}
      >
        <SelectTrigger className="w-[130px]">
          <SelectValue placeholder="Actor Type" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">All Actors</SelectItem>
          <SelectItem value="user">User</SelectItem>
          <SelectItem value="api_key">API Key</SelectItem>
          <SelectItem value="anonymous">Anonymous</SelectItem>
        </SelectContent>
      </Select>
      <Select
        value={filters.http_method || 'all'}
        onValueChange={(v) => handleSelectChange('http_method', v)}
      >
        <SelectTrigger className="w-[110px]">
          <SelectValue placeholder="Method" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">All Methods</SelectItem>
          <SelectItem value="GET">GET</SelectItem>
          <SelectItem value="POST">POST</SelectItem>
          <SelectItem value="PUT">PUT</SelectItem>
          <SelectItem value="DELETE">DELETE</SelectItem>
          <SelectItem value="PATCH">PATCH</SelectItem>
        </SelectContent>
      </Select>
      <Select
        value={filters.status_code?.toString() || 'all'}
        onValueChange={(v) => handleSelectChange('status_code', v)}
      >
        <SelectTrigger className="w-[130px]">
          <SelectValue placeholder="Status" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">All Status</SelectItem>
          <SelectItem value="200">200 OK</SelectItem>
          <SelectItem value="201">201 Created</SelectItem>
          <SelectItem value="400">400 Bad Request</SelectItem>
          <SelectItem value="401">401 Unauthorized</SelectItem>
          <SelectItem value="403">403 Forbidden</SelectItem>
          <SelectItem value="404">404 Not Found</SelectItem>
          <SelectItem value="500">500 Server Error</SelectItem>
        </SelectContent>
      </Select>
    </div>
  )
}
