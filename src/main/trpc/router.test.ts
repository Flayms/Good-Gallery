import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { DEFAULT_SETTINGS } from '@shared/settings'
import { TRPCError } from '@trpc/server'
import { eq } from 'drizzle-orm'
import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest'
import { type Db, openDatabase } from '../db'
import { libraryRoots, media, mediaTags, settings as settingsTable, tags } from '../db/schema'
import { IndexerController } from '../indexer'
import { SettingsStore } from '../settings'
import { appRouter } from './router'
import { createCallerFactory } from './trpc'

const migrationsFolder = resolve(import.meta.dirname, '../../../drizzle')
const createCaller = createCallerFactory(appRouter)

let db: Db
let indexer: IndexerController
let caller: ReturnType<typeof createCaller>
let tempDir: string
let shownInFolder: string[]
let pickedFolder: string | undefined

beforeAll(async () => {
  tempDir = await mkdtemp(join(tmpdir(), 'gg-test-'))
})

afterAll(async () => {
  await rm(tempDir, { recursive: true, force: true })
})

beforeEach(() => {
  db = openDatabase(':memory:', migrationsFolder)
  indexer = new IndexerController(() => ({ send() {}, onEvent() {}, onExit() {}, kill() {} }))
  shownInFolder = []
  pickedFolder = undefined
  const desktop = {
    showItemInFolder: (path: string) => void shownInFolder.push(path),
    pickFolder: async () => pickedFolder,
  }
  caller = createCaller({ db, indexer, settings: new SettingsStore(db), desktop })
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
    expect(indexer.status.current?.rootId).toBe(root.id)
  })

  it('rejects duplicates case-insensitively', async () => {
    await caller.libraries.add({ path: tempDir })
    await expectTrpcError(caller.libraries.add({ path: tempDir.toUpperCase() }), 'CONFLICT')
  })

  it('rejects relative and missing paths', async () => {
    await expectTrpcError(caller.libraries.add({ path: 'photos' }), 'BAD_REQUEST')
    await expectTrpcError(caller.libraries.add({ path: join(tempDir, 'missing') }), 'BAD_REQUEST')
  })

  it('removes a root with its media', async () => {
    const root = await caller.libraries.add({ path: tempDir })
    insertMedia(root.id, 'a.jpg')

    await caller.libraries.remove({ id: root.id })

    expect(db.select().from(media).all()).toEqual([])
    await expectTrpcError(caller.libraries.remove({ id: root.id }), 'NOT_FOUND')
  })
})

describe('media.layout', () => {
  const layoutIds = async (input: Parameters<typeof caller.media.layout>[0] = {}) =>
    (await caller.media.layout(input)).ids

  it('orders by date taken, newest first, with the id as tie-breaker', async () => {
    const root = insertRoot()
    // Same date on two items exercises the id tie-breaker.
    const dates = [5, 3, 3, 9, 1]
    const ids = dates.map((takenAt, i) => insertMedia(root.id, `${i}.jpg`, { takenAt }).id)

    expect(await layoutIds()).toEqual([ids[3], ids[0], ids[2], ids[1], ids[4]])
    expect(await layoutIds({ dir: 'asc' })).toEqual([ids[4], ids[1], ids[2], ids[0], ids[3]])
  })

  it('falls back to mtime and filters by kind and root', async () => {
    const root = insertRoot()
    const other = insertRoot('D:Other')
    const video = insertMedia(root.id, 'b.mp4', { kind: 'video', mtime: 10 })
    insertMedia(root.id, 'a.jpg')
    insertMedia(other.id, 'c.mp4', { kind: 'video' })

    expect(await layoutIds({ rootId: root.id, kind: 'video' })).toEqual([video.id])
  })

  it.each([
    ['name', 'asc', ['a.jpg', 'B.jpg', 'c.jpg']],
    ['name', 'desc', ['c.jpg', 'B.jpg', 'a.jpg']],
    ['modified', 'asc', ['c.jpg', 'a.jpg', 'B.jpg']],
    ['size', 'desc', ['B.jpg', 'c.jpg', 'a.jpg']],
    ['path', 'asc', ['B.jpg', 'a.jpg', 'c.jpg']],
    ['taken', 'asc', ['a.jpg', 'c.jpg', 'B.jpg']],
  ] as const)('sorts by %s %s', async (sortBy, dir, expected) => {
    const root = insertRoot()
    insertMedia(root.id, 'a.jpg', { size: 1, mtime: 2, takenAt: 1, relPath: 'z/a.jpg' })
    insertMedia(root.id, 'B.jpg', { size: 3, mtime: 3, takenAt: 3, relPath: 'a/B.jpg' })
    insertMedia(root.id, 'c.jpg', { size: 2, mtime: 1, takenAt: 2, relPath: 'z/c.jpg' })

    const { ids } = await caller.media.layout({ sortBy, dir })
    const items = await caller.media.byIds({ ids })

    expect(ids.map((id) => items.find((item) => item.id === id)?.fileName)).toEqual(expected)
  })

  it('returns clamped aspects, a square when the size is unknown', async () => {
    const root = insertRoot()
    insertMedia(root.id, 'a.jpg', { width: 4000, height: 3000, mtime: 3 })
    insertMedia(root.id, 'b.jpg', { mtime: 2 })
    insertMedia(root.id, 'c.jpg', { width: 100, height: 10_000, mtime: 1 })

    expect((await caller.media.layout({})).aspects).toEqual([750, 1000, 3000])
  })

  it('filters by folder including subfolders', async () => {
    const root = insertRoot()
    const ids = ['a/1.jpg', 'a/b/2.jpg', 'ab/3.jpg', '4.jpg'].map(
      (relPath, i) => insertMedia(root.id, `${i}.jpg`, { relPath, mtime: -i }).id,
    )

    expect(await layoutIds({ rootId: root.id, folder: 'a' })).toEqual([ids[0], ids[1]])
    expect(await layoutIds({ rootId: root.id, folder: 'a/b/' })).toEqual([ids[1]])
    await expectTrpcError(caller.media.layout({ folder: 'a' }), 'BAD_REQUEST')
  })

  it('filters by date range', async () => {
    const root = insertRoot()
    const ids = [10, 20, 30].map((takenAt) => insertMedia(root.id, `${takenAt}.jpg`, { takenAt }).id)

    expect(await layoutIds({ from: 20, to: 30 })).toEqual([ids[1]])
  })
})

