import { stat } from 'node:fs/promises'

const STAT_TIMEOUT_MS = 5_000

/** Resolves false instead of hanging when a network share is unreachable. */
export function isReachableDirectory(path: string, timeoutMs = STAT_TIMEOUT_MS): Promise<boolean> {
  return new Promise((done) => {
    const timer = setTimeout(() => done(false), timeoutMs)
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
