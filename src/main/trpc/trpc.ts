import { initTRPC } from '@trpc/server'
import superjson from 'superjson'
import type { Db } from '../db'
import type { IndexerController } from '../indexer'

export interface Context {
  db: Db
  indexer: IndexerController
}

const t = initTRPC.context<Context>().create({ transformer: superjson, isServer: true })

export const router = t.router
export const publicProcedure = t.procedure
export const createCallerFactory = t.createCallerFactory
