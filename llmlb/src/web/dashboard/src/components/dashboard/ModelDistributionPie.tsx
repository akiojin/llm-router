import { PieChart, Pie, Tooltip, Cell, ResponsiveContainer } from 'recharts'

export interface ModelDistribution {
  model: string
  request_count: number
  percentage: number
}

interface ModelDistributionPieProps {
  data: ModelDistribution[]
}

const COLORS = [
  'hsl(var(--chart-1))',
  'hsl(var(--chart-2))',
  'hsl(var(--chart-3))',
  'hsl(var(--chart-4))',
  'hsl(var(--chart-5))',
]

export function ModelDistributionPie({ data }: ModelDistributionPieProps) {
  if (data.length === 0) {
    return (
      <div className="flex items-center justify-center py-8 text-sm text-muted-foreground">
        No model data available
      </div>
    )
  }

  return (
    <ResponsiveContainer width="100%" height={300}>
      <PieChart>
        <Pie
          data={data}
          dataKey="request_count"
          nameKey="model"
          cx="50%"
          cy="50%"
          outerRadius={100}
          innerRadius={60}
          label={({ name, percent }) =>
            `${String(name ?? '')} (${((percent ?? 0) * 100).toFixed(1)}%)`
          }
        >
          {data.map((_entry, index) => (
            <Cell key={`cell-${index}`} fill={COLORS[index % COLORS.length]} />
          ))}
        </Pie>
        <Tooltip
          contentStyle={{
            backgroundColor: 'hsl(var(--popover))',
            border: '1px solid hsl(var(--border))',
            borderRadius: '6px',
            fontSize: '12px',
          }}
          labelStyle={{ color: 'hsl(var(--popover-foreground))' }}
        />
      </PieChart>
    </ResponsiveContainer>
  )
}
