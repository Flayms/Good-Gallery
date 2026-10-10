import { Link } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import { PlayIcon } from 'lucide-react'
import { useRef } from 'react'
import { MediaContextMenu } from '@/components/media-context-menu'
import { Rating } from '@/components/media-info'
import { type MediaItem, MediaThumbnail } from '@/components/media-tile'
import { useVirtualAnchor } from '@/hooks/use-virtual-anchor'
import { formatBytes, formatDuration } from '@/lib/format'
import { cn } from '@/lib/utils'

const ROW_HEIGHT = 52
const HEADER_HEIGHT = 36
const THUMB_SIZE = 40
const COLUMNS = 'grid grid-cols-[2.5rem_minmax(0,3fr)_minmax(0,2fr)_11rem_5.5rem_7.5rem_6rem] items-center gap-4 px-3'

interface MediaListProps {
  count: number
  /** Stable key of the item at `index`. */
  getKey: (index: number) => number
  /** Item data, once its chunk is loaded. */
  getItem: (index: number) => MediaItem | undefined
  /** Called with the first and last rendered index whenever the visible range changes. */
  onRangeChange?: (start: number, end: number) => void
  /** Scrolls this item into view whenever it changes (no-op if it's visible or negative). */
  scrollToIndex?: number
  className?: string
}

function folderOf(relPath: string): string {
  return relPath.slice(0, Math.max(0, relPath.lastIndexOf('/')))
}

function MediaListRow({ item }: { item: MediaItem }) {
  const date = item.takenAt ?? item.mtime
  return (
    <MediaContextMenu id={item.id}>
      <Link
        to="/media/$id"
        params={{ id: item.id }}
        search={(prev) => prev}
        title={item.fileName}
        draggable={false}
        className={cn(
          COLUMNS,
          'h-full rounded-md text-sm outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring',
        )}
      >
        <div style={{ width: THUMB_SIZE, height: THUMB_SIZE }}>
          <MediaThumbnail item={item} width={THUMB_SIZE} badge={false} />
        </div>
        <span className="flex min-w-0 items-center gap-1.5">
          <span className="truncate font-medium">{item.fileName}</span>
          {item.kind === 'video' && (
            <span className="flex shrink-0 items-center gap-1 text-muted-foreground text-xs tabular-nums">
              <PlayIcon className="size-3 fill-current" />
              {item.duration ? formatDuration(item.duration) : null}
            </span>
          )}
        </span>
        <span className="truncate text-muted-foreground" title={item.relPath}>
          {folderOf(item.relPath)}
        </span>
        <span
          className={cn('truncate tabular-nums', item.takenAt === null && 'text-muted-foreground')}
          title={item.takenAt === null ? 'Date modified (no capture date)' : undefined}
        >
          {new Date(date).toLocaleString()}
        </span>
        <span className="text-right tabular-nums">{formatBytes(item.size)}</span>
        <span className="text-muted-foreground tabular-nums">
          {item.width && item.height ? `${item.width} × ${item.height}` : null}
        </span>
        <span>{item.rating ? <Rating value={item.rating} /> : null}</span>
      </Link>
    </MediaContextMenu>
  )
}

/** Details list: one row per item with its thumbnail and file data. */
export function MediaList({ count, getKey, getItem, onRangeChange, scrollToIndex, className }: MediaListProps) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const virtualizer = useVirtualizer({
    count,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    getItemKey: getKey,
    // The sticky header sits above the rows inside the scroll element.
    scrollMargin: HEADER_HEIGHT,
    overscan: 10,
  })

  const virtualItems = virtualizer.getVirtualItems()
  useVirtualAnchor({
    virtualizer,
    layoutKey: ROW_HEIGHT,
    itemRange: [virtualItems[0]?.index ?? 0, virtualItems.at(-1)?.index ?? -1],
    scrollToIndex,
    onRangeChange,
  })

  return (
    <div ref={scrollRef} className={cn('h-full overflow-y-auto', className)}>
      <div
        role="presentation"
        className={cn(COLUMNS, 'sticky top-0 z-10 border-b bg-background font-medium text-muted-foreground text-xs')}
        style={{ height: HEADER_HEIGHT }}
      >
        <span />
        <span>Name</span>
        <span>Folder</span>
        <span>Date</span>
        <span className="text-right">Size</span>
        <span>Dimensions</span>
        <span>Rating</span>
      </div>
      <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
        {virtualItems.map((virtualItem) => {
          const item = getItem(virtualItem.index)
          return (
            <div
              key={virtualItem.key}
              className="absolute top-0 left-0 w-full"
              style={{
                height: virtualItem.size,
                transform: `translateY(${virtualItem.start - HEADER_HEIGHT}px)`,
              }}
            >
              {item ? <MediaListRow item={item} /> : <div className="mx-3 my-1.5 h-10 rounded-md bg-muted" />}
            </div>
          )
        })}
      </div>
    </div>
  )
}
