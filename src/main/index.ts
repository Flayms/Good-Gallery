import { join } from 'node:path'
import { MEDIA_SCHEME, THUMB_SCHEME } from '@shared/media-urls'
import type { Settings } from '@shared/settings'
import { app, BrowserWindow, dialog, type OpenDialogOptions, protocol, shell } from 'electron'
import { openDatabase } from './db'
import { IndexerController } from './indexer'
import { forkIndexer } from './indexer-process'
import { createMediaHandler, createThumbHandler } from './protocols'
import { scheduleRescans } from './rescan'
import { installSecurityHandlers, rendererDevUrl, rendererIndexFile } from './security'
import { SettingsStore } from './settings'
import { registerTrpcIpc } from './trpc/ipc-main'
import { appRouter } from './trpc/router'
import type { Desktop } from './trpc/trpc'

const OFFLINE_RETRY_MS = 60_000

// Isolated profile for e2e tests, which also run against packaged builds.
if (process.env.GG_USER_DATA_DIR) app.setPath('userData', process.env.GG_USER_DATA_DIR)

protocol.registerSchemesAsPrivileged([
  { scheme: THUMB_SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: true } },
  // `stream` lets `<video>` play and seek while the response is still loading.
  { scheme: MEDIA_SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true } },
])

function createWindow(): void {
  const win = new BrowserWindow({
    width: 1400,
    height: 900,
    minWidth: 800,
    minHeight: 600,
    show: false,
    backgroundColor: '#09090b',
    autoHideMenuBar: true,
    webPreferences: {
      preload: join(import.meta.dirname, '../preload/index.cjs'),
      contextIsolation: true,
      sandbox: true,
      nodeIntegration: false,
      webSecurity: true,
    },
  })

  win.once('ready-to-show', () => win.show())

  const devUrl = rendererDevUrl()
  if (devUrl) {
    void win.loadURL(devUrl)
  } else {
    void win.loadFile(rendererIndexFile)
  }
}

function migrationsFolder(): string {
  return app.isPackaged ? join(process.resourcesPath, 'migrations') : join(app.getAppPath(), 'drizzle')
}

function indexerConfig(settings: Settings) {
  return { thumbCacheBytes: Math.round(settings.thumbCacheGiB * 1024 ** 3), concurrency: settings.ioConcurrency }
}

const desktop: Desktop = {
  showItemInFolder: (path) => shell.showItemInFolder(path),
  pickFolder: async () => {
    const options: OpenDialogOptions = { title: 'Add library', properties: ['openDirectory'] }
    const window = BrowserWindow.getFocusedWindow()
    const result = await (window ? dialog.showOpenDialog(window, options) : dialog.showOpenDialog(options))
    return result.canceled ? undefined : result.filePaths[0]
  },
}

app.whenReady().then(() => {
  installSecurityHandlers()

  const dbPath = join(app.getPath('userData'), 'gallery.db')
  const thumbDir = join(app.getPath('userData'), 'thumbs')
  const db = openDatabase(dbPath, migrationsFolder())
  const settings = new SettingsStore(db)
  const indexer = new IndexerController(() => forkIndexer({ dbPath, thumbDir, ...indexerConfig(settings.get()) }))
  const rescans = scheduleRescans(db, indexer, {
    intervalMs: settings.get().rescanIntervalMinutes * 60_000,
    offlineRetryMs: OFFLINE_RETRY_MS,
  })
  settings.on('change', (next, previous) => {
    if (next.rescanIntervalMinutes !== previous.rescanIntervalMinutes) {
      rescans.setInterval(next.rescanIntervalMinutes * 60_000)
    }
    if (next.thumbCacheGiB !== previous.thumbCacheGiB || next.ioConcurrency !== previous.ioConcurrency) {
      indexer.configure(indexerConfig(next))
    }
  })
  app.on('will-quit', () => {
    rescans.stop()
    indexer.dispose()
    db.$client.close()
  })

  protocol.handle(
    THUMB_SCHEME,
    createThumbHandler({ db, cacheDir: thumbDir, render: (mediaId) => indexer.requestThumbnail(mediaId) }),
  )
  protocol.handle(MEDIA_SCHEME, createMediaHandler({ db }))

  registerTrpcIpc({
    router: appRouter,
    createContext: () => ({ db, indexer, settings, desktop }),
    onError: ({ error, path }) => {
      if (error.code === 'INTERNAL_SERVER_ERROR') console.error(`tRPC ${path}:`, error)
    },
  })

  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})
