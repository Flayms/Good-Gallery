import { useVirtualizer } from '@tanstack/react-virtual'
import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { columnWidth, tileHeight } from '@/lib/masonry'
import { cn } from '@/lib/utils'

interface MasonryGridProps {
  count: number
  /** Stable key of the item at `index`. */
  getKey: (index: number) => number
  /** Height / width of the item at `index`; known for every item up front so the scroll length is exact. */
  aspectAt: (index: number) => number
  columns: number
  gap?: number
  /** Called with the first and last rendered index whenever the visible range changes. */
  onRangeChange?: (start: number, end: number) => void
  /** Scrolls this item into view whenever it changes (no-op if it's visible or negative). */
  scrollToIndex?: number
  renderItem: (index: number, size: { width: number; height: number }) => ReactNode
  className?: string
}

export function MasonryGrid({
  count,
  getKey,
  aspectAt,
  columns,
  gap = 8,
  onRangeChange,
  scrollToIndex,
  renderItem,
  className,
}: MasonryGridProps) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const [containerWidth, setContainerWidth] = useState(0)

  useLayoutEffect(() => {
    const el = scrollRef.current
    if (!el) return
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setContainerWidth(entry.contentRect.width)
    })
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  const colWidth = columnWidth(containerWidth, columns, gap)

  const virtualizer = useVirtualizer({
    count,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) => tileHeight(aspectAt(index), colWidth),
    getItemKey: getKey,
    lanes: columns,
    gap,
    overscan: 8,
    enabled: colWidth > 0,
  })

  // Heights are derived from colWidth, so re-layout on zoom/resize and keep the first visible item in view.
  const layoutKey = `${columns}:${colWidth}`
  const prevLayoutKey = useRef(layoutKey)
  const anchorIndex = useRef(0)
  useLayoutEffect(() => {
    if (prevLayoutKey.current === layoutKey) {
      anchorIndex.current = virtualizer.range?.startIndex ?? 0
      return
    }
    prevLayoutKey.current = layoutKey
    virtualizer.measure()
    virtualizer.scrollToIndex(anchorIndex.current, { align: 'start' })
  })

  useEffect(() => {
    if (scrollToIndex !== undefined && scrollToIndex >= 0) virtualizer.scrollToIndex(scrollToIndex, { align: 'auto' })
  }, [scrollToIndex, virtualizer])

  const virtualItems = virtualizer.getVirtualItems()
  // Lanes interleave indices, so the range is the min / max rather than the first / last.
  const rangeStart = virtualItems.reduce((min, item) => Math.min(min, item.index), Number.POSITIVE_INFINITY)
  const rangeEnd = virtualItems.reduce((max, item) => Math.max(max, item.index), -1)

  useEffect(() => {
    if (rangeEnd >= 0) onRangeChange?.(rangeStart, rangeEnd)
  }, [rangeStart, rangeEnd, onRangeChange])

  return (
    <div ref={scrollRef} className={cn('h-full overflow-y-auto', className)}>
      <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
        {virtualItems.map((virtualItem) => (
          <div
            key={virtualItem.key}
            className="absolute top-0 left-0"
            style={{
              width: colWidth,
              height: virtualItem.size,
              transform: `translate(${virtualItem.lane * (colWidth + gap)}px, ${virtualItem.start}px)`,
            }}
          >
            {renderItem(virtualItem.index, { width: colWidth, height: virtualItem.size })}
          </div>
        ))}
      </div>
    </div>
  )
}
