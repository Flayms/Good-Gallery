import { on } from 'node:events'
import type { TrpcIpcResponse, TrpcIpcTransport } from '@shared/trpc-ipc'
import { createTRPCClient, isTRPCClientError } from '@trpc/client'
import { initTRPC, TRPCError } from '@trpc/server'
import superjson from 'superjson'
import { describe, expect, it, vi } from 'vitest'
import { z } from 'zod'
import { ipcLink } from '../../renderer/src/lib/ipc-link'
import { IndexerController } from '../indexer'
import { createTrpcIpcServer } from './ipc-server'

const t = initTRPC.context<{ indexer: IndexerController }>().create({ transformer: superjson })

const testRouter = t.router({
  echo: t.procedure.input(z.object({ at: z.date() })).query(({ input }) => ({ at: input.at, tags: new Set(['a']) })),
  fail: t.procedure.mutation(() => {
    throw new TRPCError({ code: 'NOT_FOUND', message: 'nope' })
  }),
  status: t.procedure.subscription(async function* ({ ctx, signal }) {
    const updates = on(ctx.indexer, 'status', { signal })
    yield ctx.indexer.status.queue
    for await (const [status] of updates) yield status.queue as number[]
  }),
})

/** Connects link and server in-process, cloning messages like Electron IPC does. */
function connect() {
  const indexer = new IndexerController()
  const server = createTrpcIpcServer({ router: testRouter, createContext: () => ({ indexer }) })
  const listeners = new Set<(message: TrpcIpcResponse) => void>()
  const client = {
    key: 1,
    send: (message: TrpcIpcResponse) => {
      const copy = structuredClone(message)
      queueMicrotask(() => {
        for (const listener of listeners) listener(copy)
      })
    },
  }
  const transport: TrpcIpcTransport = {
    send: (message) => {
      const copy = structuredClone(message)
      queueMicrotask(() => server.handle(client, copy))
    },
    onMessage: (listener) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
  }
  const trpc = createTRPCClient<typeof testRouter>({ links: [ipcLink({ transport, transformer: superjson })] })
  return { trpc, indexer }
}

describe('tRPC over IPC', () => {
  it('round-trips superjson types', async () => {
    const { trpc } = connect()
    const at = new Date('2026-01-02T03:04:05Z')

    expect(await trpc.echo.query({ at })).toEqual({ at, tags: new Set(['a']) })
  })

  it('surfaces errors as TRPCClientError with their code', async () => {
    const { trpc } = connect()

    const error = await trpc.fail.mutate().catch((e: unknown) => e)

    expect(isTRPCClientError(error) && error.data?.code).toBe('NOT_FOUND')
  })

  it('reports validation errors', async () => {
    const { trpc } = connect()

    const error = await trpc.echo.query({ at: 'x' as unknown as Date }).catch((e: unknown) => e)

    expect(isTRPCClientError(error) && error.data?.code).toBe('BAD_REQUEST')
  })

  it('streams subscription events and aborts the server side on unsubscribe', async () => {
    const { trpc, indexer } = connect()
    const received: number[][] = []

    const subscription = trpc.status.subscribe(undefined, { onData: (queue) => received.push(queue) })
    await vi.waitFor(() => expect(received).toEqual([[]]))
    indexer.requestScan(7)
    await vi.waitFor(() => expect(received).toEqual([[], [7]]))

    subscription.unsubscribe()
    await vi.waitFor(() => expect(indexer.listenerCount('status')).toBe(0))
  })
})
