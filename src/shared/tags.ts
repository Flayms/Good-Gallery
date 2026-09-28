/** Display form of a tag (`tags.name`): Unicode-normalized, whitespace collapsed. */
export function cleanTag(tag: string): string {
  return tag.normalize('NFKC').trim().replace(/\s+/g, ' ')
}

/** Canonical form used for tag lookup and uniqueness (`tags.name_norm`). */
export function normalizeTag(tag: string): string {
  return cleanTag(tag).toLowerCase()
}
