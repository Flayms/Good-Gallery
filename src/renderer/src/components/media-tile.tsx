import { pickThumbWidth, thumbUrl } from '@shared/media-urls'
import { Link } from '@tanstack/react-router'
import { ImageOffIcon, PlayIcon } from 'lucide-react'
import { useMemo, useState } from 'react'
import { formatDuration } from '@/lib/format'
import { thumbhashDataUrl } from '@/lib/thumbhash'
import type { RouterOutputs } from '@/lib/trpc'
import { cn } from '@/lib/utils'

export type MediaItem = RouterOutputs['media']['search']['items'][number]

export function MediaTile({ item, width }: { item: MediaItem; width: number }) {
  const [state, setState] = useState<'loading' | 'loaded' | 'failed'>('loading')
  const placeholder = useMemo(() => (item.thumbhash ? thumbhashDataUrl(item.thumbhash) : undefined), [item.thumbhash])
  const failed = state === 'failed' || item.thumbStatus === 'error'

  return (
    <Link
      to="/media/$id"
      params={{ id: item.id }}
      search={(prev) => prev}
      title={item.fileName}
      draggable={false}
      className="relative block size-full overflow-hidden rounded-md bg-muted bg-cover bg-center outline-none focus-visible:ring-2 focus-visible:ring-ring"
      style={placeholder ? { backgroundImage: `url("${placeholder}")` } : undefined}
    >
      {failed ? (
        <div className="flex size-full items-center justify-center text-muted-foreground">
          <ImageOffIcon className="size-6" />
        </div>
      ) : (
        <img
          src={thumbUrl(item.id, pickThumbWidth(width, window.devicePixelRatio))}
          alt={item.fileName}
          decoding="async"
          draggable={false}
          onLoad={() => setState('loaded')}
          onError={() => setState('failed')}
          className={cn(
            'size-full object-cover transition-opacity duration-300',
            state === 'loaded' ? 'opacity-100' : 'opacity-0',
          )}
        />
      )}
      {item.kind === 'video' && (
        <span className="absolute right-1.5 bottom-1.5 flex items-center gap-1 rounded bg-black/60 px-1.5 py-0.5 text-white text-xs tabular-nums">
          <PlayIcon className="size-3 fill-current" />
          {item.duration ? formatDuration(item.duration) : null}
        </span>
      )}
    </Link>
  )
}
