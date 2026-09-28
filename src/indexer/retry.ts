import { setTimeout as delay } from 'node:timers/promises'

// Errors that retrying won't fix; anything else with an errno code (EBUSY, ETIMEDOUT,
// ECONNRESET, UNKNOWN, …) is treated as a transient network hiccup.
const PERMANENT_CODES = new Set(['ENOENT', 'ENOTDIR', 'EISDIR', 'EACCES', 'EPERM', 'EINVAL', 'ENAMETOOLONG'])

export function errorCode(error: unknown): string | undefined {
  if (typeof error !== 'object' || error === null || !('code' in error)) return undefined
  return typeof error.code === 'string' ? error.code : undefined
}

export function isTransient(error: unknown): boolean {
  const code = errorCode(error)
  return code !== undefined && !PERMANENT_CODES.has(code)
}

export interface RetryOptions {
  retries?: number
  baseDelayMs?: number
  signal?: AbortSignal
}

/** Retries transient I/O errors with exponential backoff. */
export async function withRetry<T>(
  fn: () => Promise<T>,
  { retries = 3, baseDelayMs = 250, signal }: RetryOptions = {},
): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    try {
      return await fn()
    } catch (error) {
      if (attempt >= retries || !isTransient(error)) throw error
      await delay(baseDelayMs * 2 ** attempt, undefined, { signal })
    }
  }
}
