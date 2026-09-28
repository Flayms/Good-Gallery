import { createHash, randomUUID } from 'node:crypto'
import { mkdir, readdir, rename, rm, stat, unlink, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import type { ThumbWidth } from '../shared/media-urls'
import { errorCode } from './retry'

/** The file state a thumbnail was rendered from; any change yields a new cache key. */
export interface ThumbKey {
  rootId: number
  relPath: string
  size: number
  mtime: number
}

/** `<dir>/ab/cd/<sha1>.webp`, fanned out so no directory grows huge. */
export function thumbPath(dir: string, { rootId, relPath, size, mtime }: ThumbKey, width: ThumbWidth): string {
  const hash = createHash('sha1')
    .update(JSON.stringify([rootId, relPath, size, mtime, width]))
    .digest('hex')
  return join(dir, hash.slice(0, 2), hash.slice(2, 4), `${hash}.webp`)
}

interface CachedFile {
  path: string
  size: number
  mtimeMs: number
}

const TMP_SUFFIX = '.tmp'
// Evicting below the cap leaves headroom, so eviction doesn't run on every write.
const EVICT_TO = 0.9
const STAT_BATCH = 256

/**
 * Size-capped thumbnail store with LRU eviction by file mtime; main bumps the mtime of thumbnails it serves.
 * Only the indexer process writes or evicts.
 */
export class ThumbCache {
  readonly dir: string
  readonly #maxBytes: number
  #bytes = 0
  #evicting: Promise<void> | undefined
  /** Resolves once the current cache size is known. */
  readonly ready: Promise<void>
  #measured = false

  constructor(dir: string, maxBytes: number) {
    this.dir = dir
    this.#maxBytes = maxBytes
    this.ready = this.#measure()
  }

  path(key: ThumbKey, width: ThumbWidth): string {
    return thumbPath(this.dir, key, width)
  }

  hasRoom(): boolean {
    return this.#measured && this.#bytes < this.#maxBytes
  }

  /** Writes atomically, so main never serves a partial file. */
  async write(path: string, data: Uint8Array): Promise<void> {
    await mkdir(dirname(path), { recursive: true })
    const tmp = `${path}.${randomUUID()}${TMP_SUFFIX}`
    await writeFile(tmp, data)
    try {
      await rename(tmp, path)
    } catch (error) {
      await rm(tmp, { force: true })
      throw error
    }
    this.#bytes += data.byteLength
    if (this.#bytes > this.#maxBytes) void this.evict()
  }

  /** Deletes least recently used thumbnails until the cache is below its cap. */
  evict(): Promise<void> {
    this.#evicting ??= this.#evict().finally(() => {
      this.#evicting = undefined
    })
    return this.#evicting
  }

  async #evict(): Promise<void> {
    await this.ready
    const files = await this.#list(false)
    let bytes = files.reduce((sum, file) => sum + file.size, 0)
    const target = this.#maxBytes * EVICT_TO
    files.sort((a, b) => a.mtimeMs - b.mtimeMs)
    for (const file of files) {
      if (bytes <= target) break
      try {
        await unlink(file.path)
        bytes -= file.size
      } catch (error) {
        // Main may be reading the file right now (EBUSY/EPERM on Windows); it goes next time.
        if (errorCode(error) === 'ENOENT') bytes -= file.size
      }
    }
    this.#bytes = bytes
  }

  async #measure(): Promise<void> {
    try {
      const files = await this.#list(true)
      this.#bytes = files.reduce((sum, file) => sum + file.size, 0)
    } finally {
      this.#measured = true
    }
  }

  /** All cached thumbnails. At startup, temp files can only be leftovers of a crash and are removed. */
  async #list(removeTemp: boolean): Promise<CachedFile[]> {
    let paths: string[]
    try {
      const entries = await readdir(this.dir, { recursive: true, withFileTypes: true })
      paths = entries.filter((entry) => entry.isFile()).map((entry) => join(entry.parentPath, entry.name))
    } catch (error) {
      if (errorCode(error) === 'ENOENT') return []
      throw error
    }

    const files: CachedFile[] = []
    for (let i = 0; i < paths.length; i += STAT_BATCH) {
      const batch = paths.slice(i, i + STAT_BATCH)
      await Promise.all(
        batch.map(async (path) => {
          try {
            if (path.endsWith(TMP_SUFFIX)) {
              if (removeTemp) await rm(path, { force: true })
              return
            }
            const { size, mtimeMs } = await stat(path)
            files.push({ path, size, mtimeMs })
          } catch {
            // Deleted concurrently.
          }
        }),
      )
    }
    return files
  }
}
