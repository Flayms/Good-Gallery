import { Gallery } from '@/components/gallery'
import { GalleryToolbar } from '@/components/gallery-toolbar'
import { useColumns } from '@/hooks/use-columns'
import { type GallerySearch, gallerySearch } from '@/lib/search'
import { trpc } from '@/lib/trpc'
import { useQuery } from '@tanstack/react-query'
import { createFileRoute, Outlet } from '@tanstack/react-router'

// Pathless layout: the gallery stays mounted (and keeps its scroll position) while the viewer overlays it.
export const Route = createFileRoute('/_gallery')({
  validateSearch: gallerySearch,
  component: GalleryLayout,
})

function GalleryLayout() {
  const search = Route.useSearch()
  const navigate = Route.useNavigate()
  const [columns, setColumns] = useColumns()
  const libraries = useQuery(trpc.libraries.list.queryOptions())

  const onSearchChange = (patch: Partial<GallerySearch>) => {
    void navigate({ to: '/', search: (prev) => ({ ...prev, ...patch }) })
  }

  return (
    <>
      <GalleryToolbar search={search} onSearchChange={onSearchChange} columns={columns} onColumnsChange={setColumns} />
      <div className="min-h-0 flex-1">
        <Gallery search={search} columns={columns} hasLibraries={(libraries.data?.length ?? 0) > 0} />
      </div>
      <Outlet />
    </>
  )
}
