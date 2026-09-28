import { createFileRoute } from '@tanstack/react-router'
import { GeneralSettings } from '@/components/general-settings'
import { LibrarySettings } from '@/components/library-settings'
import { Separator } from '@/components/ui/separator'
import { SidebarTrigger } from '@/components/ui/sidebar'

export const Route = createFileRoute('/settings')({ component: SettingsPage })

function SettingsPage() {
  return (
    <>
      <header className="flex h-12 shrink-0 items-center gap-2 border-b px-3">
        <SidebarTrigger />
        <Separator orientation="vertical" className="mx-1 h-4!" />
        <h1 className="font-heading font-medium">Settings</h1>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex w-full max-w-3xl flex-col gap-10 p-6">
          <LibrarySettings />
          <GeneralSettings />
        </div>
      </div>
    </>
  )
}
