import { on } from 'node:events'
import type { IndexerStatus } from '../../indexer'
import { publicProcedure, router } from '../trpc'

export const indexerRouter = router({
  status: publicProcedure.subscription(async function* ({ ctx, signal }) {
    // Subscribe before the first yield so no update is missed in between.
    const updates = on(ctx.indexer, 'status', { signal })
    yield ctx.indexer.status
    for await (const [status] of updates) yield status as IndexerStatus
  }),
})
