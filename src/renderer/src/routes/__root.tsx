import { AppSidebar } from '@/components/app-sidebar'
import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar'
import { Toaster } from '@/components/ui/sonner'
import { TooltipProvider } from '@/components/ui/tooltip'
import { useIndexerStatus } from '@/hooks/use-indexer-status'
import { createRootRoute, Outlet } from '@tanstack/react-router'

export const Route = createRootRoute({ component: RootLayout })

function RootLayout() {
  const indexer = useIndexerStatus()
  return (
    <TooltipProvider>
      <SidebarProvider className="h-svh">
        <AppSidebar indexer={indexer} />
        <SidebarInset className="min-w-0 overflow-hidden">
          <Outlet />
        </SidebarInset>
      </SidebarProvider>
      <Toaster />
    </TooltipProvider>
  )
}
