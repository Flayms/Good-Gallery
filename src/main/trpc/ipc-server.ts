import type { TrpcIpcRequest, TrpcIpcResponse } from '@shared/trpc-ipc'
import {
  type AnyTRPCRouter,
  callTRPCProcedure,
  getErrorShape,
  getTRPCErrorFromUnknown,
  type inferRouterContext,
  TRPCError,
} from '@trpc/server'

type RequestMessage = Extract<TrpcIpcRequest, { kind: 'request' }>

export interface TrpcIpcClient {
  /** Identifies the connection; request ids are only unique per client. */
  key: number
  send(message: TrpcIpcResponse): void
}

export interface TrpcIpcServerOptions<TRouter extends AnyTRPCRouter> {
  router: TRouter
  createContext: (client: TrpcIpcClient) => inferRouterContext<TRouter> | Promise<inferRouterContext<TRouter>>
  onError?: (opts: { error: TRPCError; path: string; type: RequestMessage['type'] }) => void
}

function isAsyncIterable(value: unknown): value is AsyncIterable<unknown> {
  return typeof value === 'object' && value !== null && Symbol.asyncIterator in value
}

/** Transport-agnostic tRPC request handler; see `registerTrpcIpc` for the Electron wiring. */
export function createTrpcIpcServer<TRouter extends AnyTRPCRouter>(opts: TrpcIpcServerOptions<TRouter>) {
  const { router, createContext, onError } = opts
  const config = router._def._config
  const { transformer } = config
  const inflight = new Map<number, Map<number, AbortController>>()

  async function run(client: TrpcIpcClient, req: RequestMessage): Promise<void> {
    let controllers = inflight.get(client.key)
    if (!controllers) {
      controllers = new Map()
      inflight.set(client.key, controllers)
    }
    if (controllers.has(req.id)) return

    const abort = new AbortController()
    const { signal } = abort
    controllers.set(req.id, abort)

    let ctx: inferRouterContext<TRouter> | undefined
    let input: unknown
    try {
      ctx = await createContext(client)
      const result = await callTRPCProcedure({
        router,
        path: req.path,
        type: req.type,
        ctx,
        signal,
        batchIndex: 0,
        getRawInput: async () => {
          input = transformer.input.deserialize(req.input)
          return input
        },
      })

      if (req.type !== 'subscription') {
        if (!signal.aborted) client.send({ kind: 'data', id: req.id, data: transformer.output.serialize(result) })
        return
      }

      if (!isAsyncIterable(result)) {
        throw new TRPCError({ code: 'INTERNAL_SERVER_ERROR', message: 'Subscriptions must return an async iterable' })
      }
      client.send({ kind: 'started', id: req.id })
      for await (const value of result) {
        if (signal.aborted) break
        client.send({ kind: 'data', id: req.id, data: transformer.output.serialize(value) })
      }
      if (!signal.aborted) client.send({ kind: 'stopped', id: req.id })
    } catch (cause) {
      if (signal.aborted) return
      const error = getTRPCErrorFromUnknown(cause)
      onError?.({ error, path: req.path, type: req.type })
      const shape = getErrorShape({ config, error, type: req.type, path: req.path, input, ctx })
      client.send({ kind: 'error', id: req.id, error: transformer.output.serialize(shape) })
    } finally {
      controllers.delete(req.id)
    }
  }

  return {
    handle(client: TrpcIpcClient, message: TrpcIpcRequest): void {
      if (message.kind === 'stop') inflight.get(client.key)?.get(message.id)?.abort()
      else void run(client, message)
    },
    /** Aborts all in-flight operations of a client (window closed or reloaded). */
    disconnect(key: number): void {
      const controllers = inflight.get(key)
      inflight.delete(key)
      for (const controller of controllers?.values() ?? []) controller.abort()
    },
  }
}
