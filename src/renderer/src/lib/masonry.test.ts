import { describe, expect, it } from 'vitest'
import { columnWidth, tileHeight } from './masonry'

describe('columnWidth', () => {
  it('splits the container minus gaps into whole-pixel columns', () => {
    expect(columnWidth(1000, 4, 8)).toBe(244)
  })

  it('never goes negative', () => {
    expect(columnWidth(10, 4, 8)).toBe(0)
  })
})

describe('tileHeight', () => {
  it('preserves aspect ratio', () => {
    expect(tileHeight(4000, 3000, 200)).toBe(150)
  })

  it('falls back to a square when dimensions are unknown', () => {
    expect(tileHeight(null, null, 200)).toBe(200)
    expect(tileHeight(0, 100, 200)).toBe(200)
  })

  it('clamps extreme aspect ratios', () => {
    expect(tileHeight(100, 10_000, 200)).toBe(600)
    expect(tileHeight(10_000, 100, 200)).toBe(60)
  })
})
