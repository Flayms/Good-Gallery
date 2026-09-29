import { z } from 'zod'

export const settingsSchema = z.object({
  /** Size cap of the thumbnail cache. */
  thumbCacheGiB: z.number().min(0.5).max(1024),
  /** Quick scan of all libraries for changes the watchers missed; `0` checks only at startup. */
  quickCheckIntervalMinutes: z
    .int()
    .min(0)
    .max(7 * 24 * 60),
  /** Parallel file reads (scanning, metadata, thumbnails). */
  ioConcurrency: z.int().min(1).max(16),
})

export type Settings = z.infer<typeof settingsSchema>

export const DEFAULT_SETTINGS: Settings = { thumbCacheGiB: 5, quickCheckIntervalMinutes: 60, ioConcurrency: 4 }
