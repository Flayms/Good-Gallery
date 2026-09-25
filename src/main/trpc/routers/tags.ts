import { normalizeTag } from '@shared/tags'
import { and, asc, count, desc, eq, gte, lt } from 'drizzle-orm'
import { z } from 'zod'
import { mediaTags, tags } from '../../db/schema'
import { publicProcedure, router } from '../trpc'

export const tagsRouter = router({
  autocomplete: publicProcedure
    .input(
      z.object({
        prefix: z.string().max(200).default(''),
        limit: z.int().min(1).max(50).default(20),
      }),
    )
    .query(({ ctx, input }) => {
      const prefix = normalizeTag(input.prefix)
      const usage = count(mediaTags.mediaId)
      return (
        ctx.db
          .select({ id: tags.id, name: tags.name, count: usage })
          .from(tags)
          .leftJoin(mediaTags, eq(mediaTags.tagId, tags.id))
          // Range scan on the unique name_norm index instead of LIKE, which can't use it.
          .where(prefix ? and(gte(tags.nameNorm, prefix), lt(tags.nameNorm, `${prefix}\u{10FFFF}`)) : undefined)
          .groupBy(tags.id)
          .orderBy(desc(usage), asc(tags.nameNorm))
          .limit(input.limit)
          .all()
      )
    }),
})
