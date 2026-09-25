/** Canonical form used for tag lookup and uniqueness (`tags.name_norm`). */
export function normalizeTag(tag: string): string {
  return tag.normalize('NFKC').trim().replace(/\s+/g, ' ').toLowerCase()
}
