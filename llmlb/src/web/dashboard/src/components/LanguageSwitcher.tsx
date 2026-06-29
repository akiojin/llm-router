import { useTranslation } from 'react-i18next'
import { Languages } from 'lucide-react'
import { Button } from '@/components/ui/button'

/** EN / JA を切り替えるトグル。現在の言語コードを表示し、クリックでもう一方へ。 */
export function LanguageSwitcher({ className }: { className?: string }) {
  const { i18n } = useTranslation()
  const current = i18n.resolvedLanguage === 'ja' ? 'ja' : 'en'
  const next = current === 'ja' ? 'en' : 'ja'

  return (
    <Button
      variant="outline"
      size="sm"
      className={className}
      aria-label={next === 'ja' ? 'Switch to Japanese' : 'Switch to English'}
      title="Language"
      onClick={() => void i18n.changeLanguage(next)}
    >
      <Languages className="mr-1 h-4 w-4" />
      {current.toUpperCase()}
    </Button>
  )
}
