import type { Virtualizer } from '@tanstack/react-virtual'
import { useEffect, useLayoutEffect, useRef } from 'react'

const identity = (index: number) => index

interface VirtualAnchorOptions {
  virtualizer: Virtualizer<HTMLDivElement, Element>
  /**
   * Changes (by `===`) whenever the virtual item sizes change (zoom, resize, new rows); the first visible item stays
   * in view across changes.
   */
  layoutKey: unknown
  /** First and last rendered item index, or `-1` for the last one while nothing is rendered. */
  itemRange: readonly [number, number]
  /** Item index of the first item of virtual item `virtualIndex` (identity unless virtual items are rows). */
  toItem?: (virtualIndex: number) => number
  /** Virtual item containing item `itemIndex`. */
  toVirtual?: (itemIndex: number) => number
  /** Scrolls this item into view whenever it changes (no-op if it's visible or negative). */
  scrollToIndex?: number
  /** Called with the first and last rendered item index whenever they change. */
  onRangeChange?: (start: number, end: number) => void
}

/** Scroll behaviour shared by the gallery views: relayout anchoring, scrolling to an item and range reporting. */
export function useVirtualAnchor({
  virtualizer,
  layoutKey,
  itemRange: [rangeStart, rangeEnd],
  toItem = identity,
  toVirtual = identity,
  scrollToIndex,
  onRangeChange,
}: VirtualAnchorOptions): void {
  const prevLayoutKey = useRef(layoutKey)
  const anchorItem = useRef(0)
  useLayoutEffect(() => {
    if (prevLayoutKey.current === layoutKey) {
      anchorItem.current = toItem(virtualizer.range?.startIndex ?? 0)
      return
    }
    prevLayoutKey.current = layoutKey
    virtualizer.measure()
    virtualizer.scrollToIndex(toVirtual(anchorItem.current), { align: 'start' })
  })

  useEffect(() => {
    if (scrollToIndex !== undefined && scrollToIndex >= 0) {
      virtualizer.scrollToIndex(toVirtual(scrollToIndex), { align: 'auto' })
    }
  }, [scrollToIndex, virtualizer, toVirtual])

  useEffect(() => {
    if (rangeEnd >= 0) onRangeChange?.(rangeStart, rangeEnd)
  }, [rangeStart, rangeEnd, onRangeChange])
}
