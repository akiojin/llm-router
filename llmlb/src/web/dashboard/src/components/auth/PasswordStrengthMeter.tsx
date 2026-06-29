import { Check, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'

// バックエンドのパスワードポリシー（auth/password.rs validate_password）に合わせる:
// 8文字以上・大文字・数字（ASCII）。フロントでも同じ必須条件を満たさないと送信を弾く。
interface Criterion {
  label: string
  test: (pw: string) => boolean
  required: boolean
}

const CRITERIA: Criterion[] = [
  { label: 'passwordStrength.req8Chars', test: (pw) => pw.length >= 8, required: true },
  { label: 'passwordStrength.reqUppercase', test: (pw) => /[A-Z]/.test(pw), required: true },
  { label: 'passwordStrength.reqNumber', test: (pw) => /[0-9]/.test(pw), required: true },
  { label: 'passwordStrength.req12Chars', test: (pw) => pw.length >= 12, required: false },
  {
    label: 'passwordStrength.reqSymbol',
    test: (pw) => /[^A-Za-z0-9]/.test(pw),
    required: false,
  },
]

/** 必須条件（8文字以上・大文字・数字）をすべて満たすか。送信ゲートに使う。 */
export function isPasswordValid(password: string): boolean {
  return CRITERIA.filter((c) => c.required).every((c) => c.test(password))
}

function strengthLabel(
  score: number,
  t: (key: string, opts?: Record<string, unknown>) => string
): { label: string; barClass: string; textClass: string } {
  if (score <= 2)
    return { label: t('passwordStrength.weak'), barClass: 'bg-destructive', textClass: 'text-destructive' }
  if (score <= 3) return { label: t('passwordStrength.fair'), barClass: 'bg-warning', textClass: 'text-warning' }
  if (score <= 4) return { label: t('passwordStrength.strong'), barClass: 'bg-success', textClass: 'text-success' }
  return { label: t('passwordStrength.veryStrong'), barClass: 'bg-success', textClass: 'text-success' }
}

interface PasswordStrengthMeterProps {
  password: string
  className?: string
}

export function PasswordStrengthMeter({ password, className }: PasswordStrengthMeterProps) {
  const { t } = useTranslation()
  if (!password) return null

  const score = CRITERIA.reduce((acc, c) => acc + (c.test(password) ? 1 : 0), 0)
  const { label, barClass, textClass } = strengthLabel(score, t)
  const segments = CRITERIA.length

  return (
    <div className={cn('space-y-2', className)} role="status" aria-live="polite">
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
      <ul className="space-y-1" aria-label={t('passwordStrength.requirementsLabel')}>
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
              {t(c.label)}
            </li>
          )
        })}
      </ul>
    </div>
  )
}
