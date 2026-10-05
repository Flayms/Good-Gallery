import { cn } from '@/lib/utils'
import { FIT, MAX_SCALE, type Size, zoomTo, type ZoomView } from '@/lib/zoom'
import { type MouseEvent, type PointerEvent, useCallback, useEffect, useRef, useState, type WheelEvent } from 'react'

const WHEEL_SENSITIVITY = 0.002
const KEY_ZOOM_STEP = 1.25
/** Longer than this and a pointer-down → pointer-up isn't a click, it's the end of a pan. */
const DRAG_THRESHOLD = 4
/** A single click is only fired once this long has passed without a second one turning it into a double-click. */
const CLICK_DELAY = 250

function sizeOf(element: HTMLElement): Size {
  // Layout size, unaffected by the zoom transform.
  return { width: element.offsetWidth, height: element.offsetHeight }
}

interface ZoomableImageProps {
  src: string
  alt: string
  /** Fires for a plain (non-double) click that lands on the letterboxing around the image, not the image itself. */
  onBackdropClick?: () => void
}

/** Image fitted into its container; wheel / double-click / `+` `-` `0` zoom, dragging pans. */
export function ZoomableImage({ src, alt, onBackdropClick }: ZoomableImageProps) {
  const viewportRef = useRef<HTMLDivElement>(null)
  const imageRef = useRef<HTMLImageElement>(null)
  const drag = useRef<{ pointerId: number; x: number; y: number; view: ZoomView; dragged: boolean } | null>(null)
  const clickTimer = useRef<ReturnType<typeof setTimeout>>(undefined)
  const [view, setView] = useState(FIT)

  useEffect(() => () => clearTimeout(clickTimer.current), [])

  /** Zooms around a point given in client coordinates, or around the center. */
  const zoom = useCallback((scale: (current: ZoomView) => number, client?: { x: number; y: number }) => {
    const viewport = viewportRef.current
    const image = imageRef.current
    if (!viewport || !image) return
    const rect = viewport.getBoundingClientRect()
    const point = client
      ? { x: client.x - rect.left - rect.width / 2, y: client.y - rect.top - rect.height / 2 }
      : { x: 0, y: 0 }
    setView((current) => zoomTo(current, scale(current), point, sizeOf(image), sizeOf(viewport)))
  }, [])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.ctrlKey || event.altKey || event.metaKey) return
      if (event.key === '+' || event.key === '=') zoom((current) => current.scale * KEY_ZOOM_STEP)
      else if (event.key === '-') zoom((current) => current.scale / KEY_ZOOM_STEP)
      else if (event.key === '0') setView(FIT)
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [zoom])

  const onWheel = (event: WheelEvent) => {
    const factor = Math.exp(-event.deltaY * WHEEL_SENSITIVITY)
    zoom((current) => current.scale * factor, { x: event.clientX, y: event.clientY })
  }

  const onClick = (event: MouseEvent) => {
    // A pan release or a click on the image itself (not the surrounding letterboxing) never closes the viewer.
    if (!onBackdropClick || event.target !== event.currentTarget) return
    clearTimeout(clickTimer.current)
    // Delayed so a following click (a double-click, handled below) can cancel it instead of also zooming and closing.
    clickTimer.current = setTimeout(onBackdropClick, CLICK_DELAY)
  }

  const onDoubleClick = (event: MouseEvent) => {
    clearTimeout(clickTimer.current)
    const image = imageRef.current
    // Toggles between fitted and actual pixels (at least 2×, so small images zoom too).
    const actual = image ? image.naturalWidth / Math.max(1, image.offsetWidth) : 2
    zoom((current) => (current.scale > 1 ? 1 : Math.min(MAX_SCALE, Math.max(2, actual))), {
      x: event.clientX,
      y: event.clientY,
    })
  }

  const onPointerDown = (event: PointerEvent) => {
    if (event.button !== 0 || view.scale <= 1) return
    event.currentTarget.setPointerCapture(event.pointerId)
    drag.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, view, dragged: false }
  }

  const onPointerMove = (event: PointerEvent) => {
    const start = drag.current
    const viewport = viewportRef.current
    const image = imageRef.current
    if (!start || start.pointerId !== event.pointerId || !viewport || !image) return
    const dx = event.clientX - start.x
    const dy = event.clientY - start.y
    if (Math.hypot(dx, dy) > DRAG_THRESHOLD) start.dragged = true
    const moved = { ...start.view, x: start.view.x + dx, y: start.view.y + dy }
    // zoomTo at the same scale only clamps the pan.
    setView(zoomTo(moved, moved.scale, { x: 0, y: 0 }, sizeOf(image), sizeOf(viewport)))
  }

  const onPointerUp = (event: PointerEvent) => {
    if (drag.current?.pointerId !== event.pointerId) return
    // A real pan shouldn't also fire the click that follows pointer-up.
    if (drag.current.dragged) clearTimeout(clickTimer.current)
    drag.current = null
  }

  const zoomed = view.scale > 1
  return (
    // To assistive tech this is just the image; the mouse gestures mirror the global `+` `-` `0` keys.
    // biome-ignore lint/a11y/useKeyWithClickEvents: mouse-only convenience; Escape already closes the viewer.
    <div
      ref={viewportRef}
      role="img"
      aria-label={alt}
      className={cn(
        'flex size-full touch-none items-center justify-center overflow-hidden',
        zoomed ? 'cursor-grab active:cursor-grabbing' : 'cursor-zoom-in',
      )}
      onWheel={onWheel}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
    >
      <img
        ref={imageRef}
        src={src}
        alt={alt}
        draggable={false}
        className="max-h-full max-w-full select-none object-contain"
        style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }}
      />
    </div>
  )
}
