import { describe, expect, it } from 'vitest'
import { formatBytes, formatDuration } from './format'

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

describe('formatBytes', () => {
  it.each([
    [0, '0 B'],
    [1023, '1023 B'],
    [1536, '1.5 KB'],
    [5 * 1024 ** 3, '5.0 GB'],
    [3 * 1024 ** 5, '3072.0 TB'],
  ])('%s → %s', (bytes, expected) => {
    expect(formatBytes(bytes)).toBe(expected)
  })
})
