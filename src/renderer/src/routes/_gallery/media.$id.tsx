import { createFileRoute } from '@tanstack/react-router'
import { useCallback, useEffect, useMemo } from 'react'
import { z } from 'zod'
import { MediaViewer } from '@/components/media-viewer'
import { useGalleryMedia } from '@/hooks/use-gallery-media'

const mediaId = z.coerce.number().int().positive()

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
  const { ids, getItem, setRange } = useGalleryMedia(search)
  const index = useMemo(() => ids.indexOf(id), [ids, id])
  // Load the neighbours' data (for preloading) next to the current item.
  useEffect(() => {
    if (index >= 0) setRange(Math.max(0, index - 1), index + 1)
  }, [index, setRange])
  const prevId = index > 0 ? ids[index - 1] : undefined
  const nextId = index >= 0 ? ids[index + 1] : undefined
  const prevKind = getItem(index - 1)?.kind
  const nextKind = getItem(index + 1)?.kind
  const prev = useMemo(() => (prevId === undefined ? undefined : { id: prevId, kind: prevKind }), [prevId, prevKind])
  const next = useMemo(() => (nextId === undefined ? undefined : { id: nextId, kind: nextKind }), [nextId, nextKind])

  const close = useCallback(() => void navigate({ to: '/', search: (prev) => prev }), [navigate])
  // Replacing keeps Back from stepping through every viewed item.
  const show = useCallback(
    (target: number) =>
      void navigate({ to: '/media/$id', params: { id: target }, search: (prev) => prev, replace: true }),
    [navigate],
  )
  return <MediaViewer id={id} prev={prev} next={next} onNavigate={show} onClose={close} />
}
