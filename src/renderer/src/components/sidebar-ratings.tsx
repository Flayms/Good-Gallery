import { useQuery } from '@tanstack/react-query'
import { Link, useSearch } from '@tanstack/react-router'
import { StarIcon, StarOffIcon } from 'lucide-react'
import { CollapsibleSidebarGroup } from '@/components/collapsible-sidebar-group'
import {
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSkeleton,
} from '@/components/ui/sidebar'
import type { GallerySearch } from '@/lib/search'
import { trpc } from '@/lib/trpc'

function ratingLabel(rating: number): string {
  if (rating === 0) return 'Unrated'
  return rating === 1 ? '1 star' : `${rating} stars`
}

/** Search patch that adds the rating to the rating filter, or removes it if it's already there. */
function toggleRating(search: GallerySearch, rating: number): Pick<GallerySearch, 'rating'> {
  const ratings = search.rating ?? []
  const next = ratings.includes(rating) ? ratings.filter((other) => other !== rating) : [...ratings, rating]
  return { rating: next.length > 0 ? next : undefined }
}

/** Media per rating; clicking one toggles it in the rating filter. */
export function SidebarRatings() {
  const counts = useQuery(trpc.media.ratingCounts.queryOptions())
  const selected = useSearch({ strict: false, select: (search) => search.rating ?? [] })

  // Only unrated media: nothing worth filtering by.
  if (counts.data?.every((row) => row.rating === 0)) return null
  return (
    <CollapsibleSidebarGroup id="ratings" label="Ratings">
      <SidebarMenu>
        {counts.isPending && <SidebarMenuSkeleton showIcon />}
        {counts.data?.map(({ rating, count }) => {
          const active = selected.includes(rating)
          const label = ratingLabel(rating)
          return (
            <SidebarMenuItem key={rating}>
              <SidebarMenuButton asChild size="sm" isActive={active}>
                <Link
                  to="/"
                  search={(prev) => ({ ...prev, ...toggleRating(prev, rating) })}
                  title={active ? `Remove ${label} from the filter` : `Show ${label}`}
                >
                  {rating === 0 ? <StarOffIcon /> : <StarIcon />}
                  <span>{label}</span>
                </Link>
              </SidebarMenuButton>
              <SidebarMenuBadge>{count.toLocaleString()}</SidebarMenuBadge>
            </SidebarMenuItem>
          )
        })}
      </SidebarMenu>
    </CollapsibleSidebarGroup>
  )
}
