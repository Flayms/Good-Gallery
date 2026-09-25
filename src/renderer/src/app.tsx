import { ImagesIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'

export function App() {
  return (
    <main className="flex h-screen flex-col items-center justify-center gap-6">
      <ImagesIcon className="size-12 text-muted-foreground" />
      <h1 className="font-heading text-3xl font-semibold tracking-tight">Good Gallery</h1>
      <Button>Add library</Button>
    </main>
  )
}
