import { describe, expect, it } from 'vitest'
import { clampView, FIT, MAX_SCALE, zoomTo } from './zoom'

const viewport = { width: 1000, height: 800 }
const content = { width: 1000, height: 500 }

describe('zoomTo', () => {
  it('keeps the point under the cursor in place', () => {
    const point = { x: 200, y: 100 }
    const view = zoomTo(FIT, 2, point, content, viewport)

    // Content point under the cursor before: (point - x) / scale = (200, 100); after: x + 2 * c = point.
    expect(view).toEqual({ scale: 2, x: -200, y: -100 })
  })

  it('limits the scale', () => {
    expect(zoomTo(FIT, 100, { x: 0, y: 0 }, content, viewport).scale).toBe(MAX_SCALE)
    expect(zoomTo({ scale: 2, x: 300, y: 0 }, 0.5, { x: 0, y: 0 }, content, viewport)).toEqual(FIT)
  })
})

describe('clampView', () => {
  it('keeps zoomed content covering the viewport', () => {
    // At 2x the content is 2000×1000: 500 px of horizontal and 100 px of vertical slack on each side.
    expect(clampView({ scale: 2, x: 900, y: -900 }, content, viewport)).toEqual({ scale: 2, x: 500, y: -100 })
  })

  it('centers content that is smaller than the viewport', () => {
    expect(clampView({ scale: 1.5, x: 0, y: 50 }, content, viewport)).toEqual({ scale: 1.5, x: 0, y: 0 })
  })
})
