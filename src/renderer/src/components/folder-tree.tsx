import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { cn } from 'cn'
import { ChevronRightIcon, FolderIcon, FolderOpenIcon } from 'lucide-react'
import { useMemo, useState } from 'react'
import { SidebarMenuSkeleton, SidebarMenuSub, SidebarMenuSubButton, SidebarMenuSubItem } from '@/components/ui/sidebar'
import { buildFolderTree, type FolderNode, folderAncestors } from '@/lib/folders'
import { trpc } from '@/lib/trpc'

interface FolderItemsProps {
  nodes: FolderNode[]
  rootId: number
  activeFolder: string | undefined
  expanded: ReadonlySet<string>
  onToggle: (path: string, open?: boolean) => void
}

function FolderItems({ nodes, rootId, activeFolder, expanded, onToggle }: FolderItemsProps) {
  return (
    <SidebarMenuSub className="mr-0 pr-0">
      {nodes.map((node) => {
        const open = expanded.has(node.path)
        const hasChildren = node.children.length > 0
        return (
          <SidebarMenuSubItem key={node.path}>
            <div className="flex items-center">
              {hasChildren ? (
                <button
                  type="button"
                  className="flex size-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground hover:bg-sidebar-accent"
                  onClick={() => onToggle(node.path)}
                  aria-label={open ? `Collapse ${node.name}` : `Expand ${node.name}`}
                  aria-expanded={open}
                >
                  <ChevronRightIcon className={cn('size-3.5 transition-transform', open && 'rotate-90')} />
                </button>
              ) : (
                <span className="size-5 shrink-0" />
              )}
              <SidebarMenuSubButton asChild isActive={activeFolder === node.path} className="min-w-0 flex-1">
                <Link
                  to="/"
                  search={(prev) => ({ ...prev, root: rootId, folder: node.path })}
                  onClick={() => onToggle(node.path, true)}
                  title={`${node.path} · ${node.count.toLocaleString()} items`}
                >
                  {open ? <FolderOpenIcon /> : <FolderIcon />}
                  <span>{node.name}</span>
                </Link>
              </SidebarMenuSubButton>
            </div>
            {open && hasChildren && (
              <FolderItems
                nodes={node.children}
                rootId={rootId}
                activeFolder={activeFolder}
                expanded={expanded}
                onToggle={onToggle}
              />
            )}
          </SidebarMenuSubItem>
        )
      })}
    </SidebarMenuSub>
  )
}

/** Folders of a library; subtrees expand on demand, ancestors of the active folder start expanded. */
export function FolderTree({ rootId, activeFolder }: { rootId: number; activeFolder: string | undefined }) {
  const folders = useQuery(trpc.libraries.folders.queryOptions({ id: rootId }))
  const tree = useMemo(() => buildFolderTree(folders.data ?? []), [folders.data])
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(
    () => new Set(activeFolder ? folderAncestors(activeFolder) : []),
  )

  const toggle = (path: string, open = !expanded.has(path)) => {
    if (open === expanded.has(path)) return
    const next = new Set(expanded)
    if (open) next.add(path)
    else next.delete(path)
    setExpanded(next)
  }

  if (folders.isPending) return <SidebarMenuSkeleton className="ml-6" />
  if (tree.length === 0) return null
  return <FolderItems nodes={tree} rootId={rootId} activeFolder={activeFolder} expanded={expanded} onToggle={toggle} />
}
