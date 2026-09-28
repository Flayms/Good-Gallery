import type { AppRouter } from '@main/trpc/router'
import { QueryClient } from '@tanstack/react-query'
import { createTRPCClient } from '@trpc/client'
import type { inferRouterOutputs } from '@trpc/server'
import { createTRPCOptionsProxy } from '@trpc/tanstack-react-query'
import superjson from 'superjson'
import { ipcLink } from './ipc-link'

export type RouterOutputs = inferRouterOutputs<AppRouter>

export const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: 30_000, refetchOnWindowFocus: false } },
})

export const trpcClient = createTRPCClient<AppRouter>({
  links: [ipcLink({ transport: window.api.trpc, transformer: superjson })],
})

export const trpc = createTRPCOptionsProxy<AppRouter>({ client: trpcClient, queryClient })
