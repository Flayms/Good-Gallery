import { join } from 'node:path'
import { ASPECT_SCALE, tileAspect } from '@shared/aspect'
import { TRPCError } from '@trpc/server'
import { and, asc, count, desc, eq, gte, inArray, isNull, lt, or, type SQL, sql } from 'drizzle-orm'
import { z } from 'zod'
import type { Db } from '../../db'
import { libraryRoots, media, mediaTags, tags } from '../../db/schema'
import { categoriesOf } from '../../db/tag-categories'
import { tagConditions } from '../../db/tag-search'
import { publicProcedure, router } from '../trpc'

const MAX_BY_IDS = 500
const idInput = z.object({ id: z.int().positive() })
const tagList = z.array(z.string().max(200)).max(20).default([])
/** `0` means unrated. */
const ratingList = z.array(z.int().min(0).max(5)).max(6).default([])

export const SORT_FIELDS = ['name', 'taken', 'modified', 'path', 'size'] as const

const searchInput = z
  .object({
    rootId: z.int().positive().optional(),
    /** `/`-separated folder below the root; includes subfolders. */
    folder: z.string().min(1).max(1024).optional(),
    kind: z.enum(['image', 'video']).optional(),
    /** Case-insensitive substring of the file name. */
    name: z.string().trim().max(200).optional(),
    /** Media must carry every tag (or one of its descendants). */
    tags: tagList,
    /** Media must carry none of these tags (nor their descendants). */
    excludeTags: tagList,
    /** Media must have one of these ratings; `0` matches unrated media. Empty means no filter. */
    ratings: ratingList,
    /** Inclusive lower bound of the sort date, ms. */
    from: z.int().optional(),
    /** Exclusive upper bound of the sort date, ms. */
    to: z.int().optional(),
    sortBy: z.enum(SORT_FIELDS).default('taken'),
    dir: z.enum(['asc', 'desc']).default('desc'),
  })
  .refine((input) => input.folder === undefined || input.rootId !== undefined, {
    message: 'folder requires rootId',
    path: ['folder'],
  })

type SearchInput = z.infer<typeof searchInput>

function ratingFilter(ratings: number[]): SQL | undefined {
  if (ratings.length === 0) return undefined
  return or(...ratings.map((rating) => (rating === 0 ? isNull(media.rating) : eq(media.rating, rating))))
}

/** `lower()` on both sides: SQLite's (ASCII-only) folding must match the generated `file_name_lower` column. */
function nameFilter(name: string | undefined): SQL | undefined {
  return name ? sql`instr(${media.fileNameLower}, lower(${name})) > 0` : undefined
}

/** WHERE conditions of a search; a condition that can never match if a tag filter is unsatisfiable. */
function filters(db: Db, input: SearchInput): SQL | undefined {
  const tagFilters = tagConditions(db, input.tags, input.excludeTags)
  if (!tagFilters) return sql`0`
  const folder = input.folder?.replace(/^\/+|\/+$/g, '')
  return and(
    input.rootId === undefined ? undefined : eq(media.rootId, input.rootId),
    // `dir` ends with `/`, and `0` is the character after `/`: a range scan over the folder and its subfolders.
    folder ? and(gte(media.dir, `${folder}/`), lt(media.dir, `${folder}0`)) : undefined,
    input.kind === undefined ? undefined : eq(media.kind, input.kind),
    input.from === undefined ? undefined : gte(media.sortDate, input.from),
    input.to === undefined ? undefined : lt(media.sortDate, input.to),
    ratingFilter(input.ratings),
    nameFilter(input.name),
    ...tagFilters,
  )
}

const SORT_COLUMNS = {
  name: [media.fileNameLower],
  taken: [media.sortDate],
  modified: [media.mtime],
  // Unique, so it needs no id tiebreak.
  path: [media.rootId, media.relPath],
  size: [media.size],
} as const

function sortOrder(input: SearchInput): SQL[] {
  const order = input.dir === 'asc' ? asc : desc
  const columns = SORT_COLUMNS[input.sortBy]
  return [...columns.map((column) => order(column)), ...(input.sortBy === 'path' ? [] : [order(media.id)])]
}

function absolutePath(rootPath: string, relPath: string): string {
  return join(rootPath, ...relPath.split('/'))
}

export const mediaRouter = router({
  /** Media details for the viewer, with the absolute file path and direct tags, grouped by category. */
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
        rating: media.rating,
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
    const categories = categoriesOf(
      ctx.db,
      itemTags.map((tag) => tag.id),
    )
    const { rootPath, ...rest } = item
    return {
      ...rest,
      path: absolutePath(rootPath, item.relPath),
      tags: itemTags.map((tag) => ({ ...tag, category: categories.get(tag.id) })),
    }
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

  /** Ids and aspects of every match in sort order: the gallery lays out the whole result before loading any item. */
  layout: publicProcedure.input(searchInput).query(({ ctx, input }) => {
    const rows = ctx.db
      .select({ id: media.id, width: media.width, height: media.height })
      .from(media)
      .where(filters(ctx.db, input))
      .orderBy(...sortOrder(input))
      .all()
    const ids = new Array<number>(rows.length)
    const aspects = new Array<number>(rows.length)
    rows.forEach((row, i) => {
      ids[i] = row.id
      aspects[i] = Math.round(tileAspect(row.width, row.height) * ASPECT_SCALE)
    })
    return { ids, aspects }
  }),

  /** Number of media per rating (`0` = unrated), across all libraries; ratings without media are left out. */
  ratingCounts: publicProcedure.query(({ ctx }) =>
    ctx.db
      .select({ rating: sql<number>`coalesce(${media.rating}, 0)`, count: count() })
      .from(media)
      .groupBy(media.rating)
      .orderBy(desc(media.rating))
      .all(),
  ),

  /** Tile data for the given ids, in no particular order. */
  byIds: publicProcedure.input(z.object({ ids: z.array(z.int()).max(MAX_BY_IDS) })).query(({ ctx, input }) =>
    ctx.db
      .select({
        id: media.id,
        rootId: media.rootId,
        fileName: media.fileName,
        kind: media.kind,
        duration: media.duration,
        thumbhash: media.thumbhash,
        thumbStatus: media.thumbStatus,
      })
      .from(media)
      .where(inArray(media.id, input.ids))
      .all(),
  ),
})
