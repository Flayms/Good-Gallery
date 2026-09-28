import { normalizeTag } from '@shared/tags'
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
  /** `/`-separated folder below `root`, including subfolders. */
  folder: z.string().min(1).optional(),
  kind: z.enum(MEDIA_KINDS).optional(),
  sort: z.enum(SORTS).optional(),
  /** Tag display names; media must carry all of them. */
  tags: z.array(z.string().min(1)).optional(),
  /** Tag display names; media must carry none of them. */
  exclude: z.array(z.string().min(1)).optional(),
  /** Local calendar days (`YYYY-MM-DD`), both inclusive. */
  from: z.iso.date().optional(),
  to: z.iso.date().optional(),
})

export type GallerySearch = z.infer<typeof gallerySearch>

export function isMediaKind(value: string): value is MediaKind {
  return MEDIA_KINDS.some((kind) => kind === value)
}

export function isSort(value: string): value is Sort {
  return SORTS.some((sort) => sort === value)
}

function startOfDay(day: string, offsetDays = 0): number {
  const date = new Date(`${day}T00:00`)
  date.setDate(date.getDate() + offsetDays)
  return date.getTime()
}

/** Input of `media.search` for the gallery state. */
export function mediaSearchInput(search: GallerySearch) {
  return {
    rootId: search.root,
    folder: search.root === undefined ? undefined : search.folder,
    kind: search.kind,
    sort: search.sort,
    tags: search.tags ?? [],
    excludeTags: search.exclude ?? [],
    from: search.from === undefined ? undefined : startOfDay(search.from),
    to: search.to === undefined ? undefined : startOfDay(search.to, 1),
  }
}

export type TagMode = 'include' | 'exclude'

function nonEmpty(names: string[]): string[] | undefined {
  return names.length > 0 ? names : undefined
}

/** Search patch that removes a tag from both lists (compared by normalized name). */
export function withoutTag(search: GallerySearch, name: string): Pick<GallerySearch, 'tags' | 'exclude'> {
  const norm = normalizeTag(name)
  const keep = (names: string[] = []) => nonEmpty(names.filter((other) => normalizeTag(other) !== norm))
  return { tags: keep(search.tags), exclude: keep(search.exclude) }
}

/** Search patch that adds a tag (or moves it to the other list). */
export function withTag(search: GallerySearch, name: string, mode: TagMode): Pick<GallerySearch, 'tags' | 'exclude'> {
  const rest = withoutTag(search, name)
  return mode === 'include'
    ? { ...rest, tags: [...(rest.tags ?? []), name] }
    : { ...rest, exclude: [...(rest.exclude ?? []), name] }
}

export function tagMode(search: GallerySearch, name: string): TagMode | undefined {
  const norm = normalizeTag(name)
  const has = (names: string[] = []) => names.some((other) => normalizeTag(other) === norm)
  if (has(search.tags)) return 'include'
  if (has(search.exclude)) return 'exclude'
  return undefined
}
