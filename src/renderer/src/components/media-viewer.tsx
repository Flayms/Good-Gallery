import { MediaContextMenu } from '@/components/media-context-menu'
import { MediaInfo } from '@/components/media-info'
import { Button } from '@/components/ui/button'
import { ZoomableImage } from '@/components/zoomable-image'
import { trpc } from '@/lib/trpc'
import { cn } from '@/lib/utils'
import { mediaUrl } from '@shared/media-urls'
import { keepPreviousData, useMutation, useQuery } from '@tanstack/react-query'
import { ChevronLeftIcon, ChevronRightIcon, FolderOpenIcon, InfoIcon, XIcon } from 'lucide-react'
import { useEffect, useState } from 'react'
import { toast } from 'sonner'

/** A gallery neighbour; `kind` is unknown until its data is loaded. */
export interface MediaNeighbour {
  id: number
  kind?: 'image' | 'video'
}

function preload(item: MediaNeighbour | undefined) {
  if (item?.kind === 'image') new Image().src = mediaUrl(item.id)
}

interface MediaViewerProps {
  id: number
  /** Neighbours in the gallery order, if the item is part of the gallery. */
  prev?: MediaNeighbour
  next?: MediaNeighbour
  onNavigate: (id: number) => void
  onClose: () => void
}

/** Full-window lightbox: arrow keys / buttons navigate, images zoom and pan, `i` toggles the info panel. */
export function MediaViewer({ id, prev, next, onNavigate, onClose }: MediaViewerProps) {
  const { data: item, error } = useQuery(trpc.media.byId.queryOptions({ id }, { placeholderData: keepPreviousData }))
  const [showInfo, setShowInfo] = useState(false)
  const showInFolder = useMutation(
    trpc.media.showInFolder.mutationOptions({ onError: (error) => toast.error(error.message) }),
  )

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.ctrlKey || event.altKey || event.metaKey) return
      // Focused video controls use the arrow keys for seeking.
      const inVideo = event.target instanceof HTMLVideoElement
      if (event.key === 'Escape') onClose()
      else if (event.key === 'ArrowLeft' && prev && !inVideo) onNavigate(prev.id)
      else if (event.key === 'ArrowRight' && next && !inVideo) onNavigate(next.id)
      else if (event.key === 'i') setShowInfo((show) => !show)
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [prev, next, onNavigate, onClose])

  useEffect(() => {
    preload(next)
    preload(prev)
  }, [prev, next])

  const navButton = 'absolute top-1/2 -translate-y-1/2 rounded-full bg-black/40 text-white hover:bg-black/70'
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={item?.fileName ?? 'Media viewer'}
      className="fixed inset-0 z-50 flex flex-col bg-black/95 text-white"
    >
      <div className="flex h-12 shrink-0 items-center gap-1 px-3 text-sm">
        <span className="mr-auto truncate text-white/80">{item?.fileName}</span>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => showInFolder.mutate({ id })}
          aria-label="Show in Explorer"
          title="Show in Explorer"
        >
          <FolderOpenIcon />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => setShowInfo((show) => !show)}
          aria-label="Info"
          aria-pressed={showInfo}
          title="Info (i)"
          className={cn(showInfo && 'bg-white/15')}
        >
          <InfoIcon />
        </Button>
        <Button variant="ghost" size="icon" onClick={onClose} aria-label="Close" title="Close (Esc)" autoFocus>
          <XIcon />
        </Button>
      </div>
      <div className="flex min-h-0 flex-1">
        <MediaContextMenu id={id} onInformation={() => setShowInfo(true)}>
          {/* biome-ignore lint/a11y/noStaticElementInteractions: mouse-only convenience; Escape already closes. */}
          {/* biome-ignore lint/a11y/useKeyWithClickEvents: see above. */}
          <div
            className="relative flex min-w-0 flex-1 items-center justify-center p-4 pt-0"
            onClick={(event) => {
              if (event.target === event.currentTarget) onClose()
            }}
          >
            {error && <p className="text-white/60">{error.message}</p>}
            {item?.kind === 'video' && (
              // biome-ignore lint/a11y/useMediaCaption: personal videos come without caption tracks.
              <video key={item.id} src={mediaUrl(item.id)} controls autoPlay className="max-h-full max-w-full" />
            )}
            {item?.kind === 'image' && (
              <ZoomableImage key={item.id} src={mediaUrl(item.id)} alt={item.fileName} onBackdropClick={onClose} />
            )}
            {prev && (
              <Button
                variant="ghost"
                size="icon-lg"
                className={cn(navButton, 'left-3')}
                onClick={() => onNavigate(prev.id)}
                aria-label="Previous"
                title="Previous (←)"
              >
                <ChevronLeftIcon />
              </Button>
            )}
            {next && (
              <Button
                variant="ghost"
                size="icon-lg"
                className={cn(navButton, 'right-3')}
                onClick={() => onNavigate(next.id)}
                aria-label="Next"
                title="Next (→)"
              >
                <ChevronRightIcon />
              </Button>
            )}
          </div>
        </MediaContextMenu>
        {showInfo && item && (
          <aside className="flex w-80 shrink-0 flex-col overflow-y-auto border-white/10 border-l p-4 text-sm">
            <MediaInfo item={item} />
          </aside>
        )}
      </div>
    </div>
  )
}
