import { createFileRoute } from '@tanstack/react-router'
import { useCallback, useEffect } from 'react'
import { z } from 'zod'
import { MediaViewer } from '@/components/media-viewer'
import { useGalleryMedia } from '@/hooks/use-gallery-media'

const mediaId = z.coerce.number().int().positive()
/** Loads the next page once the viewer gets this close to the end of the loaded items. */
const PREFETCH_DISTANCE = 5

export const Route = createFileRoute('/_gallery/media/$id')({
  params: {
    parse: ({ id }) => ({ id: mediaId.parse(id) }),
    stringify: ({ id }) => ({ id: String(id) }),
  },
  component: MediaRoute,
})

function MediaRoute() {
  const { id } = Route.useParams()
  const search = Route.useSearch()
  const navigate = Route.useNavigate()
  const {
    query: { hasNextPage, isFetchingNextPage, fetchNextPage },
    items,
  } = useGalleryMedia(search)
  const index = items.findIndex((item) => item.id === id)
  const prev = index > 0 ? items[index - 1] : undefined
  const next = index >= 0 ? items[index + 1] : undefined

  useEffect(() => {
    if (index >= 0 && index >= items.length - PREFETCH_DISTANCE && hasNextPage && !isFetchingNextPage) {
      void fetchNextPage()
    }
  }, [index, items.length, hasNextPage, isFetchingNextPage, fetchNextPage])

  const close = useCallback(() => void navigate({ to: '/', search: (prev) => prev }), [navigate])
  // Replacing keeps Back from stepping through every viewed item.
  const show = useCallback(
    (target: number) =>
      void navigate({ to: '/media/$id', params: { id: target }, search: (prev) => prev, replace: true }),
    [navigate],
  )
  return <MediaViewer id={id} prev={prev} next={next} onNavigate={show} onClose={close} />
}
