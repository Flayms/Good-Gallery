export function columnWidth(containerWidth: number, columns: number, gap: number): number {
  return Math.max(0, Math.floor((containerWidth - gap * (columns - 1)) / columns))
}

/** Height of a tile in a column of `colWidth`, from its (clamped) height / width aspect. */
export function tileHeight(aspect: number, colWidth: number): number {
  return Math.round(colWidth * aspect)
}
