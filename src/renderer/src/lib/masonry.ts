// Clamp extreme panoramas / tall strips so a single tile can't dominate a column.
const MIN_ASPECT = 0.3
const MAX_ASPECT = 3

export function columnWidth(containerWidth: number, columns: number, gap: number): number {
  return Math.max(0, Math.floor((containerWidth - gap * (columns - 1)) / columns))
}

export function tileHeight(width: number | null, height: number | null, colWidth: number): number {
  if (!width || !height) return colWidth
  const aspect = Math.min(MAX_ASPECT, Math.max(MIN_ASPECT, height / width))
  return Math.round(colWidth * aspect)
}
