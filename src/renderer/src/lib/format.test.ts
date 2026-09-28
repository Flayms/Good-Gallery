import { describe, expect, it } from 'vitest'
import { formatDuration } from './format'

describe('formatDuration', () => {
  it.each([
    [0, '0:00'],
    [9.6, '0:10'],
    [75, '1:15'],
    [3600, '1:00:00'],
    [3725, '1:02:05'],
  ])('%s s → %s', (seconds, expected) => {
    expect(formatDuration(seconds)).toBe(expected)
  })
})
