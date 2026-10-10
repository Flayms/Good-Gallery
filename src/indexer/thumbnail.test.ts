import { execFile } from 'node:child_process'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { promisify } from 'node:util'
import ffmpegPath from 'ffmpeg-static'
import sharp from 'sharp'
import { thumbHashToAverageRGBA } from 'thumbhash'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { orient, type PreviewSource, renderMedia, type Thumbnails } from './thumbnail'

const noPreviews: PreviewSource = { extractPreview: async () => undefined }

let dir: string

beforeAll(async () => {
  dir = await mkdtemp(join(tmpdir(), 'gg-render-'))
})

afterAll(async () => {
  await rm(dir, { recursive: true, force: true })
})

function solid(width: number, height: number, background: string) {
  return sharp({ create: { width, height, channels: 3, background } })
}

async function sizes(thumbnails: Thumbnails) {
  return Promise.all(
    thumbnails.images.map(async ({ width, data }) => {
      const meta = await sharp(data).metadata()
      return { width, format: meta.format, actual: [meta.width, meta.height] }
    }),
  )
}

function averageColor(thumbnails: Thumbnails) {
  const { r, g, b } = thumbHashToAverageRGBA(Buffer.from(thumbnails.thumbhash, 'base64'))
  return [r, g, b].map((channel) => Math.round(channel))
}

describe('orient', () => {
  // A 2×3 image with one distinct pixel per position, so every flip and rotation is observable.
  const pixels = Buffer.from([...Array(6).keys()].flatMap((i) => [i * 40, 0, 0]))
  const raw = { width: 2, height: 3, channels: 3 } as const

  it.each([1, 2, 3, 4, 5, 6, 7, 8])('matches sharp auto-orientation for orientation %i', async (orientation) => {
    const tagged = await sharp(pixels, { raw }).png().withMetadata({ orientation }).toBuffer()
    const expected = await sharp(tagged).autoOrient().raw().toBuffer({ resolveWithObject: true })
    const actual = await orient(sharp(pixels, { raw }), orientation).raw().toBuffer({ resolveWithObject: true })

    expect([actual.info.width, actual.info.height]).toEqual([expected.info.width, expected.info.height])
    expect([...actual.data]).toEqual([...expected.data])
  })
})

describe('renderMedia', () => {
  it('renders every width as WebP without upscaling, plus a ThumbHash', async () => {
    const file = join(dir, 'wide.png')
    await solid(1000, 500, '#ff0000').png().toFile(file)

    const thumbnails = await renderMedia(file, { kind: 'image', duration: null }, noPreviews)

    expect(await sizes(thumbnails)).toEqual([
      { width: 400, format: 'webp', actual: [400, 200] },
      { width: 800, format: 'webp', actual: [800, 400] },
    ])
    expect(averageColor(thumbnails)).toEqual([1, 0, 0])

    const small = join(dir, 'small.png')
    await solid(300, 300, '#00ff00').png().toFile(small)
    const smallSizes = await sizes(await renderMedia(small, { kind: 'image', duration: null }, noPreviews))
    expect(smallSizes.map(({ actual }) => actual)).toEqual([
      [300, 300],
      [300, 300],
    ])
  })

  it('uses a large enough embedded JPEG preview with the original orientation', async () => {
    const file = join(dir, 'camera.jpg')
    await solid(1600, 1200, '#ff0000').jpeg().withMetadata({ orientation: 6 }).toFile(file)
    const preview = await solid(1200, 900, '#0000ff').jpeg().toBuffer()

    const thumbnails = await renderMedia(
      file,
      { kind: 'image', duration: null },
      { extractPreview: async () => preview },
    )

    expect((await sizes(thumbnails)).at(-1)?.actual).toEqual([800, 1067])
    expect(averageColor(thumbnails)).toEqual([0, 0, 1])
  })

  it('falls back to the full image when the preview is too small', async () => {
    const file = join(dir, 'tiny-preview.jpg')
    await solid(1600, 1200, '#ff0000').jpeg().toFile(file)
    const preview = await solid(160, 120, '#0000ff').jpeg().toBuffer()

    const thumbnails = await renderMedia(
      file,
      { kind: 'image', duration: null },
      { extractPreview: async () => preview },
    )

    expect(averageColor(thumbnails)).toEqual([1, 0, 0])
  })

  it('falls back to the full image when the preview is not an image', async () => {
    const file = join(dir, 'broken-preview.jpg')
    await solid(1600, 1200, '#ff0000').jpeg().toFile(file)

    const thumbnails = await renderMedia(
      file,
      { kind: 'image', duration: null },
      { extractPreview: async () => Buffer.from('not an image') },
    )

    expect(averageColor(thumbnails)).toEqual([1, 0, 0])
  })

  it('extracts a video frame with ffmpeg', async () => {
    if (!ffmpegPath) throw new Error('ffmpeg-static has no binary for this platform')
    const file = join(dir, 'clip.mp4')
    const source = ['-f', 'lavfi', '-i', 'color=c=blue:s=320x240:d=1', '-pix_fmt', 'yuv420p']
    await promisify(execFile)(ffmpegPath, ['-hide_banner', '-loglevel', 'error', ...source, file])

    const thumbnails = await renderMedia(file, { kind: 'video', duration: 1 }, noPreviews)

    expect((await sizes(thumbnails)).map(({ actual }) => actual)).toEqual([
      [320, 240],
      [320, 240],
    ])
    expect(averageColor(thumbnails)).toEqual([0, 0, 1])
  })
})
