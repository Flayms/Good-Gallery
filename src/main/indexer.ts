import { EventEmitter } from 'node:events'
import type { IndexerEvent, IndexerRequest, ScanProgress } from '../indexer/protocol'

export interface IndexerStatus {
  state: 'idle' | 'scanning'
  /** Root ids waiting to be scanned, in order. */
  queue: number[]
  current: ScanProgress | null
}

/** Transport to the indexer process, abstracted for tests. */
export interface IndexerWorker {
  send(request: IndexerRequest): void
  onEvent(listener: (event: IndexerEvent) => void): void
  onExit(listener: () => void): void
  kill(): void
}

/** Main-process facade for the indexer: queues scans and runs them one at a time in a lazily spawned worker. */
export class IndexerController extends EventEmitter<{ status: [IndexerStatus] }> {
  readonly #spawn: () => IndexerWorker
  #worker: IndexerWorker | undefined
  #disposed = false
  #status: IndexerStatus = { state: 'idle', queue: [], current: null }
  /** Resolvers of in-flight thumbnail requests by media id. */
  readonly #thumbnails = new Map<number, { promise: Promise<boolean>; resolve: (ok: boolean) => void }>()

  constructor(spawn: () => IndexerWorker) {
    super()
    this.#spawn = spawn
  }

  get status(): IndexerStatus {
    return this.#status
  }

  requestScan(rootId: number): void {
    if (this.#status.queue.includes(rootId) || this.#status.current?.rootId === rootId) return
    this.#update({ queue: [...this.#status.queue, rootId] })
    this.#dispatch()
  }

  cancelScan(rootId: number): void {
    if (this.#status.current?.rootId === rootId) {
      this.#worker?.send({ type: 'cancel', rootId })
    } else if (this.#status.queue.includes(rootId)) {
      this.#update({ queue: this.#status.queue.filter((id) => id !== rootId) })
    }
  }

  dispose(): void {
    this.#disposed = true
    this.#worker?.kill()
    this.#worker = undefined
    this.#failThumbnails()
  }

  /** Renders a thumbnail ahead of background work. Resolves true once its files are in the cache. */
  requestThumbnail(mediaId: number): Promise<boolean> {
    if (this.#disposed) return Promise.resolve(false)
    const pending = this.#thumbnails.get(mediaId)
    if (pending) return pending.promise
    const { promise, resolve } = Promise.withResolvers<boolean>()
    this.#thumbnails.set(mediaId, { promise, resolve })
    this.#ensureWorker().send({ type: 'thumbnail', mediaId })
    return promise
  }

  #dispatch(): void {
    const [rootId, ...queue] = this.#status.queue
    if (this.#disposed || this.#status.current || rootId === undefined) return
    this.#ensureWorker().send({ type: 'scan', rootId })
    this.#update({ state: 'scanning', queue, current: { rootId, scanned: 0, indexed: 0 } })
  }

  #ensureWorker(): IndexerWorker {
    if (this.#worker) return this.#worker
    const worker = this.#spawn()
    worker.onEvent((event) => this.#onEvent(event))
    worker.onExit(() => {
      if (this.#worker !== worker) return
      this.#worker = undefined
      this.#failThumbnails()
      const rootId = this.#status.current?.rootId
      if (rootId !== undefined) {
        console.error(`Indexer process exited while scanning root ${rootId}`)
        this.#finish()
      }
    })
    this.#worker = worker
    return worker
  }

  #onEvent(event: IndexerEvent): void {
    if (event.type === 'thumbnail') {
      this.#thumbnails.get(event.mediaId)?.resolve(event.ok)
      this.#thumbnails.delete(event.mediaId)
      return
    }
    if (event.rootId !== this.#status.current?.rootId) return
    if (event.type === 'progress') {
      const { type: _, ...current } = event
      this.#update({ current })
    } else {
      if (event.outcome === 'failed') console.error(`Indexer: scan of root ${event.rootId} failed: ${event.error}`)
      this.#finish()
    }
  }

  #finish(): void {
    this.#update({ state: 'idle', current: null })
    this.#dispatch()
  }

  #failThumbnails(): void {
    for (const { resolve } of this.#thumbnails.values()) resolve(false)
    this.#thumbnails.clear()
  }

  #update(patch: Partial<IndexerStatus>): void {
    this.#status = { ...this.#status, ...patch }
    this.emit('status', this.#status)
  }
}
