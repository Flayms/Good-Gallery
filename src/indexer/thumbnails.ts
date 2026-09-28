import { join } from 'node:path'
import PQueue from 'p-queue'
import type { ThumbCache } from './thumb-cache'
import type { Thumbnails } from './thumbnail'
import type { IndexWriter, ThumbSource } from './writer'

export interface ThumbnailDeps {
  writer: IndexWriter
  cache: ThumbCache
  render: (file: string, source: ThumbSource) => Promise<Thumbnails>
  isReachable: (path: string) => Promise<boolean>
}

const BACKGROUND_BATCH = 100
const BACKGROUND_PRIORITY = 0

/**
 * Renders thumbnails on a shared queue: on-demand requests (visible tiles) run first, most recent first, since
 * earlier requests have likely scrolled out of view. Pending media are rendered in the background, newest first.
 */
export class ThumbnailService {
  readonly #deps: ThumbnailDeps
  readonly #queue: PQueue
  #concurrency: number
  readonly #jobs = new Map<number, Promise<boolean>>()
  readonly #requested = new Set<number>()
  #priority = BACKGROUND_PRIORITY
  #background: Promise<void> | undefined

  constructor(deps: ThumbnailDeps, concurrency: number) {
    this.#deps = deps
    this.#concurrency = concurrency
    this.#queue = new PQueue({ concurrency })
  }

  /** Resolves true once the thumbnail files exist, false if the media can't be rendered. */
  request(mediaId: number): Promise<boolean> {
    this.#requested.add(mediaId)
    const priority = ++this.#priority
    const existing = this.#jobs.get(mediaId)
    if (!existing) return this.#enqueue(mediaId, priority)
    try {
      this.#queue.setPriority(String(mediaId), priority)
    } catch {
      // Already running.
    }
    return existing
  }

  /** Starts background rendering of pending media unless it's already running. */
  kick(): void {
    this.#background ??= this.#runBackground()
      .catch((error: unknown) => console.error('Indexer: background thumbnails failed:', error))
      .finally(() => {
        this.#background = undefined
      })
  }

  setConcurrency(concurrency: number): void {
    this.#concurrency = concurrency
    this.#queue.concurrency = concurrency
  }

  /** Resolves when all queued work is done. */
  async idle(): Promise<void> {
    await this.#background
    await this.#queue.onIdle()
  }

  async #runBackground(): Promise<void> {
    // Otherwise the cache would look empty and background thumbnails could overfill it.
    await this.#deps.cache.ready
    for (;;) {
      const ids = this.#deps.writer.pendingThumbs(BACKGROUND_BATCH, [...this.#jobs.keys()])
      if (ids.length === 0) return
      for (const id of ids) void this.#enqueue(id, BACKGROUND_PRIORITY)
      await this.#queue.onSizeLessThan(this.#concurrency)
    }
  }

  #enqueue(mediaId: number, priority: number): Promise<boolean> {
    const job = this.#queue
      .add(() => this.#render(mediaId), { priority, id: String(mediaId) })
      .catch((error: unknown) => {
        console.error(`Indexer: thumbnail of media ${mediaId} failed:`, error)
        return false
      })
      .finally(() => {
        this.#jobs.delete(mediaId)
        this.#requested.delete(mediaId)
      })
    this.#jobs.set(mediaId, job)
    return job
  }

  async #render(mediaId: number): Promise<boolean> {
    const { writer, cache, render, isReachable } = this.#deps
    const source = writer.thumbSource(mediaId)
    if (!source) return false

    let thumbnails: Thumbnails
    try {
      thumbnails = await render(join(source.rootPath, ...source.relPath.split('/')), source)
    } catch (error) {
      if (await isReachable(source.rootPath)) {
        console.warn(`Indexer: cannot render thumbnail of ${source.relPath}:`, error)
        writer.setThumbnail(source, null)
      } else {
        // Picked up again by the next scan once the root is back.
        writer.setRootStatus(source.rootId, 'offline')
      }
      return false
    }

    // A full cache only takes thumbnails someone is looking at; background work still yields the ThumbHash.
    let stored = false
    if (this.#requested.has(mediaId) || cache.hasRoom()) {
      const writes = thumbnails.images.map(({ width, data }) => cache.write(cache.path(source, width), data))
      stored = await Promise.all(writes).then(
        () => true,
        // Not the file's fault (e.g. disk full), so don't mark it failed.
        (error: unknown) => {
          console.error(`Indexer: cannot cache thumbnail of ${source.relPath}:`, error)
          return false
        },
      )
    }
    writer.setThumbnail(source, thumbnails.thumbhash)
    return stored
  }
}
