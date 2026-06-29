import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
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
  const { t } = useTranslation()
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
        placeholder={t('audit.searchPlaceholder')}
        value={searchText}
        onChange={(e) => handleSearchChange(e.target.value)}
        className="w-[200px]"
      />
      <Select
        value={filters.actor_type || 'all'}
        onValueChange={(v) => handleSelectChange('actor_type', v)}
      >
        <SelectTrigger className="w-[130px]">
          <SelectValue placeholder={t('audit.actorType')} />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">{t('audit.allActors')}</SelectItem>
          <SelectItem value="user">{t('audit.actorUser')}</SelectItem>
          <SelectItem value="api_key">{t('audit.actorApiKey')}</SelectItem>
          <SelectItem value="anonymous">{t('audit.actorAnonymous')}</SelectItem>
        </SelectContent>
      </Select>
      <Select
        value={filters.http_method || 'all'}
        onValueChange={(v) => handleSelectChange('http_method', v)}
      >
        <SelectTrigger className="w-[110px]">
          <SelectValue placeholder={t('audit.method')} />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">{t('audit.allMethods')}</SelectItem>
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
          <SelectValue placeholder={t('audit.status')} />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">{t('audit.allStatus')}</SelectItem>
          <SelectItem value="200">{t('audit.status200')}</SelectItem>
          <SelectItem value="201">{t('audit.status201')}</SelectItem>
          <SelectItem value="400">{t('audit.status400')}</SelectItem>
          <SelectItem value="401">{t('audit.status401')}</SelectItem>
          <SelectItem value="403">{t('audit.status403')}</SelectItem>
          <SelectItem value="404">{t('audit.status404')}</SelectItem>
          <SelectItem value="500">{t('audit.status500')}</SelectItem>
        </SelectContent>
      </Select>
    </div>
  )
}
