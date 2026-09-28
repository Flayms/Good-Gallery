import { z } from 'zod'

// Gallery state lives in the URL, so it survives reloads and opening/closing the viewer.

export const MEDIA_KINDS = ['image', 'video'] as const
export type MediaKind = (typeof MEDIA_KINDS)[number]

export const SORTS = ['date-desc', 'date-asc', 'name-asc'] as const
export type Sort = (typeof SORTS)[number]

export const SORT_LABELS: Record<Sort, string> = {
  'date-desc': 'Newest first',
  'date-asc': 'Oldest first',
  'name-asc': 'Name',
}

export const gallerySearch = z.object({
  root: z.int().positive().optional(),
  kind: z.enum(MEDIA_KINDS).optional(),
  sort: z.enum(SORTS).optional(),
})

export type GallerySearch = z.infer<typeof gallerySearch>

export function isMediaKind(value: string): value is MediaKind {
  return MEDIA_KINDS.some((kind) => kind === value)
}

export function isSort(value: string): value is Sort {
  return SORTS.some((sort) => sort === value)
}
