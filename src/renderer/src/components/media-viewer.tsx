import { Button } from '@/components/ui/button'
import { trpc } from '@/lib/trpc'
import { mediaUrl } from '@shared/media-urls'
import { useQuery } from '@tanstack/react-query'
import { XIcon } from 'lucide-react'
import { useEffect } from 'react'

/** Minimal full-window viewer; keyboard navigation, zoom and the metadata panel follow in Phase 6. */
export function MediaViewer({ id, onClose }: { id: number; onClose: () => void }) {
  const { data: item, error } = useQuery(trpc.media.byId.queryOptions({ id }))

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={item?.fileName ?? 'Media viewer'}
      className="fixed inset-0 z-50 flex flex-col bg-black/95 text-white"
    >
      <div className="flex h-12 shrink-0 items-center gap-3 px-3 text-sm">
        <span className="truncate text-white/80">{item?.fileName}</span>
        <Button variant="ghost" size="icon" className="ml-auto" onClick={onClose} aria-label="Close" autoFocus>
          <XIcon />
        </Button>
      </div>
      <div className="flex min-h-0 flex-1 items-center justify-center p-4 pt-0">
        {error && <p className="text-white/60">{error.message}</p>}
        {item?.kind === 'video' && (
          // biome-ignore lint/a11y/useMediaCaption: personal videos come without caption tracks.
          <video src={mediaUrl(item.id)} controls autoPlay className="max-h-full max-w-full" />
        )}
        {item?.kind === 'image' && (
          <img src={mediaUrl(item.id)} alt={item.fileName} className="max-h-full max-w-full object-contain" />
        )}
      </div>
    </div>
  )
}
