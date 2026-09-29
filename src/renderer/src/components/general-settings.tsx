import type { Settings } from '@shared/settings'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Trash2Icon } from 'lucide-react'
import { type ReactNode, useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import { Slider } from '@/components/ui/slider'
import { formatBytes } from '@/lib/format'
import { trpc } from '@/lib/trpc'

const QUICK_CHECK_INTERVALS = [0, 15, 30, 60, 180, 360, 1440]
const CACHE_SIZES_GIB = [1, 2, 5, 10, 20, 50, 100]
const MAX_CONCURRENCY = 16

function intervalLabel(minutes: number): string {
  if (minutes === 0) return 'Only at startup'
  if (minutes < 60) return `Every ${minutes} minutes`
  if (minutes === 60) return 'Every hour'
  if (minutes === 1440) return 'Daily'
  return `Every ${+(minutes / 60).toFixed(1)} hours`
}

/** The preset options plus the current value, should it have been set to something else. */
function withCurrent(options: number[], current: number): number[] {
  return options.includes(current) ? options : [...options, current].sort((a, b) => a - b)
}

function SettingRow({ label, description, children }: { label: string; description: string; children: ReactNode }) {
  return (
    <div className="flex items-center gap-6 p-3">
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="font-medium text-sm">{label}</span>
        <span className="text-muted-foreground text-xs">{description}</span>
      </div>
      <div className="flex shrink-0 items-center gap-3">{children}</div>
    </div>
  )
}

function ConcurrencySlider({ value, onCommit }: { value: number; onCommit: (value: number) => void }) {
  const [dragged, setDragged] = useState<number | undefined>(undefined)
  const shown = dragged ?? value
  return (
    <>
      <Slider
        className="w-40"
        min={1}
        max={MAX_CONCURRENCY}
        step={1}
        value={[shown]}
        onValueChange={([next]) => setDragged(next)}
        onValueCommit={([next]) => {
          setDragged(undefined)
          if (next !== undefined && next !== value) onCommit(next)
        }}
        aria-label="Parallel file reads"
      />
      <span className="w-6 text-right text-sm tabular-nums">{shown}</span>
    </>
  )
}

/** Indexing and thumbnail cache settings; changes apply immediately. */
export function GeneralSettings() {
  const queryClient = useQueryClient()
  const settings = useQuery(trpc.settings.get.queryOptions())
  const usage = useQuery(trpc.settings.cacheUsage.queryOptions(undefined, { refetchInterval: 10_000 }))
  const onError = (error: { message: string }) => toast.error(error.message)

  const update = useMutation(
    trpc.settings.update.mutationOptions({
      onSuccess: (next) => {
        queryClient.setQueryData(trpc.settings.get.queryKey(), next)
        // Lowering the cap evicts thumbnails.
        void queryClient.invalidateQueries({ queryKey: trpc.settings.cacheUsage.queryKey() })
      },
      onError,
    }),
  )
  const clear = useMutation(
    trpc.settings.clearCache.mutationOptions({
      onSuccess: (bytes) => {
        queryClient.setQueryData(trpc.settings.cacheUsage.queryKey(), bytes)
        toast.success('Thumbnail cache cleared')
      },
      onError,
    }),
  )
  const set = (patch: Partial<Settings>) => update.mutate(patch)

  if (!settings.data) {
    return settings.error ? (
      <p className="text-destructive text-sm">{settings.error.message}</p>
    ) : (
      <Skeleton className="h-48" />
    )
  }
  const { quickCheckIntervalMinutes, ioConcurrency, thumbCacheGiB } = settings.data

  return (
    <>
      <section className="flex flex-col gap-4">
        <h2 className="font-heading font-medium text-lg">Indexing</h2>
        <div className="flex flex-col divide-y rounded-lg border">
          <SettingRow
            label="Check for missed changes"
            description="Changes are picked up as they happen; this re-checks changed folders in case a notification got lost."
          >
            <Select
              value={String(quickCheckIntervalMinutes)}
              onValueChange={(value) => set({ quickCheckIntervalMinutes: Number(value) })}
            >
              <SelectTrigger className="w-56" aria-label="Check interval">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {withCurrent(QUICK_CHECK_INTERVALS, quickCheckIntervalMinutes).map((minutes) => (
                  <SelectItem key={minutes} value={String(minutes)}>
                    {intervalLabel(minutes)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </SettingRow>
          <SettingRow
            label="Parallel file reads"
            description="Higher is faster on fast disks and NAS; lower it if the share becomes sluggish."
          >
            <ConcurrencySlider value={ioConcurrency} onCommit={(value) => set({ ioConcurrency: value })} />
          </SettingRow>
        </div>
      </section>

      <section className="flex flex-col gap-4">
        <h2 className="font-heading font-medium text-lg">Thumbnail cache</h2>
        <div className="flex flex-col divide-y rounded-lg border">
          <SettingRow label="Maximum size" description="Least recently viewed thumbnails are removed first.">
            <Select value={String(thumbCacheGiB)} onValueChange={(value) => set({ thumbCacheGiB: Number(value) })}>
              <SelectTrigger className="w-32" aria-label="Maximum cache size">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {withCurrent(CACHE_SIZES_GIB, thumbCacheGiB).map((size) => (
                  <SelectItem key={size} value={String(size)}>
                    {size} GB
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </SettingRow>
          <SettingRow
            label="Used"
            description="Clearing frees the space; thumbnails are rendered again when they're next shown."
          >
            <span className="text-sm tabular-nums">
              {usage.data === undefined ? (usage.error ? 'unknown' : '…') : formatBytes(usage.data)}
            </span>
            <Button
              variant="outline"
              size="sm"
              disabled={clear.isPending}
              onClick={() => {
                if (window.confirm('Delete all cached thumbnails?')) clear.mutate()
              }}
            >
              <Trash2Icon /> Clear
            </Button>
          </SettingRow>
        </div>
      </section>
    </>
  )
}
