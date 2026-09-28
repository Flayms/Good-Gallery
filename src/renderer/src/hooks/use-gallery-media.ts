import { useInfiniteQuery } from '@tanstack/react-query'
import { useMemo } from 'react'
import { type GallerySearch, mediaSearchInput } from '@/lib/search'
import { trpc } from '@/lib/trpc'

const PAGE_SIZE = 200

/** Paged gallery media for the search state; the grid and the viewer share the cached pages. */
export function useGalleryMedia(search: GallerySearch) {
  const query = useInfiniteQuery(
    trpc.media.search.infiniteQueryOptions(
      { ...mediaSearchInput(search), limit: PAGE_SIZE },
      { getNextPageParam: (page) => page.nextCursor },
    ),
  )
  const items = useMemo(() => query.data?.pages.flatMap((page) => page.items) ?? [], [query.data])
  return { query, items }
}
