import { type MouseEvent, type PointerEvent, useCallback, useEffect, useRef, useState, type WheelEvent } from 'react'
import { cn } from '@/lib/utils'
import { FIT, MAX_SCALE, type Size, type ZoomView, zoomTo } from '@/lib/zoom'

const WHEEL_SENSITIVITY = 0.002
const KEY_ZOOM_STEP = 1.25

function sizeOf(element: HTMLElement): Size {
  // Layout size, unaffected by the zoom transform.
  return { width: element.offsetWidth, height: element.offsetHeight }
}

/** Image fitted into its container; wheel / double-click / `+` `-` `0` zoom, dragging pans. */
export function ZoomableImage({ src, alt }: { src: string; alt: string }) {
  const viewportRef = useRef<HTMLDivElement>(null)
  const imageRef = useRef<HTMLImageElement>(null)
  const drag = useRef<{ pointerId: number; x: number; y: number; view: ZoomView } | null>(null)
  const [view, setView] = useState(FIT)

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

  const onDoubleClick = (event: MouseEvent) => {
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
    drag.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, view }
  }

  const onPointerMove = (event: PointerEvent) => {
    const start = drag.current
    const viewport = viewportRef.current
    const image = imageRef.current
    if (!start || start.pointerId !== event.pointerId || !viewport || !image) return
    const moved = {
      ...start.view,
      x: start.view.x + event.clientX - start.x,
      y: start.view.y + event.clientY - start.y,
    }
    // zoomTo at the same scale only clamps the pan.
    setView(zoomTo(moved, moved.scale, { x: 0, y: 0 }, sizeOf(image), sizeOf(viewport)))
  }

  const onPointerUp = (event: PointerEvent) => {
    if (drag.current?.pointerId === event.pointerId) drag.current = null
  }

  const zoomed = view.scale > 1
  return (
    // To assistive tech this is just the image; the mouse gestures mirror the global `+` `-` `0` keys.
    <div
      ref={viewportRef}
      role="img"
      aria-label={alt}
      className={cn(
        'flex size-full touch-none items-center justify-center overflow-hidden',
        zoomed ? 'cursor-grab active:cursor-grabbing' : 'cursor-zoom-in',
      )}
      onWheel={onWheel}
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
