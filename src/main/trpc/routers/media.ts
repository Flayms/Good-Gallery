import { TRPCError } from '@trpc/server'
import { and, asc, desc, eq, type SQL, sql } from 'drizzle-orm'
import { z } from 'zod'
import { media } from '../../db/schema'
import { publicProcedure, router } from '../trpc'

const searchInput = z.object({
  rootId: z.int().positive().optional(),
  kind: z.enum(['image', 'video']).optional(),
  sort: z.enum(['date-desc', 'date-asc', 'name-asc']).default('date-desc'),
  limit: z.int().min(1).max(500).default(200),
  cursor: z.object({ value: z.union([z.number(), z.string()]), id: z.int() }).nullish(),
})

export type MediaCursor = NonNullable<z.infer<typeof searchInput>['cursor']>

export const mediaRouter = router({
  byId: publicProcedure.input(z.object({ id: z.int().positive() })).query(({ ctx, input }) => {
    const item = ctx.db
      .select({
        id: media.id,
        rootId: media.rootId,
        relPath: media.relPath,
        fileName: media.fileName,
        kind: media.kind,
        size: media.size,
        width: media.width,
        height: media.height,
        duration: media.duration,
        takenAt: media.takenAt,
        mtime: media.mtime,
        thumbhash: media.thumbhash,
      })
      .from(media)
      .where(eq(media.id, input.id))
      .get()
    if (!item) throw new TRPCError({ code: 'NOT_FOUND', message: 'Media not found' })
    return item
  }),

  search: publicProcedure.input(searchInput).query(({ ctx, input }) => {
    const sortColumn = input.sort === 'name-asc' ? media.fileName : media.sortDate
    const descending = input.sort === 'date-desc'

    const filters: (SQL | undefined)[] = [
      input.rootId === undefined ? undefined : eq(media.rootId, input.rootId),
      input.kind === undefined ? undefined : eq(media.kind, input.kind),
    ]
    if (input.cursor) {
      const { value, id } = input.cursor
      filters.push(
        descending
          ? sql`(${sortColumn}, ${media.id}) < (${value}, ${id})`
          : sql`(${sortColumn}, ${media.id}) > (${value}, ${id})`,
      )
    }

    const order = descending ? desc : asc
    const rows = ctx.db
      .select({
        id: media.id,
        rootId: media.rootId,
        fileName: media.fileName,
        kind: media.kind,
        width: media.width,
        height: media.height,
        duration: media.duration,
        takenAt: media.takenAt,
        mtime: media.mtime,
        sortDate: media.sortDate,
        thumbhash: media.thumbhash,
        thumbStatus: media.thumbStatus,
      })
      .from(media)
      .where(and(...filters))
      .orderBy(order(sortColumn), order(media.id))
      .limit(input.limit + 1)
      .all()

    const items = rows.slice(0, input.limit)
    const last = items.at(-1)
    const nextCursor: MediaCursor | null =
      rows.length > input.limit && last
        ? { value: input.sort === 'name-asc' ? last.fileName : last.sortDate, id: last.id }
        : null

    return { items, nextCursor }
  }),
})
