// Zoom/pan state of the viewer. The content is centered in the viewport and transformed by
// `translate(x, y) scale(scale)` around its center; `scale` 1 is the fitted size.

export interface ZoomView {
  scale: number
  x: number
  y: number
}

export interface Size {
  width: number
  height: number
}

export const MIN_SCALE = 1
export const MAX_SCALE = 8
export const FIT: ZoomView = { scale: 1, x: 0, y: 0 }

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value))
}

/** Limits the scale and keeps zoomed content covering the viewport; content smaller than it stays centered. */
export function clampView(view: ZoomView, content: Size, viewport: Size): ZoomView {
  const scale = clamp(view.scale, MIN_SCALE, MAX_SCALE)
  const maxX = Math.max(0, (content.width * scale - viewport.width) / 2)
  const maxY = Math.max(0, (content.height * scale - viewport.height) / 2)
  return { scale, x: clamp(view.x, -maxX, maxX), y: clamp(view.y, -maxY, maxY) }
}

/**
 * Zooms to `scale`, keeping the content point under `point` in place.
 * `point` is relative to the viewport center.
 */
export function zoomTo(
  view: ZoomView,
  scale: number,
  point: { x: number; y: number },
  content: Size,
  viewport: Size,
): ZoomView {
  const next = clamp(scale, MIN_SCALE, MAX_SCALE)
  const ratio = next / view.scale
  return clampView(
    { scale: next, x: point.x - (point.x - view.x) * ratio, y: point.y - (point.y - view.y) * ratio },
    content,
    viewport,
  )
}
