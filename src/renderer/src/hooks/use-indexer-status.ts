import type { IndexerStatus } from '@main/indexer'
import { useQueryClient } from '@tanstack/react-query'
import { useSubscription } from '@trpc/tanstack-react-query'
import { useCallback, useEffect, useRef } from 'react'
import { trpc } from '@/lib/trpc'

/** While a scan runs, the gallery refreshes at most this often so new media appear before it ends. */
const REFRESH_INTERVAL_MS = 10_000

/** Live indexer status; refreshes libraries and indexed data (media, tags, folders) as scans progress and finish. */
export function useIndexerStatus(): IndexerStatus | undefined {
  const queryClient = useQueryClient()
  const { data: status } = useSubscription(trpc.indexer.status.subscriptionOptions())
  const scanningRoot = status?.current?.rootId
  const indexed = status?.current?.indexed ?? 0
  const previousRoot = useRef<number | undefined>(undefined)
  const lastRefresh = useRef({ at: 0, indexed: 0 })

  const refreshIndexed = useCallback(() => {
    for (const queryKey of [trpc.media.search.pathKey(), trpc.tags.pathKey(), trpc.libraries.folders.pathKey()]) {
      void queryClient.invalidateQueries({ queryKey })
    }
  }, [queryClient])

  // Scans update root status (online/offline) when they start and media when they end.
  useEffect(() => {
    const finished = previousRoot.current
    if (finished === scanningRoot) return
    previousRoot.current = scanningRoot
    void queryClient.invalidateQueries({ queryKey: trpc.libraries.list.queryKey() })
    if (finished === undefined) return
    lastRefresh.current = { at: Date.now(), indexed: 0 }
    refreshIndexed()
  }, [scanningRoot, queryClient, refreshIndexed])

  useEffect(() => {
    const now = Date.now()
    if (indexed <= lastRefresh.current.indexed || now - lastRefresh.current.at < REFRESH_INTERVAL_MS) return
    lastRefresh.current = { at: now, indexed }
    refreshIndexed()
  }, [indexed, refreshIndexed])

  return status
}
