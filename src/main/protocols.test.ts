import { mediaUrl, thumbUrl } from '@shared/media-urls'
import { eq } from 'drizzle-orm'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { thumbPath } from '../indexer/thumb-cache'
import { type Db, openDatabase } from './db'
import { libraryRoots, media } from './db/schema'
import { createMediaHandler, createThumbHandler, parseRange } from './protocols'

const migrationsFolder = resolve(import.meta.dirname, '../../drizzle')

let dir: string
let db: Db
let mediaId: number
let renders: number[]

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), 'gg-protocols-'))
  db = openDatabase(':memory:', migrationsFolder)
  const rootId = db.insert(libraryRoots).values({ path: dir, label: 'Test' }).returning().get().id
  const relPath = 'trip/clip.mp4'
  await mkdir(join(dir, 'trip'))
  await writeFile(join(dir, relPath), '0123456789')
  mediaId = db
    .insert(media)
    .values({ rootId, relPath, fileName: 'clip.mp4', kind: 'video', size: 10, mtime: 5 })
    .returning()
    .get().id
  renders = []
})

afterEach(async () => {
  db.$client.close()
  await rm(dir, { recursive: true, force: true })
})

function thumbHandler(renderWrites = true) {
  const cacheDir = join(dir, 'cache')
  return createThumbHandler({
    db,
    cacheDir,
    render: async (id) => {
      renders.push(id)
      if (!renderWrites) return false
      const row = db.select().from(media).where(eq(media.id, id)).get()
      if (!row) return false
      const file = thumbPath(cacheDir, row, 400)
      await mkdir(dirname(file), { recursive: true })
      await writeFile(file, 'webp')
      return true
    },
  })
}

describe('gg-thumb handler', () => {
  it('renders missing thumbnails on demand, then serves them from the cache', async () => {
    const handle = thumbHandler()

    const first = await handle(new Request(thumbUrl(mediaId, 400)))
    const second = await handle(new Request(thumbUrl(mediaId, 400)))

    expect(first.status).toBe(200)
    expect(first.headers.get('content-type')).toBe('image/webp')
    expect(await second.text()).toBe('webp')
    expect(renders).toEqual([mediaId])
  })

  it('rejects unknown ids and malformed URLs without rendering', async () => {
    const handle = thumbHandler()

    expect((await handle(new Request(thumbUrl(mediaId + 1, 400)))).status).toBe(404)
    expect((await handle(new Request(`gg-thumb://media/${mediaId}/401`))).status).toBe(404)
    expect(renders).toEqual([])
  })

  it('does not retry thumbnails that failed before', async () => {
    db.update(media).set({ thumbStatus: 'error' }).run()
    const handle = thumbHandler()

    expect((await handle(new Request(thumbUrl(mediaId, 400)))).status).toBe(404)
    expect(renders).toEqual([])
  })

  it('answers 404 when rendering fails', async () => {
    expect((await thumbHandler(false)(new Request(thumbUrl(mediaId, 800)))).status).toBe(404)
  })
})

describe('gg-media handler', () => {
  it('streams the whole file', async () => {
    const response = await createMediaHandler({ db })(new Request(mediaUrl(mediaId)))

    expect(response.status).toBe(200)
    expect(Object.fromEntries(response.headers)).toMatchObject({
      'content-type': 'video/mp4',
      'content-length': '10',
      'accept-ranges': 'bytes',
    })
    expect(await response.text()).toBe('0123456789')
  })

  it('serves byte ranges', async () => {
    const handle = createMediaHandler({ db })

    const partial = await handle(new Request(mediaUrl(mediaId), { headers: { Range: 'bytes=2-4' } }))
    expect(partial.status).toBe(206)
    expect(partial.headers.get('content-range')).toBe('bytes 2-4/10')
    expect(await partial.text()).toBe('234')

    const beyond = await handle(new Request(mediaUrl(mediaId), { headers: { Range: 'bytes=10-' } }))
    expect(beyond.status).toBe(416)
    expect(beyond.headers.get('content-range')).toBe('bytes */10')
  })

  it('answers 404 for unknown ids and vanished files', async () => {
    const handle = createMediaHandler({ db })
    expect((await handle(new Request(mediaUrl(mediaId + 1)))).status).toBe(404)

    await rm(join(dir, 'trip'), { recursive: true })
    expect((await handle(new Request(mediaUrl(mediaId)))).status).toBe(404)
  })
})

describe('parseRange', () => {
  it.each([
    ['bytes=0-', { start: 0, end: 99 }],
    ['bytes=10-19', { start: 10, end: 19 }],
    ['bytes=90-200', { start: 90, end: 99 }],
    ['bytes=-10', { start: 90, end: 99 }],
    ['bytes=-500', { start: 0, end: 99 }],
    ['bytes=100-', 'unsatisfiable'],
    ['bytes=-0', 'unsatisfiable'],
    ['bytes=5-2', undefined],
    ['bytes=0-1,5-6', undefined],
    ['items=0-1', undefined],
    [null, undefined],
  ] as const)('%s', (header, expected) => {
    expect(parseRange(header, 100)).toEqual(expected)
  })
})
