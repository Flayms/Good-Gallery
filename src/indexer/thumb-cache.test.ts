import { mkdir, mkdtemp, readdir, rm, stat, utimes, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { ThumbCache, type ThumbKey, thumbPath } from './thumb-cache'

const key: ThumbKey = { rootId: 1, relPath: 'trip/a.jpg', size: 100, mtime: 1_700_000_000_000 }

let dir: string

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), 'gg-thumbs-'))
})

afterEach(async () => {
  await rm(dir, { recursive: true, force: true })
})

async function files(): Promise<string[]> {
  const entries = await readdir(dir, { recursive: true, withFileTypes: true })
  return entries
    .filter((entry) => entry.isFile())
    .map((entry) => entry.name)
    .sort()
}

async function seed(name: string, bytes: number, mtime: Date) {
  const path = join(dir, name.slice(0, 2), name.slice(2, 4), name)
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, Buffer.alloc(bytes))
  await utimes(path, mtime, mtime)
}

describe('thumbPath', () => {
  it('is stable and fanned out by hash prefix', () => {
    const path = thumbPath(dir, key, 400)
    expect(path).toBe(thumbPath(dir, { ...key }, 400))
    expect(path).toMatch(/[\\/]([0-9a-f]{2})[\\/]([0-9a-f]{2})[\\/]\1\2[0-9a-f]{36}\.webp$/)
  })

  it('changes with every part of the key', () => {
    const variants = [
      thumbPath(dir, key, 400),
      thumbPath(dir, key, 800),
      thumbPath(dir, { ...key, rootId: 2 }, 400),
      thumbPath(dir, { ...key, relPath: 'trip/b.jpg' }, 400),
      thumbPath(dir, { ...key, size: 101 }, 400),
      thumbPath(dir, { ...key, mtime: key.mtime + 1 }, 400),
    ]
    expect(new Set(variants).size).toBe(variants.length)
  })
})

describe('ThumbCache', () => {
  it('measures existing thumbnails and drops leftover temp files', async () => {
    await seed('aaaa.webp', 60, new Date())
    await seed('bbbb.webp.123.tmp', 10, new Date())

    const full = new ThumbCache(dir, 50)
    await full.ready
    expect(full.hasRoom()).toBe(false)
    expect(await files()).toEqual(['aaaa.webp'])

    const roomy = new ThumbCache(dir, 100)
    await roomy.ready
    expect(roomy.hasRoom()).toBe(true)
  })

  it('evicts least recently used thumbnails once over the cap', async () => {
    await seed('aaaa.webp', 40, new Date(2020, 0, 1))
    await seed('bbbb.webp', 40, new Date(2021, 0, 1))
    const cache = new ThumbCache(dir, 100)
    await cache.ready

    const path = cache.path(key, 400)
    await cache.write(path, Buffer.alloc(40))
    await cache.evict()

    // 120 bytes > 100 → down to ≤ 90: the oldest file goes.
    expect(await files()).toEqual([path.split(/[\\/]/).at(-1), 'bbbb.webp'].sort())
    expect((await stat(path)).size).toBe(40)
    expect(cache.hasRoom()).toBe(true)
  })
})
