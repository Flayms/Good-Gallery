import { Badge } from '@/components/ui/badge'
import { formatBytes, formatDuration } from '@/lib/format'
import { withTag } from '@/lib/search'
import type { RouterOutputs } from '@/lib/trpc'
import { cn } from '@/lib/utils'
import { TAG_CATEGORIES, type TagCategory } from '@shared/tags'
import { Link } from '@tanstack/react-router'
import { StarIcon } from 'lucide-react'
import type { ReactNode } from 'react'

export type MediaDetails = RouterOutputs['media']['byId']

const CATEGORY_LABELS: Record<TagCategory, string> = { people: 'People', places: 'Places' }

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="wrap-break-word">{children}</dd>
    </div>
  )
}

function Rating({ value }: { value: number }) {
  return (
    <div className="flex gap-0.5" role="img" aria-label={`Rated ${value} out of 5`}>
      {[1, 2, 3, 4, 5].map((star) => (
        <StarIcon key={star} className={cn('size-4', star <= value ? 'fill-current' : 'text-muted-foreground')} />
      ))}
    </div>
  )
}

function TagGroup({ label, tags }: { label: string; tags: MediaDetails['tags'] }) {
  if (tags.length === 0) return null
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-muted-foreground text-xs">{label}</span>
      <div className="flex flex-wrap gap-1.5">
        {tags.map((tag) => (
          <Badge key={tag.id} variant="secondary" asChild>
            <Link
              to="/"
              search={(prev) => ({ ...prev, ...withTag(prev, tag.name, 'include') })}
              title={`Show only ${tag.name}`}
            >
              {tag.name}
            </Link>
          </Badge>
        ))}
      </div>
    </div>
  )
}

/** File details, rating and tags (grouped by category), shared by the viewer's info panel and the grid's info sheet. */
export function MediaInfo({ item }: { item: MediaDetails }) {
  const byCategory = (category: TagCategory) => item.tags.filter((tag) => tag.category === category)
  const other = item.tags.filter((tag) => tag.category === undefined)
  return (
    <div className="flex flex-col gap-4">
      <dl className="flex flex-col gap-3">
        <Field label="File">{item.fileName}</Field>
        <Field label="Location">
          <span className="select-text break-all">{item.path}</span>
        </Field>
        <Field label="Library">{item.rootLabel}</Field>
        {item.takenAt !== null && <Field label="Taken">{new Date(item.takenAt).toLocaleString()}</Field>}
        <Field label="Modified">{new Date(item.mtime).toLocaleString()}</Field>
        {item.width !== null && item.height !== null && (
          <Field label="Dimensions">
            {item.width} × {item.height}
          </Field>
        )}
        {item.duration !== null && <Field label="Duration">{formatDuration(item.duration)}</Field>}
        <Field label="Size">{formatBytes(item.size)}</Field>
        {item.rating !== null && (
          <Field label="Rating">
            <Rating value={item.rating} />
          </Field>
        )}
      </dl>
      {TAG_CATEGORIES.map((category) => (
        <TagGroup key={category} label={CATEGORY_LABELS[category]} tags={byCategory(category)} />
      ))}
      <TagGroup label="Tags" tags={other} />
    </div>
  )
}
