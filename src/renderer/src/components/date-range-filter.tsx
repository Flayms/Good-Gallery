import { CalendarIcon } from 'lucide-react'
import { useId } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { GallerySearch } from '@/lib/search'

const dayFormat = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' })

function formatDay(day: string): string {
  return dayFormat.format(new Date(`${day}T00:00`))
}

function rangeLabel({ from, to }: GallerySearch): string {
  if (from && to) return from === to ? formatDay(from) : `${formatDay(from)} – ${formatDay(to)}`
  if (from) return `From ${formatDay(from)}`
  if (to) return `Until ${formatDay(to)}`
  return 'Any date'
}

export function DateRangeFilter({
  search,
  onSearchChange,
}: {
  search: GallerySearch
  onSearchChange: (patch: Partial<GallerySearch>) => void
}) {
  const fromId = useId()
  const toId = useId()
  const active = search.from !== undefined || search.to !== undefined

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant={active ? 'secondary' : 'outline'} size="sm" aria-label="Date range">
          <CalendarIcon />
          {rangeLabel(search)}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-64">
        <div className="grid grid-cols-[auto_1fr] items-center gap-2">
          <label htmlFor={fromId} className="text-muted-foreground">
            From
          </label>
          <Input
            id={fromId}
            type="date"
            value={search.from ?? ''}
            max={search.to}
            onChange={(event) => onSearchChange({ from: event.target.value || undefined })}
          />
          <label htmlFor={toId} className="text-muted-foreground">
            To
          </label>
          <Input
            id={toId}
            type="date"
            value={search.to ?? ''}
            min={search.from}
            onChange={(event) => onSearchChange({ to: event.target.value || undefined })}
          />
        </div>
        <Button
          variant="ghost"
          size="sm"
          className="self-end"
          disabled={!active}
          onClick={() => onSearchChange({ from: undefined, to: undefined })}
        >
          Clear
        </Button>
      </PopoverContent>
    </Popover>
  )
}
