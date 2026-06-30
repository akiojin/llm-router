import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { clientsApi } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Settings2 } from 'lucide-react'

export function AlertThresholdSettings() {
  const queryClient = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [inputValue, setInputValue] = useState('')

  const { data } = useQuery({
    queryKey: ['alert-threshold'],
    queryFn: () => clientsApi.getAlertThreshold(),
  })

  const mutation = useMutation({
    mutationFn: (value: string) => clientsApi.updateAlertThreshold(value),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['alert-threshold'] })
      queryClient.invalidateQueries({ queryKey: ['client-ranking'] })
      setEditing(false)
    },
  })

  const threshold = data?.value ?? '100'

  function startEditing() {
    setInputValue(threshold)
    setEditing(true)
  }

  function save() {
    const num = parseInt(inputValue, 10)
    if (!isNaN(num) && num > 0) {
      mutation.mutate(String(num))
    }
  }

  if (editing) {
    return (
      <div className="flex items-center gap-2 text-sm">
        <Settings2 className="h-4 w-4 text-muted-foreground" />
        <span className="text-muted-foreground">Alert threshold (1h):</span>
        <Input
          type="number"
          min={1}
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          className="h-7 w-24"
          onKeyDown={(e) => {
            if (e.key === 'Enter') save()
            if (e.key === 'Escape') setEditing(false)
          }}
          autoFocus
        />
        <Button size="sm" variant="outline" className="h-7" onClick={save} disabled={mutation.isPending}>
          Save
        </Button>
        <Button size="sm" variant="ghost" className="h-7" onClick={() => setEditing(false)}>
          Cancel
        </Button>
      </div>
    )
  }

  return (
    <div className="flex items-center gap-2 text-sm">
      <Settings2 className="h-4 w-4 text-muted-foreground" />
      <span className="text-muted-foreground">Alert threshold (1h):</span>
      <span className="font-medium">{`${threshold} requests`}</span>
      <Button size="sm" variant="ghost" className="h-7" onClick={startEditing}>
        Edit
      </Button>
    </div>
  )
}
