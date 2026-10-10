import { Link } from '@tanstack/react-router'
import { cn } from 'cn'
import { ChevronRightIcon, MinusIcon, TagIcon } from 'lucide-react'
import { useState } from 'react'
import {
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
} from '@/components/ui/sidebar'
import { type GallerySearch, tagMode, withoutTag, withTag } from '@/lib/search'
import type { TagNode } from '@/lib/tag-tree'

function tagSearchPatch(node: TagNode, search: GallerySearch) {
  const mode = tagMode(search, node.name)
  return {
    mode,
    patch: (prev: GallerySearch) => ({
      ...prev,
      ...(mode ? withoutTag(prev, node.name) : withTag(prev, node.name, 'include')),
    }),
  }
}

function NodeIcon({ mode }: { mode: 'include' | 'exclude' | undefined }) {
  return mode === 'exclude' ? <MinusIcon className="text-destructive" /> : <TagIcon />
}

function Chevron({ open, name, onToggle }: { open: boolean; name: string; onToggle: () => void }) {
  return (
    <button
      type="button"
      className="flex size-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground hover:bg-sidebar-accent"
      onClick={onToggle}
      aria-label={open ? `Collapse ${name}` : `Expand ${name}`}
      aria-expanded={open}
    >
      <ChevronRightIcon className={cn('size-3.5 transition-transform', open && 'rotate-90')} />
    </button>
  )
}

function TagSubItems({
  nodes,
  search,
  expanded,
  onToggle,
}: {
  nodes: TagNode[]
  search: GallerySearch
  expanded: ReadonlySet<number>
  onToggle: (id: number) => void
}) {
  return (
    <SidebarMenuSub className="mr-0 pr-0">
      {nodes.map((node) => {
        const open = expanded.has(node.id)
        const hasChildren = node.children.length > 0
        const { mode, patch } = tagSearchPatch(node, search)
        return (
          <SidebarMenuSubItem key={node.id}>
            <div className="flex items-center">
              {hasChildren ? (
                <Chevron open={open} name={node.name} onToggle={() => onToggle(node.id)} />
              ) : (
                <span className="size-5 shrink-0" />
              )}
              <SidebarMenuSubButton asChild isActive={mode === 'include'} className="min-w-0 flex-1">
                <Link to="/" search={patch} title={`${node.name} · ${node.count.toLocaleString()} items`}>
                  <NodeIcon mode={mode} />
                  <span>{node.name}</span>
                </Link>
              </SidebarMenuSubButton>
            </div>
            {open && hasChildren && (
              <TagSubItems nodes={node.children} search={search} expanded={expanded} onToggle={onToggle} />
            )}
          </SidebarMenuSubItem>
        )
      })}
    </SidebarMenuSub>
  )
}

/** A category's tags as a tree (e.g. country › city); clicking one toggles it in the tag filter. */
export function TagTree({ nodes, search }: { nodes: TagNode[]; search: GallerySearch }) {
  const [expanded, setExpanded] = useState<ReadonlySet<number>>(new Set())
  const toggle = (id: number) => {
    const next = new Set(expanded)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    setExpanded(next)
  }

  return (
    <>
      {nodes.map((node) => {
        const open = expanded.has(node.id)
        const hasChildren = node.children.length > 0
        const { mode, patch } = tagSearchPatch(node, search)
        return (
          <SidebarMenuItem key={node.id}>
            <div className="flex items-center">
              {hasChildren ? (
                <Chevron open={open} name={node.name} onToggle={() => toggle(node.id)} />
              ) : (
                <span className="size-5 shrink-0" />
              )}
              <SidebarMenuButton asChild size="sm" isActive={mode === 'include'} className="min-w-0 flex-1">
                <Link
                  to="/"
                  search={patch}
                  title={mode ? `Remove ${node.name} from the filter` : `Show only ${node.name}`}
                >
                  <NodeIcon mode={mode} />
                  <span>{node.name}</span>
                </Link>
              </SidebarMenuButton>
              <SidebarMenuBadge>{node.count.toLocaleString()}</SidebarMenuBadge>
            </div>
            {open && hasChildren && (
              <TagSubItems nodes={node.children} search={search} expanded={expanded} onToggle={toggle} />
            )}
          </SidebarMenuItem>
        )
      })}
    </>
  )
}
