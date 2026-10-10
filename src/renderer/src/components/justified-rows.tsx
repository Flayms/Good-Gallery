import { useVirtualizer } from '@tanstack/react-virtual'
import { type ReactNode, useCallback, useMemo, useRef } from 'react'
import { useContentWidth } from '@/hooks/use-content-width'
import { useVirtualAnchor } from '@/hooks/use-virtual-anchor'
import { justifiedRows, rowItemWidths, rowOfItem } from '@/lib/justified'
import { columnWidth } from '@/lib/masonry'
import { cn } from '@/lib/utils'

/** Row height relative to the masonry column width at the same zoom, so landscape photos keep about that width. */
const ROW_HEIGHT_RATIO = 0.75

interface JustifiedRowsProps {
  count: number
  /** Stable key of the item at `index`. */
  getKey: (index: number) => number
  /** Height / width of the item at `index`. */
  aspectAt: (index: number) => number
  /** Zoom level: rows are about as tall as a masonry grid with this many columns is wide per column. */
  columns: number
  gap?: number
  /** Called with the first and last rendered index whenever the visible range changes. */
  onRangeChange?: (start: number, end: number) => void
  /** Scrolls this item into view whenever it changes (no-op if it's visible or negative). */
  scrollToIndex?: number
  renderItem: (index: number, size: { width: number; height: number }) => ReactNode
  className?: string
}

/** Horizontal swimlanes: rows of equal height that fill the width, tiles keep their aspect ratio. */
export function JustifiedRows({
  count,
  getKey,
  aspectAt,
  columns,
  gap = 8,
  onRangeChange,
  scrollToIndex,
  renderItem,
  className,
}: JustifiedRowsProps) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const width = Math.floor(useContentWidth(scrollRef))
  const targetHeight = Math.round(columnWidth(width, columns, gap) * ROW_HEIGHT_RATIO)
  const rows = useMemo(
    () => justifiedRows(count, aspectAt, width, targetHeight, gap),
    [count, aspectAt, width, targetHeight, gap],
  )

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) => rows[index]?.height ?? targetHeight,
    getItemKey: (index) => getKey(rows[index]?.start ?? -1),
    gap,
    overscan: 4,
    enabled: width > 0,
  })

  const virtualRows = virtualizer.getVirtualItems()
  const firstRow = rows[virtualRows[0]?.index ?? -1]
  const lastRow = rows[virtualRows.at(-1)?.index ?? -1]
  const toItem = useCallback((rowIndex: number) => rows[rowIndex]?.start ?? 0, [rows])
  const toVirtual = useCallback((itemIndex: number) => rowOfItem(rows, itemIndex), [rows])

  useVirtualAnchor({
    virtualizer,
    // Row sizes come from `rows`, so any new layout re-measures.
    layoutKey: rows,
    itemRange: [firstRow?.start ?? 0, lastRow ? lastRow.end - 1 : -1],
    toItem,
    toVirtual,
    scrollToIndex,
    onRangeChange,
  })

  return (
    <div ref={scrollRef} className={cn('h-full overflow-y-auto', className)}>
      <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
        {virtualRows.map((virtualRow) => {
          const row = rows[virtualRow.index]
          if (!row) return null
          const widths = rowItemWidths(row, aspectAt, width, gap)
          let x = 0
          return (
            <div
              key={virtualRow.key}
              className="absolute top-0 left-0 w-full"
              style={{ height: row.height, transform: `translateY(${virtualRow.start}px)` }}
            >
              {widths.map((itemWidth, offset) => {
                const index = row.start + offset
                const left = x
                x += itemWidth + gap
                return (
                  <div
                    key={getKey(index)}
                    className="absolute top-0"
                    style={{ left, width: itemWidth, height: row.height }}
                  >
                    {renderItem(index, { width: itemWidth, height: row.height })}
                  </div>
                )
              })}
            </div>
          )
        })}
      </div>
    </div>
  )
}
