import { describe, expect, it } from 'vitest'
import type { IndexerEvent, IndexerRequest, ScanScope } from '../indexer/protocol'
import { IndexerController, type IndexerWorker, mergeScopes } from './indexer'

const FULL: ScanScope = { mode: 'full' }
const QUICK: ScanScope = { mode: 'quick' }

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

    controller.requestScan(1, QUICK)
    controller.requestScan(2, QUICK)
    controller.requestScan(1, QUICK)

    expect(controller.status).toEqual({
      state: 'scanning',
      queue: [2],
      current: { rootId: 1, mode: 'quick', scanned: 0, indexed: 0 },
    })
    workers[0]?.emit({ type: 'progress', rootId: 1, scanned: 10, indexed: 3 })
    expect(controller.status.current).toEqual({ rootId: 1, mode: 'quick', scanned: 10, indexed: 3 })

    workers[0]?.emit({ type: 'done', rootId: 1, outcome: 'completed' })
    expect(controller.status).toMatchObject({ state: 'scanning', queue: [], current: { rootId: 2 } })
    workers[0]?.emit({ type: 'done', rootId: 2, outcome: 'offline' })

    expect(controller.status).toEqual({ state: 'idle', queue: [], current: null })
    expect(workers).toHaveLength(1)
    expect(workers[0]?.sent).toEqual([
      { type: 'scan', rootId: 1, scope: QUICK },
      { type: 'scan', rootId: 2, scope: QUICK },
    ])
  })

  it('merges scans of a queued root into the one covering most', () => {
    const { controller, workers } = setup()
    controller.requestScan(1, QUICK)
    controller.requestScan(2, { mode: 'changes', paths: ['a'] })
    controller.requestScan(2, { mode: 'changes', paths: ['b', 'a'] })
    controller.requestScan(3, QUICK)
    controller.requestScan(3, FULL)

    expect(controller.status.queue).toEqual([2, 3])
    workers[0]?.emit({ type: 'done', rootId: 1, outcome: 'completed' })
    workers[0]?.emit({ type: 'done', rootId: 2, outcome: 'completed' })

    expect(workers[0]?.sent.slice(1)).toEqual([
      { type: 'scan', rootId: 2, scope: { mode: 'changes', paths: ['a', 'b'] } },
      { type: 'scan', rootId: 3, scope: FULL },
    ])
  })

  it('queues reported changes even while their root is scanned', () => {
    const { controller, workers } = setup()
    controller.requestScan(1, QUICK)

    controller.requestScan(1, QUICK)
    workers[0]?.emit({ type: 'changed', rootId: 1, paths: ['a/1.jpg'] })
    workers[0]?.emit({ type: 'changed', rootId: 2 })

    expect(controller.status.queue).toEqual([1, 2])
    workers[0]?.emit({ type: 'done', rootId: 1, outcome: 'completed' })
    expect(workers[0]?.sent.at(-1)).toEqual({
      type: 'scan',
      rootId: 1,
      scope: { mode: 'changes', paths: ['a/1.jpg'] },
    })
  })

  it('removes roots from the queue and the worker', () => {
    const { controller, workers } = setup()
    controller.requestScan(1, QUICK)
    controller.requestScan(2, QUICK)

    controller.removeRoot(2)
    controller.removeRoot(1)

    expect(controller.status.queue).toEqual([])
    expect(workers[0]?.sent.slice(1)).toEqual([
      { type: 'remove', rootId: 2 },
      { type: 'remove', rootId: 1 },
    ])
  })

  it('continues with a new worker after a crash', () => {
    const { controller, workers } = setup()
    controller.requestScan(1, QUICK)
    controller.requestScan(2, QUICK)

    workers[0]?.exit()

    expect(controller.status.current?.rootId).toBe(2)
    expect(workers[1]?.sent).toEqual([{ type: 'scan', rootId: 2, scope: QUICK }])
  })

  it('stops dispatching after dispose', () => {
    const { controller, workers } = setup()
    controller.requestScan(1, QUICK)
    controller.requestScan(2, QUICK)

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

  it('forwards settings only to a running worker', () => {
    const { controller, workers } = setup()
    controller.configure({ thumbCacheBytes: 100, concurrency: 2 })
    expect(workers).toHaveLength(0)

    controller.requestScan(1, QUICK)
    controller.configure({ thumbCacheBytes: 100, concurrency: 2 })

    expect(workers[0]?.sent.at(-1)).toEqual({ type: 'configure', thumbCacheBytes: 100, concurrency: 2 })
  })

  it('answers cache requests by id and fails them when the worker exits', async () => {
    const { controller, workers } = setup()
    const usage = controller.thumbnailCache('usage')
    const clear = controller.thumbnailCache('clear')
    const [usageRequest, clearRequest] = workers[0]?.sent ?? []
    if (usageRequest?.type !== 'cache' || clearRequest?.type !== 'cache') throw new Error('Expected cache requests')

    workers[0]?.emit({ type: 'cache', requestId: clearRequest.requestId, bytes: 0 })
    workers[0]?.emit({ type: 'cache', requestId: usageRequest.requestId, bytes: 42 })
    expect(await Promise.all([usage, clear])).toEqual([42, 0])

    const failed = controller.thumbnailCache('usage')
    workers[0]?.exit()
    await expect(failed).rejects.toThrow('exited')
  })
})

describe('mergeScopes', () => {
  it('turns too many changed paths into a quick scan', () => {
    const paths = Array.from({ length: 3000 }, (_, i) => `${i}.jpg`)
    const more = Array.from({ length: 3000 }, (_, i) => `more/${i}.jpg`)

    expect(mergeScopes({ mode: 'changes', paths }, { mode: 'changes', paths: more })).toEqual(QUICK)
    expect(mergeScopes({ mode: 'changes', paths }, FULL)).toEqual(FULL)
  })
})
