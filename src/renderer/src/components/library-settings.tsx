import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { FolderPlusIcon, RefreshCwIcon, Trash2Icon } from 'lucide-react'
import { type FormEvent, useState } from 'react'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { trpc } from '@/lib/trpc'

const STATUS_VARIANT = { online: 'secondary', offline: 'destructive', unknown: 'outline' } as const

/** Library roots management; the native folder picker comes with the full settings page (Phase 6). */
export function LibrarySettings() {
  const queryClient = useQueryClient()
  const libraries = useQuery(trpc.libraries.list.queryOptions())
  const [path, setPath] = useState('')

  const refresh = async (withMedia: boolean) => {
    await queryClient.invalidateQueries({ queryKey: trpc.libraries.list.queryKey() })
    if (withMedia) await queryClient.invalidateQueries({ queryKey: trpc.media.search.pathKey() })
  }
  const onError = (error: { message: string }) => toast.error(error.message)

  const add = useMutation(
    trpc.libraries.add.mutationOptions({
      onSuccess: (root) => {
        toast.success(`Added ${root.label}, indexing…`)
        setPath('')
        void refresh(false)
      },
      onError,
    }),
  )
  const remove = useMutation(trpc.libraries.remove.mutationOptions({ onSuccess: () => refresh(true), onError }))
  const rescan = useMutation(
    trpc.libraries.rescan.mutationOptions({ onSuccess: () => toast.success('Rescan queued'), onError }),
  )

  const onSubmit = (event: FormEvent) => {
    event.preventDefault()
    add.mutate({ path })
  }

  return (
    <section className="flex flex-col gap-4">
      <div>
        <h2 className="font-heading font-medium text-lg">Libraries</h2>
        <p className="text-muted-foreground text-sm">Folders on local disks or network shares, indexed recursively.</p>
      </div>

      <form onSubmit={onSubmit} className="flex gap-2">
        <Input
          value={path}
          onChange={(event) => setPath(event.target.value)}
          placeholder="D:\Photos or \\server\share\photos"
          aria-label="Library folder path"
          spellCheck={false}
        />
        <Button type="submit" disabled={!path.trim() || add.isPending}>
          <FolderPlusIcon /> Add
        </Button>
      </form>

      <ul className="flex flex-col divide-y rounded-lg border">
        {libraries.isPending && (
          <li className="p-3">
            <Skeleton className="h-10" />
          </li>
        )}
        {libraries.data?.length === 0 && <li className="p-3 text-muted-foreground text-sm">No libraries yet.</li>}
        {libraries.data?.map((root) => (
          <li key={root.id} className="flex items-center gap-3 p-3">
            <div className="flex min-w-0 flex-1 flex-col">
              <div className="flex items-center gap-2">
                <span className="truncate font-medium">{root.label}</span>
                <Badge variant={STATUS_VARIANT[root.status]}>{root.status}</Badge>
              </div>
              <span className="truncate text-muted-foreground text-xs" title={root.path}>
                {root.path}
                {root.lastScanAt && ` · last indexed ${new Date(root.lastScanAt).toLocaleString()}`}
              </span>
            </div>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Rescan ${root.label}`}
              title="Rescan"
              onClick={() => rescan.mutate({ id: root.id })}
            >
              <RefreshCwIcon />
            </Button>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Remove ${root.label}`}
              title="Remove from gallery (files stay untouched)"
              onClick={() => {
                if (window.confirm(`Remove "${root.label}" from the gallery? Files on disk are not touched.`)) {
                  remove.mutate({ id: root.id })
                }
              }}
            >
              <Trash2Icon />
            </Button>
          </li>
        ))}
      </ul>
    </section>
  )
}
