import { createFileRoute } from '@tanstack/react-router'
import { useCallback } from 'react'
import { z } from 'zod'
import { MediaViewer } from '@/components/media-viewer'

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
  const navigate = Route.useNavigate()
  const close = useCallback(() => void navigate({ to: '/', search: (prev) => prev }), [navigate])
  return <MediaViewer id={id} onClose={close} />
}
