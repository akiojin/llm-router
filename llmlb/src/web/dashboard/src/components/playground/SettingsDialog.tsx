import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { Separator } from '@/components/ui/separator'
import { Switch } from '@/components/ui/switch'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

interface SettingsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  systemPrompt: string
  onSystemPromptChange: (value: string) => void
  streamEnabled: boolean
  onStreamEnabledChange: (value: boolean) => void
  streamDisabled?: boolean
  temperature: number
  onTemperatureChange: (value: number) => void
  maxTokens: number
  onMaxTokensChange: (value: number) => void
  useMaxContext: boolean
  onUseMaxContextChange: (value: boolean) => void
  selectedModelMaxTokens?: number | null
  description?: string
  maxContextCheckboxId?: string
}

export function SettingsDialog({
  open,
  onOpenChange,
  systemPrompt,
  onSystemPromptChange,
  streamEnabled,
  onStreamEnabledChange,
  streamDisabled = false,
  temperature,
  onTemperatureChange,
  maxTokens,
  onMaxTokensChange,
  useMaxContext,
  onUseMaxContextChange,
  selectedModelMaxTokens,
  description,
  maxContextCheckboxId = 'use-max-context',
}: SettingsDialogProps) {
  const effectiveDescription = description ?? 'Configure chat behavior and generation parameters.'
  // プリセット/リセットは温度と最大トークンをまとめて設定する。プリセット適用時は
  // モデル最大コンテキスト指定を解除して、指定したトークン数が反映されるようにする。
  const applyPreset = (temp: number, tokens: number) => {
    onTemperatureChange(temp)
    onUseMaxContextChange(false)
    onMaxTokensChange(tokens)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>{effectiveDescription}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          <div className="space-y-2">
            <Label>Presets</Label>
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="outline" size="sm" onClick={() => applyPreset(0.2, 2048)}>
                Precise
              </Button>
              <Button type="button" variant="outline" size="sm" onClick={() => applyPreset(0.7, 4096)}>
                Balanced
              </Button>
              <Button type="button" variant="outline" size="sm" onClick={() => applyPreset(1.2, 8192)}>
                Creative
              </Button>
            </div>
          </div>

          <Separator />

          <div className="space-y-2">
            <Label>System Prompt</Label>
            <Textarea
              placeholder="You are a helpful assistant..."
              value={systemPrompt}
              onChange={(e) => onSystemPromptChange(e.target.value)}
              rows={3}
            />
          </div>

          <Separator />

          <div className="flex items-center justify-between">
            <div>
              <Label>Streaming</Label>
              <p className="text-xs text-muted-foreground">
                Stream responses while tokens are generated
              </p>
            </div>
            <Switch
              checked={streamEnabled}
              onCheckedChange={onStreamEnabledChange}
              disabled={streamDisabled}
            />
          </div>

          <Separator />

          <div className="space-y-2">
            <Label>{`Temperature: ${temperature}`}</Label>
            <input
              type="range"
              min="0"
              max="2"
              step="0.1"
              value={temperature}
              onChange={(e) => onTemperatureChange(Number.parseFloat(e.target.value))}
              className="w-full"
            />
          </div>

          <div className="space-y-2">
            <Label>Max Tokens</Label>
            <div className="flex items-center space-x-2 mb-2">
              <Checkbox
                id={maxContextCheckboxId}
                checked={useMaxContext}
                onCheckedChange={(checked) => onUseMaxContextChange(checked === true)}
                disabled={selectedModelMaxTokens == null && !useMaxContext}
              />
              <Label
                htmlFor={maxContextCheckboxId}
                className={cn("text-sm font-normal", selectedModelMaxTokens == null && "text-muted-foreground")}
              >
                {`Use model max context (${
                  selectedModelMaxTokens != null
                    ? selectedModelMaxTokens.toLocaleString()
                    : 'unknown'
                })`}
              </Label>
            </div>
            <Input
              type="number"
              value={useMaxContext && selectedModelMaxTokens != null ? selectedModelMaxTokens : maxTokens}
              onChange={(e) => onMaxTokensChange(Number.parseInt(e.target.value, 10) || 2048)}
              min={1}
              max={131072}
              disabled={useMaxContext && selectedModelMaxTokens != null}
            />
          </div>
        </div>

        <DialogFooter className="sm:justify-between">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => applyPreset(0.7, 16384)}
          >
            Reset parameters
          </Button>
          <Button onClick={() => onOpenChange(false)}>Done</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
