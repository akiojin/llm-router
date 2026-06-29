import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts'

export interface UniqueIpTimelinePoint {
  hour: string
  unique_ips: number
}

interface UniqueIpTimelineProps {
  data: UniqueIpTimelinePoint[]
}

function formatHour(isoTimestamp: string): string {
  try {
    const date = new Date(isoTimestamp)
    return `${String(date.getHours()).padStart(2, '0')}:00`
  } catch {
    return isoTimestamp
  }
}

export function UniqueIpTimeline({ data }: UniqueIpTimelineProps) {
  if (data.length === 0) {
    return (
      <div className="flex items-center justify-center py-8 text-sm text-muted-foreground">
        No timeline data available
      </div>
    )
  }

  return (
    <ResponsiveContainer width="100%" height={300}>
      <LineChart data={data} margin={{ top: 4, right: 4, left: -12, bottom: 0 }}>
        <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
        <XAxis
          dataKey="hour"
          tickFormatter={formatHour}
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
          allowDecimals={false}
        />
        <Tooltip
          contentStyle={{
            backgroundColor: 'hsl(var(--popover))',
            border: '1px solid hsl(var(--border))',
            borderRadius: '6px',
            fontSize: '12px',
          }}
          labelStyle={{ color: 'hsl(var(--popover-foreground))' }}
          formatter={(value) => [Number(value ?? 0), 'Unique IPs']}
          labelFormatter={(label) => formatHour(String(label ?? ''))}
        />
        <Line
          type="monotone"
          dataKey="unique_ips"
          stroke="hsl(var(--chart-2))"
          strokeWidth={2}
          dot={false}
        />
      </LineChart>
    </ResponsiveContainer>
  )
}
