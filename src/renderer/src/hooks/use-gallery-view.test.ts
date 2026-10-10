import { describe, expect, it } from 'vitest'
import { GALLERY_VIEWS, isGalleryView } from './use-gallery-view'

describe('isGalleryView', () => {
  it('accepts every view', () => {
    expect(GALLERY_VIEWS.every(isGalleryView)).toBe(true)
  })

  it('rejects unknown and missing values', () => {
    expect(isGalleryView('carousel')).toBe(false)
    expect(isGalleryView(null)).toBe(false)
  })
})
