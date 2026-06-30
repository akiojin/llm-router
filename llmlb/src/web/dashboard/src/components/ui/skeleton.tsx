import { cn } from '@/lib/utils'

/** ローディング中のプレースホルダ。`className` で形状（高さ・幅・角丸）を指定する。 */
export function Skeleton({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('animate-pulse rounded-md bg-muted', className)} {...props} />
}
