import { type RefObject, useEffect, useRef } from 'react'

/** Wheel distance per zoom step: one notch of a typical mouse wheel. */
export const WHEEL_STEP = 100

/**
 * Adds a wheel delta to the accumulated distance and splits it into whole zoom steps (positive = scrolled down)
 * and the remainder, so touchpads with small deltas zoom as smoothly as a mouse wheel.
 */
export function accumulateWheel(accumulated: number, deltaY: number): { steps: number; rest: number } {
  // A change of direction starts over, so a reversal reacts right away.
  const start = Math.sign(accumulated) === -Math.sign(deltaY) ? 0 : accumulated
  const total = start + deltaY
  // `|| 0`: `Math.trunc` of a small negative number is -0.
  const steps = Math.trunc(total / WHEEL_STEP) || 0
  return { steps, rest: total - steps * WHEEL_STEP }
}

/** Calls `onSteps` for Ctrl + wheel over the element (positive = scrolled down) instead of scrolling. */
export function useCtrlWheelZoom(ref: RefObject<HTMLElement | null>, onSteps: (steps: number) => void): void {
  const onStepsRef = useRef(onSteps)
  onStepsRef.current = onSteps

  useEffect(() => {
    const element = ref.current
    if (!element) return
    let accumulated = 0
    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey) return
      // React's `onWheel` is passive and can't stop the scroll, hence the native listener.
      event.preventDefault()
      // Line or page deltas (rare on Windows) count as one step each.
      const deltaY =
        event.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? event.deltaY : Math.sign(event.deltaY) * WHEEL_STEP
      const { steps, rest } = accumulateWheel(accumulated, deltaY)
      accumulated = rest
      if (steps !== 0) onStepsRef.current(steps)
    }
    element.addEventListener('wheel', onWheel, { passive: false })
    return () => element.removeEventListener('wheel', onWheel)
  }, [ref])
}
