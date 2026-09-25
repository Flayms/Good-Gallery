import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { TRPCError } from '@trpc/server'
import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest'
import { type Db, openDatabase } from '../db'
import { libraryRoots, media, mediaTags, tags } from '../db/schema'
import { IndexerController } from '../indexer'
import { appRouter } from './router'
import { createCallerFactory } from './trpc'

const migrationsFolder = resolve(import.meta.dirname, '../../../drizzle')
const createCaller = createCallerFactory(appRouter)

let db: Db
let indexer: IndexerController
let caller: ReturnType<typeof createCaller>
let tempDir: string

beforeAll(async () => {
  tempDir = await mkdtemp(join(tmpdir(), 'gg-test-'))
})

afterAll(async () => {
  await rm(tempDir, { recursive: true, force: true })
})

beforeEach(() => {
  db = openDatabase(':memory:', migrationsFolder)
  indexer = new IndexerController()
  caller = createCaller({ db, indexer })
})

function insertRoot(path = 'C:\\Photos') {
  return db.insert(libraryRoots).values({ path, label: 'Photos' }).returning().get()
}

function insertMedia(rootId: number, fileName: string, extra: Partial<typeof media.$inferInsert> = {}) {
  return db
    .insert(media)
    .values({ rootId, relPath: fileName, fileName, kind: 'image', size: 1, mtime: 0, ...extra })
    .returning()
    .get()
}

async function expectTrpcError(promise: Promise<unknown>, code: TRPCError['code']) {
  await expect(promise).rejects.toSatisfy((error) => error instanceof TRPCError && error.code === code)
}

describe('libraries', () => {
  it('adds a reachable folder and queues a scan', async () => {
    const root = await caller.libraries.add({ path: tempDir })

    expect(root).toMatchObject({ path: resolve(tempDir), status: 'online' })
    expect(await caller.libraries.list()).toEqual([root])
    expect(indexer.status.queue).toEqual([root.id])
  })

  it('rejects duplicates case-insensitively', async () => {
    await caller.libraries.add({ path: tempDir })
    await expectTrpcError(caller.libraries.add({ path: tempDir.toUpperCase() }), 'CONFLICT')
  })

  it('rejects relative and missing paths', async () => {
    await expectTrpcError(caller.libraries.add({ path: 'photos' }), 'BAD_REQUEST')
    await expectTrpcError(caller.libraries.add({ path: join(tempDir, 'missing') }), 'BAD_REQUEST')
  })

  it('removes a root with its media and cancels its scan', async () => {
    const root = await caller.libraries.add({ path: tempDir })
    insertMedia(root.id, 'a.jpg')

    await caller.libraries.remove({ id: root.id })

    expect(db.select().from(media).all()).toEqual([])
    expect(indexer.status.queue).toEqual([])
    await expectTrpcError(caller.libraries.remove({ id: root.id }), 'NOT_FOUND')
  })
})

describe('media.search', () => {
  it('pages through all items by date without gaps or duplicates', async () => {
    const root = insertRoot()
    // Same date on two items exercises the id tie-breaker.
    const dates = [5, 3, 3, 9, 1]
    const ids = dates.map((takenAt, i) => insertMedia(root.id, `${i}.jpg`, { takenAt }).id)

    const seen: number[] = []
    let cursor: Awaited<ReturnType<typeof caller.media.search>>['nextCursor'] = null
    do {
      const page = await caller.media.search({ limit: 2, cursor })
      seen.push(...page.items.map((item) => item.id))
      cursor = page.nextCursor
    } while (cursor)

    expect(seen).toEqual([ids[3], ids[0], ids[2], ids[1], ids[4]])
  })

  it('falls back to mtime and filters by kind and root', async () => {
    const root = insertRoot()
    const other = insertRoot('D:\\Other')
    const video = insertMedia(root.id, 'b.mp4', { kind: 'video', mtime: 10 })
    insertMedia(root.id, 'a.jpg')
    insertMedia(other.id, 'c.mp4', { kind: 'video' })

    const page = await caller.media.search({ rootId: root.id, kind: 'video' })

    expect(page.items.map((item) => item.id)).toEqual([video.id])
    expect(page.items[0]?.sortDate).toBe(10)
    expect(page.nextCursor).toBeNull()
  })

  it('sorts by name', async () => {
    const root = insertRoot()
    for (const name of ['c.jpg', 'a.jpg', 'b.jpg']) insertMedia(root.id, name)

    const first = await caller.media.search({ sort: 'name-asc', limit: 2 })
    const second = await caller.media.search({ sort: 'name-asc', limit: 2, cursor: first.nextCursor })

    expect([...first.items, ...second.items].map((item) => item.fileName)).toEqual(['a.jpg', 'b.jpg', 'c.jpg'])
  })
})

describe('tags.autocomplete', () => {
  it('matches normalized prefixes ordered by usage', async () => {
    const root = insertRoot()
    const insertTag = (name: string) => db.insert(tags).values({ name, nameNorm: name.toLowerCase() }).returning().get()
    const berlin = insertTag('Berlin')
    const bern = insertTag('Bern')
    const paris = insertTag('Paris')
    const files = [1, 2, 3].map((i) => insertMedia(root.id, `${i}.jpg`))
    db.insert(mediaTags)
      .values([
        ...files.map((file) => ({ mediaId: file.id, tagId: bern.id })),
        { mediaId: files[0]?.id ?? 0, tagId: berlin.id },
        { mediaId: files[0]?.id ?? 0, tagId: paris.id },
      ])
      .run()

    expect(await caller.tags.autocomplete({ prefix: ' BER' })).toEqual([
      { id: bern.id, name: 'Bern', count: 3 },
      { id: berlin.id, name: 'Berlin', count: 1 },
    ])
    expect(await caller.tags.autocomplete({ limit: 1 })).toEqual([{ id: bern.id, name: 'Bern', count: 3 }])
  })
})
