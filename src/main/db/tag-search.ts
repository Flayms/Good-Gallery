import { normalizeTag } from '@shared/tags'
import { and, eq, exists, inArray, notExists, type SQL, sql } from 'drizzle-orm'
import type { Db } from '.'
import { media, mediaTags, tags } from './schema'

/**
 * Query shapes by (estimated) matches of an included tag, measured on 500k synthetic media
 * (2k tags, ~1M links; p95 11 ms for a common tag, 0.8 ms for a rare one, 36 ms worst case):
 * - up to `DRIVING_MAX`, the rarest tag drives: its media are looked up by id and sorted (~3 µs per match);
 * - otherwise the sort index is walked and rows are checked against each tag's materialized media ids
 *   (~0.2 µs per id to build, cheap per row);
 * - beyond `MATERIALIZE_MAX`, building the list costs more than a primary key probe per row, and such a
 *   common tag matches often enough that few rows are walked.
 */
const DRIVING_MAX = 5000
const MATERIALIZE_MAX = 100_000

interface Subtree {
  /** The tag and all its descendants. */
  ids: number[]
  /** Upper bound of matching media: sum of the direct counts. */
  estimate: number
}

function normalizeTags(names: string[]): string[] {
  return [...new Set(names.map(normalizeTag))].filter(Boolean)
}

/** Subtrees of the given tags (normalized names), keyed by name. Unknown tags are missing. */
function subtrees(db: Db, names: string[]): Map<string, Subtree> {
  const result = new Map<string, Subtree>()
  if (names.length === 0) return result

  // UNION (not UNION ALL) also terminates on parent cycles.
  const rows = db.all<{ name: string; id: number; count: number }>(sql`
    WITH RECURSIVE tree(name, id) AS (
      SELECT ${tags.nameNorm}, ${tags.id} FROM ${tags} WHERE ${inArray(tags.nameNorm, names)}
      UNION
      SELECT tree.name, ${tags.id} FROM ${tags} JOIN tree ON ${tags.parentId} = tree.id
    )
    SELECT tree.name, tree.id, ${tags.mediaCount} AS count FROM tree JOIN ${tags} ON ${tags.id} = tree.id`)
  for (const { name, id, count } of rows) {
    const subtree = result.get(name)
    if (subtree) {
      subtree.ids.push(id)
      subtree.estimate += count
    } else {
      result.set(name, { ids: [id], estimate: count })
    }
  }
  return result
}

function taggedMedia(db: Db, tagIds: number[]) {
  return db.select({ id: mediaTags.mediaId }).from(mediaTags).where(inArray(mediaTags.tagId, tagIds))
}

/** Correlated check via the primary key, per candidate row. */
function linksTo(db: Db, tagIds: number[]) {
  // Unary `+` pins the plan: `+media.id` stops SQLite from rewriting EXISTS into an id lookup, `+tag_id` from
  // probing once per id of a large subtree; instead it reads the few tags of each media and checks the list.
  return db
    .select({ one: sql`1` })
    .from(mediaTags)
    .where(and(eq(mediaTags.mediaId, sql`+${media.id}`), inArray(sql`+${mediaTags.tagId}`, tagIds)))
}

/**
 * Conditions on `media` for including all of `include` and none of `exclude` (tag names, matching descendants),
 * or `null` if an included tag doesn't exist, so nothing can match.
 */
export function tagConditions(db: Db, include: string[], exclude: string[]): SQL[] | null {
  const includeNames = normalizeTags(include)
  const included = subtrees(db, includeNames)
  if (included.size < includeNames.length) return null

  const [driving, ...rest] = [...included.values()].sort((a, b) => a.estimate - b.estimate)
  const conditions: SQL[] = []
  if (driving && driving.estimate <= DRIVING_MAX) {
    conditions.push(inArray(media.id, taggedMedia(db, driving.ids)))
    for (const subtree of rest) conditions.push(exists(linksTo(db, subtree.ids)))
  } else if (driving) {
    for (const subtree of [driving, ...rest]) {
      conditions.push(
        subtree.estimate <= MATERIALIZE_MAX
          ? // `+media.id` rules out id lookups, so SQLite keeps the sort index and builds the id list once.
            inArray(sql`+${media.id}`, taggedMedia(db, subtree.ids))
          : exists(linksTo(db, subtree.ids)),
      )
    }
  }

  const excluded = [...subtrees(db, normalizeTags(exclude)).values()].flatMap((subtree) => subtree.ids)
  if (excluded.length > 0) conditions.push(notExists(linksTo(db, excluded)))
  return conditions
}
