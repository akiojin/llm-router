import { Fragment } from 'react'
import { useTranslation } from 'react-i18next'

export interface HeatmapCell {
  day_of_week: number
  hour: number
  count: number
}

interface RequestHeatmapProps {
  data: HeatmapCell[]
}

const DAY_KEYS = ['dayMon', 'dayTue', 'dayWed', 'dayThu', 'dayFri', 'daySat', 'daySun'] as const

/** Map day_of_week (0=Sun) to row index (Mon=0 .. Sun=6) */
function dayToRow(dayOfWeek: number): number {
  return dayOfWeek === 0 ? 6 : dayOfWeek - 1
}

export function RequestHeatmap({ data }: RequestHeatmapProps) {
  const { t } = useTranslation()
  const maxCount = data.reduce((max, cell) => Math.max(max, cell.count), 0)

  // Build a lookup: grid[row][hour] = count
  const grid: number[][] = Array.from({ length: 7 }, () => Array(24).fill(0))
  for (const cell of data) {
    const row = dayToRow(cell.day_of_week)
    if (row >= 0 && row < 7 && cell.hour >= 0 && cell.hour < 24) {
      grid[row][cell.hour] = cell.count
    }
  }

  function cellOpacity(count: number): number {
    if (maxCount === 0) return 0.05
    if (count === 0) return 0.05
    return 0.05 + (count / maxCount) * 0.95
  }

  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: 'auto repeat(24, 1fr)',
        gridTemplateRows: 'auto repeat(7, 1fr)',
        gap: '2px',
      }}
    >
      {/* Top-left empty cell */}
      <div />

      {/* Column headers: hours 0-23 */}
      {Array.from({ length: 24 }, (_, h) => (
        <div
          key={`h-${h}`}
          className="text-xs text-muted-foreground"
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            minWidth: 24,
            height: 20,
          }}
        >
          {h}
        </div>
      ))}

      {/* Rows: one per day */}
      {DAY_KEYS.map((dayKey, row) => {
        const label = t(`requests.${dayKey}`)
        return (
        <Fragment key={`row-${row}`}>
          {/* Row label */}
          <div
            className="text-xs text-muted-foreground"
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'flex-end',
              paddingRight: 6,
              height: 28,
            }}
          >
            {label}
          </div>

          {/* 24 hour cells for this day */}
          {grid[row].map((count, hour) => (
            <div
              key={`cell-${row}-${hour}`}
              title={t('requests.heatmapCellTitle', {
                day: label,
                time: `${String(hour).padStart(2, '0')}:00`,
                count,
              })}
              style={{
                width: '100%',
                aspectRatio: '1',
                minWidth: 24,
                maxWidth: 32,
                borderRadius: 3,
                backgroundColor: `hsl(var(--chart-1))`,
                opacity: cellOpacity(count),
              }}
            />
          ))}
        </Fragment>
        )
      })}
    </div>
  )
}
