import { Link, useParams } from '@tanstack/react-router'
import { FolderPlusIcon, ImagesIcon, TriangleAlertIcon } from 'lucide-react'
import { type ReactNode, useCallback, useMemo } from 'react'
import { JustifiedRows } from '@/components/justified-rows'
import { MasonryGrid } from '@/components/masonry-grid'
import { MediaList } from '@/components/media-list'
import { MediaTile } from '@/components/media-tile'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { useGalleryMedia } from '@/hooks/use-gallery-media'
import type { GalleryView } from '@/hooks/use-gallery-view'
import type { GallerySearch } from '@/lib/search'

const GAP = 8
const SKELETON_HEIGHTS = [220, 160, 280, 190, 240, 150, 260, 200, 170, 230, 180, 250]
const SQUARE = () => 1

function EmptyState({ icon, title, children }: { icon: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center text-muted-foreground">
      {icon}
      <p className="font-medium text-foreground">{title}</p>
      {children}
    </div>
  )
}

function GallerySkeleton({ view, columns }: { view: GalleryView; columns: number }) {
  if (view === 'list') {
    return (
      <div className="flex flex-col gap-3 overflow-hidden p-3">
        {Array.from({ length: 12 }, (_, i) => i).map((row) => (
          <Skeleton key={`row-${row}`} className="h-10" />
        ))}
      </div>
    )
  }
  return (
    <div className="flex gap-2 overflow-hidden p-3">
      {Array.from({ length: columns }, (_, i) => i).map((column) => (
        <div key={`column-${column}`} className="flex min-w-0 flex-1 flex-col gap-2">
          {[0, 1, 2].map((row) => (
            <Skeleton
              key={`tile-${row}`}
              style={{ height: SKELETON_HEIGHTS[(column * 3 + row) % SKELETON_HEIGHTS.length] }}
            />
          ))}
        </div>
      ))}
    </div>
  )
}

export function Gallery({
  search,
  view,
  columns,
  hasLibraries,
}: {
  search: GallerySearch
  view: GalleryView
  columns: number
  hasLibraries: boolean
}) {
  const {
    layout: { error, isPending },
    ids,
    count,
    aspectAt,
    getItem,
    setRange,
  } = useGalleryMedia(search)
  // The viewer navigates through the same items; keep the one it shows in view behind it.
  const viewedId = useParams({ strict: false, select: (params) => params.id })
  const viewedIndex = useMemo(() => (viewedId === undefined ? undefined : ids.indexOf(viewedId)), [ids, viewedId])
  const getKey = useCallback((index: number) => ids[index] ?? -index - 1, [ids])

  if (isPending) return <GallerySkeleton view={view} columns={columns} />
  if (error) {
    return (
      <EmptyState icon={<TriangleAlertIcon className="size-10" />} title="Couldn't load media">
        <p className="text-sm">{error.message}</p>
      </EmptyState>
    )
  }
  if (count === 0) {
    return hasLibraries ? (
      <EmptyState icon={<ImagesIcon className="size-10" />} title="No media found">
        <p className="text-sm">Nothing matches the current filters yet — new files appear while indexing.</p>
      </EmptyState>
    ) : (
      <EmptyState icon={<FolderPlusIcon className="size-10" />} title="No libraries yet">
        <Button asChild>
          <Link to="/settings">Add a library</Link>
        </Button>
      </EmptyState>
    )
  }

  if (view === 'list') {
    return (
      <MediaList
        count={count}
        getKey={getKey}
        getItem={getItem}
        onRangeChange={setRange}
        scrollToIndex={viewedIndex}
        className="py-1"
      />
    )
  }

  const renderTile = (index: number, size: { width: number }) => {
    const item = getItem(index)
    return item ? <MediaTile item={item} width={size.width} /> : <div className="size-full rounded-md bg-muted" />
  }
  const Layout = view === 'justified' ? JustifiedRows : MasonryGrid
  return (
    <Layout
      count={count}
      getKey={getKey}
      aspectAt={view === 'grid' ? SQUARE : aspectAt}
      columns={columns}
      gap={GAP}
      onRangeChange={setRange}
      scrollToIndex={viewedIndex}
      className="p-3"
      renderItem={renderTile}
    />
  )
}
