import { parseMediaUrl, parseThumbUrl } from '@shared/media-urls'
import { eq } from 'drizzle-orm'
import { createReadStream } from 'node:fs'
import { readFile, stat, utimes } from 'node:fs/promises'
import { join } from 'node:path'
import { Readable } from 'node:stream'
import { mimeType } from '../indexer/media-types'
import { errorCode } from '../indexer/retry'
import { thumbPath } from '../indexer/thumb-cache'
import type { Db } from './db'
import { libraryRoots, media } from './db/schema'

// Handlers for the custom protocols; transport-agnostic (fetch `Request`/`Response`) so they run in tests.

/** Serving a thumbnail refreshes its LRU timestamp at most this often. */
const TOUCH_INTERVAL_MS = 60 * 60_000

const notFound = () => new Response(null, { status: 404 })

async function readThumb(file: string): Promise<Buffer | undefined> {
  try {
    const { mtimeMs } = await stat(file)
    const data = await readFile(file)
    const now = new Date()
    if (now.getTime() - mtimeMs > TOUCH_INTERVAL_MS) {
      utimes(file, now, now).catch((error: unknown) => console.warn(`Cannot touch thumbnail ${file}:`, error))
    }
    return data
  } catch (error) {
    // Not rendered yet, or evicted between stat and read.
    if (errorCode(error) === 'ENOENT') return undefined
    throw error
  }
}

export interface ThumbHandlerDeps {
  db: Db
  cacheDir: string
  /** Renders a missing thumbnail; resolves true once it's in the cache. */
  render: (mediaId: number) => Promise<boolean>
}

/** `gg-thumb://media/<id>/<width>`: serves cached thumbnails and renders missing ones on demand. */
export function createThumbHandler({ db, cacheDir, render }: ThumbHandlerDeps) {
  return async (request: Request): Promise<Response> => {
    const target = parseThumbUrl(request.url)
    if (!target) return notFound()
    const row = db
      .select({
        rootId: media.rootId,
        relPath: media.relPath,
        size: media.size,
        mtime: media.mtime,
        thumbStatus: media.thumbStatus,
      })
      .from(media)
      .where(eq(media.id, target.id))
      .get()
    if (!row) return notFound()

    const file = thumbPath(cacheDir, row, target.width)
    let data = await readThumb(file)
    // Known failures aren't retried on every scroll; a changed file resets the status.
    if (!data && row.thumbStatus !== 'error' && (await render(target.id))) data = await readThumb(file)
    return data ? new Response(data, { headers: { 'Content-Type': 'image/webp' } }) : notFound()
  }
}

/** `gg-media://media/<id>`: streams originals with Range support, which `<video>` seeking relies on. */
export function createMediaHandler({ db }: { db: Db }) {
  return async (request: Request): Promise<Response> => {
    const target = parseMediaUrl(request.url)
    if (!target) return notFound()
    const row = db
      .select({ rootPath: libraryRoots.path, relPath: media.relPath, fileName: media.fileName })
      .from(media)
      .innerJoin(libraryRoots, eq(libraryRoots.id, media.rootId))
      .where(eq(media.id, target.id))
      .get()
    if (!row) return notFound()
    const path = join(row.rootPath, ...row.relPath.split('/'))
    return fileResponse(path, mimeType(row.fileName), request.headers.get('range'))
  }
}

export type ByteRange = { start: number; end: number }

/** Parses a single `bytes=` range. Malformed and multi-range headers yield `undefined` (serve everything). */
export function parseRange(header: string | null, size: number): ByteRange | 'unsatisfiable' | undefined {
  const match = header?.trim().match(/^bytes=(\d*)-(\d*)$/)
  if (!match) return undefined
  const [, first = '', last = ''] = match
  if (!first) {
    if (!last) return undefined
    const suffix = Number(last)
    if (suffix === 0 || size === 0) return 'unsatisfiable'
    return { start: Math.max(0, size - suffix), end: size - 1 }
  }
  const start = Number(first)
  if (last && Number(last) < start) return undefined
  if (start >= size) return 'unsatisfiable'
  return { start, end: last ? Math.min(Number(last), size - 1) : size - 1 }
}

export async function fileResponse(path: string, contentType: string, rangeHeader: string | null): Promise<Response> {
  let size: number
  try {
    size = (await stat(path)).size
  } catch {
    // Missing file or unreachable share.
    return notFound()
  }

  const range = parseRange(rangeHeader, size)
  if (range === 'unsatisfiable')
    return new Response(null, { status: 416, headers: { 'Content-Range': `bytes */${size}` } })
  const headers: Record<string, string> = { 'Content-Type': contentType, 'Accept-Ranges': 'bytes' }
  if (size === 0) return new Response(null, { headers: { ...headers, 'Content-Length': '0' } })

  const { start, end } = range ?? { start: 0, end: size - 1 }
  headers['Content-Length'] = String(end - start + 1)
  if (range) headers['Content-Range'] = `bytes ${start}-${end}/${size}`
  const body = Readable.toWeb(createReadStream(path, { start, end }))
  return new Response(body, { status: range ? 206 : 200, headers })
}
