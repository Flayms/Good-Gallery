// Writes the lucide icons the app uses as SVG files next to this script, from the reference's `lucide-react`
// (ISC license, see LICENSE.lucide.txt). `stroke` is white so egui can tint the rendered icon to any color.
//
// Regenerate after adding a name to ICONS:
//   node crates/app/assets/icons/make-icons.mjs ../good-gallery
import { readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const ICONS = [
  'arrow-down-wide-narrow', 'arrow-up-narrow-wide', 'calendar', 'check', 'chevron-down', 'chevron-left',
  'chevron-right', 'chevron-up', 'circle', 'file', 'file-search', 'film', 'folder', 'folder-open', 'folder-plus',
  'gallery-horizontal', 'image', 'image-off', 'images', 'info', 'layers', 'loader-circle', 'map-pin', 'minus',
  'panel-left', 'pause', 'play', 'plus', 'refresh-cw', 'search', 'settings', 'sliders-horizontal', 'star',
  'star-off', 'tag', 'trash-2', 'triangle-alert', 'unplug', 'user', 'video', 'volume-2', 'volume-x', 'x',
  'layout-dashboard', 'layout-grid', 'list', 'rows-3', 'zoom-in', 'zoom-out',
]

const reference = resolve(process.argv[2] ?? '../good-gallery')
const require = createRequire(join(reference, 'package.json'))
const iconsDir = join(dirname(require.resolve('lucide-react/package.json')), 'dist', 'esm', 'icons')
const here = dirname(fileURLToPath(import.meta.url))

/** The icon's data; renamed icons are modules that only re-export the new name. */
async function iconData(name) {
  const file = join(iconsDir, `${name}.mjs`)
  const { __iconData } = await import(pathToFileURL(file).href)
  if (__iconData) return __iconData
  const target = /from '\.\/(.+)\.mjs'/.exec(readFileSync(file, 'utf8'))?.[1]
  if (!target) throw new Error(`no icon data in ${file}`)
  return iconData(target)
}

for (const file of readdirSync(here)) if (file.endsWith('.svg')) rmSync(join(here, file))
for (const name of ICONS) {
  const __iconData = await iconData(name)
  const children = __iconData.node
    .map(([tag, attrs]) => {
      const list = Object.entries(attrs)
        .filter(([key]) => key !== 'key')
        .map(([key, value]) => `${key}="${value}"`)
        .join(' ')
      return `<${tag} ${list}/>`
    })
    .join('')
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">${children}</svg>\n`
  writeFileSync(join(here, `${name}.svg`), svg)
}
const license = readFileSync(join(dirname(require.resolve('lucide-react/package.json')), 'LICENSE'), 'utf8')
writeFileSync(join(here, 'LICENSE.lucide.txt'), license)
console.log(`wrote ${ICONS.length} icons to ${here}`)
