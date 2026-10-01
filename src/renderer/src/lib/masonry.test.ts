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
  it('scales the column width by the aspect', () => {
    expect(tileHeight(0.75, 200)).toBe(150)
  })
})
