/** Display form of a tag (`tags.name`): Unicode-normalized, whitespace collapsed. */
export function cleanTag(tag: string): string {
  return tag.normalize('NFKC').trim().replace(/\s+/g, ' ')
}

/** Canonical form used for tag lookup and uniqueness (`tags.name_norm`). */
export function normalizeTag(tag: string): string {
  return cleanTag(tag).toLowerCase()
}

export const TAG_CATEGORIES = ['people', 'places'] as const
export type TagCategory = (typeof TAG_CATEGORIES)[number]

/** Display name the indexer uses as the top-level tag for face-region names. */
export const PEOPLE_TAG_ROOT = 'People'

/**
 * Top-level tag names (normalized) recognized as a category, regardless of which app wrote the hierarchy.
 * A tag belongs to a category when one of its ancestors (or itself) has one of these names.
 */
export const TAG_CATEGORY_ROOTS: Record<TagCategory, readonly string[]> = {
  people: ['people', 'person', 'persons', 'faces'],
  places: ['places', 'place', 'locations', 'location'],
}
