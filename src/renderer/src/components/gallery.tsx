import { Link, useParams } from '@tanstack/react-router'
import { FolderPlusIcon, ImagesIcon, TriangleAlertIcon } from 'lucide-react'
import { type ReactNode, useCallback } from 'react'
import { MasonryGrid } from '@/components/masonry-grid'
import { MediaTile } from '@/components/media-tile'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { useGalleryMedia } from '@/hooks/use-gallery-media'
import type { GallerySearch } from '@/lib/search'

const GAP = 8
const SKELETON_HEIGHTS = [220, 160, 280, 190, 240, 150, 260, 200, 170, 230, 180, 250]

function EmptyState({ icon, title, children }: { icon: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center text-muted-foreground">
      {icon}
      <p className="font-medium text-foreground">{title}</p>
      {children}
    </div>
  )
}

function GallerySkeleton({ columns }: { columns: number }) {
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
  columns,
  hasLibraries,
}: {
  search: GallerySearch
  columns: number
  hasLibraries: boolean
}) {
  const {
    query: { error, isPending, hasNextPage, isFetchingNextPage, fetchNextPage },
    items,
  } = useGalleryMedia(search)
  // The viewer navigates through the same items; keep the one it shows in view behind it.
  const viewedId = useParams({ strict: false, select: (params) => params.id })
  const viewedIndex = viewedId === undefined ? undefined : items.findIndex((item) => item.id === viewedId)
  const loadMore = useCallback(() => {
    if (!isFetchingNextPage) void fetchNextPage()
  }, [isFetchingNextPage, fetchNextPage])

  if (isPending) return <GallerySkeleton columns={columns} />
  if (error) {
    return (
      <EmptyState icon={<TriangleAlertIcon className="size-10" />} title="Couldn't load media">
        <p className="text-sm">{error.message}</p>
      </EmptyState>
    )
  }
  if (items.length === 0) {
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

  return (
    <MasonryGrid
      items={items}
      columns={columns}
      gap={GAP}
      hasMore={hasNextPage}
      onLoadMore={loadMore}
      scrollToIndex={viewedIndex}
      className="p-3"
      renderItem={(item, size) => <MediaTile item={item} width={size.width} />}
    />
  )
}
