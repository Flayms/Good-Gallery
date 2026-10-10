import type { Stats } from 'node:fs'
import { stat } from 'node:fs/promises'
import { join, posix } from 'node:path'
import PQueue from 'p-queue'
import { METADATA_VERSION } from '../shared/metadata'
import { EMPTY_METADATA, type MediaMetadata, type MetadataSource } from './metadata'
import type { ScanOutcome, ScanProgress, ScanScope } from './protocol'
import { errorCode, isTransient, withRetry } from './retry'
import { type DirListing, findSidecar, type WalkOptions, walk } from './walk'
import type { ExistingEntry, IndexedFile, IndexWriter } from './writer'

export interface ScanDeps {
  writer: IndexWriter
  metadata: MetadataSource
  isReachable: (path: string) => Promise<boolean>
}

export interface ScanOptions {
  /** Parallel file operations (stat + metadata). */
  concurrency: number
  /** Files written per transaction. */
  batchSize?: number
  /** Longest time read files wait to be written, so they show up while a slow scan still runs. */
  flushIntervalMs?: number
  progressIntervalMs?: number
  signal: AbortSignal
  onProgress: (progress: ScanProgress) => void
}

/** Mtime stored for folders that couldn't be listed, so the next quick scan lists them again. */
const UNLISTED = -1

function isWithin(relPath: string, relDir: string): boolean {
  return relDir === '' || relPath.startsWith(`${relDir}/`)
}

function parentDir(relDir: string): string {
  const slash = relDir.lastIndexOf('/')
  return slash === -1 ? '' : relDir.slice(0, slash)
}

/** Known subfolders by parent folder. */
function childIndex(stored: Map<string, number>): Map<string, string[]> {
  const children = new Map<string, string[]>()
  for (const relDir of stored.keys()) {
    if (relDir === '') continue
    const parent = parentDir(relDir)
    const siblings = children.get(parent)
    if (siblings) siblings.push(relDir)
    else children.set(parent, [relDir])
  }
  return children
}

/** Which folders a scope lists, given the folders stored by earlier scans. */
function traversal(
  scope: ScanScope,
  stored: Map<string, number>,
  children: Map<string, string[]>,
): Pick<WalkOptions, 'from' | 'skip' | 'descend'> {
  switch (scope.mode) {
    case 'full':
      return {}
    case 'quick':
      // An unchanged mtime means no entries were added, removed or renamed, so the listing would be the same.
      return { skip: (relDir, mtime) => (stored.get(relDir) === mtime ? (children.get(relDir) ?? []) : undefined) }
    case 'changes':
      // Listing the parent notices added and removed entries, including folders.
      return {
        from: [...new Set(scope.paths.map(parentDir))],
        descend: (listing) => listing.subdirs.filter((relDir) => !stored.has(relDir)),
      }
  }
}

/**
 * For `changes` scans: whether a known file must be checked, because it or one of its possible sidecars was reported.
 * Other files of the listed folders are unchanged. Compared case-insensitively, which at worst checks a file too many.
 */
