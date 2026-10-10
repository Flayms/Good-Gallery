import { ChevronRightIcon } from 'lucide-react'
import { type ReactNode, useState } from 'react'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { SidebarGroup, SidebarGroupContent, SidebarGroupLabel } from '@/components/ui/sidebar'

const STORAGE_PREFIX = 'sidebar.group.'

function readOpen(id: string): boolean {
  try {
    return localStorage.getItem(STORAGE_PREFIX + id) !== 'closed'
  } catch {
    return true
  }
}

function writeOpen(id: string, open: boolean): void {
  try {
    localStorage.setItem(STORAGE_PREFIX + id, open ? 'open' : 'closed')
  } catch {
    // Only a convenience: the group just opens again next time.
  }
}

/** Sidebar group whose label collapses it; the open state is remembered per `id`. */
export function CollapsibleSidebarGroup({
  id,
  label,
  onOpenChange,
  children,
}: {
  id: string
  label: string
  onOpenChange?: (open: boolean) => void
  children: ReactNode
}) {
  const [open, setOpen] = useState(() => readOpen(id))
  const changeOpen = (next: boolean) => {
    setOpen(next)
    writeOpen(id, next)
    onOpenChange?.(next)
  }

  return (
    <Collapsible open={open} onOpenChange={changeOpen} className="group/collapsible">
      <SidebarGroup>
        <SidebarGroupLabel asChild>
          <CollapsibleTrigger className="w-full hover:bg-sidebar-accent hover:text-sidebar-accent-foreground">
            {label}
            <ChevronRightIcon className="ml-auto transition-transform group-data-[state=open]/collapsible:rotate-90" />
          </CollapsibleTrigger>
        </SidebarGroupLabel>
        <CollapsibleContent>
          <SidebarGroupContent>{children}</SidebarGroupContent>
        </CollapsibleContent>
      </SidebarGroup>
    </Collapsible>
  )
}
