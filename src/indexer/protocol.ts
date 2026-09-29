import { z } from 'zod'

// Messages between the main process and the indexer utilityProcess, validated on receipt.

const rootId = z.int().positive()
const mediaId = z.int().positive()
const requestId = z.int().nonnegative()

const indexerConfig = z.object({
  thumbCacheBytes: z.int().positive(),
  concurrency: z.int().min(1).max(32),
})

export type IndexerConfig = z.infer<typeof indexerConfig>

/** Upper bound of paths per `changes` scan; more changes than that are cheaper to find with a quick scan. */
export const MAX_CHANGED_PATHS = 5000

/** Files or folders relative to the root, `/`-separated. */
const relPaths = z.array(z.string().min(1).max(4096)).min(1).max(MAX_CHANGED_PATHS)

export const scanScope = z.discriminatedUnion('mode', [
  /** Lists every folder and stats every file. */
  z.object({ mode: z.literal('full') }),
  /** Only re-lists folders whose mtime changed since they were last listed; falls back to `full` without folder data. */
  z.object({ mode: z.literal('quick') }),
  /**
   * Changed paths reported by the watcher: re-lists their folders, stats new files and the changed ones, and walks
   * new subfolders.
   */
  z.object({ mode: z.literal('changes'), paths: relPaths }),
])

export type ScanScope = z.infer<typeof scanScope>

export const indexerRequest = z.discriminatedUnion('type', [
  indexerConfig.extend({
    type: z.literal('init'),
    dbPath: z.string().min(1),
    thumbDir: z.string().min(1),
  }),
  /** Applies changed settings to the running process. */
  indexerConfig.extend({ type: z.literal('configure') }),
  z.object({ type: z.literal('scan'), rootId, scope: scanScope }),
  /** The root was removed: cancels its scan and stops watching it. */
  z.object({ type: z.literal('remove'), rootId }),
  z.object({ type: z.literal('thumbnail'), mediaId }),
  /** Reports (`usage`) or empties (`clear`) the thumbnail cache; answered with its size afterwards. */
  z.object({ type: z.literal('cache'), requestId, action: z.enum(['usage', 'clear']) }),
])

export type IndexerRequest = z.infer<typeof indexerRequest>

const scanProgress = z.object({
  rootId,
  /** Files seen so far. */
  scanned: z.int().nonnegative(),
  /** New or changed files written to the index so far. */
  indexed: z.int().nonnegative(),
})

export type ScanProgress = z.infer<typeof scanProgress>

const scanOutcome = z.enum(['completed', 'offline', 'cancelled', 'failed'])

export type ScanOutcome = z.infer<typeof scanOutcome>

export const indexerEvent = z.discriminatedUnion('type', [
  scanProgress.extend({ type: z.literal('progress') }),
  z.object({ type: z.literal('done'), rootId, outcome: scanOutcome, error: z.string().optional() }),
  /** Paths changed in a watched root; without `paths` the changes are unknown (events were lost). */
  z.object({ type: z.literal('changed'), rootId, paths: relPaths.optional() }),
  /** `ok` means the thumbnail files are in the cache. */
  z.object({ type: z.literal('thumbnail'), mediaId, ok: z.boolean() }),
  /** `bytes` is undefined if the request failed. */
  z.object({ type: z.literal('cache'), requestId, bytes: z.int().nonnegative().optional() }),
])

export type IndexerEvent = z.infer<typeof indexerEvent>
