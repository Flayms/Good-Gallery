import { join } from 'node:path'
import { TRPCError } from '@trpc/server'
import { and, asc, desc, eq, gte, lt, type SQL, sql } from 'drizzle-orm'
import { z } from 'zod'
import { libraryRoots, media, mediaTags, tags } from '../../db/schema'
import { tagConditions } from '../../db/tag-search'
import { publicProcedure, router } from '../trpc'

const idInput = z.object({ id: z.int().positive() })
const tagList = z.array(z.string().max(200)).max(20).default([])

const searchInput = z
  .object({
    rootId: z.int().positive().optional(),
    /** `/`-separated folder below the root; includes subfolders. */
    folder: z.string().min(1).max(1024).optional(),
    kind: z.enum(['image', 'video']).optional(),
    /** Media must carry every tag (or one of its descendants). */
    tags: tagList,
    /** Media must carry none of these tags (nor their descendants). */
    excludeTags: tagList,
    /** Inclusive lower bound of the sort date, ms. */
    from: z.int().optional(),
    /** Exclusive upper bound of the sort date, ms. */
    to: z.int().optional(),
    sort: z.enum(['date-desc', 'date-asc', 'name-asc']).default('date-desc'),
    limit: z.int().min(1).max(500).default(200),
    cursor: z.object({ value: z.union([z.number(), z.string()]), id: z.int() }).nullish(),
  })
  .refine((input) => input.folder === undefined || input.rootId !== undefined, {
    message: 'folder requires rootId',
    path: ['folder'],
  })

export type MediaCursor = NonNullable<z.infer<typeof searchInput>['cursor']>

function absolutePath(rootPath: string, relPath: string): string {
  return join(rootPath, ...relPath.split('/'))
}

export const mediaRouter = router({
  /** Media details for the viewer, with the absolute file path and direct tags. */
  byId: publicProcedure.input(idInput).query(({ ctx, input }) => {
    const item = ctx.db
      .select({
        id: media.id,
        rootId: media.rootId,
        rootLabel: libraryRoots.label,
        rootPath: libraryRoots.path,
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
      .innerJoin(libraryRoots, eq(libraryRoots.id, media.rootId))
      .where(eq(media.id, input.id))
      .get()
    if (!item) throw new TRPCError({ code: 'NOT_FOUND', message: 'Media not found' })
    const itemTags = ctx.db
      .select({ id: tags.id, name: tags.name })
      .from(mediaTags)
      .innerJoin(tags, eq(tags.id, mediaTags.tagId))
      .where(eq(mediaTags.mediaId, input.id))
      .orderBy(asc(tags.nameNorm))
      .all()
    const { rootPath, ...rest } = item
    return { ...rest, path: absolutePath(rootPath, item.relPath), tags: itemTags }
  }),

  showInFolder: publicProcedure.input(idInput).mutation(({ ctx, input }) => {
    const row = ctx.db
      .select({ rootPath: libraryRoots.path, relPath: media.relPath })
      .from(media)
      .innerJoin(libraryRoots, eq(libraryRoots.id, media.rootId))
      .where(eq(media.id, input.id))
      .get()
    if (!row) throw new TRPCError({ code: 'NOT_FOUND', message: 'Media not found' })
    ctx.desktop.showItemInFolder(absolutePath(row.rootPath, row.relPath))
  }),

  search: publicProcedure.input(searchInput).query(({ ctx, input }) => {
    const sortColumn = input.sort === 'name-asc' ? media.fileName : media.sortDate
    const descending = input.sort === 'date-desc'

    const tagFilters = tagConditions(ctx.db, input.tags, input.excludeTags)
    if (!tagFilters) return { items: [], nextCursor: null }

    const folder = input.folder?.replace(/^\/+|\/+$/g, '')
    const filters: (SQL | undefined)[] = [
      input.rootId === undefined ? undefined : eq(media.rootId, input.rootId),
      // `dir` ends with `/`, and `0` is the character after `/`: a range scan over the folder and its subfolders.
      folder ? and(gte(media.dir, `${folder}/`), lt(media.dir, `${folder}0`)) : undefined,
      input.kind === undefined ? undefined : eq(media.kind, input.kind),
      input.from === undefined ? undefined : gte(media.sortDate, input.from),
      input.to === undefined ? undefined : lt(media.sortDate, input.to),
      ...tagFilters,
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
