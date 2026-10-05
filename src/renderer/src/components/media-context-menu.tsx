import { MediaInfo } from '@/components/media-info'
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from '@/components/ui/context-menu'
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { trpc } from '@/lib/trpc'
import { useMutation, useQuery } from '@tanstack/react-query'
import { FolderOpenIcon, InfoIcon } from 'lucide-react'
import { type ReactNode, useState } from 'react'
import { toast } from 'sonner'

function MediaInfoSheet({
  id,
  open,
  onOpenChange,
}: {
  id: number
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { data: item } = useQuery(trpc.media.byId.queryOptions({ id }, { enabled: open }))
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent className="overflow-y-auto">
        <SheetHeader>
          <SheetTitle>{item?.fileName ?? 'Information'}</SheetTitle>
        </SheetHeader>
        {item && (
          <div className="px-4">
            <MediaInfo item={item} />
          </div>
        )}
      </SheetContent>
    </Sheet>
  )
}

interface MediaContextMenuProps {
  id: number
  children: ReactNode
  /** Called instead of opening the built-in info sheet, e.g. to toggle an already-visible info panel. */
  onInformation?: () => void
}

/** Right-click menu with "Show in Explorer" and "Information", shared by the masonry grid and the viewer. */
export function MediaContextMenu({ id, children, onInformation }: MediaContextMenuProps) {
  const [sheetOpen, setSheetOpen] = useState(false)
  const showInFolder = useMutation(
    trpc.media.showInFolder.mutationOptions({ onError: (error) => toast.error(error.message) }),
  )

  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
        <ContextMenuContent>
          <ContextMenuItem onSelect={() => showInFolder.mutate({ id })}>
            <FolderOpenIcon /> Show in Explorer
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => (onInformation ? onInformation() : setSheetOpen(true))}>
            <InfoIcon /> Information
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
      {!onInformation && <MediaInfoSheet id={id} open={sheetOpen} onOpenChange={setSheetOpen} />}
    </>
  )
}
