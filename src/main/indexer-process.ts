import { utilityProcess } from 'electron'
import indexerPath from '../indexer/index?modulePath'
import { type IndexerRequest, indexerEvent } from '../indexer/protocol'
import type { IndexerWorker } from './indexer'

export type IndexerInit = Omit<Extract<IndexerRequest, { type: 'init' }>, 'type'>

export function forkIndexer(config: IndexerInit): IndexerWorker {
  const child = utilityProcess.fork(indexerPath, [], { serviceName: 'Good Gallery Indexer' })
  child.postMessage({ type: 'init', ...config })
  return {
    send: (request) => child.postMessage(request),
    onEvent: (listener) => child.on('message', (message) => listener(indexerEvent.parse(message))),
    onExit: (listener) => child.on('exit', listener),
    kill: () => child.kill(),
  }
}
