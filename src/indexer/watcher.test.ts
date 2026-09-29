import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { changedPath, RootWatcher, type WatchFn } from './watcher'

/** Watch double that lets tests emit events and errors. */
function fakeWatch() {
  const handle = {
    change: (_filename: string | null) => {},
    error: (_error: unknown) => {},
    closed: false,
  }
  const watch: WatchFn = (_path, onChange, onError) => {
    handle.change = onChange
    handle.error = onError
    return { close: () => (handle.closed = true) }
  }
  return { watch, handle }
}

function setup() {
  const { watch, handle } = fakeWatch()
  const onChange = vi.fn<(paths: string[] | undefined) => void>()
  const onError = vi.fn<(error: unknown) => void>()
  const watcher = new RootWatcher('/root', { watch, onChange, onError, debounceMs: 100, maxDelayMs: 1000 })
  return { watcher, handle, onChange, onError }
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('changedPath', () => {
  it('normalizes separators and skips housekeeping paths', () => {
    expect(changedPath('trip\\day 1\\IMG_1.jpg')).toBe('trip/day 1/IMG_1.jpg')
    expect(changedPath('trip')).toBe('trip')
    expect(changedPath('@eaDir\\IMG_1.jpg\\SYNOFILE_THUMB_M.jpg')).toBeUndefined()
    expect(changedPath('trip\\._IMG_1.jpg')).toBeUndefined()
    expect(changedPath('trip\\Thumbs.db')).toBeUndefined()
  })
})

describe('RootWatcher', () => {
  it('reports the changed paths once changes settle', () => {
    const { handle, onChange } = setup()

    handle.change('a\\1.jpg')
    vi.advanceTimersByTime(50)
    handle.change('a\\1.jpg')
    handle.change('a\\desktop.ini')
    handle.change('b')
    vi.advanceTimersByTime(99)
    expect(onChange).not.toHaveBeenCalled()

    vi.advanceTimersByTime(1)
    expect(onChange).toHaveBeenCalledExactlyOnceWith(['a/1.jpg', 'b'])
  })

  it('reports within the maximum delay while changes keep coming', () => {
    const { handle, onChange } = setup()

    for (let elapsed = 0; elapsed < 1000; elapsed += 50) {
      handle.change(`a\\${elapsed}.jpg`)
      vi.advanceTimersByTime(50)
    }

    expect(onChange).toHaveBeenCalledOnce()
    expect(onChange.mock.calls[0]?.[0]).toHaveLength(20)
  })

  it('reports unknown changes when events were lost', () => {
    const { handle, onChange } = setup()

    handle.change('a\\1.jpg')
    handle.change(null)
    vi.advanceTimersByTime(100)

    expect(onChange).toHaveBeenCalledExactlyOnceWith(undefined)
  })

  it('closes itself on errors', () => {
    const { handle, onChange, onError } = setup()
    const error = new Error('share disconnected')

    handle.change('a\\1.jpg')
    handle.error(error)
    vi.advanceTimersByTime(1000)

    expect(handle.closed).toBe(true)
    expect(onError).toHaveBeenCalledExactlyOnceWith(error)
    expect(onChange).not.toHaveBeenCalled()
  })
})
