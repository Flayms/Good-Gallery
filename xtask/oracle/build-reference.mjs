// Bundles an entry script (default reference-index.ts) with the reference's vite into one CommonJS file, so Node can run the reference's
// TypeScript (its imports have no extensions and use aliases). Packages stay external and are resolved from the
// reference's node_modules at run time (NODE_PATH), apart from ESM-only ones, which are bundled.
//   node build-reference.mjs <reference> <out.cjs> [entry.ts]
import { createRequire } from 'node:module'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const [referenceArg, outArg, entryArg = 'reference-index.ts'] = process.argv.slice(2)
if (!referenceArg || !outArg) throw new Error('usage: build-reference.mjs <reference> <out.cjs> [entry.ts]')
const reference = resolve(referenceArg)
const out = resolve(outArg)
const require = createRequire(join(reference, 'package.json'))
const { build } = await import(pathToFileURL(require.resolve('vite')).href)

const here = dirname(fileURLToPath(import.meta.url))
const isLocal = (id) => id.startsWith('.') || id.startsWith('@ref') || id.startsWith('@shared') || /^([A-Za-z]:|\/|\0)/.test(id)
// ESM-only packages: Node can require them, but the bundle's CommonJS interop then misses their default export.
const bundled = new Set(['p-queue', 'thumbhash'])

await build({
  configFile: false,
  logLevel: 'warn',
  root: here,
  resolve: {
    alias: {
      '@ref': join(reference, 'src'),
      '@shared': join(reference, 'src', 'shared'),
    },
  },
  build: {
    ssr: join(here, entryArg),
    outDir: dirname(out),
    emptyOutDir: false,
    minify: false,
    target: 'node22',
    rollupOptions: {
      // What bundled packages import is bundled too: pnpm doesn't hoist their dependencies.
      external: (id, importer) =>
        id.startsWith('node:') || (!isLocal(id) && !bundled.has(id) && !importer?.includes('node_modules')),
      output: { format: 'cjs', entryFileNames: basename(out) },
    },
  },
})
console.log(`bundled ${out}`)
