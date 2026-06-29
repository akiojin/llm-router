import { useTranslation } from 'react-i18next'
import {
  Activity,
  AlertTriangle,
  CheckCircle2,
  Clock,
  Cpu,
  Gauge,
  HardDrive,
  ListChecks,
  MessageSquare,
  Server,
} from 'lucide-react'

import { type DashboardOverview } from '@/lib/api'
import {
  formatBytes,
  formatFullNumber,
  formatPercentage,
} from '@/lib/utils'

interface OperationsOverviewProps {
  overview?: DashboardOverview
  isLoading: boolean
}

interface MetricCellProps {
  dataStat: string
  label: string
  value: string
  detail?: string
  icon: React.ReactNode
}

function metricValue(value: number | null | undefined): string {
  return value == null ? '-' : formatFullNumber(value)
}

function byteValue(value: number | null | undefined): string {
  return value == null ? '-' : formatBytes(value, 1)
}

function tpsValue(value: number | null | undefined): string {
  if (value == null) return '-'
  return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)} tok/s`
}

function healthLabel(health: string | undefined, t: (key: string) => string): string {
  if (health === 'healthy') return t('dashboard.overview.healthy')
  if (health === 'attention') return t('dashboard.overview.attention')
  if (health === 'empty') return t('dashboard.overview.noEndpoints')
  return t('dashboard.overview.unknown')
}

function healthIcon(health: string | undefined) {
  if (health === 'healthy') {
    return <CheckCircle2 className="h-5 w-5 text-success" />
  }
  return <AlertTriangle className="h-5 w-5 text-warning" />
}

function MetricCell({ dataStat, label, value, detail, icon }: MetricCellProps) {
  return (
    <div
      data-stat={dataStat}
      className="min-h-28 rounded-md border border-border/70 bg-card px-4 py-3"
    >
      <div className="mb-3 flex items-center justify-between gap-3">
        <p className="text-xs font-medium uppercase text-muted-foreground">{label}</p>
        <div className="text-muted-foreground">{icon}</div>
      </div>
      <p className="text-3xl font-semibold tracking-tight">{value}</p>
      {detail ? <p className="mt-2 text-xs text-muted-foreground">{detail}</p> : null}
    </div>
  )
}

export function OperationsOverview({ overview, isLoading }: OperationsOverviewProps) {
  const { t } = useTranslation()
  const operations = overview?.operations
  const capacity = overview?.capacity
  const actionItems = overview?.action_items ?? []
  const loadingValue = isLoading ? '-' : undefined
  const failedAndImpaired =
    (operations?.failed_requests ?? 0) +
    (operations?.offline_endpoints ?? 0) +
    (operations?.queued_requests ?? 0)

  return (
    <section className="space-y-4" aria-label={t('dashboard.overview.regionLabel')}>
      <div className="grid gap-4 lg:grid-cols-[1.15fr_0.85fr]">
        <div
          data-stat="operational-health"
          className="rounded-md border border-border/70 bg-card px-5 py-4"
        >
          <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
            <div className="flex items-start gap-3">
              <div className="flex h-10 w-10 items-center justify-center rounded-md bg-muted">
                {healthIcon(operations?.health)}
              </div>
              <div>
                <p className="text-xs font-medium uppercase text-muted-foreground">
                  {t('dashboard.overview.operations')}
                </p>
                <p className="mt-1 text-3xl font-semibold tracking-tight">
                  {loadingValue ?? healthLabel(operations?.health, t)}
                </p>
                <p className="mt-2 text-sm text-muted-foreground">
                  {t('dashboard.overview.summary', {
                    online: metricValue(operations?.online_endpoints),
                    review: metricValue(failedAndImpaired),
                  })}
                </p>
              </div>
            </div>
            <div className="grid min-w-48 grid-cols-2 gap-2 text-sm">
              <div className="rounded-md bg-muted/50 px-3 py-2">
                <p className="text-xs text-muted-foreground">{t('dashboard.overview.active')}</p>
                <p className="font-semibold tabular-nums">
                  {metricValue(operations?.active_requests)}
                </p>
              </div>
              <div className="rounded-md bg-muted/50 px-3 py-2">
                <p className="text-xs text-muted-foreground">{t('dashboard.overview.queued')}</p>
                <p className="font-semibold tabular-nums">
                  {metricValue(operations?.queued_requests)}
                </p>
              </div>
            </div>
          </div>
        </div>

        <div
          data-section="action-items"
          className="rounded-md border border-border/70 bg-card px-5 py-4"
        >
          <div className="mb-3 flex items-center gap-2">
            <ListChecks className="h-4 w-4 text-muted-foreground" />
            <p className="text-sm font-medium">{t('dashboard.overview.actionItems')}</p>
          </div>
          <div className="space-y-2">
            {(isLoading ? [] : actionItems).map((item, index) => (
              <div
                key={`${item.title}-${index}`}
                className="flex items-start justify-between gap-3 rounded-md bg-muted/40 px-3 py-2"
              >
                <div className="min-w-0">
                  <p className="truncate text-sm font-medium">{item.title}</p>
                  <p className="truncate text-xs text-muted-foreground">{item.detail}</p>
                </div>
                <span className="rounded-md bg-background px-2 py-0.5 text-xs tabular-nums">
                  {item.count}
                </span>
              </div>
            ))}
            {isLoading ? (
              <div className="h-12 rounded-md bg-muted/50" />
            ) : null}
          </div>
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4 xl:grid-cols-8">
        <MetricCell
          dataStat="total-endpoints"
          label={t('dashboard.overview.endpoints')}
          value={loadingValue ?? metricValue(operations?.total_endpoints)}
          detail={t('dashboard.overview.detailOnline', {
            value: metricValue(operations?.online_endpoints),
          })}
          icon={<Server className="h-4 w-4" />}
        />
        <MetricCell
          dataStat="total-requests"
          label={t('dashboard.overview.requests')}
          value={loadingValue ?? metricValue(operations?.total_requests)}
          detail={t('dashboard.overview.detailSuccessful', {
            value: metricValue(operations?.successful_requests),
          })}
          icon={<Activity className="h-4 w-4" />}
        />
        <MetricCell
          dataStat="active-requests"
          label={t('dashboard.overview.active')}
          value={loadingValue ?? metricValue(operations?.active_requests)}
          detail={t('dashboard.overview.detailInFlight')}
          icon={<Cpu className="h-4 w-4" />}
        />
        <MetricCell
          dataStat="queued-requests"
          label={t('dashboard.overview.queued')}
          value={loadingValue ?? metricValue(operations?.queued_requests)}
          detail={t('dashboard.overview.detailWaiting')}
          icon={<Clock className="h-4 w-4" />}
        />
        <MetricCell
          dataStat="success-rate"
          label={t('dashboard.overview.success')}
          value={loadingValue ?? formatPercentage(operations?.success_rate)}
          detail={t('dashboard.overview.detailFailed', {
            value: metricValue(operations?.failed_requests),
          })}
          icon={<CheckCircle2 className="h-4 w-4" />}
        />
        <MetricCell
          dataStat="output-tps"
          label={t('dashboard.overview.outputTps')}
          value={loadingValue ?? tpsValue(operations?.output_tps)}
          detail={t('dashboard.overview.detailOutputTokens', {
            value: metricValue(operations?.total_output_tokens),
          })}
          icon={<Gauge className="h-4 w-4" />}
        />
        <MetricCell
          dataStat="total-tokens"
          label={t('dashboard.overview.tokens')}
          value={loadingValue ?? metricValue(operations?.total_tokens)}
          detail={t('dashboard.overview.detailOut', {
            value: metricValue(operations?.total_output_tokens),
          })}
          icon={<MessageSquare className="h-4 w-4" />}
        />
        <MetricCell
          dataStat="gpu-capacity"
          label={t('dashboard.overview.gpuEndpoints')}
          value={loadingValue ?? metricValue(capacity?.gpu_capable_endpoints)}
          detail={t('dashboard.overview.detailReporting', {
            value: metricValue(capacity?.gpu_telemetry_endpoints),
          })}
          icon={<HardDrive className="h-4 w-4" />}
        />
      </div>

      <div className="grid gap-4 lg:grid-cols-3">
        <div className="rounded-md border border-border/70 bg-card px-4 py-3">
          <p className="text-xs font-medium uppercase text-muted-foreground">
            {t('dashboard.overview.modelCapacity')}
          </p>
          <p className="mt-2 text-2xl font-semibold tracking-tight">
            {loadingValue ?? metricValue(capacity?.total_models)}
          </p>
        </div>
        <div className="rounded-md border border-border/70 bg-card px-4 py-3">
          <p className="text-xs font-medium uppercase text-muted-foreground">
            {t('dashboard.overview.gpuMemory')}
          </p>
          <p className="mt-2 text-2xl font-semibold tracking-tight">
            {loadingValue ?? byteValue(capacity?.used_gpu_memory_bytes)}
          </p>
          <p className="mt-1 text-xs text-muted-foreground">
            {t('dashboard.overview.detailTotal', {
              value: byteValue(capacity?.total_gpu_memory_bytes),
            })}
            {', '}
            {formatPercentage(capacity?.gpu_memory_usage_percent)}
          </p>
        </div>
        <div className="rounded-md border border-border/70 bg-card px-4 py-3">
          <p className="text-xs font-medium uppercase text-muted-foreground">
            {t('dashboard.overview.telemetry')}
          </p>
          <p className="mt-2 text-2xl font-semibold tracking-tight">
            {loadingValue ?? capacity?.telemetry_status ?? '-'}
          </p>
        </div>
      </div>
    </section>
  )
}
