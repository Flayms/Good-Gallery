import type { Stats } from 'node:fs'
import { stat } from 'node:fs/promises'
import { join } from 'node:path'
import PQueue from 'p-queue'
import { EMPTY_METADATA, type MediaMetadata, type MetadataSource } from './metadata'
import type { ScanOutcome, ScanProgress } from './protocol'
import { errorCode, isTransient, withRetry } from './retry'
import { type DirListing, findSidecar, walk } from './walk'
import type { IndexedFile, IndexWriter } from './writer'

export interface ScanDeps {
  writer: IndexWriter
  metadata: MetadataSource
  isReachable: (path: string) => Promise<boolean>
}

export interface ScanOptions {
  /** Parallel file operations (stat + metadata). */
  concurrency: number
  batchSize?: number
  progressIntervalMs?: number
  signal: AbortSignal
  onProgress: (progress: ScanProgress) => void
}

function isWithin(relPath: string, relDir: string): boolean {
  return relDir === '' || relPath.startsWith(`${relDir}/`)
}

async function statIfExists(path: string, signal: AbortSignal): Promise<Stats | undefined> {
  try {
    return await withRetry(() => stat(path), { signal })
  } catch (error) {
    if (errorCode(error) === 'ENOENT') return undefined
    throw error
  }
}

/**
 * Incrementally syncs one library root with the index: new or changed files (by size, mtime and sidecar
 * mtime) are (re-)read, vanished files are removed. Files in directories that couldn't be listed are kept.
 */
export async function scanRoot(
  { writer, metadata, isReachable }: ScanDeps,
  rootId: number,
  { concurrency, batchSize = 1000, progressIntervalMs = 250, signal, onProgress }: ScanOptions,
): Promise<ScanOutcome> {
  const root = writer.root(rootId)
  if (!root) return 'cancelled'
  if (!(await isReachable(root.path))) {
    writer.setRootStatus(rootId, 'offline')
    return 'offline'
  }
  writer.setRootStatus(rootId, 'online')

  const offline = new AbortController()
  const scanSignal = AbortSignal.any([signal, offline.signal])
  let reachabilityCheck: Promise<void> | undefined
  /** Distinguishes a dropped connection from a single bad file, aborting the scan in the former case. */
  const checkReachable = (): Promise<void> => {
    reachabilityCheck ??= isReachable(root.path).then((reachable) => {
      if (!reachable) offline.abort()
      reachabilityCheck = undefined
    })
    return reachabilityCheck
  }

  // Entries left over after the walk no longer exist on disk.
  const unseen = writer.existing(rootId)
  const failedDirs: string[] = []
  const pending: IndexedFile[] = []
  const progress: ScanProgress = { rootId, scanned: 0, indexed: 0 }
  let lastReport = 0
  const report = (force = false) => {
    const now = Date.now()
    if (!force && now - lastReport < progressIntervalMs) return
    lastReport = now
    onProgress({ ...progress })
  }
  const flush = () => {
    if (pending.length === 0) return
    writer.write(rootId, pending.splice(0))
    report(true)
  }

  const readMetadata = async (file: string, sidecar?: string): Promise<MediaMetadata | undefined> => {
    try {
      return await metadata.read(file, sidecar)
    } catch (error) {
      await checkReachable()
      if (scanSignal.aborted) return undefined
      console.warn(`Indexer: unreadable metadata in ${file}:`, error)
      return EMPTY_METADATA
    }
  }

  const processFile = async (listing: DirListing, { name, kind }: DirListing['files'][number]) => {
    if (scanSignal.aborted) return
    const relPath = listing.relDir ? `${listing.relDir}/${name}` : name
    const absPath = join(listing.absDir, name)
    const sidecarName = findSidecar(listing, name)
    const sidecarPath = sidecarName ? join(listing.absDir, sidecarName) : undefined
    try {
      const [stats, sidecarStats] = await Promise.all([
        withRetry(() => stat(absPath), { signal: scanSignal }),
        sidecarPath ? statIfExists(sidecarPath, scanSignal) : undefined,
      ])
      const known = unseen.get(relPath)
      unseen.delete(relPath)
      progress.scanned++

      const mtime = Math.trunc(stats.mtimeMs)
      const sidecarMtime = sidecarStats ? Math.trunc(sidecarStats.mtimeMs) : null
      if (known?.size === stats.size && known.mtime === mtime && known.sidecarMtime === sidecarMtime) return

      const fileMetadata = await readMetadata(absPath, sidecarStats ? sidecarPath : undefined)
      if (!fileMetadata) return
      pending.push({ relPath, fileName: name, kind, size: stats.size, mtime, sidecarMtime, metadata: fileMetadata })
      progress.indexed++
    } catch (error) {
      if (scanSignal.aborted) return
      // Vanished files stay in `unseen` and get removed; others keep their last indexed state.
      if (errorCode(error) === 'ENOENT') return
      unseen.delete(relPath)
      if (isTransient(error)) await checkReachable()
      console.warn(`Indexer: cannot read ${absPath}:`, error)
    }
  }

  const queue = new PQueue({ concurrency })
  try {
    const listings = walk(root.path, {
      signal: scanSignal,
      onError: (relDir, error) => {
        failedDirs.push(relDir)
        console.warn(`Indexer: cannot list ${join(root.path, relDir)}:`, error)
        if (isTransient(error)) void checkReachable()
      },
    })
    for await (const listing of listings) {
      for (const file of listing.files) {
        // Backpressure: don't queue a whole share's worth of files up front.
        await queue.onSizeLessThan(concurrency * 4)
        if (scanSignal.aborted) break
        // processFile handles its own errors.
        void queue.add(() => processFile(listing, file))
        if (pending.length >= batchSize) flush()
        report()
      }
    }
  } catch (error) {
    if (!scanSignal.aborted) throw error
  } finally {
    if (scanSignal.aborted) queue.clear()
    await queue.onIdle()
    await reachabilityCheck
  }

  // A removed root cancels its scan; its rows are gone, so nothing may be written anymore.
  if (signal.aborted) return 'cancelled'
  flush()
  if (offline.signal.aborted) {
    writer.setRootStatus(rootId, 'offline')
    return 'offline'
  }

  const stale = [...unseen].filter(([relPath]) => !failedDirs.some((dir) => isWithin(relPath, dir)))
  writer.remove(stale.map(([, entry]) => entry.id))
  writer.pruneTags()
  writer.setRootStatus(rootId, 'online', Date.now())
  report(true)
  return 'completed'
}
