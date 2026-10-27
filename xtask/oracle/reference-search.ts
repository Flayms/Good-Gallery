// Runs searches and the sidebar's queries with the reference's own tRPC router on an existing index.
// Bundled by build-reference.mjs with the reference's vite; `@ref` is the reference's `src/`.
//   node reference-search.cjs <index.db> <reference>/drizzle <queries.json>
// The queries file holds `{ layout: [input…], suggest: [prefix…] }` (dates as local days); prints the results as JSON,
// media items by relative path and tags by name.
import { readFileSync } from 'node:fs'
import { openDatabase } from '@ref/main/db'
import { libraryRoots, media, tags } from '@ref/main/db/schema'
import { IndexerController } from '@ref/main/indexer'
import { SettingsStore } from '@ref/main/settings'
import { appRouter } from '@ref/main/trpc/router'
import { createCallerFactory } from '@ref/main/trpc/trpc'

interface LayoutQuery {
  rootId?: number
  folder?: string
  kind?: 'image' | 'video'
  name?: string
  tags: string[]
  excludeTags: string[]
  ratings: number[]
  from?: string
  to?: string
  sortBy: 'name' | 'taken' | 'modified' | 'path' | 'size'
  dir: 'asc' | 'desc'
}

/** Start of a local day in ms, as the reference's `mediaSearchInput` computes it. */
function startOfDay(day: string, offsetDays = 0): number {
  const date = new Date(`${day}T00:00`)
  date.setDate(date.getDate() + offsetDays)
  return date.getTime()
}

async function main(): Promise<void> {
  const [index, migrations, queriesFile] = process.argv.slice(2)
  if (!index || !migrations || !queriesFile) throw new Error('usage: reference-search <index> <migrations> <queries>')
  const queries: { layout: LayoutQuery[]; suggest: string[] } = JSON.parse(readFileSync(queriesFile, 'utf8'))
  const db = openDatabase(index, migrations)
  const indexer = new IndexerController(() => ({ send() {}, onEvent() {}, onExit() {}, kill() {} }))
  const desktop = { showItemInFolder() {}, pickFolder: async () => undefined }
  const caller = createCallerFactory(appRouter)({ db, indexer, settings: new SettingsStore(db), desktop })
  const paths = new Map(db.select({ id: media.id, relPath: media.relPath }).from(media).all().map((r) => [r.id, r.relPath]))
  const tagNames = new Map(db.select({ id: tags.id, name: tags.name }).from(tags).all().map((t) => [t.id, t.name]))

  const layout = []
  for (const { from, to, ...query } of queries.layout) {
    const input = {
      ...query,
      from: from === undefined ? undefined : startOfDay(from),
      to: to === undefined ? undefined : startOfDay(to, 1),
    }
    layout.push((await caller.media.layout(input)).ids.map((id) => paths.get(id)))
  }
  const suggest = []
  for (const prefix of queries.suggest) {
    suggest.push((await caller.tags.autocomplete({ prefix, limit: 10 })).map((t) => [t.name, t.count]))
  }
  const category = async (category: 'people' | 'places') =>
    (await caller.tags.category({ category })).map((t) => [
      t.name,
      t.parentId === null ? null : (tagNames.get(t.parentId) ?? null),
      t.count,
    ])
  const roots = db.select().from(libraryRoots).all()
  const result = {
    layout,
    suggest,
    people: await category('people'),
    places: await category('places'),
    popular: (await caller.tags.popular({ limit: 15 })).map((t) => [t.name, t.count]),
    ratings: (await caller.media.ratingCounts()).map((r) => [r.rating, r.count]),
    folders: (await caller.libraries.folders({ id: roots[0]?.id ?? 1 })).map((f) => [f.path, f.count]),
  }
  console.log(JSON.stringify(result))
  db.$client.close()
}

main().then(
  () => process.exit(0),
  (error: unknown) => {
    console.error(error)
    process.exit(1)
  },
)
