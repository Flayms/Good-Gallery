import { normalizeTag } from '@shared/tags'
import { and, gte, lt, sql } from 'drizzle-orm'
import { z } from 'zod'
import { tags } from '../../db/schema'
import { publicProcedure, router } from '../trpc'

export const tagsRouter = router({
  /** Tags by name prefix, most used first; usage counts include descendants, since a search for a tag matches them. */
  autocomplete: publicProcedure
    .input(
      z.object({
        prefix: z.string().max(200).default(''),
        limit: z.int().min(1).max(50).default(20),
      }),
    )
    .query(({ ctx, input }) => {
      const prefix = normalizeTag(input.prefix)
      // Range scan on the unique name_norm index instead of LIKE, which can't use it.
      const matches = prefix ? and(gte(tags.nameNorm, prefix), lt(tags.nameNorm, `${prefix}\u{10FFFF}`)) : sql`1`
      return ctx.db.all<{ id: number; name: string; count: number }>(sql`
        WITH RECURSIVE tree(root, id) AS (
          SELECT ${tags.id}, ${tags.id} FROM ${tags} WHERE ${matches}
          UNION
          SELECT tree.root, ${tags.id} FROM ${tags} JOIN tree ON ${tags.parentId} = tree.id
        )
        SELECT t.id, t.name, sum(d.media_count) AS count
        FROM tree JOIN ${tags} AS d ON d.id = tree.id JOIN ${tags} AS t ON t.id = tree.root
        GROUP BY tree.root
        -- Unused tags linger until the indexer prunes them after its next scan.
        HAVING count > 0
        ORDER BY count DESC, t.name_norm
        LIMIT ${input.limit}`)
    }),
})
