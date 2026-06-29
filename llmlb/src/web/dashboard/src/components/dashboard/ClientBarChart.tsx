import { type ClientIpRanking } from '@/lib/api'
import { BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts'

interface ClientBarChartProps {
  rankings: ClientIpRanking[]
}

function truncateIp(ip: string): string {
  if (ip.length > 20) {
    return ip.slice(0, 17) + '...'
  }
  return ip
}

export function ClientBarChart({ rankings }: ClientBarChartProps) {
  const chartData = rankings.slice(0, 10).map((r) => ({
    ip: truncateIp(r.ip),
    fullIp: r.ip,
    requests: r.request_count,
  }))

  if (chartData.length === 0) {
    return (
      <div className="flex items-center justify-center py-8 text-sm text-muted-foreground">
        No client data available
      </div>
    )
  }

  return (
    <ResponsiveContainer width="100%" height={300}>
      <BarChart data={chartData} margin={{ top: 4, right: 4, left: -12, bottom: 0 }}>
        <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
        <XAxis
          dataKey="ip"
          tick={{ fontSize: 10 }}
          className="fill-muted-foreground"
          tickLine={false}
          axisLine={false}
          angle={-45}
          textAnchor="end"
          height={80}
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
          formatter={(value) => [Number(value ?? 0), 'Requests']}
          labelFormatter={(label, payload) =>
            payload?.[0]?.payload?.fullIp ?? String(label ?? '')
          }
        />
        <Bar
          dataKey="requests"
          fill="hsl(var(--chart-1))"
          radius={[4, 4, 0, 0]}
        />
      </BarChart>
    </ResponsiveContainer>
  )
}