describe('media.byIds', () => {
  it('returns the tile data of the requested items only', async () => {
    const root = insertRoot()
    const a = insertMedia(root.id, 'a.jpg')
    insertMedia(root.id, 'b.jpg')

    const items = await caller.media.byIds({ ids: [a.id, 999] })

    expect(items.map((item) => item.fileName)).toEqual(['a.jpg'])
  })
})

describe('media.layout tags', () => {
  // Tree: places > france > paris; flat: cat, dog.
  function setup() {
    const root = insertRoot()
    const insertTag = (name: string, parentId: number | null = null) =>
      db.insert(tags).values({ name, nameNorm: name.toLowerCase(), parentId }).returning().get().id
    const places = insertTag('Places')
    const france = insertTag('France', places)
    const paris = insertTag('Paris', france)
    const cat = insertTag('Cat')
    const dog = insertTag('Dog')
    const file = (name: string, tagIds: number[], mtime: number) => {
      const id = insertMedia(root.id, name, { mtime }).id
      if (tagIds.length > 0)
        db.insert(mediaTags)
          .values(tagIds.map((tagId) => ({ mediaId: id, tagId })))
          .run()
      return id
    }
    return {
      parisCat: file('paris-cat.jpg', [paris, cat], 5),
      franceDog: file('france-dog.jpg', [france, dog], 4),
      cat: file('cat.jpg', [cat], 3),
      untagged: file('none.jpg', [], 2),
    }
  }

  const search = async (include: string[], exclude: string[] = []) =>
    (await caller.media.layout({ tags: include, excludeTags: exclude })).ids

  it('requires all included tags, matching descendants', async () => {
    const ids = setup()

    expect(await search(['places'])).toEqual([ids.parisCat, ids.franceDog])
    expect(await search([' FRANCE ', 'cat'])).toEqual([ids.parisCat])
    expect(await search(['paris', 'dog'])).toEqual([])
  })

  it('excludes tags with their descendants', async () => {
    const ids = setup()

    expect(await search([], ['france'])).toEqual([ids.cat, ids.untagged])
    expect(await search(['cat'], ['paris'])).toEqual([ids.cat])
  })

  it('matches nothing for unknown included tags and ignores unknown exclusions', async () => {
    const ids = setup()

    expect(await search(['cat', 'unknown'])).toEqual([])
    expect(await search(['cat'], ['unknown'])).toEqual([ids.parisCat, ids.cat])
  })
})

