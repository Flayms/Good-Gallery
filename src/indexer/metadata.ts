import { ExifDateTime, ExifTool, type Tags } from 'exiftool-vendored'
import { cleanTag, normalizeTag } from '../shared/tags'
import type { PreviewSource } from './thumbnail'

export interface MediaMetadata {
  width: number | null
  height: number | null
  /** Seconds. */
  duration: number | null
  takenAt: number | null
  /** Tag paths, outermost first; flat keywords are single-element paths. */
  tags: string[][]
}

export const EMPTY_METADATA: MediaMetadata = { width: null, height: null, duration: null, takenAt: null, tags: [] }

export interface MetadataSource {
  read(file: string, sidecar?: string): Promise<MediaMetadata>
}

/** Reads metadata through a pool of long-running exiftool processes (`-stay_open`). */
export class ExifToolMetadataSource implements MetadataSource, PreviewSource {
  readonly #exiftool: ExifTool

  constructor(maxProcs: number) {
    this.#exiftool = new ExifTool({ maxProcs, taskTimeoutMillis: 60_000 })
  }

  async read(file: string, sidecar?: string): Promise<MediaMetadata> {
    const [tags, sidecarTags] = await Promise.all([
      this.#exiftool.read(file),
      sidecar ? this.#exiftool.read(sidecar) : undefined,
    ])
    return extractMetadata(tags, sidecarTags)
  }

  async extractPreview(file: string): Promise<Buffer | undefined> {
    try {
      const preview = await this.#exiftool.extractBinaryTagToBuffer('PreviewImage', file)
      return preview.length > 0 ? preview : undefined
    } catch {
      // Most files have no preview; real read errors resurface when decoding the file itself.
      return undefined
    }
  }

  end(): Promise<void> {
    return this.#exiftool.end()
  }
}

const DATE_TAGS = ['SubSecDateTimeOriginal', 'DateTimeOriginal', 'CreateDate', 'MediaCreateDate'] as const

function isPositive(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0
}

function dateMillis(value: unknown): number | null {
  if (!(value instanceof ExifDateTime)) return null
  const millis = value.toMillis()
  // Cameras without a clock write zeroed dates, which parse to the epoch or earlier.
  return isPositive(millis) ? millis : null
}

function textValues(value: unknown): string[] {
  // exiftool-vendored turns numeric-looking keywords such as "2024" into numbers.
  return (Array.isArray(value) ? value : [value]).flatMap((item) =>
    typeof item === 'string' ? [item] : typeof item === 'number' ? [String(item)] : [],
  )
}

/** Merges tags from all sources into deduplicated tag paths. */
export function extractTags(sources: Tags[]): string[][] {
  const hierarchical: string[][] = []
  const flat: string[] = []
  for (const tags of sources) {
    for (const value of textValues(tags.HierarchicalSubject)) hierarchical.push(value.split('|'))
    flat.push(...textValues(tags.Keywords), ...textValues(tags.Subject))
    for (const value of textValues(tags.XPKeywords)) flat.push(...value.split(';'))
  }

  const paths = new Map<string, string[]>()
  const add = (path: string[]) => {
    const names = path.map(cleanTag).filter(Boolean)
    const key = names.map(normalizeTag).join('|')
    if (names.length > 0 && !paths.has(key)) paths.set(key, names)
  }
  for (const path of hierarchical) add(path)
  // Lightroom also writes every hierarchy level as a flat keyword; the hierarchy already covers those.
  const inHierarchy = new Set(hierarchical.flat().map(normalizeTag))
  for (const name of flat) if (!inHierarchy.has(normalizeTag(name))) add([name])
  return [...paths.values()]
}

/** Sidecar values fill gaps in the embedded metadata; tags from both are merged. */
export function extractMetadata(tags: Tags, sidecar?: Tags): MediaMetadata {
  const sources = sidecar ? [tags, sidecar] : [tags]
  let width: number | null = null
  let height: number | null = null
  if (isPositive(tags.ImageWidth) && isPositive(tags.ImageHeight)) {
    // EXIF orientations 5–8 and 90°/270° video rotations swap the displayed axes.
    const swapped = (tags.Orientation ?? 1) >= 5 || tags.Rotation === 90 || tags.Rotation === 270
    width = swapped ? tags.ImageHeight : tags.ImageWidth
    height = swapped ? tags.ImageWidth : tags.ImageHeight
  }
  const takenAt =
    sources.flatMap((source) => DATE_TAGS.map((name) => dateMillis(source[name]))).find((ms) => ms !== null) ?? null
  return {
    width,
    height,
    duration: isPositive(tags.Duration) ? tags.Duration : null,
    takenAt,
    tags: extractTags(sources),
  }
}
