import { and, inArray, isNull, sql } from 'drizzle-orm'
import { TAG_CATEGORIES, TAG_CATEGORY_ROOTS, type TagCategory } from '../../shared/tags'
import type { Db } from '.'
import { tags } from './schema'

export interface CategoryTag {
  id: number
  name: string
  /** `null` for tags directly under the category root; the root itself isn't a real tag. */
  parentId: number | null
  /** Media tagged with this tag or one of its descendants. */
  count: number
}

/** Ids of the category's root tag(s) and every descendant, regardless of which app named the root. */
function categoryTagIds(db: Db, category: TagCategory): number[] {
  const rootNames = [...TAG_CATEGORY_ROOTS[category]]
  const rows = db.all<{ id: number }>(sql`
    WITH RECURSIVE roots(id) AS (
      SELECT ${tags.id} FROM ${tags} WHERE ${and(isNull(tags.parentId), inArray(tags.nameNorm, rootNames))}
    ),
    descendants(id) AS (
      SELECT ${tags.id} FROM ${tags} WHERE ${tags.parentId} IN (SELECT id FROM roots)
      UNION
      SELECT ${tags.id} FROM ${tags} JOIN descendants ON ${tags.parentId} = descendants.id
    )
    SELECT id FROM roots
    UNION
    SELECT id FROM descendants`)
  return rows.map((row) => row.id)
}

/** Every tag of a category (excluding the root itself), most used first, with counts including descendants. */
export function categoryTags(db: Db, category: TagCategory): CategoryTag[] {
  const rootNames = [...TAG_CATEGORY_ROOTS[category]]
  return db.all<CategoryTag>(sql`
    WITH RECURSIVE roots(id) AS (
      SELECT ${tags.id} FROM ${tags} WHERE ${and(isNull(tags.parentId), inArray(tags.nameNorm, rootNames))}
    ),
    descendants(id) AS (
      SELECT ${tags.id} FROM ${tags} WHERE ${tags.parentId} IN (SELECT id FROM roots)
      UNION
      SELECT ${tags.id} FROM ${tags} JOIN descendants ON ${tags.parentId} = descendants.id
    ),
    tree(root, id) AS (
      SELECT id, id FROM descendants
      UNION
      SELECT tree.root, ${tags.id} FROM ${tags} JOIN tree ON ${tags.parentId} = tree.id
    )
    SELECT t.id AS id, t.name AS name,
      CASE WHEN t.parent_id IN (SELECT id FROM roots) THEN NULL ELSE t.parent_id END AS parentId,
      sum(d.media_count) AS count
    FROM tree
    JOIN ${tags} AS d ON d.id = tree.id
    JOIN ${tags} AS t ON t.id = tree.root
    GROUP BY tree.root
    -- Unused tags linger until the indexer prunes them after its next scan.
    HAVING count > 0
    ORDER BY count DESC, t.name_norm`)
}

/** Ids of every tag that belongs to a category (any root and its descendants). */
export function categorizedTagIds(db: Db): number[] {
  return TAG_CATEGORIES.flatMap((category) => categoryTagIds(db, category))
}

/**
 * The category of each given tag id, keyed by id: a tag belongs to a category when its outermost ancestor
 * (or itself) is one of that category's root names. Tags outside any category are omitted.
 */
export function categoriesOf(db: Db, tagIds: number[]): Map<number, TagCategory> {
  const result = new Map<number, TagCategory>()
  if (tagIds.length === 0) return result
  const rootNameOf: Record<string, TagCategory> = Object.fromEntries(
    TAG_CATEGORIES.flatMap((category) => TAG_CATEGORY_ROOTS[category].map((name) => [name, category])),
  )
  const rows = db.all<{ id: number; rootName: string }>(sql`
    WITH RECURSIVE ancestors(start, id) AS (
      SELECT ${tags.id}, ${tags.id} FROM ${tags} WHERE ${inArray(tags.id, tagIds)}
      UNION
      SELECT ancestors.start, t.parent_id FROM ${tags} AS t
        JOIN ancestors ON t.id = ancestors.id WHERE t.parent_id IS NOT NULL
    )
    SELECT ancestors.start AS id, t.name_norm AS rootName
    FROM ancestors JOIN ${tags} AS t ON t.id = ancestors.id
    WHERE t.parent_id IS NULL`)
  for (const { id, rootName } of rows) {
    const category = rootNameOf[rootName]
    if (category) result.set(id, category)
  }
  return result
}
