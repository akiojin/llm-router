import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { usersApi, type User, type CreateUserResponse } from '@/lib/api'
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
import { ScrollArea } from '@/components/ui/scroll-area'
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
  Users,
  Plus,
  Trash2,
  Edit,
  Loader2,
  RefreshCw,
  Shield,
  User as UserIcon,
  Copy,
  Check,
} from 'lucide-react'

interface UserModalProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function UserModal({ open, onOpenChange }: UserModalProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [createOpen, setCreateOpen] = useState(false)
  const [editUser, setEditUser] = useState<User | null>(null)
  const [deleteUser, setDeleteUser] = useState<User | null>(null)
  const [generatedPassword, setGeneratedPassword] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  // Form state
  const [formUsername, setFormUsername] = useState('')
  const [formPassword, setFormPassword] = useState('')
  const [formRole, setFormRole] = useState<'admin' | 'viewer'>('viewer')

  // Fetch users
  const { data: users, isLoading, refetch } = useQuery({
    queryKey: ['users'],
    queryFn: usersApi.list,
    enabled: open,
  })

  // Create user mutation
  const createMutation = useMutation({
    mutationFn: (data: { username: string; role: string }) =>
      usersApi.create(data),
    onSuccess: (result: CreateUserResponse) => {
      queryClient.invalidateQueries({ queryKey: ['users'] })
      resetForm()
      setCreateOpen(false)
      setGeneratedPassword(result.generated_password)
    },
    onError: (error) => {
      toast({
        title: t('users.failedToCreateUser'),
        description:
          error instanceof Error ? error.message : t('users.unknownError'),
        variant: 'destructive',
      })
    },
  })

  // Update user mutation
  const updateMutation = useMutation({
    mutationFn: ({
      id,
      data,
    }: {
      id: string
      data: { username?: string; password?: string; role?: string }
    }) => usersApi.update(id, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['users'] })
      resetForm()
      setEditUser(null)
      toast({ title: t('users.userUpdated') })
    },
    onError: (error) => {
      toast({
        title: t('users.failedToUpdateUser'),
        description:
          error instanceof Error ? error.message : t('users.unknownError'),
        variant: 'destructive',
      })
    },
  })

  // Delete user mutation
  const deleteMutation = useMutation({
    mutationFn: (id: string) => usersApi.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['users'] })
      setDeleteUser(null)
      toast({ title: t('users.userDeleted') })
    },
    onError: (error) => {
      toast({
        title: t('users.failedToDeleteUser'),
        description:
          error instanceof Error ? error.message : t('users.unknownError'),
        variant: 'destructive',
      })
    },
  })

  // Reset form
  const resetForm = () => {
    setFormUsername('')
    setFormPassword('')
    setFormRole('viewer')
  }

  useEffect(() => {
    if (!open) {
      cleanupManualCopyBuffer()
    }
  }, [open])

  const handleMainOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) {
      cleanupManualCopyBuffer()
    }
    onOpenChange(nextOpen)
  }

  const handleCreateOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) {
      resetForm()
    }
    setCreateOpen(nextOpen)
  }

  const handleOpenCreateDialog = () => {
    resetForm()
    setCreateOpen(true)
  }

  const handleOpenEditDialog = (user: User) => {
    setFormUsername(user.username)
    setFormPassword('')
    setFormRole(user.role as 'admin' | 'viewer')
    setEditUser(user)
  }

  const handleCloseEditDialog = () => {
    setEditUser(null)
    resetForm()
  }

  const handleCreate = () => {
    createMutation.mutate({
      username: formUsername,
      role: formRole,
    })
  }

  const handleUpdate = () => {
    if (!editUser) return
    const data: { username?: string; password?: string; role?: string } = {}
    if (formUsername !== editUser.username) data.username = formUsername
    if (formPassword) data.password = formPassword
    if (formRole !== editUser.role) data.role = formRole
    updateMutation.mutate({ id: editUser.id, data })
  }

  const handleCopyPassword = async () => {
    if (!generatedPassword) return
    try {
      const { method } = await copyToClipboard(generatedPassword)
      if (method !== 'manual') {
        setCopied(true)
        setTimeout(() => setCopied(false), 2000)
        toast({ title: t('users.copiedToClipboard') })
        return
      }

      setCopied(false)
      selectTextForManualCopy(generatedPassword)
      toast({
        title: t('users.autoCopyUnavailable'),
        description: t('users.autoCopyUnavailableDescription'),
      })
    } catch {
      toast({ title: t('users.failedToCopy'), variant: 'destructive' })
    }
  }

  const getRoleBadge = (role: string) => {
    if (role === 'admin') {
      return (
        <Badge variant="default" className="gap-1">
          <Shield className="h-3 w-3" />
          {t('users.admin')}
        </Badge>
      )
    }
    return (
      <Badge variant="secondary" className="gap-1">
        <UserIcon className="h-3 w-3" />
        {t('users.viewer')}
      </Badge>
    )
  }

  return (
    <>
      <Dialog open={open} onOpenChange={handleMainOpenChange}>
        <DialogContent className="max-w-2xl max-h-[80vh] overflow-hidden">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Users className="h-5 w-5" />
              {t('users.title')}
            </DialogTitle>
            <DialogDescription>
              {t('users.description')}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-4">
            {/* Actions */}
            <div className="flex justify-between">
              <Button onClick={handleOpenCreateDialog}>
                <Plus className="mr-2 h-4 w-4" />
                {t('users.addUser')}
              </Button>
              <Button variant="outline" size="icon" onClick={() => refetch()}>
                <RefreshCw className="h-4 w-4" />
              </Button>
            </div>

            {/* Users Table */}
            <ScrollArea className="h-64 rounded-md border">
              {isLoading ? (
                <div className="flex h-full items-center justify-center">
                  <Loader2 className="h-6 w-6 animate-spin" />
                </div>
              ) : !users || (users as User[]).length === 0 ? (
                <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
                  <Users className="h-8 w-8" />
                  <p>{t('users.noUsers')}</p>
                </div>
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t('users.username')}</TableHead>
                      <TableHead>{t('users.role')}</TableHead>
                      <TableHead>{t('users.created')}</TableHead>
                      <TableHead className="text-right">{t('users.actions')}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {(users as User[]).map((user) => (
                      <TableRow key={user.id}>
                        <TableCell className="font-medium">
                          {user.username}
                        </TableCell>
                        <TableCell>{getRoleBadge(user.role)}</TableCell>
                        <TableCell className="text-sm text-muted-foreground">
                          {formatRelativeTime(user.created_at)}
                        </TableCell>
                        <TableCell className="text-right">
                          <div className="flex justify-end gap-1">
                            <Button
                              variant="outline"
                              size="icon"
                              className="h-8 w-8"
                              onClick={() => handleOpenEditDialog(user)}
                            >
                              <Edit className="h-4 w-4" />
                            </Button>
                            <Button
                              variant="outline"
                              size="icon"
                              className="h-8 w-8"
                              onClick={() => setDeleteUser(user)}
                            >
                              <Trash2 className="h-4 w-4 text-destructive" />
                            </Button>
                          </div>
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

      {/* Create User Dialog */}
      <Dialog open={createOpen} onOpenChange={handleCreateOpenChange}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('users.createTitle')}</DialogTitle>
            <DialogDescription>
              {t('users.createDescription')}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-4">
            <div className="space-y-2">
              <Label htmlFor="create-username">{t('users.username')}</Label>
              <Input
                id="create-username"
                placeholder={t('users.usernamePlaceholder')}
                value={formUsername}
                onChange={(e) => setFormUsername(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="create-role">{t('users.role')}</Label>
              <Select value={formRole} onValueChange={(v) => setFormRole(v as 'admin' | 'viewer')}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="viewer">{t('users.viewer')}</SelectItem>
                  <SelectItem value="admin">{t('users.admin')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => handleCreateOpenChange(false)}>
              {t('users.cancel')}
            </Button>
            <Button
              onClick={handleCreate}
              disabled={!formUsername || createMutation.isPending}
            >
              {createMutation.isPending && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              {t('users.create')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Generated Password Dialog */}
      <Dialog
        open={!!generatedPassword}
        onOpenChange={(open) => {
          if (!open) {
            setGeneratedPassword(null)
            setCopied(false)
            cleanupManualCopyBuffer()
          }
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('users.userCreatedTitle')}</DialogTitle>
            <DialogDescription>
              {t('users.userCreatedDescription')}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-4">
            <div className="flex items-center gap-2">
              <code className="flex-1 rounded-md bg-muted px-3 py-2 font-mono text-sm">
                {generatedPassword}
              </code>
              <Button
                variant="outline"
                size="icon"
                onClick={handleCopyPassword}
              >
                {copied ? (
                  <Check className="h-4 w-4 text-green-500" />
                ) : (
                  <Copy className="h-4 w-4" />
                )}
              </Button>
            </div>
          </div>
          <DialogFooter>
            <Button
              onClick={() => {
                setGeneratedPassword(null)
                setCopied(false)
                cleanupManualCopyBuffer()
              }}
            >
              {t('users.savedPasswordButton')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit User Dialog */}
      <Dialog
        open={!!editUser}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) {
            handleCloseEditDialog()
          }
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('users.editTitle')}</DialogTitle>
            <DialogDescription>
              {t('users.editDescription', { username: editUser?.username })}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-4">
            <div className="space-y-2">
              <Label htmlFor="edit-username">{t('users.username')}</Label>
              <Input
                id="edit-username"
                value={formUsername}
                onChange={(e) => setFormUsername(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-password">{t('users.passwordLabel')}</Label>
              <Input
                id="edit-password"
                type="password"
                placeholder="••••••••"
                value={formPassword}
                onChange={(e) => setFormPassword(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-role">{t('users.role')}</Label>
              <Select value={formRole} onValueChange={(v) => setFormRole(v as 'admin' | 'viewer')}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="viewer">{t('users.viewer')}</SelectItem>
                  <SelectItem value="admin">{t('users.admin')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={handleCloseEditDialog}>
              {t('users.cancel')}
            </Button>
            <Button
              onClick={handleUpdate}
              disabled={!formUsername || updateMutation.isPending}
            >
              {updateMutation.isPending && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              {t('users.update')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation Dialog */}
      <AlertDialog open={!!deleteUser} onOpenChange={() => setDeleteUser(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('users.deleteTitle')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('users.deleteConfirmDescription', {
                username: deleteUser?.username,
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('users.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => deleteUser && deleteMutation.mutate(deleteUser.id)}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {deleteMutation.isPending && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              {t('users.delete')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
