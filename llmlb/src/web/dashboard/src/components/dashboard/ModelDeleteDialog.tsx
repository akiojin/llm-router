import { useMutation, useQueryClient } from '@tanstack/react-query'
import { queryKeys } from '@/lib/query-keys';
import { endpointsApi } from '@/lib/api'
import { toast } from '@/hooks/use-toast'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { AlertCircle, Loader2, Trash2 } from 'lucide-react'

interface ModelDeleteDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  modelId: string
  endpointId: string
  endpointName: string
  endpointType: string
  onDeleted?: () => void
}

const DELETABLE_TYPES = new Set(['xllm', 'ollama'])

export function ModelDeleteDialog({
  open,
  onOpenChange,
  modelId,
  endpointId,
  endpointName,
  endpointType,
  onDeleted,
}: ModelDeleteDialogProps) {
  const queryClient = useQueryClient()
  const supportsDelete = DELETABLE_TYPES.has(endpointType)

  const deleteMutation = useMutation({
    mutationFn: () => endpointsApi.deleteModel(endpointId, modelId),
    onSuccess: () => {
      toast({
        title: 'Model deleted',
        description: `${modelId} has been removed from ${endpointName}`,
      })
      queryClient.invalidateQueries({ queryKey: queryKeys.endpointModels(endpointId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.dashboardEndpoints })
      queryClient.invalidateQueries({ queryKey: queryKeys.models })
      onOpenChange(false)
      onDeleted?.()
    },
    onError: (error) => {
      toast({
        title: 'Delete failed',
        description: error instanceof Error ? error.message : 'Unknown error',
        variant: 'destructive',
      })
    },
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Trash2 className="h-5 w-5 text-destructive" />
            Delete Model
          </DialogTitle>
          <DialogDescription>
            Remove a model from an endpoint
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          <div className="space-y-2">
            <div className="flex items-center gap-2">
              <span className="text-sm text-muted-foreground">Model:</span>
              <span className="font-mono text-sm font-medium">{modelId}</span>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-sm text-muted-foreground">Endpoint:</span>
              <span className="text-sm font-medium">{endpointName}</span>
              <Badge variant="outline" className="text-xs">
                {endpointType}
              </Badge>
            </div>
          </div>

          {!supportsDelete && (
            <div className="flex items-start gap-2 p-3 rounded-md bg-destructive/10 text-destructive">
              <AlertCircle className="h-4 w-4 mt-0.5 shrink-0" />
              <div className="text-sm">
                {`This endpoint type (${endpointType}) does not support model deletion from the dashboard. Please remove the model directly from the endpoint.`}
              </div>
            </div>
          )}

          {supportsDelete && (
            <div className="flex items-start gap-2 p-3 rounded-md bg-destructive/10 text-destructive">
              <AlertCircle className="h-4 w-4 mt-0.5 shrink-0" />
              <div className="text-sm">
                This action will permanently remove <strong>{modelId}</strong> from{' '}
                <strong>{endpointName}</strong>. This cannot be undone.
              </div>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          {supportsDelete && (
            <Button
              variant="destructive"
              onClick={() => deleteMutation.mutate()}
              disabled={deleteMutation.isPending}
            >
              {deleteMutation.isPending && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              Delete
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