describe('media.byId', () => {
  it('returns a single item or NOT_FOUND', async () => {
    const root = insertRoot()
    const item = insertMedia(root.id, 'a.jpg', { width: 3, height: 2 })

    expect(await caller.media.byId({ id: item.id })).toMatchObject({ fileName: 'a.jpg', width: 3, height: 2 })
    await expectTrpcError(caller.media.byId({ id: item.id + 1 }), 'NOT_FOUND')
  })

  it('includes the file path, library and tags', async () => {
    const root = insertRoot()
    const item = insertMedia(root.id, 'b.jpg', { relPath: 'trip/b.jpg' })
    const tagIds = ['Zoo', 'Cat'].map(
      (name) => db.insert(tags).values({ name, nameNorm: name.toLowerCase() }).returning().get().id,
    )
    db.insert(mediaTags)
      .values(tagIds.map((tagId) => ({ mediaId: item.id, tagId })))
      .run()

    expect(await caller.media.byId({ id: item.id })).toMatchObject({
      path: join('C:\\Photos', 'trip', 'b.jpg'),
      rootLabel: 'Photos',
      tags: [
        { id: tagIds[1], name: 'Cat' },
        { id: tagIds[0], name: 'Zoo' },
      ],
    })
  })
})

describe('media.showInFolder', () => {
  it('reveals the file resolved from the index', async () => {
    const root = insertRoot()
    const item = insertMedia(root.id, 'b.jpg', { relPath: 'trip/b.jpg' })

    await caller.media.showInFolder({ id: item.id })

    expect(shownInFolder).toEqual([join('C:\\Photos', 'trip', 'b.jpg')])
    await expectTrpcError(caller.media.showInFolder({ id: item.id + 1 }), 'NOT_FOUND')
  })
})

describe('libraries.pickFolder', () => {
  it('returns the chosen folder or null when cancelled', async () => {
    expect(await caller.libraries.pickFolder()).toBeNull()
    pickedFolder = 'D:\\Pictures'
    expect(await caller.libraries.pickFolder()).toBe('D:\\Pictures')
  })
})

describe('settings', () => {
  it('starts with defaults and persists updates', async () => {
    expect(await caller.settings.get()).toEqual(DEFAULT_SETTINGS)

    const updated = await caller.settings.update({ ioConcurrency: 8 })

    expect(updated).toEqual({ ...DEFAULT_SETTINGS, ioConcurrency: 8 })
    expect(new SettingsStore(db).get()).toEqual(updated)
  })

  it('rejects out-of-range values', async () => {
    await expectTrpcError(caller.settings.update({ ioConcurrency: 0 }), 'BAD_REQUEST')
    await expectTrpcError(caller.settings.update({ thumbCacheGiB: -1 }), 'BAD_REQUEST')
  })

  it('falls back to defaults for stored values that no longer validate', () => {
    db.insert(settingsTable).values({ key: 'ioConcurrency', value: 'many' }).run()

    expect(new SettingsStore(db).get()).toEqual(DEFAULT_SETTINGS)
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

  it('counts descendants and follows deletions', async () => {
    const root = insertRoot()
    const places = db.insert(tags).values({ name: 'Places', nameNorm: 'places' }).returning().get()
    const paris = db.insert(tags).values({ name: 'Paris', nameNorm: 'paris', parentId: places.id }).returning().get()
    const files = [1, 2].map((i) => insertMedia(root.id, `${i}.jpg`))
    db.insert(mediaTags)
      .values(files.map((file) => ({ mediaId: file.id, tagId: paris.id })))
      .run()

    expect(await caller.tags.autocomplete({ prefix: 'pl' })).toEqual([{ id: places.id, name: 'Places', count: 2 }])

    db.delete(media)
      .where(eq(media.id, files[0]?.id ?? 0))
      .run()
    expect(await caller.tags.autocomplete({})).toEqual([
      { id: paris.id, name: 'Paris', count: 1 },
      { id: places.id, name: 'Places', count: 1 },
    ])

    db.delete(media).run()
    expect(await caller.tags.autocomplete({})).toEqual([])
  })
})

describe('libraries.folders', () => {
  it('lists folders with direct media counts', async () => {
    const root = insertRoot()
    for (const relPath of ['top.jpg', 'a/1.jpg', 'a/2.jpg', 'a/b/3.jpg']) insertMedia(root.id, relPath, { relPath })

    expect(await caller.libraries.folders({ id: root.id })).toEqual([
      { path: 'a', count: 2 },
      { path: 'a/b', count: 1 },
    ])
  })
})
