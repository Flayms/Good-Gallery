import { z } from 'zod'

export const settingsSchema = z.object({
  /** Size cap of the thumbnail cache. */
  thumbCacheGiB: z.number().min(0.5).max(1024),
  /** Full rescan of all libraries; `0` scans only at startup and on demand. */
  rescanIntervalMinutes: z
    .int()
    .min(0)
    .max(7 * 24 * 60),
  /** Parallel file reads (scanning, metadata, thumbnails). */
  ioConcurrency: z.int().min(1).max(16),
})

export type Settings = z.infer<typeof settingsSchema>

export const DEFAULT_SETTINGS: Settings = { thumbCacheGiB: 5, rescanIntervalMinutes: 30, ioConcurrency: 4 }
