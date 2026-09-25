import { TRPC_IPC_CHANNEL, type TrpcIpcRequest } from '@shared/trpc-ipc'
import type { AnyTRPCRouter } from '@trpc/server'
import { ipcMain } from 'electron'
import { z } from 'zod'
import { isTrustedRendererUrl } from '../security'
import { createTrpcIpcServer, type TrpcIpcServerOptions } from './ipc-server'

const requestSchema: z.ZodType<TrpcIpcRequest> = z.discriminatedUnion('kind', [
  z.object({
    kind: z.literal('request'),
    id: z.int().nonnegative(),
    type: z.enum(['query', 'mutation', 'subscription']),
    path: z.string().max(200),
    input: z.unknown(),
  }),
  z.object({ kind: z.literal('stop'), id: z.int().nonnegative() }),
])

export function registerTrpcIpc<TRouter extends AnyTRPCRouter>(opts: TrpcIpcServerOptions<TRouter>): void {
  const server = createTrpcIpcServer(opts)
  const connected = new Set<number>()

  ipcMain.on(TRPC_IPC_CHANNEL, (event, raw: unknown) => {
    const contents = event.sender
    const frame = event.senderFrame
    if (!frame || frame !== contents.mainFrame || !isTrustedRendererUrl(frame.url)) return

    const parsed = requestSchema.safeParse(raw)
    if (!parsed.success) return

    const key = contents.id
    if (!connected.has(key)) {
      connected.add(key)
      contents.on('did-start-navigation', (details) => {
        if (details.isMainFrame && !details.isSameDocument) server.disconnect(key)
      })
      contents.once('destroyed', () => {
        connected.delete(key)
        server.disconnect(key)
      })
    }

    server.handle(
      {
        key,
        send: (message) => {
          if (!contents.isDestroyed()) contents.send(TRPC_IPC_CHANNEL, message)
        },
      },
      parsed.data,
    )
  })
}
