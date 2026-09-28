import { trpc } from '@/lib/trpc'
import type { IndexerStatus } from '@main/indexer'
import { useQueryClient } from '@tanstack/react-query'
import { useSubscription } from '@trpc/tanstack-react-query'
import { useEffect, useRef } from 'react'

/** While a scan runs, the gallery refreshes at most this often so new media appear before it ends. */
const REFRESH_INTERVAL_MS = 10_000

/** Live indexer status; refreshes libraries and media as scans progress and finish. */
export function useIndexerStatus(): IndexerStatus | undefined {
  const queryClient = useQueryClient()
  const { data: status } = useSubscription(trpc.indexer.status.subscriptionOptions())
  const scanningRoot = status?.current?.rootId
  const indexed = status?.current?.indexed ?? 0
  const previousRoot = useRef<number | undefined>(undefined)
  const lastRefresh = useRef({ at: 0, indexed: 0 })

  // Scans update root status (online/offline) when they start and media when they end.
  useEffect(() => {
    const finished = previousRoot.current
    if (finished === scanningRoot) return
    previousRoot.current = scanningRoot
    void queryClient.invalidateQueries({ queryKey: trpc.libraries.list.queryKey() })
    if (finished === undefined) return
    lastRefresh.current = { at: Date.now(), indexed: 0 }
    void queryClient.invalidateQueries({ queryKey: trpc.media.search.pathKey() })
  }, [scanningRoot, queryClient])

  useEffect(() => {
    const now = Date.now()
    if (indexed <= lastRefresh.current.indexed || now - lastRefresh.current.at < REFRESH_INTERVAL_MS) return
    lastRefresh.current = { at: now, indexed }
    void queryClient.invalidateQueries({ queryKey: trpc.media.search.pathKey() })
  }, [indexed, queryClient])

  return status
}
