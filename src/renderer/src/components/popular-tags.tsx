import {
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSkeleton,
} from '@/components/ui/sidebar'
import { tagMode, withoutTag, withTag } from '@/lib/search'
import { trpc } from '@/lib/trpc'
import { useQuery } from '@tanstack/react-query'
import { Link, useSearch } from '@tanstack/react-router'
import { MinusIcon, TagIcon } from 'lucide-react'

const POPULAR_TAGS = 15

/** Most used tags outside a category (people, places); clicking one toggles it in the tag filter. */
export function PopularTags() {
  const tags = useQuery(trpc.tags.popular.queryOptions({ limit: POPULAR_TAGS }))
  const search = useSearch({ strict: false })

  if (tags.data?.length === 0) return null
  return (
    <SidebarGroup>
      <SidebarGroupLabel>Tags</SidebarGroupLabel>
      <SidebarMenu>
        {tags.isPending && <SidebarMenuSkeleton showIcon />}
        {tags.data?.map((tag) => {
          const mode = tagMode(search, tag.name)
          return (
            <SidebarMenuItem key={tag.id}>
              <SidebarMenuButton asChild size="sm" isActive={mode === 'include'}>
                <Link
                  to="/"
                  search={(prev) => ({
                    ...prev,
                    ...(mode ? withoutTag(prev, tag.name) : withTag(prev, tag.name, 'include')),
                  })}
                  title={mode ? `Remove ${tag.name} from the filter` : `Show only ${tag.name}`}
                >
                  {mode === 'exclude' ? <MinusIcon className="text-destructive" /> : <TagIcon />}
                  <span>{tag.name}</span>
                </Link>
              </SidebarMenuButton>
              <SidebarMenuBadge>{tag.count.toLocaleString()}</SidebarMenuBadge>
            </SidebarMenuItem>
          )
        })}
      </SidebarMenu>
    </SidebarGroup>
  )
}
