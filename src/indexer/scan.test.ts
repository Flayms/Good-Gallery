import { mkdir, mkdtemp, rm, stat, unlink, utimes, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { basename, dirname, join, resolve } from 'node:path'
import { asc, eq } from 'drizzle-orm'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { type Db, openDatabase } from '../main/db'
import { folders, libraryRoots, media, mediaTags, tags } from '../main/db/schema'
import { EMPTY_METADATA, type MediaMetadata, type MetadataSource } from './metadata'
import type { ScanScope } from './protocol'
import { type ScanDeps, type ScanOptions, scanRoot } from './scan'
import { IndexWriter } from './writer'

const migrationsFolder = resolve(import.meta.dirname, '../../drizzle')

/** Returns metadata keyed by file name and records which files were read. */
class FakeMetadata implements MetadataSource {
  readonly byName = new Map<string, Partial<MediaMetadata>>()
  /** Reads of these files wait until the promise resolves. */
  readonly blocked = new Map<string, Promise<void>>()
  reads: string[] = []

  async read(file: string, sidecar?: string): Promise<MediaMetadata> {
    const name = basename(file)
    this.reads.push(sidecar ? `${name}+xmp` : name)
    await this.blocked.get(name)
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

const PAST = new Date(2020, 0, 1)

/** Dates the given folders back, so that later changes in them are noticed by their mtime. */
async function age(...relDirs: string[]) {
  for (const relDir of relDirs) await utimes(join(dir, relDir), PAST, PAST)
}

function scan(options: Partial<ScanOptions> = {}, scope: ScanScope = { mode: 'full' }) {
  return scanRoot(deps, rootId, scope, {
    concurrency: 2,
    signal: new AbortController().signal,
    onProgress() {},
    ...options,
  })
}

function indexed() {
  return db.select().from(media).orderBy(asc(media.relPath)).all()
}

function indexedPaths() {
  return indexed().map((row) => row.relPath)
}

function storedFolders() {
  return db
    .select({ relDir: folders.relDir })
    .from(folders)
    .orderBy(asc(folders.relDir))
    .all()
    .map((row) => row.relDir)
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

  it('writes read files while the scan still runs', async () => {
    await touch('a.jpg')
    await touch('b.jpg')
    const release = Promise.withResolvers<void>()
    metadata.blocked.set('b.jpg', release.promise)
    const progress: number[] = []

    const running = scan({ concurrency: 1, flushIntervalMs: 10, onProgress: (p) => progress.push(p.indexed) })

    await vi.waitFor(() => expect(indexedPaths()).toEqual(['a.jpg']))
    expect(progress).toContain(1)
    release.resolve()
    expect(await running).toBe('completed')
    expect(indexedPaths()).toEqual(['a.jpg', 'b.jpg'])
  })

  it('quick scans only re-list folders whose entries changed', async () => {
    await touch('keep/a.jpg', 'x', PAST)
    await touch('changed/b.jpg', 'x', PAST)
    await touch('old/c.jpg', 'x', PAST)
    await touch('old/sub/d.jpg', 'x', PAST)
    await age('keep', 'changed', 'old/sub', 'old', '')
    await scan()
    expect(storedFolders()).toEqual(['', 'changed', 'keep', 'old', 'old/sub'])
    metadata.reads = []

    // Changed in place: the folder mtime stays, so only a full scan notices.
    const keepMtime = (await stat(join(dir, 'keep'))).mtime
    await touch('keep/a.jpg', 'xy')
    await utimes(join(dir, 'keep'), keepMtime, keepMtime)
    await touch('changed/new.jpg')
    await unlink(join(dir, 'changed/b.jpg'))
    await rm(join(dir, 'old'), { recursive: true })
    await touch('fresh/deep/e.jpg')

    expect(await scan({}, { mode: 'quick' })).toBe('completed')
    expect(metadata.reads.sort()).toEqual(['e.jpg', 'new.jpg'])
    expect(indexedPaths()).toEqual(['changed/new.jpg', 'fresh/deep/e.jpg', 'keep/a.jpg'])
    expect(storedFolders()).toEqual(['', 'changed', 'fresh', 'fresh/deep', 'keep'])

    metadata.reads = []
    await scan()
    expect(metadata.reads).toEqual(['a.jpg'])
  })

  it('falls back to a full scan without stored folders', async () => {
    await touch('a/b.jpg')

    expect(await scan({}, { mode: 'quick' })).toBe('completed')
    expect(indexedPaths()).toEqual(['a/b.jpg'])
    expect(storedFolders()).toEqual(['', 'a'])
  })

  it('checks only reported files of changed folders', async () => {
    await touch('a/x.jpg', 'x', PAST)
    await touch('a/y.jpg', 'x', PAST)
    await touch('a/unreported.jpg', 'x', PAST)
    await touch('b/z.jpg', 'x', PAST)
    await scan()
    metadata.reads = []

    await touch('a/x.jpg', 'xy')
    await touch('a/y.xmp')
    await touch('a/unreported.jpg', 'xy')
    await touch('a/new.jpg')
    await rm(join(dir, 'b'), { recursive: true })
    await touch('c/w.jpg')
    const paths = ['a/x.jpg', 'a/y.xmp', 'a/new.jpg', 'b', 'c']

    expect(await scan({}, { mode: 'changes', paths })).toBe('completed')
    expect(metadata.reads.sort()).toEqual(['new.jpg', 'w.jpg', 'x.jpg', 'y.jpg+xmp'])
    expect(indexedPaths()).toEqual(['a/new.jpg', 'a/unreported.jpg', 'a/x.jpg', 'a/y.jpg', 'c/w.jpg'])
    expect(storedFolders()).toEqual(['', 'a', 'c'])
  })

  it('writes nothing when cancelled', async () => {
    await touch('a.jpg')
    const controller = new AbortController()
    controller.abort()

    expect(await scan({ signal: controller.signal })).toBe('cancelled')
    expect(indexed()).toEqual([])
  })
})
