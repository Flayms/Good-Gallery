// Custom protocol URLs: built by the renderer, parsed by main. Only media ids travel over them, never paths.

export const THUMB_SCHEME = 'gg-thumb'
export const MEDIA_SCHEME = 'gg-media'

export const THUMB_WIDTHS = [400, 800] as const
export type ThumbWidth = (typeof THUMB_WIDTHS)[number]
export const MAX_THUMB_WIDTH = 800 satisfies ThumbWidth

// Standard schemes canonicalize numeric hosts as IPv4 (`//12` → `//0.0.0.12`), so the id lives in the path.
const HOST = 'media'
const ID = /^[1-9]\d{0,14}$/

export function thumbUrl(id: number, width: ThumbWidth): string {
  return `${THUMB_SCHEME}://${HOST}/${id}/${width}`
}

export function mediaUrl(id: number): string {
  return `${MEDIA_SCHEME}://${HOST}/${id}`
}

/** Smallest thumbnail covering `cssWidth` at the given pixel ratio, else the largest. */
export function pickThumbWidth(cssWidth: number, pixelRatio: number): ThumbWidth {
  const needed = cssWidth * pixelRatio
  return THUMB_WIDTHS.find((width) => width >= needed) ?? MAX_THUMB_WIDTH
}

function pathSegments(url: string, scheme: string): string[] | undefined {
  let parsed: URL
  try {
    parsed = new URL(url)
  } catch {
    return undefined
  }
  if (parsed.protocol !== `${scheme}:` || parsed.host !== HOST || parsed.search || parsed.hash) return undefined
  return parsed.pathname.split('/').slice(1)
}

export function parseThumbUrl(url: string): { id: number; width: ThumbWidth } | undefined {
  const [id, width, ...rest] = pathSegments(url, THUMB_SCHEME) ?? []
  if (!id || !ID.test(id) || rest.length > 0) return undefined
  const thumbWidth = THUMB_WIDTHS.find((w) => String(w) === width)
  return thumbWidth ? { id: Number(id), width: thumbWidth } : undefined
}

export function parseMediaUrl(url: string): { id: number } | undefined {
  const [id, ...rest] = pathSegments(url, MEDIA_SCHEME) ?? []
  if (!id || !ID.test(id) || rest.length > 0) return undefined
  return { id: Number(id) }
}
