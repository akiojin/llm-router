import { Badge } from '@/components/ui/badge'

interface ModelIdentityProps {
  id: string
  canonicalName?: string | null
  aliases?: string[]
  isCanonical?: boolean
}

function CanonicalBadge() {
  return (
    <Badge
      variant="outline"
      className="h-5 shrink-0 border-primary/30 bg-primary/10 px-1.5 py-0 text-[10px] font-semibold text-primary"
    >
      canonical
    </Badge>
  )
}

export function ModelIdentity({
  id,
  canonicalName,
  aliases = [],
  isCanonical = false,
}: ModelIdentityProps) {
  const visibleAliases = aliases.filter(
    (alias) => alias.length > 0 && alias !== id && alias !== canonicalName
  )
  const showCanonicalInline = isCanonical && (!canonicalName || canonicalName === id)
  const showCanonicalLine = Boolean(canonicalName && canonicalName !== id)

  return (
    <div className="min-w-0 space-y-1">
      <div className="flex min-w-0 items-center gap-2">
        <span className="truncate font-mono text-sm" title={id}>
          {id}
        </span>
        {showCanonicalInline && <CanonicalBadge />}
      </div>

      {showCanonicalLine && (
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate font-mono text-xs text-muted-foreground" title={canonicalName ?? ''}>
            {canonicalName}
          </span>
          <CanonicalBadge />
        </div>
      )}

      {visibleAliases.length > 0 && (
        <div className="flex min-w-0 flex-wrap gap-1">
          {visibleAliases.map((alias) => (
            <span
              key={alias}
              className="max-w-full truncate rounded border bg-muted/40 px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground"
              title={alias}
            >
              {alias}
            </span>
          ))}
        </div>
      )}
    </div>
  )
}
