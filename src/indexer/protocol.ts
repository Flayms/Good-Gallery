import { z } from 'zod'

// Messages between the main process and the indexer utilityProcess, validated on receipt.

const rootId = z.int().positive()
const mediaId = z.int().positive()

export const indexerRequest = z.discriminatedUnion('type', [
  z.object({
    type: z.literal('init'),
    dbPath: z.string().min(1),
    thumbDir: z.string().min(1),
    thumbCacheBytes: z.int().positive(),
    concurrency: z.int().min(1).max(32),
  }),
  z.object({ type: z.literal('scan'), rootId }),
  z.object({ type: z.literal('cancel'), rootId }),
  z.object({ type: z.literal('thumbnail'), mediaId }),
])

export type IndexerRequest = z.infer<typeof indexerRequest>

const scanProgress = z.object({
  rootId,
  /** Files seen so far. */
  scanned: z.int().nonnegative(),
  /** New or changed files written to the index. */
  indexed: z.int().nonnegative(),
})

export type ScanProgress = z.infer<typeof scanProgress>

const scanOutcome = z.enum(['completed', 'offline', 'cancelled', 'failed'])

export type ScanOutcome = z.infer<typeof scanOutcome>

export const indexerEvent = z.discriminatedUnion('type', [
  scanProgress.extend({ type: z.literal('progress') }),
  z.object({ type: z.literal('done'), rootId, outcome: scanOutcome, error: z.string().optional() }),
  /** `ok` means the thumbnail files are in the cache. */
  z.object({ type: z.literal('thumbnail'), mediaId, ok: z.boolean() }),
])

export type IndexerEvent = z.infer<typeof indexerEvent>
