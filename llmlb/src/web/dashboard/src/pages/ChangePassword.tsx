import { useState, useEffect } from 'react'
import { authApi } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  PasswordStrengthMeter,
  isPasswordValid,
} from '@/components/auth/PasswordStrengthMeter'
import { toast } from '@/hooks/use-toast'
import { Cpu, Lock } from 'lucide-react'

export default function ChangePasswordPage() {
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const [isCheckingAuth, setIsCheckingAuth] = useState(true)

  useEffect(() => {
    // Check if user is authenticated
    authApi.me()
      .then(() => setIsCheckingAuth(false))
      .catch(() => {
        window.location.href = '/dashboard/login.html'
      })
  }, [])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()

    if (!isPasswordValid(newPassword)) {
      toast({
        variant: 'destructive',
        title: 'Password too weak',
        description: 'Use at least 8 characters including an uppercase letter and a number.',
      })
      return
    }

    if (newPassword !== confirmPassword) {
      toast({
        variant: 'destructive',
        title: 'Validation error',
        description: 'Passwords do not match',
      })
      return
    }

    setIsLoading(true)

    try {
      await authApi.changePassword(newPassword)
      toast({
        title: 'Password changed',
        description: 'Please sign in with your new password',
      })
      // Redirect to login after short delay so toast is visible
      setTimeout(() => {
        window.location.href = '/dashboard/login.html'
      }, 1500)
    } catch {
      toast({
        variant: 'destructive',
        title: 'Failed to change password',
        description: 'Please try again',
      })
    } finally {
      setIsLoading(false)
    }
  }

  if (isCheckingAuth) {
    return (
      <div className="flex h-screen w-full items-center justify-center bg-background">
        <div className="flex flex-col items-center gap-4">
          <div className="relative">
            <div className="h-12 w-12 rounded-full border-4 border-primary/20" />
            <div className="absolute inset-0 h-12 w-12 animate-spin rounded-full border-4 border-transparent border-t-primary" />
          </div>
          <p className="text-sm text-muted-foreground">Loading...</p>
        </div>
      </div>
    )
  }

  return (
    <div className="relative min-h-screen w-full overflow-hidden bg-background">
      {/* Background Grid Pattern */}
      <div className="absolute inset-0 bg-grid opacity-30" />

      {/* Gradient Orbs */}
      <div className="absolute -left-40 -top-40 h-80 w-80 rounded-full bg-primary/20 blur-[100px]" />
      <div className="absolute -bottom-40 -right-40 h-80 w-80 rounded-full bg-primary/10 blur-[100px]" />

      {/* Content */}
      <div className="relative flex min-h-screen items-center justify-center p-4">
        <div className="w-full max-w-md animate-fade-up">
          {/* Logo */}
          <div className="mb-8 flex flex-col items-center gap-4">
            <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-primary/10 glow-sm">
              <Cpu className="h-8 w-8 text-primary" />
            </div>
            <div className="text-center">
              <h1 className="font-display text-3xl font-bold tracking-tight">
                LLM Load Balancer
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                Mission Control Dashboard
              </p>
            </div>
          </div>

          {/* Change Password Card */}
          <Card className="glass border-border/50">
            <CardHeader className="space-y-1">
              <CardTitle className="text-2xl font-semibold">Change Password</CardTitle>
              <CardDescription>You must change your password before continuing.</CardDescription>
            </CardHeader>
            <CardContent>
              <form onSubmit={handleSubmit} className="space-y-4">
                <div className="space-y-2">
                  <Label htmlFor="new-password">New Password</Label>
                  <div className="relative">
                    <Lock className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                    <Input
                      id="new-password"
                      type="password"
                      placeholder="Enter a new password"
                      value={newPassword}
                      onChange={(e) => setNewPassword(e.target.value)}
                      className="pl-10"
                      required
                      autoComplete="new-password"
                      autoFocus
                      aria-describedby={newPassword ? 'new-password-strength' : undefined}
                    />
                  </div>
                  <PasswordStrengthMeter id="new-password-strength" password={newPassword} />
                </div>

                <div className="space-y-2">
                  <Label htmlFor="confirm-password">Confirm Password</Label>
                  <div className="relative">
                    <Lock className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                    <Input
                      id="confirm-password"
                      type="password"
                      placeholder="Confirm new password"
                      value={confirmPassword}
                      onChange={(e) => setConfirmPassword(e.target.value)}
                      className="pl-10"
                      required
                      autoComplete="new-password"
                    />
                  </div>
                </div>

                <Button
                  type="submit"
                  variant="glow"
                  className="w-full"
                  disabled={isLoading || !newPassword || !confirmPassword}
                >
                  {isLoading ? (
                    <div className="flex items-center gap-2">
                      <div className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
                      Changing password...
                    </div>
                  ) : (
                    'Change Password'
                  )}
                </Button>
              </form>
            </CardContent>
          </Card>

          {/* Footer */}
          <p className="mt-6 text-center text-xs text-muted-foreground">
            LLM Load Balancer Dashboard v1.0
          </p>
        </div>
      </div>
    </div>
  )
}
