import { Button } from '@/components/ui/button'
import { ChevronLeft, ChevronRight } from 'lucide-react'

interface TablePaginationProps {
  currentPage: number
  totalPages: number
  /** 全件数（任意）。指定すると "N entries total" を左側に表示する。 */
  totalCount?: number
  /** 件数の単位ラベル（既定: "entries"）。 */
  unitLabel?: string
  onPageChange: (page: number) => void
}

/** テーブルのページング UI を統一する（Previous / Page X of Y / Next）。 */
export function TablePagination({
  currentPage,
  totalPages,
  totalCount,
  unitLabel = 'entries',
  onPageChange,
}: TablePaginationProps) {
  if (totalPages <= 1) return null

  return (
    <div className="mt-4 flex items-center justify-between gap-2">
      <p className="text-sm text-muted-foreground">
        {totalCount != null ? `${totalCount.toLocaleString()} ${unitLabel} total` : ''}
      </p>
      <div className="flex items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          disabled={currentPage <= 1}
          onClick={() => onPageChange(currentPage - 1)}
          aria-label="Previous page"
        >
          <ChevronLeft className="h-4 w-4" />
          Previous
        </Button>
        <span className="text-sm text-muted-foreground" aria-live="polite">
          Page {currentPage} / {totalPages}
        </span>
        <Button
          variant="outline"
          size="sm"
          disabled={currentPage >= totalPages}
          onClick={() => onPageChange(currentPage + 1)}
          aria-label="Next page"
        >
          Next
          <ChevronRight className="h-4 w-4" />
        </Button>
      </div>
    </div>
  )
}
