import { PopularTags } from '@/components/popular-tags'
import { TagTree } from '@/components/tag-tree'
import {
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSkeleton,
} from '@/components/ui/sidebar'
import { buildTagTree } from '@/lib/tag-tree'
import { trpc } from '@/lib/trpc'
import { TAG_CATEGORIES, type TagCategory } from '@shared/tags'
import { useQuery } from '@tanstack/react-query'
import { useSearch } from '@tanstack/react-router'
import { useMemo, useState } from 'react'

const CATEGORY_LABELS: Record<TagCategory, string> = { people: 'People', places: 'Places' }

/** Root-level tags shown before "Show all" is clicked. */
const TOP_LEVEL_PREVIEW = 10

function CategoryGroup({ category }: { category: TagCategory }) {
  const query = useQuery(trpc.tags.category.queryOptions({ category }))
  const tree = useMemo(() => buildTagTree(query.data ?? []), [query.data])
  const [showAll, setShowAll] = useState(false)
  const search = useSearch({ strict: false })

  if (query.data?.length === 0) return null
  const visible = showAll ? tree : tree.slice(0, TOP_LEVEL_PREVIEW)
  return (
    <SidebarGroup>
      <SidebarGroupLabel>{CATEGORY_LABELS[category]}</SidebarGroupLabel>
      <SidebarMenu>
        {query.isPending && <SidebarMenuSkeleton showIcon />}
        <TagTree nodes={visible} search={search} />
        {tree.length > TOP_LEVEL_PREVIEW && (
          <SidebarMenuItem>
            <SidebarMenuButton size="sm" onClick={() => setShowAll((show) => !show)}>
              {showAll ? 'Show less' : `Show all (${tree.length})`}
            </SidebarMenuButton>
          </SidebarMenuItem>
        )}
      </SidebarMenu>
    </SidebarGroup>
  )
}

/** Sidebar tag sections: people and places (from face regions and keyword hierarchies), then everything else. */
export function SidebarTags() {
  return (
    <>
      {TAG_CATEGORIES.map((category) => (
        <CategoryGroup key={category} category={category} />
      ))}
      <PopularTags />
    </>
  )
}
