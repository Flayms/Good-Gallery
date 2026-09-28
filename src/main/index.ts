import { join } from 'node:path'
import { MEDIA_SCHEME, THUMB_SCHEME } from '@shared/media-urls'
import { app, BrowserWindow, protocol } from 'electron'
import { openDatabase } from './db'
import { IndexerController } from './indexer'
import { forkIndexer } from './indexer-process'
import { createMediaHandler, createThumbHandler } from './protocols'
import { scheduleRescans } from './rescan'
import { installSecurityHandlers, rendererDevUrl, rendererIndexFile } from './security'
import { registerTrpcIpc } from './trpc/ipc-main'
import { appRouter } from './trpc/router'

// Defaults until they become settings (Phase 6).
const IO_CONCURRENCY = 4
const RESCAN_SCHEDULE = { intervalMs: 30 * 60_000, offlineRetryMs: 60_000 }
const THUMB_CACHE_BYTES = 5 * 1024 ** 3

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

app.whenReady().then(() => {
  installSecurityHandlers()

  const dbPath = join(app.getPath('userData'), 'gallery.db')
  const thumbDir = join(app.getPath('userData'), 'thumbs')
  const db = openDatabase(dbPath, migrationsFolder())
  const indexer = new IndexerController(() =>
    forkIndexer({ dbPath, thumbDir, thumbCacheBytes: THUMB_CACHE_BYTES, concurrency: IO_CONCURRENCY }),
  )
  const stopRescans = scheduleRescans(db, indexer, RESCAN_SCHEDULE)
  app.on('will-quit', () => {
    stopRescans()
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
    createContext: () => ({ db, indexer }),
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
