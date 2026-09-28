import { openDatabase } from '../main/db'
import { isReachableDirectory } from '../main/reachability'
import { ExifToolMetadataSource } from './metadata'
import { type IndexerEvent, indexerRequest } from './protocol'
import { type ScanDeps, scanRoot } from './scan'
import { IndexWriter } from './writer'

// Entry point of the indexer utilityProcess. Main serializes scans; this process just runs what it is told.

const port = process.parentPort
let config: { deps: ScanDeps; concurrency: number } | undefined
const scans = new Map<number, AbortController>()

function send(event: IndexerEvent): void {
  port.postMessage(event)
}

async function scan(rootId: number): Promise<void> {
  if (scans.has(rootId)) return
  const controller = new AbortController()
  scans.set(rootId, controller)
  try {
    if (!config) throw new Error('Indexer received a scan request before init')
    const outcome = await scanRoot(config.deps, rootId, {
      concurrency: config.concurrency,
      signal: controller.signal,
      onProgress: (progress) => send({ type: 'progress', ...progress }),
    })
    send({ type: 'done', rootId, outcome })
  } catch (error) {
    console.error(`Indexer: scan of root ${rootId} failed:`, error)
    send({ type: 'done', rootId, outcome: 'failed', error: error instanceof Error ? error.message : String(error) })
  } finally {
    scans.delete(rootId)
  }
}

port.on('message', ({ data }) => {
  const request = indexerRequest.parse(data)
  switch (request.type) {
    case 'init': {
      const metadata = new ExifToolMetadataSource(request.concurrency)
      const deps = {
        writer: new IndexWriter(openDatabase(request.dbPath)),
        metadata,
        isReachable: isReachableDirectory,
      }
      config = { deps, concurrency: request.concurrency }
      break
    }
    case 'scan':
      void scan(request.rootId)
      break
    case 'cancel':
      scans.get(request.rootId)?.abort()
      break
  }
})
