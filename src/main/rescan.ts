import { eq } from 'drizzle-orm'
import type { Db } from './db'
import { libraryRoots } from './db/schema'
import type { IndexerController } from './indexer'

export interface RescanSchedule {
  /** Full incremental rescan of every root (`fs.watch` is unreliable on SMB); `0` disables it. */
  intervalMs: number
  /** Reachability re-check of offline roots, so reconnects are picked up quickly. */
  offlineRetryMs: number
}

export interface RescanScheduler {
  /** Restarts the full-rescan timer without scanning right away; `0` disables it. */
  setInterval(intervalMs: number): void
  stop(): void
}

/** Scans all roots now and then periodically. */
export function scheduleRescans(db: Db, indexer: IndexerController, schedule: RescanSchedule): RescanScheduler {
  const scanAll = () => {
    for (const { id } of db.select({ id: libraryRoots.id }).from(libraryRoots).all()) indexer.requestScan(id)
  }
  const scanOffline = () => {
    const offline = db.select({ id: libraryRoots.id }).from(libraryRoots).where(eq(libraryRoots.status, 'offline'))
    for (const { id } of offline.all()) indexer.requestScan(id)
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
