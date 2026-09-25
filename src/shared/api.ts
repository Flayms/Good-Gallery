import type { TrpcIpcTransport } from './trpc-ipc'

/** Surface exposed to the renderer via `contextBridge` as `window.api`. */
export interface AppApi {
  trpc: TrpcIpcTransport
}
