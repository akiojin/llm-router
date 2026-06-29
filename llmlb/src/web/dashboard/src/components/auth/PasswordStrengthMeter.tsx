import { Check, X } from 'lucide-react'
import { cn } from '@/lib/utils'

// バックエンドのパスワードポリシー（auth/password.rs validate_password）に合わせる:
// 8文字以上・大文字・数字（ASCII）。フロントでも同じ必須条件を満たさないと送信を弾く。
interface Criterion {
  label: string
  test: (pw: string) => boolean
  required: boolean
}

const CRITERIA: Criterion[] = [
  { label: '8文字以上', test: (pw) => pw.length >= 8, required: true },
  { label: '大文字を含む', test: (pw) => /[A-Z]/.test(pw), required: true },
  { label: '数字を含む', test: (pw) => /[0-9]/.test(pw), required: true },
  { label: '12文字以上（推奨）', test: (pw) => pw.length >= 12, required: false },
  { label: '記号を含む（推奨）', test: (pw) => /[^A-Za-z0-9]/.test(pw), required: false },
]

/** 必須条件（8文字以上・大文字・数字）をすべて満たすか。送信ゲートに使う。 */
export function isPasswordValid(password: string): boolean {
  return CRITERIA.filter((c) => c.required).every((c) => c.test(password))
}

function strengthLabel(score: number): { label: string; barClass: string; textClass: string } {
  if (score <= 2) return { label: '弱い', barClass: 'bg-destructive', textClass: 'text-destructive' }
  if (score <= 3) return { label: '普通', barClass: 'bg-warning', textClass: 'text-warning' }
  if (score <= 4) return { label: '強い', barClass: 'bg-success', textClass: 'text-success' }
  return { label: '非常に強い', barClass: 'bg-success', textClass: 'text-success' }
}

interface PasswordStrengthMeterProps {
  password: string
  className?: string
}

export function PasswordStrengthMeter({ password, className }: PasswordStrengthMeterProps) {
  if (!password) return null

  const score = CRITERIA.reduce((acc, c) => acc + (c.test(password) ? 1 : 0), 0)
  const { label, barClass, textClass } = strengthLabel(score)
  const segments = CRITERIA.length

  return (
    <div className={cn('space-y-2', className)}>
      <div className="flex items-center gap-2">
        <div className="flex h-1.5 flex-1 gap-1" aria-hidden="true">
          {Array.from({ length: segments }).map((_, i) => (
            <div
              key={i}
              className={cn('flex-1 rounded-full', i < score ? barClass : 'bg-muted')}
            />
          ))}
        </div>
        <span className={cn('text-xs font-medium', textClass)}>{label}</span>
      </div>
      <ul className="space-y-1" aria-label="Password requirements">
        {CRITERIA.map((c) => {
          const ok = c.test(password)
          return (
            <li
              key={c.label}
              className={cn(
                'flex items-center gap-1.5 text-xs',
                ok ? 'text-success' : c.required ? 'text-muted-foreground' : 'text-muted-foreground/70'
              )}
            >
              {ok ? <Check className="h-3 w-3 shrink-0" /> : <X className="h-3 w-3 shrink-0" />}
              {c.label}
            </li>
          )
        })}
      </ul>
    </div>
  )
}
