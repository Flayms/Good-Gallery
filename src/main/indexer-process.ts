import { utilityProcess } from 'electron'
import indexerPath from '../indexer/index?modulePath'
import { indexerEvent } from '../indexer/protocol'
import type { IndexerWorker } from './indexer'

export function forkIndexer(options: { dbPath: string; concurrency: number }): IndexerWorker {
  const child = utilityProcess.fork(indexerPath, [], { serviceName: 'Good Gallery Indexer' })
  child.postMessage({ type: 'init', ...options })
  return {
    send: (request) => child.postMessage(request),
    onEvent: (listener) => child.on('message', (message) => listener(indexerEvent.parse(message))),
    onExit: (listener) => child.on('exit', listener),
    kill: () => child.kill(),
  }
}
