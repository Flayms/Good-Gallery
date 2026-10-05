import { eq, lt } from 'drizzle-orm'
import { METADATA_VERSION } from '../shared/metadata'
import type { Db } from './db'
import { libraryRoots, media } from './db/schema'
import type { IndexerController } from './indexer'

export interface RescanSchedule {
  /** Quick scan of every root, for changes the watchers missed; `0` disables it. */
  intervalMs: number
  /** Reachability re-check of offline roots, so reconnects are picked up quickly. */
  offlineRetryMs: number
}

export interface RescanScheduler {
  /** Restarts the quick-scan timer without scanning right away; `0` disables it. */
  setInterval(intervalMs: number): void
  stop(): void
}

/**
 * Quick-scans all roots now and then periodically. Between scans the indexer watches the roots; a completed scan
 * (re)starts the watch, so the periodic scan also recovers watches that broke silently.
 */
export function scheduleRescans(db: Db, indexer: IndexerController, schedule: RescanSchedule): RescanScheduler {
  const scanAll = () => {
    // Roots with rows from an older metadata extractor get a full scan instead, once, to pick up the new fields.
    const outdated = new Set(
      db
        .selectDistinct({ rootId: media.rootId })
        .from(media)
        .where(lt(media.metaVersion, METADATA_VERSION))
        .all()
        .map((row) => row.rootId),
    )
    for (const { id } of db.select({ id: libraryRoots.id }).from(libraryRoots).all()) {
      indexer.requestScan(id, { mode: outdated.has(id) ? 'full' : 'quick' })
    }
  }
  const scanOffline = () => {
    const offline = db.select({ id: libraryRoots.id }).from(libraryRoots).where(eq(libraryRoots.status, 'offline'))
    for (const { id } of offline.all()) indexer.requestScan(id, { mode: 'quick' })
  }

  scanAll()
  const offlineTimer = setInterval(scanOffline, schedule.offlineRetryMs)
  let fullTimer: NodeJS.Timeout | undefined
  const setFullInterval = (intervalMs: number) => {
    clearInterval(fullTimer)
    fullTimer = intervalMs > 0 ? setInterval(scanAll, intervalMs) : undefined
  }
  setFullInterval(schedule.intervalMs)

  return {
    setInterval: setFullInterval,
    stop: () => {
      clearInterval(offlineTimer)
      clearInterval(fullTimer)
    },
  }
}
