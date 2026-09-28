import { describe, expect, it } from 'vitest'
import { mediaUrl, parseMediaUrl, parseThumbUrl, pickThumbWidth, thumbUrl } from './media-urls'

describe('media URLs', () => {
  it('round-trips ids and widths', () => {
    expect(parseThumbUrl(thumbUrl(42, 800))).toEqual({ id: 42, width: 800 })
    expect(parseMediaUrl(mediaUrl(7))).toEqual({ id: 7 })
  })

  it.each([
    'gg-thumb://media/42/500',
    'gg-thumb://media/0/400',
    'gg-thumb://media/01/400',
    'gg-thumb://media/42/400/x',
    'gg-thumb://media/42/400?x=1',
    'gg-thumb://other/42/400',
    'gg-thumb://media/..%2F..%2Fetc/400',
    'gg-media://media/42/400',
    'not a url',
  ])('rejects thumbnail URL %s', (url) => {
    expect(parseThumbUrl(url)).toBeUndefined()
  })

  it.each(['gg-media://media/abc', 'gg-media://media/1/2', 'gg-thumb://media/1', 'gg-media://media/'])(
    'rejects media URL %s',
    (url) => {
      expect(parseMediaUrl(url)).toBeUndefined()
    },
  )

  it('picks the smallest thumbnail covering the physical width', () => {
    expect(pickThumbWidth(300, 1)).toBe(400)
    expect(pickThumbWidth(300, 1.5)).toBe(800)
    expect(pickThumbWidth(900, 2)).toBe(800)
  })
})
