import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { app, session, shell } from 'electron'

const ALLOWED_PERMISSIONS = new Set(['fullscreen'])

export const rendererIndexFile = join(import.meta.dirname, '../renderer/index.html')

export function rendererDevUrl(): string | undefined {
  return app.isPackaged ? undefined : process.env.ELECTRON_RENDERER_URL
}

/** True if `url` belongs to our own renderer bundle (dev server or packaged file). */
export function isTrustedRendererUrl(url: string): boolean {
  const devUrl = rendererDevUrl()
  if (devUrl) return url.startsWith(`${new URL(devUrl).origin}/`)
  return url.startsWith(pathToFileURL(rendererIndexFile).href)
}

export function installSecurityHandlers(): void {
  session.defaultSession.setPermissionRequestHandler((webContents, permission, callback) => {
    callback(ALLOWED_PERMISSIONS.has(permission) && isTrustedRendererUrl(webContents.getURL()))
  })
  session.defaultSession.setPermissionCheckHandler((webContents, permission) => {
    return ALLOWED_PERMISSIONS.has(permission) && !!webContents && isTrustedRendererUrl(webContents.getURL())
  })

  app.on('web-contents-created', (_event, contents) => {
    // Hash-history routing never triggers `will-navigate`, so any real navigation is foreign.
    contents.on('will-navigate', (event) => event.preventDefault())
    contents.on('will-redirect', (event) => event.preventDefault())
    contents.on('will-attach-webview', (event) => event.preventDefault())
    contents.setWindowOpenHandler(({ url }) => {
      if (url.startsWith('https:')) void shell.openExternal(url)
      return { action: 'deny' }
    })
  })
}
