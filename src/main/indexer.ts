import { EventEmitter } from 'node:events'

export interface IndexerStatus {
  state: 'idle' | 'scanning'
  /** Root ids waiting to be scanned, in order. */
  queue: number[]
  current: { rootId: number; scanned: number } | null
}

/**
 * Main-process facade for the indexer. Phase 2 forwards requests to the
 * indexer utilityProcess; for now it only tracks the scan queue.
 */
export class IndexerController extends EventEmitter<{ status: [IndexerStatus] }> {
  #status: IndexerStatus = { state: 'idle', queue: [], current: null }

  get status(): IndexerStatus {
    return this.#status
  }

  requestScan(rootId: number): void {
    if (this.#status.queue.includes(rootId) || this.#status.current?.rootId === rootId) return
    this.#update({ queue: [...this.#status.queue, rootId] })
  }

  cancelScan(rootId: number): void {
    if (!this.#status.queue.includes(rootId)) return
    this.#update({ queue: this.#status.queue.filter((id) => id !== rootId) })
  }

  #update(patch: Partial<IndexerStatus>): void {
    this.#status = { ...this.#status, ...patch }
    this.emit('status', this.#status)
  }
}
