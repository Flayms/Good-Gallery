// Renders thumbnails with the reference's own code (renderMedia: sharp, ffmpeg-static, the exiftool preview source).
// Bundled by build-reference.mjs with the reference's vite; `@ref` is the reference's `src/`.
//   node reference-thumbs.cjs <list.json> <out dir>
// The list holds `{ name, file, kind, duration }`; each item gives `<name>-<width>.webp` and `<name>.thumbhash`, or
// `<name>.error`.
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { ExifToolMetadataSource } from '@ref/indexer/metadata'
import { renderMedia } from '@ref/indexer/thumbnail'

interface Item {
  name: string
  file: string
  kind: 'image' | 'video'
  duration: number | null
}

async function main(): Promise<void> {
  const [list, out] = process.argv.slice(2)
  if (!list || !out) throw new Error('usage: reference-thumbs <list.json> <out dir>')
  const items: Item[] = JSON.parse(readFileSync(list, 'utf8'))
  const previews = new ExifToolMetadataSource(2)
  try {
    for (const { name, file, kind, duration } of items) {
      try {
        const thumbnails = await renderMedia(file, { kind, duration }, previews)
        for (const { width, data } of thumbnails.images) writeFileSync(join(out, `${name}-${width}.webp`), data)
        writeFileSync(join(out, `${name}.thumbhash`), thumbnails.thumbhash)
      } catch (error) {
        writeFileSync(join(out, `${name}.error`), String(error))
      }
    }
  } finally {
    await previews.end()
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
