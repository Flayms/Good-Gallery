import { mkdir, mkdtemp, rm, unlink, utimes, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { basename, dirname, join, resolve } from 'node:path'
import { asc, eq } from 'drizzle-orm'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { type Db, openDatabase } from '../main/db'
import { libraryRoots, media, mediaTags, tags } from '../main/db/schema'
import { EMPTY_METADATA, type MediaMetadata, type MetadataSource } from './metadata'
import { type ScanDeps, type ScanOptions, scanRoot } from './scan'
import { IndexWriter } from './writer'

const migrationsFolder = resolve(import.meta.dirname, '../../drizzle')

/** Returns metadata keyed by file name and records which files were read. */
class FakeMetadata implements MetadataSource {
  readonly byName = new Map<string, Partial<MediaMetadata>>()
  reads: string[] = []

  async read(file: string, sidecar?: string): Promise<MediaMetadata> {
    const name = basename(file)
    this.reads.push(sidecar ? `${name}+xmp` : name)
    return { ...EMPTY_METADATA, ...this.byName.get(name) }
  }
}

let dir: string
let db: Db
let rootId: number
let metadata: FakeMetadata
let deps: ScanDeps

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), 'gg-scan-'))
  db = openDatabase(':memory:', migrationsFolder)
  rootId = db.insert(libraryRoots).values({ path: dir, label: 'Test' }).returning().get().id
  metadata = new FakeMetadata()
  deps = { writer: new IndexWriter(db), metadata, isReachable: async () => true }
})

afterEach(async () => {
  db.$client.close()
  await rm(dir, { recursive: true, force: true })
})

async function touch(relPath: string, content = 'x', mtime?: Date) {
  const path = join(dir, relPath)
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, content)
  if (mtime) await utimes(path, mtime, mtime)
}

function scan(options: Partial<ScanOptions> = {}) {
  return scanRoot(deps, rootId, { concurrency: 2, signal: new AbortController().signal, onProgress() {}, ...options })
}

function indexed() {
  return db.select().from(media).orderBy(asc(media.relPath)).all()
}

function tagsOf(mediaId: number) {
  return db
    .select({ name: tags.name })
    .from(mediaTags)
    .innerJoin(tags, eq(tags.id, mediaTags.tagId))
    .where(eq(mediaTags.mediaId, mediaId))
    .orderBy(asc(tags.name))
    .all()
    .map((tag) => tag.name)
}

describe('scanRoot', () => {
  it('indexes media recursively and skips housekeeping files', async () => {
    await touch('a.jpg')
    await touch('trip/b.MP4')
    await touch('trip/notes.txt')
    await touch('trip/._b.MP4')
    await touch('@eaDir/a.jpg/SYNOFILE_THUMB_M.jpg')
    await touch('.hidden/c.jpg')
    metadata.byName.set('a.jpg', { width: 30, height: 20, takenAt: 1000 })
    const progress: number[] = []

    const outcome = await scan({ onProgress: (p) => progress.push(p.indexed) })

    expect(outcome).toBe('completed')
    expect(indexed()).toMatchObject([
      { relPath: 'a.jpg', fileName: 'a.jpg', kind: 'image', size: 1, width: 30, height: 20, sortDate: 1000 },
      { relPath: 'trip/b.MP4', fileName: 'b.MP4', kind: 'video', width: null },
    ])
    expect(progress.at(-1)).toBe(2)
    expect(db.select().from(libraryRoots).get()).toMatchObject({ status: 'online', lastScanAt: expect.any(Number) })
  })

  it('stores tag hierarchies and links media to the leaf tag', async () => {
    await touch('a.jpg')
    await touch('b.jpg')
    metadata.byName.set('a.jpg', { tags: [['Places', 'Berlin'], ['Travel']] })
    // A reversed hierarchy must not create a parent cycle.
    metadata.byName.set('b.jpg', { tags: [['berlin', 'places']] })

    // Sequential, so `a.jpg` defines the hierarchy first.
    await scan({ concurrency: 1 })

    const [a, b] = indexed()
    expect(tagsOf(a?.id ?? 0)).toEqual(['Berlin', 'Travel'])
    expect(tagsOf(b?.id ?? 0)).toEqual(['Places'])
    const parents = Object.fromEntries(
      db
        .select()
        .from(tags)
        .all()
        .map((t) => [t.name, t.parentId]),
    )
    const placesId = db.select().from(tags).where(eq(tags.nameNorm, 'places')).get()?.id
    expect(parents).toEqual({ Places: null, Berlin: placesId, Travel: null })
  })

  it('rescans incrementally', async () => {
    const past = new Date(2020, 0, 1)
    await touch('same.jpg', 'x', past)
    await touch('changed.jpg', 'x', past)
    await touch('sidecar.jpg', 'x', past)
    await touch('gone.jpg', 'x', past)
    metadata.byName.set('gone.jpg', { tags: [['Old']] })
    await scan()
    db.update(media).set({ thumbStatus: 'ready', thumbhash: 'abc' }).run()
    metadata.reads = []

    await touch('changed.jpg', 'xy')
    await touch('sidecar.xmp')
    await unlink(join(dir, 'gone.jpg'))
    await touch('new.png')
    const outcome = await scan()

    expect(outcome).toBe('completed')
    expect(metadata.reads.sort()).toEqual(['changed.jpg', 'new.png', 'sidecar.jpg+xmp'])
    expect(indexed()).toMatchObject([
      { relPath: 'changed.jpg', size: 2, thumbStatus: 'pending', thumbhash: null },
      { relPath: 'new.png', thumbStatus: 'pending' },
      { relPath: 'same.jpg', thumbStatus: 'ready' },
      { relPath: 'sidecar.jpg', sidecarMtime: expect.any(Number), thumbStatus: 'ready', thumbhash: 'abc' },
    ])
    expect(db.select().from(tags).all()).toEqual([])
  })

  it('keeps the index of unreachable roots', async () => {
    await touch('a.jpg')
    await scan()
    deps.isReachable = async () => false

    expect(await scan()).toBe('offline')
    expect(indexed()).toHaveLength(1)
    expect(db.select().from(libraryRoots).get()?.status).toBe('offline')
  })

  it('writes nothing when cancelled', async () => {
    await touch('a.jpg')
    const controller = new AbortController()
    controller.abort()

    expect(await scan({ signal: controller.signal })).toBe('cancelled')
    expect(indexed()).toEqual([])
  })
})
