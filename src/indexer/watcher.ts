import { watch as fsWatch } from 'node:fs'
import { isIgnoredDir } from './media-types'

export interface Watch {
  close(): void
}

/** Starts a recursive watch; `onChange` gets the changed path relative to the root, or null if it is unknown. */
export type WatchFn = (
  path: string,
  onChange: (filename: string | null) => void,
  onError: (error: unknown) => void,
) => Watch

/** Recursive `fs.watch`: ReadDirectoryChangesW on Windows, which SMB shares support as change notifications. */
export const watchRecursive: WatchFn = (path, onChange, onError) => {
  const watcher = fsWatch(path, { recursive: true, persistent: false }, (_event, filename) => onChange(filename))
  watcher.on('error', onError)
  return watcher
}

// OS files that change without the media changing.
const IGNORED_FILES = new Set(['thumbs.db', 'desktop.ini'])

/** A changed path (relative to the root, any separator) as `/`-separated path; undefined for housekeeping paths. */
export function changedPath(filename: string): string | undefined {
  const parts = filename.split(/[\\/]/).filter(Boolean)
  const name = parts.at(-1)
  // `isIgnoredDir` also covers dotfiles (e.g. AppleDouble files) and changed housekeeping folders themselves.
  if (name === undefined || parts.some(isIgnoredDir) || IGNORED_FILES.has(name.toLowerCase())) return undefined
  return parts.join('/')
}

export interface RootWatcherOptions {
  /** Quiet period before changes are reported, so a copy of many files becomes one report. */
  debounceMs?: number
  /** Longest delay of a report while changes keep coming. */
  maxDelayMs?: number
  /** Changed paths; undefined if the changes are unknown (the event buffer overflowed). */
  onChange: (paths: string[] | undefined) => void
  /** The watch broke (e.g. the share disconnected) and was closed. */
  onError: (error: unknown) => void
  watch?: WatchFn
}

/** Watches a root recursively and reports changed paths, debounced. */
export class RootWatcher {
  readonly #options: Required<Omit<RootWatcherOptions, 'watch'>>
  readonly #watch: Watch
  readonly #paths = new Set<string>()
  #unknown = false
  #timer: NodeJS.Timeout | undefined
  #firstChangeAt = 0

  constructor(rootPath: string, { watch = watchRecursive, ...options }: RootWatcherOptions) {
    this.#options = { debounceMs: 2000, maxDelayMs: 10_000, ...options }
    this.#watch = watch(
      rootPath,
      (filename) => this.#onEvent(filename),
      (error) => {
        this.close()
        this.#options.onError(error)
      },
    )
  }

  close(): void {
    clearTimeout(this.#timer)
    this.#timer = undefined
    this.#watch.close()
  }

  #onEvent(filename: string | null): void {
    if (filename === null) {
      this.#unknown = true
    } else {
      const path = changedPath(filename)
      if (path === undefined) return
      this.#paths.add(path)
    }
    const now = Date.now()
    if (this.#timer === undefined) this.#firstChangeAt = now
    clearTimeout(this.#timer)
    const delay = Math.min(this.#options.debounceMs, this.#firstChangeAt + this.#options.maxDelayMs - now)
    this.#timer = setTimeout(() => this.#report(), Math.max(0, delay))
  }

  #report(): void {
    this.#timer = undefined
    const paths = this.#unknown ? undefined : [...this.#paths]
    this.#paths.clear()
    this.#unknown = false
    this.#options.onChange(paths)
  }
}
