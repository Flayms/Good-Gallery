import { join } from 'node:path'
import { app, BrowserWindow } from 'electron'
import { openDatabase } from './db'
import { IndexerController } from './indexer'
import { installSecurityHandlers, rendererDevUrl, rendererIndexFile } from './security'
import { registerTrpcIpc } from './trpc/ipc-main'
import { appRouter } from './trpc/router'

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

  const db = openDatabase(join(app.getPath('userData'), 'gallery.db'), migrationsFolder())
  app.on('will-quit', () => db.$client.close())

  const indexer = new IndexerController()
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
