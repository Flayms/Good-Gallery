import { initTRPC } from '@trpc/server'
import superjson from 'superjson'
import type { Db } from '../db'
import type { IndexerController } from '../indexer'
import type { SettingsStore } from '../settings'

/** Native shell integration, abstracted for tests. */
export interface Desktop {
  showItemInFolder(path: string): void
  /** Resolves with the chosen folder, or undefined if the dialog was cancelled. */
  pickFolder(): Promise<string | undefined>
}

export interface Context {
  db: Db
  indexer: IndexerController
  settings: SettingsStore
  desktop: Desktop
}

const t = initTRPC.context<Context>().create({ transformer: superjson, isServer: true })

export const router = t.router
export const publicProcedure = t.procedure
export const createCallerFactory = t.createCallerFactory
