import { useQuery } from '@tanstack/react-query'
import { useSubscription } from '@trpc/tanstack-react-query'
import { ImagesIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { trpc } from '@/lib/trpc'

export function App() {
  const libraries = useQuery(trpc.libraries.list.queryOptions())
  const indexer = useSubscription(trpc.indexer.status.subscriptionOptions())

  return (
    <main className="flex h-screen flex-col items-center justify-center gap-6">
      <ImagesIcon className="size-12 text-muted-foreground" />
      <h1 className="font-heading text-3xl font-semibold tracking-tight">Good Gallery</h1>
      <Button>Add library</Button>
      <p className="text-sm text-muted-foreground">
        {libraries.data ? `${libraries.data.length} libraries` : 'Loading…'} · Indexer{' '}
        {indexer.data?.state ?? indexer.status}
      </p>
    </main>
  )
}
