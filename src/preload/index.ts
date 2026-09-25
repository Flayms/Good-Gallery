import type { AppApi } from '@shared/api'
import { contextBridge } from 'electron'

const api: AppApi = {
  platform: process.platform,
}

contextBridge.exposeInMainWorld('api', api)
