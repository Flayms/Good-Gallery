import { ArrowDownIcon, ArrowUpIcon, ImageIcon, VideoIcon, ZoomInIcon, ZoomOutIcon } from 'lucide-react'
import { DateRangeFilter } from '@/components/date-range-filter'
import { RatingFilter } from '@/components/rating-filter'
import { TagSearch } from '@/components/tag-search'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Separator } from '@/components/ui/separator'
import { SidebarTrigger } from '@/components/ui/sidebar'
import { Slider } from '@/components/ui/slider'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { MAX_COLUMNS, MIN_COLUMNS } from '@/hooks/use-columns'
import {
  DEFAULT_DIR,
  DEFAULT_SORT,
  type GallerySearch,
  isMediaKind,
  isSortField,
  SORT_FIELDS,
  SORT_LABELS,
} from '@/lib/search'

interface GalleryToolbarProps {
  search: GallerySearch
  onSearchChange: (patch: Partial<GallerySearch>) => void
  columns: number
  onColumnsChange: (update: number | ((columns: number) => number)) => void
}

export function GalleryToolbar({ search, onSearchChange, columns, onColumnsChange }: GalleryToolbarProps) {
  const descending = (search.dir ?? DEFAULT_DIR) === 'desc'
  return (
    <header className="flex min-h-12 shrink-0 items-center gap-2 border-b px-3 py-2">
      <SidebarTrigger />
      <Separator orientation="vertical" className="mx-1 h-4!" />
      <TagSearch search={search} onSearchChange={onSearchChange} />
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
        <DateRangeFilter search={search} onSearchChange={onSearchChange} />
        <RatingFilter search={search} onSearchChange={onSearchChange} />
        <div className="flex items-center gap-1">
          <Select
            value={search.sortBy ?? DEFAULT_SORT}
            onValueChange={(value) =>
              onSearchChange({ sortBy: isSortField(value) && value !== DEFAULT_SORT ? value : undefined })
            }
          >
            <SelectTrigger size="sm" className="w-36" aria-label="Sort by">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {SORT_FIELDS.map((field) => (
                <SelectItem key={field} value={field}>
                  {SORT_LABELS[field]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            variant="outline"
            size="icon-sm"
            onClick={() => onSearchChange({ dir: descending ? 'asc' : undefined })}
            aria-label={descending ? 'Descending' : 'Ascending'}
            title={descending ? 'Descending' : 'Ascending'}
          >
            {descending ? <ArrowDownIcon /> : <ArrowUpIcon />}
          </Button>
        </div>

        <div className="flex items-center gap-1">
          {/* Zooming in means fewer, wider columns. */}
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => onColumnsChange((count) => count + 1)}
            disabled={columns >= MAX_COLUMNS}
            aria-label="Zoom out"
            title="Zoom out (Ctrl + wheel)"
          >
            <ZoomOutIcon />
          </Button>
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
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => onColumnsChange((count) => count - 1)}
            disabled={columns <= MIN_COLUMNS}
            aria-label="Zoom in"
            title="Zoom in (Ctrl + wheel)"
          >
            <ZoomInIcon />
          </Button>
        </div>
      </div>
    </header>
  )
}
