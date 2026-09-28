import { resolve } from 'node:path'
import tailwindcss from '@tailwindcss/vite'
import { tanstackRouter } from '@tanstack/router-plugin/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'electron-vite'

const shared = resolve(import.meta.dirname, 'src/shared')
const rendererSrc = resolve(import.meta.dirname, 'src/renderer/src')

export default defineConfig({
  main: {
    resolve: { alias: { '@shared': shared } },
  },
  preload: {
    resolve: { alias: { '@shared': shared } },
    build: {
      rollupOptions: {
        // Sandboxed preload scripts must be CommonJS.
        output: { format: 'cjs', entryFileNames: '[name].cjs' },
      },
    },
  },
  renderer: {
    resolve: {
      alias: {
        '@': rendererSrc,
        '@shared': shared,
      },
    },
    plugins: [
      // Must run before the React plugin. Code splitting buys nothing for a locally loaded bundle.
      tanstackRouter({
        target: 'react',
        routesDirectory: resolve(rendererSrc, 'routes'),
        generatedRouteTree: resolve(rendererSrc, 'route-tree.gen.ts'),
        autoCodeSplitting: false,
      }),
      react(),
      tailwindcss(),
    ],
  },
})
