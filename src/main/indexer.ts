import { EventEmitter } from 'node:events'
import {
  type IndexerConfig,
  type IndexerEvent,
  type IndexerRequest,
  MAX_CHANGED_PATHS,
  type ScanProgress,
  type ScanScope,
} from '../indexer/protocol'

export interface IndexerStatus {
  state: 'idle' | 'scanning'
  /** Root ids waiting to be scanned, in order. */
  queue: number[]
  current: (ScanProgress & { mode: ScanScope['mode'] }) | null
}

interface ScanJob {
  rootId: number
  scope: ScanScope
}

/** How much of a root a scope covers: a larger one also finds everything a smaller one would. */
const COVERAGE: Record<ScanScope['mode'], number> = { changes: 0, quick: 1, full: 2 }

/** A scope covering both scans of a root. */
export function mergeScopes(a: ScanScope, b: ScanScope): ScanScope {
  if (a.mode === 'changes' && b.mode === 'changes') {
    const paths = [...new Set([...a.paths, ...b.paths])]
    return paths.length <= MAX_CHANGED_PATHS ? { mode: 'changes', paths } : { mode: 'quick' }
  }
  return COVERAGE[a.mode] >= COVERAGE[b.mode] ? a : b
}

/** Transport to the indexer process, abstracted for tests. */
export interface IndexerWorker {
  send(request: IndexerRequest): void
  onEvent(listener: (event: IndexerEvent) => void): void
  onExit(listener: () => void): void
  kill(): void
}

/**
 * Main-process facade for the indexer: queues scans (one per root, merged) and runs them one at a time in a lazily
 * spawned worker. Changes reported by the worker's watchers are queued as scans too.
 */
export class IndexerController extends EventEmitter<{ status: [IndexerStatus] }> {
  readonly #spawn: () => IndexerWorker
  #worker: IndexerWorker | undefined
  #disposed = false
  #status: IndexerStatus = { state: 'idle', queue: [], current: null }
  readonly #jobs: ScanJob[] = []
  #running: ScanJob | undefined
  /** Resolvers of in-flight thumbnail requests by media id. */
  readonly #thumbnails = new Map<number, { promise: Promise<boolean>; resolve: (ok: boolean) => void }>()
  readonly #cacheRequests = new Map<number, PromiseWithResolvers<number>>()
  #nextRequestId = 0

  constructor(spawn: () => IndexerWorker) {
    super()
    this.#spawn = spawn
  }

  get status(): IndexerStatus {
    return this.#status
  }

  requestScan(rootId: number, scope: ScanScope): void {
    const running = this.#running
    // Changes may be in folders the running scan already passed, so only other scopes can be covered by it.
    if (
      running?.rootId === rootId &&
      scope.mode !== 'changes' &&
      COVERAGE[running.scope.mode] >= COVERAGE[scope.mode]
    ) {
      return
    }
    const queued = this.#jobs.find((job) => job.rootId === rootId)
    if (queued) {
      queued.scope = mergeScopes(queued.scope, scope)
      return
    }
    this.#jobs.push({ rootId, scope })
    this.#update({ queue: this.#jobs.map((job) => job.rootId) })
    this.#dispatch()
  }

  /** Forgets a removed root: drops its queued scan, cancels a running one and stops watching it. */
  removeRoot(rootId: number): void {
    const index = this.#jobs.findIndex((job) => job.rootId === rootId)
    if (index !== -1) {
      this.#jobs.splice(index, 1)
      this.#update({ queue: this.#jobs.map((job) => job.rootId) })
    }
    this.#worker?.send({ type: 'remove', rootId })
  }

  dispose(): void {
    this.#disposed = true
    this.#worker?.kill()
    this.#worker = undefined
    this.#failPending()
  }

  /** Applies changed settings to a running worker; a newly spawned one reads them at spawn time. */
  configure(config: IndexerConfig): void {
    this.#worker?.send({ type: 'configure', ...config })
  }

  /** Resolves with the thumbnail cache size in bytes, after emptying it for `clear`. */
  thumbnailCache(action: 'usage' | 'clear'): Promise<number> {
    if (this.#disposed) return Promise.reject(new Error('Indexer is shut down'))
    const requestId = this.#nextRequestId++
    const request = Promise.withResolvers<number>()
    this.#cacheRequests.set(requestId, request)
    this.#ensureWorker().send({ type: 'cache', requestId, action })
    return request.promise
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
    if (this.#disposed || this.#running) return
    const job = this.#jobs.shift()
    if (!job) return
    this.#running = job
    const { rootId, scope } = job
    this.#ensureWorker().send({ type: 'scan', rootId, scope })
    this.#update({
      state: 'scanning',
      queue: this.#jobs.map((queued) => queued.rootId),
      current: { rootId, mode: scope.mode, scanned: 0, indexed: 0 },
    })
  }

  #ensureWorker(): IndexerWorker {
    if (this.#worker) return this.#worker
    const worker = this.#spawn()
    worker.onEvent((event) => this.#onEvent(event))
    worker.onExit(() => {
      if (this.#worker !== worker) return
      this.#worker = undefined
      this.#failPending()
      if (this.#running) {
        console.error(`Indexer process exited while scanning root ${this.#running.rootId}`)
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
    if (event.type === 'cache') {
      const request = this.#cacheRequests.get(event.requestId)
      this.#cacheRequests.delete(event.requestId)
      if (event.bytes === undefined) request?.reject(new Error('Thumbnail cache request failed'))
      else request?.resolve(event.bytes)
      return
    }
    if (event.type === 'changed') {
      this.requestScan(event.rootId, event.paths ? { mode: 'changes', paths: event.paths } : { mode: 'quick' })
      return
    }
    const current = this.#status.current
    if (event.rootId !== current?.rootId) return
    if (event.type === 'progress') {
      const { type: _, ...progress } = event
      this.#update({ current: { ...current, ...progress } })
    } else {
      if (event.outcome === 'failed') console.error(`Indexer: scan of root ${event.rootId} failed: ${event.error}`)
      this.#finish()
    }
  }

  #finish(): void {
    this.#running = undefined
    this.#update({ state: 'idle', current: null })
    this.#dispatch()
  }

  #failPending(): void {
    for (const { resolve } of this.#thumbnails.values()) resolve(false)
    this.#thumbnails.clear()
    for (const { reject } of this.#cacheRequests.values()) reject(new Error('Indexer process exited'))
    this.#cacheRequests.clear()
  }

  #update(patch: Partial<IndexerStatus>): void {
    this.#status = { ...this.#status, ...patch }
    this.emit('status', this.#status)
  }
}
