import { normalizeTag } from '@shared/tags'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { Command as CommandPrimitive } from 'cmdk'
import { cn } from 'cn'
import { MinusIcon, PlusIcon, SearchIcon, TagIcon, XIcon } from 'lucide-react'
import { type KeyboardEvent, useId, useRef, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { CommandEmpty, CommandItem, CommandList, CommandShortcut } from '@/components/ui/command'
import { type GallerySearch, type TagMode, withoutTag, withTag } from '@/lib/search'
import { trpc } from '@/lib/trpc'

const SUGGESTIONS = 10

interface TagSearchProps {
  search: GallerySearch
  onSearchChange: (patch: Partial<GallerySearch>) => void
}

function TagChip({
  name,
  mode,
  onToggle,
  onRemove,
}: {
  name: string
  mode: TagMode
  onToggle: () => void
  onRemove: () => void
}) {
  const excluded = mode === 'exclude'
  return (
    <Badge variant={excluded ? 'destructive' : 'secondary'} className="h-6 gap-0.5 pr-0.5">
      <button
        type="button"
        className="flex items-center gap-1"
        onClick={onToggle}
        title={excluded ? 'Excluded – click to include' : 'Included – click to exclude'}
      >
        {excluded ? <MinusIcon className="size-3" /> : <TagIcon className="size-3" />}
        <span className={cn(excluded && 'line-through')}>{name}</span>
      </button>
      <button
        type="button"
        className="rounded-full p-0.5 hover:bg-foreground/10"
        onClick={onRemove}
        aria-label={`Remove ${name}`}
      >
        <XIcon className="size-3" />
      </button>
    </Badge>
  )
}

/** Tag filter: chips for the selected tags and an autocomplete input; a leading `-` adds an exclusion. */
export function TagSearch({ search, onSearchChange }: TagSearchProps) {
  const inputRef = useRef<HTMLInputElement>(null)
  const inputId = useId()
  const [query, setQuery] = useState('')
  const [open, setOpen] = useState(false)
  const mode: TagMode = query.startsWith('-') ? 'exclude' : 'include'
  const prefix = mode === 'exclude' ? query.slice(1) : query

  const selected = [
    ...(search.tags ?? []).map((name) => ({ name, mode: 'include' as const })),
    ...(search.exclude ?? []).map((name) => ({ name, mode: 'exclude' as const })),
  ]
  // Selected tags stay in the list, so ask for enough extra to still fill it.
  const suggestions = useQuery(
    trpc.tags.autocomplete.queryOptions(
      { prefix, limit: SUGGESTIONS + selected.length },
      { enabled: open, placeholderData: keepPreviousData },
    ),
  )
  const selectedNames = new Set(selected.map((tag) => normalizeTag(tag.name)))
  const options = (suggestions.data ?? [])
    .filter((tag) => !selectedNames.has(normalizeTag(tag.name)))
    .slice(0, SUGGESTIONS)
  // cmdk only highlights the first item when the query changes, before the new suggestions arrive.
  const [highlighted, setHighlighted] = useState('')
  const highlightedValue = options.some((tag) => String(tag.id) === highlighted)
    ? highlighted
    : String(options[0]?.id ?? '')

  const add = (name: string) => {
    onSearchChange(withTag(search, name, mode))
    setQuery('')
  }

  const onKeyDown = (event: KeyboardEvent) => {
    const last = selected.at(-1)
    if (event.key === 'Backspace' && query === '' && last) {
      onSearchChange(withoutTag(search, last.name))
    } else if (event.key === 'Escape') {
      if (query) setQuery('')
      else inputRef.current?.blur()
    }
  }

  return (
    <CommandPrimitive
      shouldFilter={false}
      loop
      value={highlightedValue}
      onValueChange={setHighlighted}
      className="relative min-w-48 max-w-xl flex-1"
      onKeyDown={onKeyDown}
    >
      {/* A label, so clicks on the padding focus the input; `htmlFor`, since the chip buttons are labelable too. */}
      <label
        htmlFor={inputId}
        className="flex min-h-8 cursor-text flex-wrap items-center gap-1 rounded-lg border border-input bg-input/30 px-2 py-1 focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/50"
      >
        <SearchIcon className="size-4 shrink-0 text-muted-foreground" />
        {selected.map((tag) => (
          <TagChip
            key={`${tag.mode}:${tag.name}`}
            name={tag.name}
            mode={tag.mode}
            onToggle={() => onSearchChange(withTag(search, tag.name, tag.mode === 'include' ? 'exclude' : 'include'))}
            onRemove={() => onSearchChange(withoutTag(search, tag.name))}
          />
        ))}
        <CommandPrimitive.Input
          id={inputId}
          ref={inputRef}
          value={query}
          onValueChange={setQuery}
          onFocus={() => setOpen(true)}
          onBlur={() => setOpen(false)}
          placeholder={selected.length === 0 ? 'Filter by tags… (-tag to exclude)' : undefined}
          aria-label="Filter by tags"
          className="h-6 min-w-24 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
        />
      </label>
      {open && (
        <div className="absolute inset-x-0 top-full z-50 mt-1 rounded-lg bg-popover p-1 text-popover-foreground shadow-md ring-1 ring-foreground/10">
          {/* Keeps focus in the input while clicking a suggestion. */}
          <CommandList onMouseDown={(event) => event.preventDefault()}>
            {!suggestions.isPending && <CommandEmpty>No matching tags</CommandEmpty>}
            {options.map((tag) => (
              <CommandItem key={tag.id} value={String(tag.id)} onSelect={() => add(tag.name)}>
                {mode === 'exclude' ? <MinusIcon className="text-destructive" /> : <PlusIcon />}
                <span className="truncate">{tag.name}</span>
                <CommandShortcut className="tracking-normal tabular-nums">{tag.count.toLocaleString()}</CommandShortcut>
              </CommandItem>
            ))}
          </CommandList>
        </div>
      )}
    </CommandPrimitive>
  )
}
