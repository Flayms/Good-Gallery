// Indexes a library with the reference's own code (scanRoot + IndexWriter + exiftool source) into a new index.
// Bundled by build-reference.mjs with the reference's vite; `@ref` is the reference's `src/`.
//   node reference-index.cjs <library> <index.db> <reference>/drizzle
import { openDatabase } from '@ref/main/db'
import { libraryRoots } from '@ref/main/db/schema'
import { ExifToolMetadataSource } from '@ref/indexer/metadata'
import { scanRoot } from '@ref/indexer/scan'
import { IndexWriter } from '@ref/indexer/writer'

async function main(): Promise<void> {
  const [library, index, migrations] = process.argv.slice(2)
  if (!library || !index || !migrations) throw new Error('usage: reference-index <library> <index.db> <migrations>')
  const db = openDatabase(index, migrations)
  const root = db.insert(libraryRoots).values({ path: library, label: 'Fixtures' }).returning().get()
  const metadata = new ExifToolMetadataSource(2)
  try {
    const outcome = await scanRoot(
      { writer: new IndexWriter(db), metadata, isReachable: async () => true },
      root.id,
      { mode: 'full' },
      { concurrency: 2, signal: new AbortController().signal, onProgress() {} },
    )
    console.log(`reference scan: ${outcome}`)
  } finally {
    await metadata.end()
    db.$client.close()
  }
}

// exiftool-vendored's timers keep the event loop alive after `end()`.
main().then(
  () => process.exit(0),
  (error: unknown) => {
    console.error(error)
    process.exit(1)
  },
)
