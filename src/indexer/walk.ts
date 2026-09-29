import { opendir, stat } from 'node:fs/promises'
import { join, parse } from 'node:path'
import { isIgnoredDir, isSidecar, type MediaKind, mediaKind } from './media-types'
import { withRetry } from './retry'

export interface DirListing {
  /** Relative to the root, `/`-separated; `''` for the root itself. */
  relDir: string
  absDir: string
  /** Folder mtime, taken before listing it. */
  mtime: number
  files: { name: string; kind: MediaKind }[]
  /** Lower-cased sidecar file name → actual name. */
  sidecars: Map<string, string>
  /** Relative paths of the subfolders, housekeeping folders excluded. */
  subdirs: string[]
}

export interface WalkOptions {
  signal?: AbortSignal
  /** Called for folders that couldn't be (fully) listed; they aren't yielded. */
  onError: (relDir: string, error: unknown) => void
  /** Folders to start from; defaults to the root. */
  from?: string[]
  /**
   * Called with a folder's mtime before listing it. Returning folders skips the listing and continues with those
   * instead (e.g. the known subfolders of an unchanged folder).
   */
  skip?: (relDir: string, mtime: number) => string[] | undefined
  /** Subfolders of a listed folder to descend into; defaults to all of them. */
  descend?: (listing: DirListing) => string[]
}

/** Depth-first walk yielding one listing per folder, each at most once. Skips symlinks and housekeeping folders. */
export async function* walk(
  rootPath: string,
  { signal, onError, from = [''], skip, descend = (listing) => listing.subdirs }: WalkOptions,
): AsyncGenerator<DirListing> {
  const pending = from.toReversed()
  const visited = new Set<string>()
  for (let relDir = pending.pop(); relDir !== undefined; relDir = pending.pop()) {
    signal?.throwIfAborted()
    if (visited.has(relDir)) continue
    visited.add(relDir)
    const absDir = relDir ? join(rootPath, relDir) : rootPath
    const listing: DirListing = { relDir, absDir, mtime: 0, files: [], sidecars: new Map(), subdirs: [] }
    try {
      listing.mtime = Math.trunc((await withRetry(() => stat(absDir), { signal })).mtimeMs)
      const next = skip?.(relDir, listing.mtime)
      if (next) {
        pending.push(...next.toReversed())
        continue
      }
      const dir = await withRetry(() => opendir(absDir, { bufferSize: 256 }), { signal })
      for await (const entry of dir) {
        if (entry.isDirectory()) {
          if (!isIgnoredDir(entry.name)) listing.subdirs.push(relDir ? `${relDir}/${entry.name}` : entry.name)
        } else if (entry.isFile()) {
          const kind = mediaKind(entry.name)
          if (kind) listing.files.push({ name: entry.name, kind })
          else if (isSidecar(entry.name)) listing.sidecars.set(entry.name.toLowerCase(), entry.name)
        }
      }
    } catch (error) {
      if (signal?.aborted) throw error
      onError(relDir, error)
      continue
    }
    pending.push(...descend(listing).toReversed())
    yield listing
  }
}

/** Matches `IMG_1.jpg.xmp` (darktable, digiKam) before `IMG_1.xmp` (Lightroom). */
export function findSidecar(listing: DirListing, fileName: string): string | undefined {
  const lower = fileName.toLowerCase()
  return listing.sidecars.get(`${lower}.xmp`) ?? listing.sidecars.get(`${parse(lower).name}.xmp`)
}
