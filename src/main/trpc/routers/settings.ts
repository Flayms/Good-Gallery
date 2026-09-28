import { settingsSchema } from '@shared/settings'
import { publicProcedure, router } from '../trpc'

export const settingsRouter = router({
  get: publicProcedure.query(({ ctx }) => ctx.settings.get()),

  update: publicProcedure.input(settingsSchema.partial()).mutation(({ ctx, input }) => ctx.settings.update(input)),

  /** Size of the thumbnail cache in bytes. */
  cacheUsage: publicProcedure.query(({ ctx }) => ctx.indexer.thumbnailCache('usage')),

  /** Empties the thumbnail cache; thumbnails are rendered again on demand. Returns the remaining size in bytes. */
  clearCache: publicProcedure.mutation(({ ctx }) => ctx.indexer.thumbnailCache('clear')),
})
