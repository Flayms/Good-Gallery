import { useVirtualizer } from '@tanstack/react-virtual'
import { type ReactNode, useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { columnWidth, tileHeight } from '@/lib/masonry'
import { cn } from '@/lib/utils'

export interface MasonryItem {
  id: number | string
  width: number | null
  height: number | null
}

interface MasonryGridProps<T extends MasonryItem> {
  items: readonly T[]
  columns: number
  gap?: number
  hasMore?: boolean
  onLoadMore?: () => void
  renderItem: (item: T, size: { width: number; height: number }) => ReactNode
  className?: string
}

export function MasonryGrid<T extends MasonryItem>({
  items,
  columns,
  gap = 8,
  hasMore = false,
  onLoadMore,
  renderItem,
  className,
}: MasonryGridProps<T>) {
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

  const getItemKey = useCallback((index: number) => items[index]?.id ?? index, [items])

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) => {
      const item = items[index]
      return item ? tileHeight(item.width, item.height, colWidth) : colWidth
    },
    getItemKey,
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

  const virtualItems = virtualizer.getVirtualItems()
  const lastIndex = virtualItems.at(-1)?.index ?? -1

  useEffect(() => {
    if (hasMore && items.length > 0 && lastIndex >= items.length - columns * 2) onLoadMore?.()
  }, [hasMore, lastIndex, items.length, columns, onLoadMore])

  return (
    <div ref={scrollRef} className={cn('h-full overflow-y-auto', className)}>
      <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
        {virtualItems.map((virtualItem) => {
          const item = items[virtualItem.index]
          if (!item) return null
          return (
            <div
              key={virtualItem.key}
              className="absolute top-0 left-0"
              style={{
                width: colWidth,
                height: virtualItem.size,
                transform: `translate(${virtualItem.lane * (colWidth + gap)}px, ${virtualItem.start}px)`,
              }}
            >
              {renderItem(item, { width: colWidth, height: virtualItem.size })}
            </div>
          )
        })}
      </div>
    </div>
  )
}
