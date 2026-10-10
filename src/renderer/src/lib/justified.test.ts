import { describe, expect, it } from 'vitest'
import { justifiedRows, rowItemWidths, rowOfItem } from './justified'

const aspects = (values: number[]) => (index: number) => values[index] ?? 1

describe('justifiedRows', () => {
  it('fills rows to the container width and keeps the last row at the target height', () => {
    // Landscape 4:3 at 150px is 200px wide: four fit into 840px with gaps, the fifth overflows the row.
    const aspectAt = aspects([0.75, 0.75, 0.75, 0.75, 0.75, 0.75])
    const rows = justifiedRows(6, aspectAt, 840, 150, 10)

    expect(rows).toEqual([
      { start: 0, end: 5, height: 120, full: true },
      { start: 5, end: 6, height: 150, full: false },
    ])
    for (const row of rows.filter((row) => row.full)) {
      const widths = rowItemWidths(row, aspectAt, 840, 10)
      expect(widths.reduce((sum, width) => sum + width, 0) + 10 * (widths.length - 1)).toBe(840)
    }
  })

  it('gives an over-wide panorama a row of its own', () => {
    const rows = justifiedRows(2, aspects([0.3, 1]), 400, 200, 8)

    expect(rows[0]).toEqual({ start: 0, end: 1, height: 120, full: true })
    expect(rows[1]).toMatchObject({ start: 1, end: 2, full: false })
  })

  it('has no rows without items or width', () => {
    expect(justifiedRows(0, aspects([]), 800, 200, 8)).toEqual([])
    expect(justifiedRows(3, aspects([1, 1, 1]), 0, 200, 8)).toEqual([])
  })
})

describe('rowOfItem', () => {
  it('finds the row containing an item', () => {
    const rows = justifiedRows(10, () => 1, 300, 100, 0)

    expect(rows.map((row) => row.start)).toEqual([0, 3, 6, 9])
    expect([0, 2, 3, 8, 9].map((index) => rowOfItem(rows, index))).toEqual([0, 0, 1, 2, 3])
  })
})
