import { useEffect, useState } from 'react'

export const GALLERY_VIEWS = ['masonry', 'justified', 'grid', 'list'] as const
export type GalleryView = (typeof GALLERY_VIEWS)[number]

export const VIEW_LABELS: Record<GalleryView, string> = {
  masonry: 'Masonry',
  justified: 'Justified',
  grid: 'Grid',
  list: 'List',
}

const DEFAULT_VIEW: GalleryView = 'masonry'
const STORAGE_KEY = 'gallery.view'

export function isGalleryView(value: unknown): value is GalleryView {
  return GALLERY_VIEWS.some((view) => view === value)
}

/** Layout of the gallery, persisted across sessions. */
export function useGalleryView(): [GalleryView, (view: GalleryView) => void] {
  const [view, setView] = useState<GalleryView>(() => {
    const stored = localStorage.getItem(STORAGE_KEY)
    return isGalleryView(stored) ? stored : DEFAULT_VIEW
  })
  useEffect(() => localStorage.setItem(STORAGE_KEY, view), [view])
  return [view, setView]
}
