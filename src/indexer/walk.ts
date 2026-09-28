import { opendir } from 'node:fs/promises'
import { join, parse } from 'node:path'
import { isIgnoredDir, isSidecar, type MediaKind, mediaKind } from './media-types'
import { withRetry } from './retry'

export interface DirListing {
  /** Relative to the root, `/`-separated; `''` for the root itself. */
  relDir: string
  absDir: string
  files: { name: string; kind: MediaKind }[]
  /** Lower-cased sidecar file name → actual name. */
  sidecars: Map<string, string>
}

export interface WalkOptions {
  signal?: AbortSignal
  /** Called for directories that couldn't be (fully) listed; they aren't yielded. */
  onError: (relDir: string, error: unknown) => void
}

/** Depth-first walk yielding one listing per directory. Skips symlinks and housekeeping folders. */
export async function* walk(rootPath: string, { signal, onError }: WalkOptions): AsyncGenerator<DirListing> {
  const pending = ['']
  for (let relDir = pending.pop(); relDir !== undefined; relDir = pending.pop()) {
    signal?.throwIfAborted()
    const absDir = relDir ? join(rootPath, relDir) : rootPath
    const listing: DirListing = { relDir, absDir, files: [], sidecars: new Map() }
    const subdirs: string[] = []
    try {
      const dir = await withRetry(() => opendir(absDir, { bufferSize: 256 }), { signal })
      for await (const entry of dir) {
        if (entry.isDirectory()) {
          if (!isIgnoredDir(entry.name)) subdirs.push(relDir ? `${relDir}/${entry.name}` : entry.name)
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
    pending.push(...subdirs.reverse())
    yield listing
  }
}

/** Matches `IMG_1.jpg.xmp` (darktable, digiKam) before `IMG_1.xmp` (Lightroom). */
export function findSidecar(listing: DirListing, fileName: string): string | undefined {
  const lower = fileName.toLowerCase()
  return listing.sidecars.get(`${lower}.xmp`) ?? listing.sidecars.get(`${parse(lower).name}.xmp`)
}
