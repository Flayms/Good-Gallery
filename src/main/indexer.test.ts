import { describe, expect, it } from 'vitest'
import type { IndexerEvent, IndexerRequest } from '../indexer/protocol'
import { IndexerController, type IndexerWorker } from './indexer'

class FakeWorker implements IndexerWorker {
  readonly sent: IndexerRequest[] = []
  killed = false
  #onEvent: (event: IndexerEvent) => void = () => {}
  #onExit: () => void = () => {}

  send(request: IndexerRequest) {
    this.sent.push(request)
  }
  onEvent(listener: (event: IndexerEvent) => void) {
    this.#onEvent = listener
  }
  onExit(listener: () => void) {
    this.#onExit = listener
  }
  kill() {
    this.killed = true
  }
  emit(event: IndexerEvent) {
    this.#onEvent(event)
  }
  exit() {
    this.#onExit()
  }
}

function setup() {
  const workers: FakeWorker[] = []
  const controller = new IndexerController(() => {
    const worker = new FakeWorker()
    workers.push(worker)
    return worker
  })
  return { controller, workers }
}

describe('IndexerController', () => {
  it('runs scans one at a time in request order', () => {
    const { controller, workers } = setup()

    controller.requestScan(1)
    controller.requestScan(2)
    controller.requestScan(1)

    expect(controller.status).toEqual({ state: 'scanning', queue: [2], current: { rootId: 1, scanned: 0, indexed: 0 } })
    workers[0]?.emit({ type: 'progress', rootId: 1, scanned: 10, indexed: 3 })
    expect(controller.status.current).toEqual({ rootId: 1, scanned: 10, indexed: 3 })

    workers[0]?.emit({ type: 'done', rootId: 1, outcome: 'completed' })
    expect(controller.status).toMatchObject({ state: 'scanning', queue: [], current: { rootId: 2 } })
    workers[0]?.emit({ type: 'done', rootId: 2, outcome: 'offline' })

    expect(controller.status).toEqual({ state: 'idle', queue: [], current: null })
    expect(workers).toHaveLength(1)
    expect(workers[0]?.sent).toEqual([
      { type: 'scan', rootId: 1 },
      { type: 'scan', rootId: 2 },
    ])
  })

  it('cancels queued scans locally and running scans in the worker', () => {
    const { controller, workers } = setup()
    controller.requestScan(1)
    controller.requestScan(2)

    controller.cancelScan(2)
    controller.cancelScan(1)

    expect(controller.status.queue).toEqual([])
    expect(workers[0]?.sent.at(-1)).toEqual({ type: 'cancel', rootId: 1 })
  })

  it('continues with a new worker after a crash', () => {
    const { controller, workers } = setup()
    controller.requestScan(1)
    controller.requestScan(2)

    workers[0]?.exit()

    expect(controller.status.current?.rootId).toBe(2)
    expect(workers[1]?.sent).toEqual([{ type: 'scan', rootId: 2 }])
  })

  it('stops dispatching after dispose', () => {
    const { controller, workers } = setup()
    controller.requestScan(1)
    controller.requestScan(2)

    controller.dispose()
    workers[0]?.exit()

    expect(workers[0]?.killed).toBe(true)
    expect(workers).toHaveLength(1)
  })

  it('deduplicates thumbnail requests and resolves them from worker events', async () => {
    const { controller, workers } = setup()

    const first = controller.requestThumbnail(7)
    const second = controller.requestThumbnail(7)
    workers[0]?.emit({ type: 'thumbnail', mediaId: 7, ok: true })

    expect(await Promise.all([first, second])).toEqual([true, true])
    expect(workers[0]?.sent).toEqual([{ type: 'thumbnail', mediaId: 7 }])
  })

  it('fails pending thumbnail requests when the worker exits', async () => {
    const { controller, workers } = setup()
    const pending = controller.requestThumbnail(7)

    workers[0]?.exit()

    expect(await pending).toBe(false)
  })
})
