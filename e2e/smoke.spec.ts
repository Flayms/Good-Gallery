import { execFile } from 'node:child_process'
import { mkdir, mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { promisify } from 'node:util'
import {
  type ElectronApplication,
  _electron as electron,
  expect,
  type Locator,
  type Page,
  test,
} from '@playwright/test'
import { ExifTool, type WriteTags } from 'exiftool-vendored'
import ffmpegPath from 'ffmpeg-static'
import sharp from 'sharp'

// Smoke test of the built app (`out/`, or a packaged executable via GG_E2E_APP) against a generated library.

const projectRoot = resolve(import.meta.dirname, '..')
// A window behind other windows counts as hidden, and hidden windows never lay out the masonry grid.
const CHROMIUM_FLAGS = ['--disable-features=CalculateNativeWinOcclusion', '--disable-backgrounding-occluded-windows']
const INDEXING_TIMEOUT = 60_000

const FIXTURES: { name: string; width: number; height: number; color: string; tags: WriteTags }[] = [
  { name: 'beach.jpg', width: 1200, height: 800, color: '#3b82f6', tags: { Subject: ['Beach', 'Summer'] } },
  { name: 'forest.jpg', width: 800, height: 1200, color: '#16a34a', tags: { HierarchicalSubject: ['Places|Forest'] } },
  { name: 'city.jpg', width: 1000, height: 1000, color: '#f97316', tags: { Keywords: ['Summer'] } },
]
const VIDEO = 'clip.mp4'

let tempDir: string
let library: string
let app: ElectronApplication
let page: Page

async function createLibrary(dir: string): Promise<void> {
  await mkdir(dir, { recursive: true })
  const exiftool = new ExifTool()
  try {
    for (const { name, width, height, color, tags } of FIXTURES) {
      const file = join(dir, name)
      await sharp({ create: { width, height, channels: 3, background: color } })
        .jpeg()
        .toFile(file)
      await exiftool.write(file, tags, { writeArgs: ['-overwrite_original'] })
    }
  } finally {
    await exiftool.end()
  }
  if (!ffmpegPath) throw new Error('ffmpeg-static has no binary for this platform')
  await promisify(execFile)(ffmpegPath, [
    ...['-f', 'lavfi', '-i', 'testsrc=size=320x240:duration=2:rate=10'],
    ...['-pix_fmt', 'yuv420p', join(dir, VIDEO)],
  ])
}

function naturalWidth(image: Locator): Promise<number> {
  return image.evaluate((element) => (element instanceof HTMLImageElement ? element.naturalWidth : 0))
}

test.beforeAll(async () => {
  tempDir = await mkdtemp(join(tmpdir(), 'gg-e2e-'))
  library = join(tempDir, 'library')
  await createLibrary(library)

  const { ELECTRON_RENDERER_URL: _, ...env } = process.env
  const executable = process.env.GG_E2E_APP
  app = await electron.launch({
    executablePath: executable,
    args: executable ? CHROMIUM_FLAGS : [...CHROMIUM_FLAGS, '.'],
    cwd: projectRoot,
    env: { ...env, GG_USER_DATA_DIR: join(tempDir, 'user-data') },
  })
  page = await app.firstWindow()
})

test.afterAll(async () => {
  await app?.close()
  await rm(tempDir, { recursive: true, force: true, maxRetries: 5 })
})

test('indexes a library, browses it, filters by tag and shows details', async () => {
  await page.getByRole('link', { name: 'Settings' }).click()
  await page.getByLabel('Library folder path').fill(library)
  await page.getByRole('button', { name: 'Add', exact: true }).click()
  await expect(page.getByText(library)).toBeVisible()

  await page.getByRole('link', { name: 'All libraries' }).click()
  const tiles = page.getByRole('link', { name: /\.(jpg|mp4)\b/ })
  await expect(tiles).toHaveCount(FIXTURES.length + 1, { timeout: INDEXING_TIMEOUT })
  // Thumbnails (sharp, ffmpeg for the video) are rendered by the indexer and served over gg-thumb://.
  const thumbsLoaded = () =>
    tiles.evaluateAll((links) => links.every((link) => (link.querySelector('img')?.naturalWidth ?? 0) > 0))
  await expect.poll(thumbsLoaded, { timeout: INDEXING_TIMEOUT }).toBe(true)

  // Keyboard navigation follows the gallery order.
  const names = await tiles.evaluateAll((links) => links.map((link) => link.getAttribute('title')))
  await tiles.first().click()
  await expect(page.getByRole('dialog', { name: names[0] ?? '' })).toBeVisible()
  await page.keyboard.press('ArrowRight')
  await expect(page.getByRole('dialog', { name: names[1] ?? '' })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog')).toBeHidden()

  await page.getByRole('combobox', { name: 'Filter by tags' }).fill('bea')
  await page.getByRole('option', { name: /Beach/ }).click()
  // Closes the suggestions, which stay open for adding more tags.
  await page.keyboard.press('Escape')
  await expect(tiles).toHaveCount(1)

  await tiles.first().click()
  const viewer = page.getByRole('dialog', { name: 'beach.jpg' })
  // The original is streamed over gg-media://.
  await expect.poll(() => naturalWidth(viewer.locator('img'))).toBe(1200)
  const transform = () => viewer.locator('img').evaluate((image) => image.style.transform)
  await viewer.locator('img').hover()
  await page.mouse.wheel(0, -300)
  await expect.poll(transform).not.toContain('scale(1)')
  await page.keyboard.press('0')
  await expect.poll(transform).toContain('scale(1)')
  await page.keyboard.press('i')
  await expect(viewer.getByText(join(library, 'beach.jpg'))).toBeVisible()
  await expect(viewer.getByRole('link', { name: 'Summer' })).toBeVisible()

  // Clicking a tag filters the gallery by it and closes the viewer.
  await viewer.getByRole('link', { name: 'Summer' }).click()
  await expect(page.getByRole('dialog')).toBeHidden()
  await expect(page.getByRole('button', { name: 'Remove Summer' })).toBeVisible()
  await expect(tiles).toHaveCount(1)

  // The cache is owned by the indexer process; settings changes and cache requests are forwarded to it.
  await page.getByRole('link', { name: 'Settings' }).click()
  await page.getByRole('combobox', { name: 'Maximum cache size' }).click()
  await page.getByRole('option', { name: '1 GB' }).click()
  await expect(page.getByRole('combobox', { name: 'Maximum cache size' })).toHaveText('1 GB')
  await expect(page.getByText(/^\d+(\.\d)? KB$/)).toBeVisible()
  page.once('dialog', (dialog) => void dialog.accept())
  await page.getByRole('button', { name: 'Clear' }).click()
  await expect(page.getByText('0 B', { exact: true })).toBeVisible()
})
