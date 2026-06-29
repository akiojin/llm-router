import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { auditLogApi, HashChainVerifyResult } from '@/lib/api'
import { ShieldCheck, ShieldAlert, Loader2 } from 'lucide-react'

export function HashChainStatus() {
  const [loading, setLoading] = useState(false)
  const [result, setResult] = useState<HashChainVerifyResult | null>(null)
  const [error, setError] = useState<string | null>(null)

  const handleVerify = async () => {
    setLoading(true)
    setError(null)
    try {
      const res = await auditLogApi.verify()
      setResult(res)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Verification failed')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="flex items-center gap-3">
      <Button
        variant="outline"
        size="sm"
        onClick={handleVerify}
        disabled={loading}
      >
        {loading ? (
          <Loader2 className="mr-2 h-4 w-4 animate-spin" />
        ) : (
          <ShieldCheck className="mr-2 h-4 w-4" />
        )}
        Verify Hash Chain
      </Button>
      {result && (
        result.valid ? (
          <Badge variant="default" className="bg-green-600">
            <ShieldCheck className="mr-1 h-3 w-3" />
            {`Verified (${result.batches_checked} batches)`}
          </Badge>
        ) : (
          <Badge variant="destructive">
            <ShieldAlert className="mr-1 h-3 w-3" />
            {`Tampered: Batch ${result.tampered_batch}`}
          </Badge>
        )
      )}
      {error && (
        <Badge variant="destructive">{error}</Badge>
      )}
    </div>
  )
}
