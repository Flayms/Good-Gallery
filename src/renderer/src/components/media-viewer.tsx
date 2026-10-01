import { mediaUrl } from '@shared/media-urls'
import { keepPreviousData, useMutation, useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ChevronLeftIcon, ChevronRightIcon, FolderOpenIcon, InfoIcon, XIcon } from 'lucide-react'
import { type ReactNode, useEffect, useState } from 'react'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ZoomableImage } from '@/components/zoomable-image'
import { formatBytes, formatDuration } from '@/lib/format'
import { withTag } from '@/lib/search'
import { type RouterOutputs, trpc } from '@/lib/trpc'
import { cn } from '@/lib/utils'

type MediaDetails = RouterOutputs['media']['byId']

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-white/50 text-xs">{label}</dt>
      <dd className="break-words">{children}</dd>
    </div>
  )
}

function InfoPanel({ item }: { item: MediaDetails }) {
  return (
    <aside className="flex w-80 shrink-0 flex-col gap-4 overflow-y-auto border-white/10 border-l p-4 text-sm">
      <dl className="flex flex-col gap-3">
        <Field label="File">{item.fileName}</Field>
        <Field label="Location">
          <span className="select-text break-all">{item.path}</span>
        </Field>
        <Field label="Library">{item.rootLabel}</Field>
        {item.takenAt !== null && <Field label="Taken">{new Date(item.takenAt).toLocaleString()}</Field>}
        <Field label="Modified">{new Date(item.mtime).toLocaleString()}</Field>
        {item.width !== null && item.height !== null && (
          <Field label="Dimensions">
            {item.width} × {item.height}
          </Field>
        )}
        {item.duration !== null && <Field label="Duration">{formatDuration(item.duration)}</Field>}
        <Field label="Size">{formatBytes(item.size)}</Field>
      </dl>
      {item.tags.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <span className="text-white/50 text-xs">Tags</span>
          <div className="flex flex-wrap gap-1.5">
            {item.tags.map((tag) => (
              <Badge key={tag.id} variant="secondary" asChild>
                <Link
                  to="/"
                  search={(prev) => ({ ...prev, ...withTag(prev, tag.name, 'include') })}
                  title={`Show only ${tag.name}`}
                >
                  {tag.name}
                </Link>
              </Badge>
            ))}
          </div>
        </div>
      )}
    </aside>
  )
}

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
        <div className="relative flex min-w-0 flex-1 items-center justify-center p-4 pt-0">
          {error && <p className="text-white/60">{error.message}</p>}
          {item?.kind === 'video' && (
            // biome-ignore lint/a11y/useMediaCaption: personal videos come without caption tracks.
            <video key={item.id} src={mediaUrl(item.id)} controls autoPlay className="max-h-full max-w-full" />
          )}
          {item?.kind === 'image' && <ZoomableImage key={item.id} src={mediaUrl(item.id)} alt={item.fileName} />}
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
        {showInfo && item && <InfoPanel item={item} />}
      </div>
    </div>
  )
}
