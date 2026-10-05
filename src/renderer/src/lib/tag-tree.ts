export interface TagNode {
  id: number
  name: string
  /** Media tagged with this tag or one of its descendants. */
  count: number
  children: TagNode[]
}

/** Builds a tree from a category's flat tag list (`tags.category`), in the order given. */
export function buildTagTree(tags: { id: number; name: string; parentId: number | null; count: number }[]): TagNode[] {
  const nodes = new Map<number, TagNode>(
    tags.map((tag) => [tag.id, { id: tag.id, name: tag.name, count: tag.count, children: [] }]),
  )
  const roots: TagNode[] = []
  for (const tag of tags) {
    const node = nodes.get(tag.id)
    if (!node) continue
    const parent = tag.parentId !== null ? nodes.get(tag.parentId) : undefined
    ;(parent?.children ?? roots).push(node)
  }
  return roots
}
