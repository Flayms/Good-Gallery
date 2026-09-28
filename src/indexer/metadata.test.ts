import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { crc32, deflateSync } from 'node:zlib'
import { ExifDateTime, type Tags } from 'exiftool-vendored'
import { describe, expect, it } from 'vitest'
import { ExifToolMetadataSource, extractMetadata, extractTags } from './metadata'

describe('extractTags', () => {
  it('merges keyword fields case-insensitively and keeps the first spelling', () => {
    const tags: Tags = { Keywords: ['Beach', ' sunset '], Subject: ['beach'], XPKeywords: 'Family;;SUNSET' }

    expect(extractTags([tags])).toEqual([['Beach'], ['sunset'], ['Family']])
  })

  it('keeps hierarchies and drops flat keywords that are part of one', () => {
    const tags: Tags = {
      HierarchicalSubject: ['Places|Germany|Berlin', 'Places|Germany'],
      Keywords: ['Places', 'Germany', 'Berlin', 'Travel'],
    }

    expect(extractTags([tags])).toEqual([['Places', 'Germany', 'Berlin'], ['Places', 'Germany'], ['Travel']])
  })

  it('accepts numeric keywords', () => {
    expect(extractTags([{ Keywords: [2024, 'x'] as unknown as string[] }])).toEqual([['2024'], ['x']])
  })
})

describe('extractMetadata', () => {
  it('swaps dimensions for rotated images', () => {
    expect(extractMetadata({ ImageWidth: 4000, ImageHeight: 3000, Orientation: 6 })).toMatchObject({
      width: 3000,
      height: 4000,
    })
    expect(extractMetadata({ ImageWidth: 4000, ImageHeight: 3000, Orientation: 1 })).toMatchObject({
      width: 4000,
      height: 3000,
    })
  })

  it('prefers embedded capture dates and falls back to the sidecar', () => {
    const date = (iso: string) => ExifDateTime.fromISO(iso)
    const embedded: Tags = { DateTimeOriginal: date('2020-01-02T03:04:05Z'), CreateDate: date('2021-01-01T00:00:00Z') }
    const sidecar: Tags = { DateTimeOriginal: date('2019-01-01T00:00:00Z') }

    expect(extractMetadata(embedded, sidecar).takenAt).toBe(Date.UTC(2020, 0, 2, 3, 4, 5))
    expect(extractMetadata({}, sidecar).takenAt).toBe(Date.UTC(2019, 0, 1))
    expect(extractMetadata({ DateTimeOriginal: '0000:00:00 00:00:00' }).takenAt).toBeNull()
  })

  it('reads video duration', () => {
    expect(extractMetadata({ Duration: 12.5 }).duration).toBe(12.5)
    expect(extractMetadata({ Duration: '0 s' }).duration).toBeNull()
  })
})

describe('ExifToolMetadataSource', () => {
  function grayscalePng(width: number, height: number): Buffer {
    const chunk = (type: string, data: Buffer) => {
      const body = Buffer.concat([Buffer.from(type, 'ascii'), data])
      const frame = Buffer.alloc(8)
      frame.writeUInt32BE(data.length, 0)
      frame.writeUInt32BE(crc32(body), 4)
      return Buffer.concat([frame.subarray(0, 4), body, frame.subarray(4)])
    }
    const header = Buffer.alloc(13)
    header.writeUInt32BE(width, 0)
    header.writeUInt32BE(height, 4)
    header[8] = 8 // bit depth; color type 0 (grayscale) and the rest stay 0
    const pixels = Buffer.alloc((width + 1) * height) // one filter byte per row
    const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
    return Buffer.concat([
      signature,
      chunk('IHDR', header),
      chunk('IDAT', deflateSync(pixels)),
      chunk('IEND', Buffer.alloc(0)),
    ])
  }
  const xmp = `<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:lr="http://ns.adobe.com/lightroom/1.0/">
      <dc:subject><rdf:Bag><rdf:li>Holiday</rdf:li></rdf:Bag></dc:subject>
      <lr:hierarchicalSubject><rdf:Bag><rdf:li>People|Anna</rdf:li></rdf:Bag></lr:hierarchicalSubject>
    </rdf:Description></rdf:RDF></x:xmpmeta>`

  it('reads dimensions from the file and tags from its sidecar', { timeout: 30_000 }, async () => {
    const dir = await mkdtemp(join(tmpdir(), 'gg-exif-'))
    const source = new ExifToolMetadataSource(1)
    try {
      await writeFile(join(dir, 'a.png'), grayscalePng(2, 1))
      await writeFile(join(dir, 'a.xmp'), xmp)

      const metadata = await source.read(join(dir, 'a.png'), join(dir, 'a.xmp'))

      expect(metadata).toMatchObject({ width: 2, height: 1, tags: [['People', 'Anna'], ['Holiday']] })
    } finally {
      await source.end()
      await rm(dir, { recursive: true, force: true })
    }
  })
})