function touchedFiles(scope: ScanScope): ((relPath: string) => boolean) | undefined {
  if (scope.mode !== 'changes') return undefined
  const touched = new Set(scope.paths.map((path) => path.toLowerCase()))
  return (relPath) => {
    const lower = relPath.toLowerCase()
    const { dir, name } = posix.parse(lower)
    const stem = dir ? `${dir}/${name}` : name
    return touched.has(lower) || touched.has(`${lower}.xmp`) || touched.has(`${stem}.xmp`)
  }
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
 * Incrementally syncs a library root (or the folders of `scope`) with the index: new or changed files (by size,
 * mtime and sidecar mtime) are (re-)read, vanished files and folders are removed. Files in folders that couldn't be
 * listed are kept. Read files are written in batches while the scan runs; removals happen once it completes.
 */
export async function scanRoot(
  { writer, metadata, isReachable }: ScanDeps,
  rootId: number,
  requested: ScanScope,
  { concurrency, batchSize = 250, flushIntervalMs = 2000, progressIntervalMs = 250, signal, onProgress }: ScanOptions,
): Promise<ScanOutcome> {
  const root = writer.root(rootId)
  if (!root) return 'cancelled'
  if (!(await isReachable(root.path))) {
    writer.setRootStatus(rootId, 'offline')
    return 'offline'
  }
  writer.setRootStatus(rootId, 'online')

  const stored = requested.mode === 'full' ? new Map<string, number>() : writer.folders(rootId)
  // Without folder data (new root, or no scan completed since) only a full scan finds everything.
  const scope: ScanScope = stored.size === 0 ? { mode: 'full' } : requested
  const full = scope.mode === 'full'
  const children = childIndex(stored)
  const isTouched = touchedFiles(scope)

  const offline = new AbortController()
  const failure = new AbortController()
  const scanSignal = AbortSignal.any([signal, offline.signal, failure.signal])
  let reachabilityCheck: Promise<void> | undefined
  /** Distinguishes a dropped connection from a single bad file, aborting the scan in the former case. */
  const checkReachable = (): Promise<void> => {
    reachabilityCheck ??= isReachable(root.path).then((reachable) => {
      if (!reachable) offline.abort()
      reachabilityCheck = undefined
    })
    return reachabilityCheck
  }

  // Entries left over after their folder was listed no longer exist on disk. A full scan preloads the whole root,
  // so files in vanished folders are left over too; other scans load each listed folder.
  const unseen = full ? writer.existing(rootId) : new Map<string, ExistingEntry>()
  const failedDirs: string[] = []
  const removedDirs: string[] = []
  /** Mtimes of the listed folders, stored once the scan completes. */
  const listed = new Map<string, number>()
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
    // A removed root cancels its scan; its rows are gone, so nothing may be written anymore.
    if (pending.length === 0 || signal.aborted) return
    const batch = pending.splice(0)
    writer.write(rootId, batch)
    progress.indexed += batch.length
    report(true)
  }
  const flushTimer = setInterval(() => {
    try {
      flush()
    } catch (error) {
      failure.abort(error)
    }
  }, flushIntervalMs)

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
    const known = unseen.get(relPath)
    if (known && isTouched && !isTouched(relPath)) {
      unseen.delete(relPath)
      return
    }
    try {
      const [stats, sidecarStats] = await Promise.all([
        withRetry(() => stat(absPath), { signal: scanSignal }),
        sidecarPath ? statIfExists(sidecarPath, scanSignal) : undefined,
      ])
      unseen.delete(relPath)
      progress.scanned++

      const mtime = Math.trunc(stats.mtimeMs)
      const sidecarMtime = sidecarStats ? Math.trunc(sidecarStats.mtimeMs) : null
      if (
        known?.size === stats.size &&
        known.mtime === mtime &&
        known.sidecarMtime === sidecarMtime &&
        known.metaVersion === METADATA_VERSION
      )
        return

      const fileMetadata = await readMetadata(absPath, sidecarStats ? sidecarPath : undefined)
      if (!fileMetadata) return
      pending.push({ relPath, fileName: name, kind, size: stats.size, mtime, sidecarMtime, metadata: fileMetadata })
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
      ...traversal(scope, stored, children),
      signal: scanSignal,
      onError: (relDir, error) => {
        if (relDir !== '' && errorCode(error) === 'ENOENT') {
          removedDirs.push(relDir)
          return
        }
        failedDirs.push(relDir)
        listed.set(relDir, UNLISTED)
        console.warn(`Indexer: cannot list ${join(root.path, relDir)}:`, error)
        if (isTransient(error)) void checkReachable()
      },
    })
    for await (const listing of listings) {
      listed.set(listing.relDir, listing.mtime)
      if (!full) {
        for (const [relPath, entry] of writer.existingIn(rootId, listing.relDir)) unseen.set(relPath, entry)
        const present = new Set(listing.subdirs)
        removedDirs.push(...(children.get(listing.relDir) ?? []).filter((relDir) => !present.has(relDir)))
      }
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
    clearInterval(flushTimer)
  }

  if (signal.aborted) return 'cancelled'
  if (failure.signal.aborted) throw failure.signal.reason
  flush()
  if (offline.signal.aborted) {
    writer.setRootStatus(rootId, 'offline')
    return 'offline'
  }

  const stale = [...unseen].filter(([relPath]) => !failedDirs.some((dir) => isWithin(relPath, dir)))
  writer.remove(stale.map(([, entry]) => entry.id))
  writer.removeFolders(
    rootId,
    removedDirs.filter((relDir) => !listed.has(relDir)),
  )
  writer.saveFolders(rootId, listed, { replace: full })
  writer.pruneTags()
  writer.setRootStatus(rootId, 'online', Date.now())
  report(true)
  return 'completed'
}
