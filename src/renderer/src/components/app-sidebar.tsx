import type { IndexerStatus } from '@main/indexer'
import { useQuery } from '@tanstack/react-query'
import { Link, useLocation, useSearch } from '@tanstack/react-router'
import { FolderIcon, ImagesIcon, LayersIcon, Loader2Icon, SettingsIcon, UnplugIcon } from 'lucide-react'
import { FolderTree } from '@/components/folder-tree'
import { PopularTags } from '@/components/popular-tags'
import { Badge } from '@/components/ui/badge'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSkeleton,
} from '@/components/ui/sidebar'
import { type RouterOutputs, trpc } from '@/lib/trpc'

type Library = RouterOutputs['libraries']['list'][number]

function IndexerProgress({ status, libraries }: { status: IndexerStatus; libraries: Library[] }) {
  const { current, queue } = status
  if (!current) return null
  const label = libraries.find((root) => root.id === current.rootId)?.label ?? 'library'
  return (
    <div className="flex flex-col gap-0.5 px-2 py-1 text-muted-foreground text-xs" aria-live="polite">
      <div className="flex items-center gap-1.5 text-foreground">
        <Loader2Icon className="size-3.5 animate-spin" />
        <span className="truncate">Indexing {label}</span>
      </div>
      <span className="tabular-nums">
        {current.scanned.toLocaleString()} files · {current.indexed.toLocaleString()} updated
        {queue.length > 0 && ` · ${queue.length} queued`}
      </span>
    </div>
  )
}

export function AppSidebar({ indexer }: { indexer: IndexerStatus | undefined }) {
  const libraries = useQuery(trpc.libraries.list.queryOptions())
  const { root: activeRoot, folder: activeFolder } = useSearch({ strict: false })
  const onSettings = useLocation({ select: (location) => location.pathname === '/settings' })

  return (
    <Sidebar>
      <SidebarHeader>
        <div className="flex items-center gap-2 px-2 py-1.5 font-heading font-semibold">
          <ImagesIcon className="size-5" />
          Good Gallery
        </div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Libraries</SidebarGroupLabel>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton asChild isActive={!onSettings && activeRoot === undefined}>
                <Link to="/" search={(prev) => ({ ...prev, root: undefined, folder: undefined })}>
                  <LayersIcon />
                  <span>All libraries</span>
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
            {libraries.isPending && <SidebarMenuSkeleton showIcon />}
            {libraries.data?.map((root) => (
              <SidebarMenuItem key={root.id}>
                <SidebarMenuButton
                  asChild
                  isActive={!onSettings && activeRoot === root.id && activeFolder === undefined}
                >
                  <Link to="/" search={(prev) => ({ ...prev, root: root.id, folder: undefined })} title={root.path}>
                    {root.status === 'offline' ? <UnplugIcon /> : <FolderIcon />}
                    <span>{root.label}</span>
                  </Link>
                </SidebarMenuButton>
                {indexer?.current?.rootId === root.id ? (
                  <SidebarMenuBadge>
                    <Loader2Icon className="size-3.5 animate-spin" aria-label="Indexing" />
                  </SidebarMenuBadge>
                ) : (
                  root.status === 'offline' && (
                    <SidebarMenuBadge>
                      <Badge variant="destructive">offline</Badge>
                    </SidebarMenuBadge>
                  )
                )}
                {!onSettings && activeRoot === root.id && <FolderTree rootId={root.id} activeFolder={activeFolder} />}
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroup>
        <PopularTags />
      </SidebarContent>
      <SidebarFooter>
        {indexer && <IndexerProgress status={indexer} libraries={libraries.data ?? []} />}
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton asChild isActive={onSettings}>
              <Link to="/settings">
                <SettingsIcon />
                <span>Settings</span>
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  )
}
