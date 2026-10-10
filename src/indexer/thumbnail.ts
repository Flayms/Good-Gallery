import { execFile } from 'node:child_process'
import { extname } from 'node:path'
import { promisify } from 'node:util'
import ffmpegStatic from 'ffmpeg-static'
import sharp, { type Sharp } from 'sharp'
import { rgbaToThumbHash } from 'thumbhash'
import { MAX_THUMB_WIDTH, THUMB_WIDTHS, type ThumbWidth } from '../shared/media-urls'
import type { MediaKind } from './media-types'

// libvips' operation cache keeps source files open, which blocks renames/deletes on Windows shares.
sharp.cache(false)

// Matches the masonry aspect clamp (height ≤ 3 × width); taller strips are cropped by the tile anyway.
const MAX_HEIGHT = MAX_THUMB_WIDTH * 3
const WEBP_QUALITY = 80
const THUMBHASH_SIZE = 100
const VIDEO_POSITION = 0.1
const FFMPEG_TIMEOUT_MS = 60_000
const PREVIEW_ASPECT_TOLERANCE = 0.02

const execFileAsync = promisify(execFile)
// The binary can't be executed from inside the asar archive.
const ffmpegPath = ffmpegStatic?.replace('app.asar', 'app.asar.unpacked')

export interface Thumbnails {
  /** Base64-encoded ThumbHash. */
  thumbhash: string
  images: { width: ThumbWidth; data: Buffer }[]
}

export interface PreviewSource {
  /** Embedded JPEG preview of an image, if it has one. */
  extractPreview(file: string): Promise<Buffer | undefined>
}

export interface RenderSource {
  kind: MediaKind
  /** Seconds. */
  duration: number | null
}

// Verified against sharp's own auto-orientation in thumbnail.test.ts.
const ORIENTATIONS: Record<number, { angle: number; flop: boolean }> = {
  2: { angle: 0, flop: true },
  3: { angle: 180, flop: false },
  4: { angle: 180, flop: true },
  5: { angle: 270, flop: true },
  6: { angle: 90, flop: false },
  7: { angle: 90, flop: true },
  8: { angle: 270, flop: false },
}

/** Applies an EXIF orientation (1–8) explicitly, for images whose own EXIF doesn't carry it. */
export function orient(image: Sharp, orientation: number): Sharp {
  const transform = ORIENTATIONS[orientation]
  return transform ? image.rotate(transform.angle).flop(transform.flop) : image
}

/** Camera JPEGs often embed a large preview; decoding it avoids reading the full file over the network. */
async function loadPreview(file: string, previews: PreviewSource): Promise<Sharp | undefined> {
  const buffer = await previews.extractPreview(file)
  if (!buffer) return undefined
  const [original, preview] = await Promise.all([
    sharp(file).metadata(),
    // Some cameras (e.g. DJI) point PreviewImage at data that isn't an image.
    sharp(buffer)
      .metadata()
      .catch(() => undefined),
  ])
  if (!preview) return undefined
  const orientation = original.orientation ?? 1
  const previewWidth = orientation >= 5 ? preview.height : preview.width
  const aspectRatio = preview.width / preview.height / (original.width / original.height)
  const largeEnough = previewWidth >= Math.min(MAX_THUMB_WIDTH, original.autoOrient.width)
  if (!largeEnough || Math.abs(aspectRatio - 1) > PREVIEW_ASPECT_TOLERANCE) return undefined
  return orient(sharp(buffer), orientation)
}

async function loadImage(file: string, previews: PreviewSource): Promise<Sharp> {
  const ext = extname(file).toLowerCase()
  const preview = ext === '.jpg' || ext === '.jpeg' ? await loadPreview(file, previews) : undefined
  // Truncated files (interrupted copies) still decode partially.
  return preview ?? sharp(file, { failOn: 'none' }).autoOrient()
}

async function loadVideoFrame(file: string, duration: number | null): Promise<Sharp> {
  if (!ffmpegPath) throw new Error('ffmpeg is not available on this platform')
  const position = duration ? duration * VIDEO_POSITION : 0
  const { stdout } = await execFileAsync(
    ffmpegPath,
    [
      ...['-hide_banner', '-loglevel', 'error', '-nostdin'],
      // Input seeking (before `-i`) reads only around the target frame.
      ...['-ss', position.toFixed(3), '-i', file],
      ...[
        '-frames:v',
        '1',
        '-vf',
        `scale='min(${MAX_THUMB_WIDTH},iw)':-2`,
        '-f',
        'image2pipe',
        '-c:v',
        'png',
        'pipe:1',
      ],
    ],
    { encoding: 'buffer', maxBuffer: 64 * 1024 * 1024, timeout: FFMPEG_TIMEOUT_MS, windowsHide: true },
  )
  if (stdout.length === 0) throw new Error(`ffmpeg extracted no frame from ${file}`)
  return sharp(stdout)
}

/** Renders every thumbnail width and the ThumbHash from a single decode. */
export async function renderThumbnails(image: Sharp): Promise<Thumbnails> {
  const { data, info } = await image
    .resize({ width: MAX_THUMB_WIDTH, height: MAX_HEIGHT, fit: 'inside', withoutEnlargement: true })
    .toColourspace('srgb')
    .raw({ depth: 'uchar' })
    .toBuffer({ resolveWithObject: true })
  const decoded = () => sharp(data, { raw: { width: info.width, height: info.height, channels: info.channels } })

  const [hash, images] = await Promise.all([
    decoded()
      .resize({ width: THUMBHASH_SIZE, height: THUMBHASH_SIZE, fit: 'inside' })
      .ensureAlpha()
      .raw()
      .toBuffer({ resolveWithObject: true }),
    Promise.all(
      THUMB_WIDTHS.map(async (width) => ({
        width,
        data: await decoded().resize({ width, withoutEnlargement: true }).webp({ quality: WEBP_QUALITY }).toBuffer(),
      })),
    ),
  ])
  return {
    thumbhash: Buffer.from(rgbaToThumbHash(hash.info.width, hash.info.height, hash.data)).toString('base64'),
    images,
  }
}

export async function renderMedia(
  file: string,
  { kind, duration }: RenderSource,
  previews: PreviewSource,
): Promise<Thumbnails> {
  const image = kind === 'video' ? await loadVideoFrame(file, duration) : await loadImage(file, previews)
  return renderThumbnails(image)
}
