/** A row of the justified layout: items `start` (inclusive) to `end` (exclusive), all `height` pixels tall. */
export interface JustifiedRow {
  start: number
  end: number
  height: number
  /** Whether the row spans the whole width; only the last row may not. */
  full: boolean
}

/**
 * Splits the items into rows that fill `containerWidth` exactly, each about `targetHeight` tall: items are added at
 * the target height until the row is full, then the row is scaled down to fit. The last row keeps the target height
 * instead of being stretched. `aspectAt` is height / width.
 */
export function justifiedRows(
  count: number,
  aspectAt: (index: number) => number,
  containerWidth: number,
  targetHeight: number,
  gap: number,
): JustifiedRow[] {
  if (containerWidth <= 0 || targetHeight <= 0) return []
  const rows: JustifiedRow[] = []
  let start = 0
  // Sum of width / height of the items in the current row.
  let ratioSum = 0
  for (let index = 0; index < count; index++) {
    ratioSum += 1 / aspectAt(index)
    const available = containerWidth - gap * (index - start)
    if (ratioSum * targetHeight >= available) {
      rows.push({ start, end: index + 1, height: Math.max(1, Math.round(available / ratioSum)), full: true })
      start = index + 1
      ratioSum = 0
    }
  }
  if (start < count) rows.push({ start, end: count, height: targetHeight, full: false })
  return rows
}

/** Whole-pixel widths of the row's items; a full row's rounding remainder goes to its last item so it ends flush. */
export function rowItemWidths(
  row: JustifiedRow,
  aspectAt: (index: number) => number,
  containerWidth: number,
  gap: number,
): number[] {
  const widths: number[] = []
  for (let index = row.start; index < row.end; index++) widths.push(Math.round(row.height / aspectAt(index)))
  const last = widths.length - 1
  if (row.full && last >= 0) {
    const used = widths.reduce((sum, width) => sum + width, 0) + gap * last
    widths[last] = Math.max(1, (widths[last] ?? 0) + containerWidth - used)
  }
  return widths
}

/** Index of the row containing item `index` (rows are sorted and contiguous). */
export function rowOfItem(rows: readonly JustifiedRow[], index: number): number {
  let low = 0
  let high = rows.length - 1
  while (low < high) {
    const mid = Math.ceil((low + high) / 2)
    if ((rows[mid]?.start ?? 0) <= index) low = mid
    else high = mid - 1
  }
  return low
}
