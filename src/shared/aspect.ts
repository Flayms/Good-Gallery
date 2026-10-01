// Clamp extreme panoramas / tall strips so a single tile can't dominate a column.
const MIN_ASPECT = 0.3
const MAX_ASPECT = 3

/** `layout` sends aspects as integers of this scale to keep the payload small. */
export const ASPECT_SCALE = 1000

/** Tile height / width, clamped; a square when the dimensions are unknown. */
export function tileAspect(width: number | null, height: number | null): number {
  if (!width || !height) return 1
  return Math.min(MAX_ASPECT, Math.max(MIN_ASPECT, height / width))
}
