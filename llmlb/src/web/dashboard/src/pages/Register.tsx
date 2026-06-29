import { useState } from 'react'
import { authApi, ApiError } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  PasswordStrengthMeter,
  isPasswordValid,
} from '@/components/auth/PasswordStrengthMeter'
import { toast } from '@/hooks/use-toast'
import { Cpu, Lock, User, Ticket, CheckCircle2 } from 'lucide-react'

export default function RegisterPage() {
  const [invitationCode, setInvitationCode] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const [isSuccess, setIsSuccess] = useState(false)

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()

    if (password !== confirmPassword) {
      toast({
        variant: 'destructive',
        title: 'Password mismatch',
        description: 'Passwords do not match',
      })
      return
    }

    if (!isPasswordValid(password)) {
      toast({
        variant: 'destructive',
        title: 'Password too weak',
        description: 'Use at least 8 characters including an uppercase letter and a number.',
      })
      return
    }

    setIsLoading(true)

    try {
      await authApi.register({
        invitation_code: invitationCode,
        username,
        password,
      })
      setIsSuccess(true)
      toast({
        title: 'Registration successful',
        description: 'You can now sign in with your credentials',
      })
    } catch (error) {
      let message = 'Registration failed'
      if (error instanceof ApiError) {
        if (error.status === 400) {
          message = 'Invalid or expired invitation code'
        } else if (error.status === 409) {
          message = 'Username already exists'
        } else if (error.message) {
          message = error.message
        }
      }
      toast({
        variant: 'destructive',
        title: 'Registration failed',
        description: message,
      })
    } finally {
      setIsLoading(false)
    }
  }

  if (isSuccess) {
    return (
      <div className="relative min-h-screen w-full overflow-hidden bg-background">
        <div className="absolute inset-0 bg-grid opacity-30" />
        <div className="absolute -left-40 -top-40 h-80 w-80 rounded-full bg-primary/20 blur-[100px]" />
        <div className="absolute -bottom-40 -right-40 h-80 w-80 rounded-full bg-primary/10 blur-[100px]" />

        <div className="relative flex min-h-screen items-center justify-center p-4">
          <div className="w-full max-w-md animate-fade-up">
            <div className="mb-8 flex flex-col items-center gap-4">
              <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-green-500/10">
                <CheckCircle2 className="h-8 w-8 text-green-500" />
              </div>
              <div className="text-center">
                <h1 className="font-display text-3xl font-bold tracking-tight">
                  Registration Complete
                </h1>
                <p className="mt-2 text-sm text-muted-foreground">
                  Your account has been created successfully.
                </p>
              </div>
            </div>

            <Card className="glass border-border/50">
              <CardContent className="pt-6">
                <div className="space-y-4 text-center">
                  <p className="text-muted-foreground">
                    You can now sign in with your new credentials.
                  </p>
                  <Button
                    variant="glow"
                    className="w-full"
                    onClick={() => window.location.href = '/dashboard/login.html'}
                  >
                    Go to Sign In
                  </Button>
                </div>
              </CardContent>
            </Card>
          </div>
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
                Create your account
              </p>
            </div>
          </div>

          {/* Register Card */}
          <Card className="glass border-border/50">
            <CardHeader className="space-y-1">
              <CardTitle className="text-2xl font-semibold">Register</CardTitle>
              <CardDescription>Enter your invitation code and create an account</CardDescription>
            </CardHeader>
            <CardContent>
              <form onSubmit={handleSubmit} className="space-y-4">
                <div className="space-y-2">
                  <Label htmlFor="invitation-code">Invitation Code</Label>
                  <div className="relative">
                    <Ticket className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                    <Input
                      id="invitation-code"
                      type="text"
                      placeholder="inv_xxxxxxxxxxxxxxxx"
                      value={invitationCode}
                      onChange={(e) => setInvitationCode(e.target.value)}
                      className="pl-10 font-mono"
                      required
                      autoFocus
                    />
                  </div>
                </div>

                <div className="space-y-2">
                  <Label htmlFor="username">Username</Label>
                  <div className="relative">
                    <User className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                    <Input
                      id="username"
                      type="text"
                      placeholder="Choose a username"
                      value={username}
                      onChange={(e) => setUsername(e.target.value)}
                      className="pl-10"
                      required
                      autoComplete="username"
                    />
                  </div>
                </div>

                <div className="space-y-2">
                  <Label htmlFor="password">Password</Label>
                  <div className="relative">
                    <Lock className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                    <Input
                      id="password"
                      type="password"
                      placeholder="Create a password"
                      value={password}
                      onChange={(e) => setPassword(e.target.value)}
                      className="pl-10"
                      required
                      autoComplete="new-password"
                      aria-describedby={password ? 'password-strength' : undefined}
                    />
                  </div>
                  <PasswordStrengthMeter id="password-strength" password={password} />
                </div>

                <div className="space-y-2">
                  <Label htmlFor="confirm-password">Confirm Password</Label>
                  <div className="relative">
                    <Lock className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                    <Input
                      id="confirm-password"
                      type="password"
                      placeholder="Confirm your password"
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
                  disabled={isLoading}
                >
                  {isLoading ? (
                    <div className="flex items-center gap-2">
                      <div className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
                      Creating account...
                    </div>
                  ) : (
                    'Create Account'
                  )}
                </Button>
              </form>

              <div className="mt-4 text-center text-sm text-muted-foreground">
                Already have an account?{' '}
                <a
                  href="/dashboard/login.html"
                  className="text-primary hover:underline"
                >
                  Sign in
                </a>
              </div>
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
