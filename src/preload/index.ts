import type { AppApi } from '@shared/api'
import { TRPC_IPC_CHANNEL, type TrpcIpcResponse } from '@shared/trpc-ipc'
import { contextBridge, type IpcRendererEvent, ipcRenderer } from 'electron'

const api: AppApi = {
  trpc: {
    send: (message) => ipcRenderer.send(TRPC_IPC_CHANNEL, message),
    onMessage: (listener) => {
      const handler = (_event: IpcRendererEvent, message: TrpcIpcResponse) => listener(message)
      ipcRenderer.on(TRPC_IPC_CHANNEL, handler)
      return () => ipcRenderer.removeListener(TRPC_IPC_CHANNEL, handler)
    },
  },
}

contextBridge.exposeInMainWorld('api', api)
