import { eq } from 'drizzle-orm'
import type { Db } from './db'
import { libraryRoots } from './db/schema'
import type { IndexerController } from './indexer'

export interface RescanSchedule {
  /** Full incremental rescan of every root (`fs.watch` is unreliable on SMB). */
  intervalMs: number
  /** Reachability re-check of offline roots, so reconnects are picked up quickly. */
  offlineRetryMs: number
}

/** Scans all roots now and then periodically. Returns a function that stops the schedule. */
export function scheduleRescans(db: Db, indexer: IndexerController, schedule: RescanSchedule): () => void {
  const scanAll = () => {
    for (const { id } of db.select({ id: libraryRoots.id }).from(libraryRoots).all()) indexer.requestScan(id)
  }
  const scanOffline = () => {
    const offline = db.select({ id: libraryRoots.id }).from(libraryRoots).where(eq(libraryRoots.status, 'offline'))
    for (const { id } of offline.all()) indexer.requestScan(id)
  }

  scanAll()
  const timers = [setInterval(scanAll, schedule.intervalMs), setInterval(scanOffline, schedule.offlineRetryMs)]
  return () => {
    for (const timer of timers) clearInterval(timer)
  }
}
