import { mkdtemp, readdir, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { asc } from 'drizzle-orm'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { type Db, openDatabase } from '../main/db'
import { libraryRoots, media } from '../main/db/schema'
import { ThumbCache } from './thumb-cache'
import type { Thumbnails } from './thumbnail'
import { type ThumbnailDeps, ThumbnailService } from './thumbnails'
import { IndexWriter, type ThumbSource } from './writer'

const migrationsFolder = resolve(import.meta.dirname, '../../drizzle')
const THUMBNAILS: Thumbnails = {
  thumbhash: 'aGFzaA==',
  images: [
    { width: 400, data: Buffer.from('small') },
    { width: 800, data: Buffer.from('large') },
  ],
}

let dir: string
let db: Db
let rootId: number
let rendered: string[]
let deps: ThumbnailDeps

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), 'gg-thumbnails-'))
  db = openDatabase(':memory:', migrationsFolder)
  rootId = db.insert(libraryRoots).values({ path: dir, label: 'Test' }).returning().get().id
  rendered = []
  deps = {
    writer: new IndexWriter(db),
    cache: new ThumbCache(join(dir, 'cache'), 1024),
    render: async (_file, source: ThumbSource) => {
      rendered.push(source.relPath)
      return THUMBNAILS
    },
    isReachable: async () => true,
  }
})

afterEach(async () => {
  db.$client.close()
  await rm(dir, { recursive: true, force: true })
})

function insertMedia(relPath: string, takenAt: number) {
  return db
    .insert(media)
    .values({ rootId, relPath, fileName: relPath, kind: 'image', size: 1, mtime: 0, takenAt })
    .returning()
    .get().id
}

function thumbStates() {
  return db
    .select({ relPath: media.relPath, thumbStatus: media.thumbStatus, thumbhash: media.thumbhash })
    .from(media)
    .orderBy(asc(media.relPath))
    .all()
}

async function cachedFiles() {
  const entries = await readdir(deps.cache.dir, { recursive: true, withFileTypes: true }).catch(() => [])
  return entries.filter((entry) => entry.isFile()).length
}

describe('ThumbnailService', () => {
  it('renders pending media in the background, newest first', async () => {
    insertMedia('old.jpg', 1)
    insertMedia('new.jpg', 2)
    const service = new ThumbnailService(deps, 1)

    service.kick()
    await service.idle()

    expect(rendered).toEqual(['new.jpg', 'old.jpg'])
    expect(thumbStates()).toEqual([
      { relPath: 'new.jpg', thumbStatus: 'ready', thumbhash: THUMBNAILS.thumbhash },
      { relPath: 'old.jpg', thumbStatus: 'ready', thumbhash: THUMBNAILS.thumbhash },
    ])
    expect(await cachedFiles()).toBe(4)
  })

  it('runs the latest on-demand request first', async () => {
    const ids = ['a.jpg', 'b.jpg', 'c.jpg', 'd.jpg'].map((name, i) => insertMedia(name, i))
    const service = new ThumbnailService(deps, 1)
    service.kick()
    await deps.cache.ready

    const results = await Promise.all([service.request(ids[0] ?? 0), service.request(ids[1] ?? 0)])

    expect(results).toEqual([true, true])
    await service.idle()
    // d.jpg was already running when the requests arrived.
    expect(rendered).toEqual(['d.jpg', 'b.jpg', 'a.jpg', 'c.jpg'])
  })

  it('marks unrenderable media as failed, but only while the root is reachable', async () => {
    const broken = insertMedia('broken.jpg', 1)
    deps.render = async () => {
      throw new Error('corrupt')
    }
    const service = new ThumbnailService(deps, 1)

    expect(await service.request(broken)).toBe(false)
    expect(thumbStates()).toMatchObject([{ thumbStatus: 'error' }])

    const offline = insertMedia('offline.jpg', 2)
    deps.isReachable = async () => false
    expect(await service.request(offline)).toBe(false)
    expect(thumbStates()).toMatchObject([{ thumbStatus: 'error' }, { thumbStatus: 'pending' }])
    expect(db.select().from(libraryRoots).get()?.status).toBe('offline')
  })

  it('stores only requested thumbnails once the cache is full', async () => {
    // Exactly the size of one rendered pair: full, but not over the cap.
    deps.cache = new ThumbCache(join(dir, 'cache'), 10)
    const requested = insertMedia('requested.jpg', 1)
    insertMedia('background.jpg', 2)
    const service = new ThumbnailService(deps, 1)
    await deps.cache.ready
    expect(await service.request(requested)).toBe(true)

    service.kick()
    await service.idle()

    expect(thumbStates().map((row) => row.thumbStatus)).toEqual(['ready', 'ready'])
    expect(await cachedFiles()).toBe(2)
  })
})
