import { useState, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  apiKeysApi,
  type ApiKey,
  type ApiKeyPermission,
  type CreateApiKeyResponse,
} from '@/lib/api'
import { useAuth } from '@/hooks/useAuth'
import {
  copyToClipboard,
  formatRelativeTime,
  selectTextForManualCopy,
  cleanupManualCopyBuffer,
} from '@/lib/utils'
import { toast } from '@/hooks/use-toast'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Checkbox } from '@/components/ui/checkbox'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import {
  Key,
  Plus,
  Trash2,
  Copy,
  Check,
  Eye,
  EyeOff,
  Loader2,
  RefreshCw,
} from 'lucide-react'

interface ApiKeyModalProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

const VIEWER_FIXED_PERMISSIONS: ApiKeyPermission[] = [
  'openai.inference',
  'openai.models.read',
]

const ADMIN_PERMISSION_OPTIONS: ApiKeyPermission[] = [
  'openai.inference',
  'openai.models.read',
  'endpoints.read',
  'endpoints.manage',
  'api_keys.manage',
  'users.manage',
  'invitations.manage',
  'models.manage',
  'registry.read',
  'logs.read',
  'metrics.read',
]

export function ApiKeyModal({ open, onOpenChange }: ApiKeyModalProps) {
  const { t } = useTranslation()
  const { user } = useAuth()
  const isAdmin = user?.role === 'admin'
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [deleteKey, setDeleteKey] = useState<ApiKey | null>(null)
  const [newKeyName, setNewKeyName] = useState('')
  const [newKeyExpires, setNewKeyExpires] = useState('')
  const [selectedPermissions, setSelectedPermissions] = useState<ApiKeyPermission[]>(
    VIEWER_FIXED_PERMISSIONS
  )
  const [createdKey, setCreatedKey] = useState<string | null>(null)
  const [showKey, setShowKey] = useState<string | null>(null)
  const [copiedId, setCopiedId] = useState<string | null>(null)
  const createdKeyCodeRef = useRef<HTMLElement | null>(null)

  // Fetch API keys
  const {
    data: apiKeys,
    isLoading,
    refetch,
  } = useQuery({
    queryKey: ['api-keys'],
    queryFn: apiKeysApi.list,
    enabled: open,
    // Plaintext keys are only shown once at creation time. We must not auto-refresh
    // this query while the modal is open, otherwise it becomes unclear whether the
    // key is still "copyable".
    refetchInterval: false,
    refetchOnWindowFocus: false,
  })

  const clearCreatedKeyState = () => {
    setCreatedKey(null)
    setShowKey(null)
    setCopiedId(null)
    cleanupManualCopyBuffer()
  }

  const resetCreateForm = () => {
    setNewKeyName('')
    setNewKeyExpires('')
    setSelectedPermissions(VIEWER_FIXED_PERMISSIONS)
  }

  const handleMainOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) {
      clearCreatedKeyState()
    }
    onOpenChange(nextOpen)
  }

  const handleCreateOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) {
      resetCreateForm()
    }
    setCreateOpen(nextOpen)
  }

  const handleOpenCreateDialog = () => {
    resetCreateForm()
    setCreateOpen(true)
  }

  const selectCreatedKeyForManualCopy = (text: string) => {
    if (typeof window === 'undefined' || typeof document === 'undefined') {
      return
    }

    const selection = window.getSelection()
    if (selection && createdKeyCodeRef.current) {
      const range = document.createRange()
      range.selectNodeContents(createdKeyCodeRef.current)
      selection.removeAllRanges()
      selection.addRange(range)
      return
    }

    selectTextForManualCopy(text)
  }

  // Create API key mutation
  const createMutation = useMutation({
    mutationFn: (data: {
      name: string
      expires_at?: string
      permissions?: ApiKeyPermission[]
    }) =>
      apiKeysApi.create(data),
    onSuccess: (data: CreateApiKeyResponse) => {
      // Update list without refetching, so the "created key" stays visible/copyable
      // until the user explicitly refreshes or closes the modal.
      queryClient.setQueryData(['api-keys'], (old?: ApiKey[]) => {
        const next = Array.isArray(old) ? old : []
        const withoutDup = next.filter((k) => k.id !== data.id)
        const created: ApiKey = {
          id: data.id,
          name: data.name,
          key_prefix: data.key_prefix,
          created_at: data.created_at,
          expires_at: data.expires_at,
          permissions: data.permissions,
        }
        return [created, ...withoutDup]
      })

      setCreatedKey(data.key)
      setShowKey(null)
      setCopiedId(null)
      resetCreateForm()
      setCreateOpen(false)
      toast({ title: t('apiKeys.toastCreated') })
    },
    onError: (error) => {
      toast({
        title: t('apiKeys.toastCreateFailed'),
        description: error instanceof Error ? error.message : t('apiKeys.unknownError'),
        variant: 'destructive',
      })
    },
  })

  // Delete API key mutation
  const deleteMutation = useMutation({
    mutationFn: (id: string) => apiKeysApi.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['api-keys'] })
      setDeleteKey(null)
      toast({ title: t('apiKeys.toastDeleted') })
    },
    onError: (error) => {
      toast({
        title: t('apiKeys.toastDeleteFailed'),
        description: error instanceof Error ? error.message : t('apiKeys.unknownError'),
        variant: 'destructive',
      })
    },
  })

  const handleCopy = async (text: string, id: string) => {
    try {
      const { method } = await copyToClipboard(text)
      if (method !== 'manual') {
        setCopiedId(id)
        setTimeout(() => setCopiedId(null), 2000)
        toast({ title: t('apiKeys.toastCopied') })
        return
      }

      setCopiedId(null)
      if (id === 'created') {
        setShowKey('created')
        window.setTimeout(() => selectCreatedKeyForManualCopy(text), 0)
      } else {
        selectTextForManualCopy(text)
      }
      toast({
        title: t('apiKeys.toastAutoCopyUnavailable'),
        description: t('apiKeys.toastAutoCopyDescription'),
      })
    } catch {
      toast({ title: t('apiKeys.toastCopyFailed'), variant: 'destructive' })
    }
  }

  const handleCreate = () => {
    const payload: {
      name: string
      expires_at?: string
      permissions?: ApiKeyPermission[]
    } = {
      name: newKeyName,
      expires_at: newKeyExpires || undefined,
    }

    if (isAdmin) {
      payload.permissions = selectedPermissions
    }

    createMutation.mutate(payload)
  }

  const handleTogglePermission = (permission: ApiKeyPermission, checked: boolean) => {
    if (!isAdmin) return

    setSelectedPermissions((prev) => {
      if (checked) {
        if (prev.includes(permission)) return prev
        return [...prev, permission]
      }
      return prev.filter((p) => p !== permission)
    })
  }

  const isExpired = (expiresAt: string | null | undefined) => {
    if (!expiresAt) return false
    return new Date(expiresAt) < new Date()
  }

  return (
    <>
      <Dialog open={open} onOpenChange={handleMainOpenChange}>
        <DialogContent id="api-keys-modal" className="max-w-3xl max-h-[80vh] overflow-hidden">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Key className="h-5 w-5" />
              {t('apiKeys.title')}
            </DialogTitle>
            <DialogDescription>
              {t('apiKeys.description')}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-4">
            {/* Actions */}
            <div className="flex justify-between">
              <Button id="create-api-key" onClick={handleOpenCreateDialog}>
                <Plus className="mr-2 h-4 w-4" />
                {t('apiKeys.createKey')}
              </Button>
              <Button
                variant="outline"
                size="icon"
                aria-label={t('apiKeys.refresh')}
                title={t('apiKeys.refresh')}
                onClick={() => {
                  // Enforce: plaintext keys are copyable only immediately after creation.
                  // Any "refresh" action should make copying impossible, requiring re-creation.
                  clearCreatedKeyState()
                  refetch()
                }}
              >
                <RefreshCw className="h-4 w-4" />
              </Button>
            </div>

            {/* Created Key Alert */}
            {createdKey && (
              <div className="rounded-lg border border-success/50 bg-success/10 p-4">
                <p className="text-sm font-medium text-success mb-2">
                  {t('apiKeys.createdSuccess')}
                </p>
                <p className="text-xs text-muted-foreground mb-2">
                  {t('apiKeys.copyNowWarning')}
                </p>
                <div className="flex items-center gap-2">
                  <code
                    ref={createdKeyCodeRef}
                    className="flex-1 rounded bg-muted px-2 py-1 text-xs font-mono break-all"
                  >
                    {showKey === 'created' ? createdKey : '•'.repeat(32)}
                  </code>
                  <Button
                    variant="outline"
                    size="icon"
                    aria-label={
                      showKey === 'created'
                        ? t('apiKeys.hideApiKey')
                        : t('apiKeys.showApiKey')
                    }
                    onClick={() => setShowKey(showKey === 'created' ? null : 'created')}
                  >
                    {showKey === 'created' ? (
                      <EyeOff className="h-4 w-4" />
                    ) : (
                      <Eye className="h-4 w-4" />
                    )}
                  </Button>
                  <Button
                    id="copy-api-key"
                    variant="outline"
                    size="icon"
                    aria-label={t('apiKeys.copyFullKey')}
                    title={t('apiKeys.copyFullKey')}
                    data-copied={copiedId === 'created' ? 'true' : 'false'}
                    onClick={() => handleCopy(createdKey, 'created')}
                  >
                    {copiedId === 'created' ? (
                      <Check className="h-4 w-4" />
                    ) : (
                      <Copy className="h-4 w-4" />
                    )}
                  </Button>
                </div>
              </div>
            )}

            {/* API Keys Table */}
            <ScrollArea className="h-64 rounded-md border">
              {isLoading ? (
                <div className="flex h-full items-center justify-center">
                  <Loader2 className="h-6 w-6 animate-spin" />
                </div>
              ) : !apiKeys || (apiKeys as ApiKey[]).length === 0 ? (
                <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
                  <Key className="h-8 w-8" />
                  <p>{t('apiKeys.noKeys')}</p>
                </div>
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t('apiKeys.columnName')}</TableHead>
                      <TableHead>{t('apiKeys.columnAccess')}</TableHead>
                      <TableHead>{t('apiKeys.columnKeyPrefix')}</TableHead>
                      <TableHead>{t('apiKeys.columnCreated')}</TableHead>
                      <TableHead>{t('apiKeys.columnExpires')}</TableHead>
                      <TableHead className="text-right">{t('apiKeys.columnActions')}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {(apiKeys as ApiKey[]).map((key) => (
                      <TableRow key={key.id}>
                        <TableCell className="font-medium">{key.name}</TableCell>
                        <TableCell>
                          <div className="flex flex-wrap gap-1">
                            {key.permissions.map((permission) => (
                              <Badge key={`${key.id}-${permission}`} variant="secondary">
                                {permission}
                              </Badge>
                            ))}
                          </div>
                        </TableCell>
                        <TableCell>
                          <span
                            className="text-xs text-muted-foreground select-none"
                            title={t('apiKeys.keyPrefixTooltip')}
                          >
                            ••••••••••
                          </span>
                        </TableCell>
                        <TableCell className="text-sm text-muted-foreground">
                          {formatRelativeTime(key.created_at)}
                        </TableCell>
                        <TableCell>
                          {key.expires_at ? (
                            <Badge
                              variant={isExpired(key.expires_at) ? 'destructive' : 'outline'}
                            >
                              {isExpired(key.expires_at)
                                ? t('apiKeys.expired')
                                : formatRelativeTime(key.expires_at)}
                            </Badge>
                          ) : (
                            <Badge variant="secondary">{t('apiKeys.never')}</Badge>
                          )}
                        </TableCell>
                        <TableCell className="text-right">
                          <Button
                            variant="outline"
                            size="icon"
                            className="h-8 w-8"
                            onClick={() => setDeleteKey(key)}
                          >
                            <Trash2 className="h-4 w-4 text-destructive" />
                          </Button>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </ScrollArea>
          </div>
        </DialogContent>
      </Dialog>

      {/* Create Key Dialog */}
      <Dialog open={createOpen} onOpenChange={handleCreateOpenChange}>
        <DialogContent className="max-w-xl max-h-[80vh] overflow-y-auto border-border bg-card text-card-foreground">
          <DialogHeader>
            <DialogTitle>{t('apiKeys.createTitle')}</DialogTitle>
            <DialogDescription>
              {t('apiKeys.createDescription')}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-4">
            <div className="space-y-2">
              <Label htmlFor="api-key-name">{t('apiKeys.nameLabel')}</Label>
              <Input
                id="api-key-name"
                placeholder={t('apiKeys.namePlaceholder')}
                value={newKeyName}
                onChange={(e) => setNewKeyName(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="key-expires">{t('apiKeys.expiresLabel')}</Label>
              <Input
                id="key-expires"
                type="datetime-local"
                value={newKeyExpires}
                onChange={(e) => setNewKeyExpires(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label className="text-foreground">{t('apiKeys.accessLabel')}</Label>
              <div className="rounded-md border border-border/70 bg-muted/20 p-3 text-sm text-muted-foreground">
                {isAdmin ? (
                  <div className="space-y-3">
                    <p>{t('apiKeys.selectPermissions')}</p>
                    <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                      {ADMIN_PERMISSION_OPTIONS.map((permission) => {
                        const checked = selectedPermissions.includes(permission)
                        return (
                          <label
                            key={permission}
                            className="flex items-center gap-2 rounded border border-border/50 bg-background/50 px-2 py-1.5"
                          >
                            <Checkbox
                              checked={checked}
                              onCheckedChange={(value) =>
                                handleTogglePermission(permission, value === true)
                              }
                            />
                            <span className="font-mono text-xs text-foreground">
                              {permission}
                            </span>
                          </label>
                        )
                      })}
                    </div>
                  </div>
                ) : (
                  <>
                    {t('apiKeys.viewerKeysInclude')}
                    <div className="mt-2 flex flex-wrap gap-1">
                      <Badge variant="secondary">openai.inference</Badge>
                      <Badge variant="secondary">openai.models.read</Badge>
                    </div>
                  </>
                )}
              </div>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => handleCreateOpenChange(false)}>
              {t('apiKeys.cancel')}
            </Button>
            <Button
              onClick={handleCreate}
              disabled={
                !newKeyName ||
                createMutation.isPending ||
                (isAdmin && selectedPermissions.length === 0)
              }
            >
              {createMutation.isPending && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              {t('apiKeys.create')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation Dialog */}
      <AlertDialog open={!!deleteKey} onOpenChange={() => setDeleteKey(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('apiKeys.deleteTitle')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('apiKeys.deleteConfirm', { name: deleteKey?.name })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('apiKeys.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => deleteKey && deleteMutation.mutate(deleteKey.id)}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {deleteMutation.isPending && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              {t('apiKeys.delete')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
