import { ASPECT_SCALE } from '@shared/aspect'
import { useQueries, useQuery } from '@tanstack/react-query'
import { useCallback, useMemo, useRef, useState } from 'react'
import type { MediaItem } from '@/components/media-tile'
import { type GallerySearch, mediaSearchInput } from '@/lib/search'
import { trpc } from '@/lib/trpc'

/** Items are loaded in chunks of consecutive gallery positions. */
const CHUNK_SIZE = 200
/** Above this many remembered items the memory of already loaded ones is dropped. */
const MAX_REMEMBERED = 20_000
const NO_IDS: readonly number[] = []
const NO_ASPECTS: readonly number[] = []

/**
 * The gallery for the search state: the full ordered id list with aspects up front (so the grid knows its exact
 * height), and the item data of the visible range loaded on demand. The grid and the viewer share the query cache.
 */
export function useGalleryMedia(search: GallerySearch) {
  const layout = useQuery(trpc.media.layout.queryOptions(mediaSearchInput(search)))
  const ids = layout.data?.ids ?? NO_IDS
  const aspects = layout.data?.aspects ?? NO_ASPECTS

  const [range, setRangeState] = useState<readonly [number, number]>([0, 0])
  const setRange = useCallback((start: number, end: number) => {
    setRangeState((current) => (current[0] === start && current[1] === end ? current : [start, end]))
  }, [])

  const chunkQueries = useMemo(() => {
    const first = Math.floor(Math.max(0, range[0]) / CHUNK_SIZE)
    const last = Math.floor(Math.max(0, Math.min(range[1], ids.length - 1)) / CHUNK_SIZE)
    const queries = []
    for (let chunk = first; chunk <= last && ids.length > 0; chunk++) {
      queries.push(trpc.media.byIds.queryOptions({ ids: ids.slice(chunk * CHUNK_SIZE, (chunk + 1) * CHUNK_SIZE) }))
    }
    return queries
  }, [range, ids])

  const loaded = useQueries({ queries: chunkQueries, combine: (results) => results.map((result) => result.data) })

  // Items stay available while a refreshed layout refetches their chunks.
  const remembered = useRef(new Map<number, MediaItem>())
  if (remembered.current.size > MAX_REMEMBERED) remembered.current.clear()
  for (const chunk of loaded) {
    for (const item of chunk ?? []) remembered.current.set(item.id, item)
  }

  // biome-ignore lint/correctness/useExhaustiveDependencies: `loaded` signals that `remembered` gained items.
  const getItem = useCallback((index: number) => remembered.current.get(ids[index] ?? -1), [ids, loaded])
  const aspectAt = useCallback((index: number) => (aspects[index] ?? ASPECT_SCALE) / ASPECT_SCALE, [aspects])

  return { layout, ids, count: ids.length, aspectAt, getItem, setRange }
}
