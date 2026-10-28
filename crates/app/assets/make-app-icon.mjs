// Renders the app icon (`app-icon.svg`, the reference's `build/icon.svg`) into `app-icon.png` (256 px, the window
// icon) and `app-icon.ico` (16–256 px PNG entries, the executable's icon and the installer's), with the reference's
// sharp.
//
// Regenerate after changing app-icon.svg:
//   node crates/app/assets/make-app-icon.mjs ../good-gallery
import { readFileSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]

const reference = resolve(process.argv[2] ?? '../good-gallery')
const sharp = createRequire(join(reference, 'package.json'))('sharp')
const here = dirname(fileURLToPath(import.meta.url))
const svg = readFileSync(join(here, 'app-icon.svg'))

const render = (size) => sharp(svg, { density: (72 * size) / 512 }).resize(size, size).png().toBuffer()

const pngs = await Promise.all(SIZES.map(render))
writeFileSync(join(here, 'app-icon.png'), pngs.at(-1))

// ICO: header, one 16-byte directory entry per image, then the PNG files. 0 in width / height means 256.
const header = Buffer.alloc(6)
header.writeUInt16LE(0, 0)
header.writeUInt16LE(1, 2)
header.writeUInt16LE(SIZES.length, 4)
let offset = 6 + 16 * SIZES.length
const entries = SIZES.map((size, i) => {
  const entry = Buffer.alloc(16)
  entry.writeUInt8(size % 256, 0)
  entry.writeUInt8(size % 256, 1)
  entry.writeUInt16LE(1, 4) // color planes
  entry.writeUInt16LE(32, 6) // bits per pixel
  entry.writeUInt32LE(pngs[i].length, 8)
  entry.writeUInt32LE(offset, 12)
  offset += pngs[i].length
  return entry
})
writeFileSync(join(here, 'app-icon.ico'), Buffer.concat([header, ...entries, ...pngs]))
console.log(`app-icon.png, app-icon.ico (${SIZES.join(', ')} px)`)
