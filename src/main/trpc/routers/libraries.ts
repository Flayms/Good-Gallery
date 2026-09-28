import { basename, isAbsolute, resolve } from 'node:path'
import { TRPCError } from '@trpc/server'
import { and, asc, count, eq, ne, sql } from 'drizzle-orm'
import { z } from 'zod'
import { libraryRoots, media } from '../../db/schema'
import { isReachableDirectory } from '../../reachability'
import { publicProcedure, router } from '../trpc'

const idInput = z.object({ id: z.int().positive() })

export const librariesRouter = router({
  list: publicProcedure.query(({ ctx }) => ctx.db.select().from(libraryRoots).orderBy(asc(libraryRoots.label)).all()),

  /** Folders that directly contain media, with their direct media counts (`/`-separated, no trailing `/`). */
  folders: publicProcedure.input(idInput).query(({ ctx, input }) =>
    ctx.db
      .select({ path: sql<string>`rtrim(${media.dir}, '/')`, count: count() })
      .from(media)
      .where(and(eq(media.rootId, input.id), ne(media.dir, '')))
      .groupBy(media.dir)
      .orderBy(asc(media.dir))
      .all(),
  ),

  add: publicProcedure
    .input(
      z.object({
        path: z.string().trim().min(1).max(1024),
        label: z.string().trim().min(1).max(100).optional(),
      }),
    )
    .mutation(async ({ ctx, input }) => {
      if (!isAbsolute(input.path)) throw new TRPCError({ code: 'BAD_REQUEST', message: 'Path must be absolute' })
      const path = resolve(input.path)
      if (!(await isReachableDirectory(path))) {
        throw new TRPCError({ code: 'BAD_REQUEST', message: 'Folder not found or not reachable' })
      }

      // Windows paths are case-insensitive.
      const existing = ctx.db
        .select({ id: libraryRoots.id })
        .from(libraryRoots)
        .where(sql`lower(${libraryRoots.path}) = lower(${path})`)
        .get()
      if (existing) throw new TRPCError({ code: 'CONFLICT', message: 'Library already added' })

      const root = ctx.db
        .insert(libraryRoots)
        .values({ path, label: input.label ?? (basename(path) || path), status: 'online' })
        .returning()
        .get()
      ctx.indexer.requestScan(root.id)
      return root
    }),

  remove: publicProcedure.input(idInput).mutation(({ ctx, input }) => {
    const removed = ctx.db
      .delete(libraryRoots)
      .where(eq(libraryRoots.id, input.id))
      .returning({ id: libraryRoots.id })
      .get()
    if (!removed) throw new TRPCError({ code: 'NOT_FOUND', message: 'Library not found' })
    ctx.indexer.cancelScan(input.id)
  }),

  rescan: publicProcedure.input(idInput).mutation(({ ctx, input }) => {
    const root = ctx.db.select({ id: libraryRoots.id }).from(libraryRoots).where(eq(libraryRoots.id, input.id)).get()
    if (!root) throw new TRPCError({ code: 'NOT_FOUND', message: 'Library not found' })
    ctx.indexer.requestScan(root.id)
  }),
})
