import { indexerRouter } from './routers/indexer'
import { librariesRouter } from './routers/libraries'
import { mediaRouter } from './routers/media'
import { settingsRouter } from './routers/settings'
import { tagsRouter } from './routers/tags'
import { router } from './trpc'

export const appRouter = router({
  libraries: librariesRouter,
  media: mediaRouter,
  tags: tagsRouter,
  indexer: indexerRouter,
  settings: settingsRouter,
})

export type AppRouter = typeof appRouter
