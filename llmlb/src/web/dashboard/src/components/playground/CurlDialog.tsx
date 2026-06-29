import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Check, Copy } from 'lucide-react'

interface CurlDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  curlCommand: string
  copied: boolean
  onCopy: (text: string) => void
  copyDisabled?: boolean
  description?: string
}

export function CurlDialog({
  open,
  onOpenChange,
  curlCommand,
  copied,
  onCopy,
  copyDisabled = false,
  description,
}: CurlDialogProps) {
  const { t } = useTranslation()
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t('playground.curlTitle')}</DialogTitle>
          <DialogDescription>{description ?? t('playground.curlDescription')}</DialogDescription>
        </DialogHeader>

        <div className="relative">
          <Button
            variant="outline"
            size="sm"
            className="absolute right-2 top-2"
            onClick={() => void onCopy(curlCommand)}
            disabled={copyDisabled}
          >
            {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
          </Button>
          <ScrollArea className="h-64 rounded-md border bg-muted">
            <pre className="p-4 text-xs font-mono whitespace-pre-wrap">{curlCommand}</pre>
          </ScrollArea>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('playground.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
