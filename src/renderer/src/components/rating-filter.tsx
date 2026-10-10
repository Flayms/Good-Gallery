import { StarIcon, StarOffIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { GallerySearch } from '@/lib/search'

const RATINGS = [0, 1, 2, 3, 4, 5] as const

function ratingLabel(ratings: number[]): string {
  if (ratings.length === 0) return 'Any rating'
  if (ratings.length === 1 && ratings[0] === 0) return 'Unrated'
  return ratings
    .filter((rating) => rating > 0)
    .sort((a, b) => a - b)
    .map((rating) => `${rating}★`)
    .join(', ')
}

export function RatingFilter({
  search,
  onSearchChange,
}: {
  search: GallerySearch
  onSearchChange: (patch: Partial<GallerySearch>) => void
}) {
  const ratings = search.rating ?? []
  const active = ratings.length > 0

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant={active ? 'secondary' : 'outline'} size="sm" aria-label="Rating">
          <StarIcon />
          {ratingLabel(ratings)}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-auto">
        <ToggleGroup
          type="multiple"
          size="sm"
          variant="outline"
          spacing={0}
          value={ratings.map(String)}
          onValueChange={(value) => onSearchChange({ rating: value.length > 0 ? value.map(Number) : undefined })}
          aria-label="Ratings"
        >
          {RATINGS.map((rating) => (
            <ToggleGroupItem
              key={rating}
              value={String(rating)}
              aria-label={rating === 0 ? 'Unrated' : `${rating} stars`}
            >
              {rating === 0 ? <StarOffIcon /> : rating}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <Button
          variant="ghost"
          size="sm"
          className="mt-2 w-full"
          disabled={!active}
          onClick={() => onSearchChange({ rating: undefined })}
        >
          Clear
        </Button>
      </PopoverContent>
    </Popover>
  )
}
