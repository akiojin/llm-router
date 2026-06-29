import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { invitationsApi, type Invitation, type CreateInvitationResponse } from '@/lib/api'
import {
  copyToClipboard,
  formatRelativeTime,
  selectTextForManualCopy,
  cleanupManualCopyBuffer,
} from '@/lib/utils'
import { toast } from '@/hooks/use-toast'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
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
  Ticket,
  Plus,
  Ban,
  Loader2,
  RefreshCw,
  Copy,
  Check,
  Clock,
  CheckCircle2,
  XCircle,
} from 'lucide-react'

interface InvitationModalProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function InvitationModal({ open, onOpenChange }: InvitationModalProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [expiresInHours, setExpiresInHours] = useState<number>(72)
  const [revokeInvitation, setRevokeInvitation] = useState<Invitation | null>(null)
  const [createdCode, setCreatedCode] = useState<CreateInvitationResponse | null>(null)
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (!open) {
      cleanupManualCopyBuffer()
    }
  }, [open])

  // Fetch invitations
  const { data: invitations, isLoading, refetch } = useQuery({
    queryKey: ['invitations'],
    queryFn: invitationsApi.list,
    enabled: open,
  })

  // Create invitation mutation
  const createMutation = useMutation({
    mutationFn: (hours: number) => invitationsApi.create(hours),
    onSuccess: (data) => {
      queryClient.invalidateQueries({ queryKey: ['invitations'] })
      setCreatedCode(data)
      toast({ title: t('invitations.createdToast') })
    },
    onError: (error) => {
      toast({
        title: t('invitations.createFailedTitle'),
        description: error instanceof Error ? error.message : t('invitations.unknownError'),
        variant: 'destructive',
      })
    },
  })

  // Revoke invitation mutation
  const revokeMutation = useMutation({
    mutationFn: (id: string) => invitationsApi.revoke(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['invitations'] })
      setRevokeInvitation(null)
      toast({ title: t('invitations.revokedToast') })
    },
    onError: (error) => {
      toast({
        title: t('invitations.revokeFailedTitle'),
        description: error instanceof Error ? error.message : t('invitations.unknownError'),
        variant: 'destructive',
      })
    },
  })

  const handleCreate = () => {
    createMutation.mutate(expiresInHours)
  }

  const handleCopy = async () => {
    if (createdCode?.code) {
      try {
        const { method } = await copyToClipboard(createdCode.code)
        if (method !== 'manual') {
          setCopied(true)
          setTimeout(() => setCopied(false), 2000)
          toast({ title: t('invitations.copiedToast') })
          return
        }

        setCopied(false)
        selectTextForManualCopy(createdCode.code)
        toast({
          title: t('invitations.autoCopyUnavailableTitle'),
          description: t('invitations.autoCopyUnavailableDescription'),
        })
      } catch {
        toast({ title: t('invitations.copyFailedTitle'), variant: 'destructive' })
      }
    }
  }

  const handleCloseCreatedDialog = () => {
    setCreatedCode(null)
    setCreateOpen(false)
    setCopied(false)
    cleanupManualCopyBuffer()
  }

  const getStatusBadge = (status: string) => {
    switch (status) {
      case 'active':
        return (
          <Badge variant="default" className="gap-1 bg-green-600">
            <Clock className="h-3 w-3" />
            {t('invitations.statusActive')}
          </Badge>
        )
      case 'used':
        return (
          <Badge variant="secondary" className="gap-1">
            <CheckCircle2 className="h-3 w-3" />
            {t('invitations.statusUsed')}
          </Badge>
        )
      case 'revoked':
        return (
          <Badge variant="destructive" className="gap-1">
            <XCircle className="h-3 w-3" />
            {t('invitations.statusRevoked')}
          </Badge>
        )
      default:
        return <Badge variant="outline">{status}</Badge>
    }
  }

  const isExpired = (expiresAt: string) => {
    return new Date(expiresAt) < new Date()
  }

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="max-w-3xl max-h-[80vh] overflow-hidden">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Ticket className="h-5 w-5" />
              {t('invitations.title')}
            </DialogTitle>
            <DialogDescription>
              {t('invitations.description')}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-4">
            {/* Actions */}
            <div className="flex justify-between">
              <Button onClick={() => setCreateOpen(true)}>
                <Plus className="mr-2 h-4 w-4" />
                {t('invitations.createCodeButton')}
              </Button>
              <Button variant="outline" size="icon" onClick={() => refetch()}>
                <RefreshCw className="h-4 w-4" />
              </Button>
            </div>

            {/* Invitations Table */}
            <ScrollArea className="h-80 rounded-md border">
              {isLoading ? (
                <div className="flex h-full items-center justify-center">
                  <Loader2 className="h-6 w-6 animate-spin" />
                </div>
              ) : !invitations || invitations.length === 0 ? (
                <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
                  <Ticket className="h-8 w-8" />
                  <p>{t('invitations.emptyState')}</p>
                </div>
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t('invitations.columnId')}</TableHead>
                      <TableHead>{t('invitations.columnStatus')}</TableHead>
                      <TableHead>{t('invitations.columnCreated')}</TableHead>
                      <TableHead>{t('invitations.columnExpires')}</TableHead>
                      <TableHead>{t('invitations.columnUsedBy')}</TableHead>
                      <TableHead className="text-right">{t('invitations.columnActions')}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {invitations.map((invitation) => (
                      <TableRow key={invitation.id}>
                        <TableCell className="font-mono text-xs">
                          {invitation.id.slice(0, 8)}...
                        </TableCell>
                        <TableCell>
                          {invitation.status === 'active' && isExpired(invitation.expires_at) ? (
                            <Badge variant="outline" className="gap-1">
                              <XCircle className="h-3 w-3" />
                              {t('invitations.statusExpired')}
                            </Badge>
                          ) : (
                            getStatusBadge(invitation.status)
                          )}
                        </TableCell>
                        <TableCell className="text-sm text-muted-foreground">
                          {formatRelativeTime(invitation.created_at)}
                        </TableCell>
                        <TableCell className="text-sm text-muted-foreground">
                          {formatRelativeTime(invitation.expires_at)}
                        </TableCell>
                        <TableCell className="font-mono text-xs text-muted-foreground">
                          {invitation.used_by ? `${invitation.used_by.slice(0, 8)}...` : '-'}
                        </TableCell>
                        <TableCell className="text-right">
                          {invitation.status === 'active' && !isExpired(invitation.expires_at) && (
                            <Button
                              variant="outline"
                              size="icon"
                              className="h-8 w-8"
                              onClick={() => setRevokeInvitation(invitation)}
                            >
                              <Ban className="h-4 w-4 text-destructive" />
                            </Button>
                          )}
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

      {/* Create Invitation Dialog */}
      <Dialog open={createOpen && !createdCode} onOpenChange={(open) => !open && setCreateOpen(false)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('invitations.createTitle')}</DialogTitle>
            <DialogDescription>
              {t('invitations.createDescription')}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-4">
            <div className="space-y-2">
              <Label htmlFor="expires-in">{t('invitations.expiresInLabel')}</Label>
              <Select
                value={String(expiresInHours)}
                onValueChange={(v) => setExpiresInHours(Number(v))}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="24">{t('invitations.expires24')}</SelectItem>
                  <SelectItem value="48">{t('invitations.expires48')}</SelectItem>
                  <SelectItem value="72">{t('invitations.expires72')}</SelectItem>
                  <SelectItem value="168">{t('invitations.expires168')}</SelectItem>
                  <SelectItem value="720">{t('invitations.expires720')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>
              {t('invitations.cancel')}
            </Button>
            <Button onClick={handleCreate} disabled={createMutation.isPending}>
              {createMutation.isPending && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              {t('invitations.create')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Created Code Display Dialog */}
      <Dialog open={!!createdCode} onOpenChange={handleCloseCreatedDialog}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2 text-green-600">
              <CheckCircle2 className="h-5 w-5" />
              {t('invitations.createdTitle')}
            </DialogTitle>
            <DialogDescription>
              {t('invitations.createdDescription')}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-4">
            <div className="space-y-2">
              <Label>{t('invitations.codeLabel')}</Label>
              <div className="flex gap-2">
                <code className="flex-1 rounded-md bg-muted px-4 py-3 font-mono text-sm break-all">
                  {createdCode?.code}
                </code>
                <Button
                  variant="outline"
                  size="icon"
                  onClick={handleCopy}
                  className="shrink-0"
                >
                  {copied ? (
                    <Check className="h-4 w-4 text-green-600" />
                  ) : (
                    <Copy className="h-4 w-4" />
                  )}
                </Button>
              </div>
            </div>
            <div className="text-sm text-muted-foreground">
              <p>{t('invitations.createdExpires', { value: createdCode ? new Date(createdCode.expires_at).toLocaleString() : '' })}</p>
            </div>
          </div>
          <DialogFooter>
            <Button onClick={handleCloseCreatedDialog}>{t('invitations.done')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Revoke Confirmation Dialog */}
      <AlertDialog open={!!revokeInvitation} onOpenChange={() => setRevokeInvitation(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('invitations.revokeTitle')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('invitations.revokeDescription')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('invitations.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => revokeInvitation && revokeMutation.mutate(revokeInvitation.id)}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {revokeMutation.isPending && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              {t('invitations.revoke')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
