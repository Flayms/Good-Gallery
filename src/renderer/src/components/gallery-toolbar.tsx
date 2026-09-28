import { ImageIcon, VideoIcon, ZoomInIcon, ZoomOutIcon } from 'lucide-react'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Separator } from '@/components/ui/separator'
import { SidebarTrigger } from '@/components/ui/sidebar'
import { Slider } from '@/components/ui/slider'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { MAX_COLUMNS, MIN_COLUMNS } from '@/hooks/use-columns'
import { type GallerySearch, isMediaKind, isSort, SORT_LABELS, SORTS } from '@/lib/search'

interface GalleryToolbarProps {
  search: GallerySearch
  onSearchChange: (patch: Partial<GallerySearch>) => void
  columns: number
  onColumnsChange: (columns: number) => void
}

export function GalleryToolbar({ search, onSearchChange, columns, onColumnsChange }: GalleryToolbarProps) {
  return (
    <header className="flex h-12 shrink-0 items-center gap-2 border-b px-3">
      <SidebarTrigger />
      <Separator orientation="vertical" className="mx-1 h-4!" />
      <ToggleGroup
        type="single"
        size="sm"
        variant="outline"
        spacing={0}
        value={search.kind ?? 'all'}
        onValueChange={(value) => onSearchChange({ kind: isMediaKind(value) ? value : undefined })}
        aria-label="Media type"
      >
        <ToggleGroupItem value="all">All</ToggleGroupItem>
        <ToggleGroupItem value="image">
          <ImageIcon /> Photos
        </ToggleGroupItem>
        <ToggleGroupItem value="video">
          <VideoIcon /> Videos
        </ToggleGroupItem>
      </ToggleGroup>

      <div className="ml-auto flex items-center gap-4">
        <Select
          value={search.sort ?? 'date-desc'}
          onValueChange={(value) => onSearchChange({ sort: isSort(value) ? value : undefined })}
        >
          <SelectTrigger size="sm" className="w-36" aria-label="Sort order">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SORTS.map((sort) => (
              <SelectItem key={sort} value={sort}>
                {SORT_LABELS[sort]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <div className="flex items-center gap-2 text-muted-foreground">
          <ZoomOutIcon className="size-4" />
          {/* Zooming in means fewer, wider columns. */}
          <Slider
            className="w-28"
            min={MIN_COLUMNS}
            max={MAX_COLUMNS}
            step={1}
            value={[MIN_COLUMNS + MAX_COLUMNS - columns]}
            onValueChange={([zoom]) => {
              if (zoom !== undefined) onColumnsChange(MIN_COLUMNS + MAX_COLUMNS - zoom)
            }}
            aria-label="Zoom"
          />
          <ZoomInIcon className="size-4" />
        </div>
      </div>
    </header>
  )
}
