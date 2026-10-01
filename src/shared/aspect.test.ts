import { describe, expect, it } from 'vitest'
import { tileAspect } from './aspect'

describe('tileAspect', () => {
  it('is height / width', () => {
    expect(tileAspect(4000, 3000)).toBe(0.75)
  })

  it('falls back to a square when dimensions are unknown', () => {
    expect(tileAspect(null, null)).toBe(1)
    expect(tileAspect(0, 100)).toBe(1)
  })

  it('clamps extreme aspect ratios', () => {
    expect(tileAspect(100, 10_000)).toBe(3)
    expect(tileAspect(10_000, 100)).toBe(0.3)
  })
})
