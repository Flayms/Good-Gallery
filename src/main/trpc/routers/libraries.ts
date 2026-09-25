import { stat } from 'node:fs/promises'
import { basename, isAbsolute, resolve } from 'node:path'
import { TRPCError } from '@trpc/server'
import { asc, eq, sql } from 'drizzle-orm'
import { z } from 'zod'
import { libraryRoots } from '../../db/schema'
import { publicProcedure, router } from '../trpc'

const STAT_TIMEOUT_MS = 5_000

const idInput = z.object({ id: z.int().positive() })

/** Resolves false instead of hanging when a network share is unreachable. */
function isReachableDirectory(path: string): Promise<boolean> {
  return new Promise((done) => {
    const timer = setTimeout(() => done(false), STAT_TIMEOUT_MS)
    stat(path).then(
      (stats) => {
        clearTimeout(timer)
        done(stats.isDirectory())
      },
      () => {
        clearTimeout(timer)
        done(false)
      },
    )
  })
}

export const librariesRouter = router({
  list: publicProcedure.query(({ ctx }) => ctx.db.select().from(libraryRoots).orderBy(asc(libraryRoots.label)).all()),

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
