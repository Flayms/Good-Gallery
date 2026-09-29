import { openDatabase } from '../main/db'
import { isReachableDirectory } from '../main/reachability'
import { ExifToolMetadataSource } from './metadata'
import { type IndexerEvent, indexerRequest, MAX_CHANGED_PATHS, type ScanScope } from './protocol'
import { type ScanDeps, scanRoot } from './scan'
import { ThumbCache } from './thumb-cache'
import { renderMedia } from './thumbnail'
import { ThumbnailService } from './thumbnails'
import { RootWatcher } from './watcher'
import { IndexWriter } from './writer'

// Entry point of the indexer utilityProcess. Main serializes scans; this process just runs what it is told.

const port = process.parentPort
let config:
  | {
      deps: ScanDeps
      metadata: ExifToolMetadataSource
      cache: ThumbCache
      thumbnails: ThumbnailService
      concurrency: number
    }
  | undefined
const scans = new Map<number, AbortController>()
const watchers = new Map<number, RootWatcher>()

function send(event: IndexerEvent): void {
  port.postMessage(event)
}

function initialized(): NonNullable<typeof config> {
  if (!config) throw new Error('Indexer received a request before init')
  return config
}

/** Reports changes of a root to main, which queues a scan of the changed folders. */
function startWatching(rootId: number, rootPath: string): void {
  if (watchers.has(rootId)) return
  try {
    const watcher = new RootWatcher(rootPath, {
      onChange: (paths) => {
        send({ type: 'changed', rootId, paths: paths && paths.length <= MAX_CHANGED_PATHS ? paths : undefined })
      },
      onError: (error) => {
        console.warn(`Indexer: watching ${rootPath} failed:`, error)
        watchers.delete(rootId)
        // Changes may have been missed; the scan also notices if the root went offline.
        send({ type: 'changed', rootId })
      },
    })
    watchers.set(rootId, watcher)
  } catch (error) {
    // Only the scheduled quick scans will notice changes then.
    console.warn(`Indexer: cannot watch ${rootPath}:`, error)
  }
}

function stopWatching(rootId: number): void {
  watchers.get(rootId)?.close()
  watchers.delete(rootId)
}

async function scan(rootId: number, scope: ScanScope): Promise<void> {
  if (scans.has(rootId)) return
  const controller = new AbortController()
  scans.set(rootId, controller)
  try {
    const { deps, thumbnails, concurrency } = initialized()
    const outcome = await scanRoot(deps, rootId, scope, {
      concurrency,
      signal: controller.signal,
      onProgress: (progress) => {
        send({ type: 'progress', ...progress })
        // Written batches may hold new media, so rendering starts before the scan finishes.
        thumbnails.kick()
      },
    })
    thumbnails.kick()
    const root = deps.writer.root(rootId)
    if (outcome === 'completed' && root) startWatching(rootId, root.path)
    else if (outcome === 'offline') stopWatching(rootId)
    send({ type: 'done', rootId, outcome })
  } catch (error) {
    console.error(`Indexer: scan of root ${rootId} failed:`, error)
    send({ type: 'done', rootId, outcome: 'failed', error: error instanceof Error ? error.message : String(error) })
  } finally {
    scans.delete(rootId)
  }
}

async function thumbnail(mediaId: number): Promise<void> {
  const ok = await initialized().thumbnails.request(mediaId)
  send({ type: 'thumbnail', mediaId, ok })
}

async function cacheRequest(requestId: number, action: 'usage' | 'clear'): Promise<void> {
  try {
    const { cache } = initialized()
    if (action === 'clear') await cache.clear()
    send({ type: 'cache', requestId, bytes: await cache.usage() })
  } catch (error) {
    console.error(`Indexer: thumbnail cache ${action} failed:`, error)
    send({ type: 'cache', requestId })
  }
}

port.on('message', ({ data }) => {
  const request = indexerRequest.parse(data)
  switch (request.type) {
    case 'init': {
      const metadata = new ExifToolMetadataSource(request.concurrency)
      const writer = new IndexWriter(openDatabase(request.dbPath))
      const cache = new ThumbCache(request.thumbDir, request.thumbCacheBytes)
      const thumbnails = new ThumbnailService(
        {
          writer,
          cache,
          render: (file, source) => renderMedia(file, source, metadata),
          isReachable: isReachableDirectory,
        },
        request.concurrency,
      )
      const deps = { writer, metadata, isReachable: isReachableDirectory }
      config = { deps, metadata, cache, thumbnails, concurrency: request.concurrency }
      thumbnails.kick()
      break
    }
    case 'configure': {
      const current = initialized()
      // Running scans keep their concurrency; the next one picks up the new value.
      current.concurrency = request.concurrency
      current.metadata.setMaxProcs(request.concurrency)
      current.thumbnails.setConcurrency(request.concurrency)
      current.cache.setMaxBytes(request.thumbCacheBytes)
      break
    }
    case 'scan':
      void scan(request.rootId, request.scope)
      break
    case 'remove':
      scans.get(request.rootId)?.abort()
      stopWatching(request.rootId)
      break
    case 'thumbnail':
      thumbnail(request.mediaId).catch((error: unknown) => {
        console.error(`Indexer: thumbnail request for media ${request.mediaId} failed:`, error)
        send({ type: 'thumbnail', mediaId: request.mediaId, ok: false })
      })
      break
    case 'cache':
      void cacheRequest(request.requestId, request.action)
      break
  }
})
