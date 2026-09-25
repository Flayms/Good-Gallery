import type { TrpcIpcResponse, TrpcIpcTransport } from '@shared/trpc-ipc'
import { TRPCClientError, type TRPCLink } from '@trpc/client'
import { getTransformer, type TransformerOptions } from '@trpc/client/unstable-internals'
import { observable } from '@trpc/server/observable'
import type { AnyRouter, inferClientTypes } from '@trpc/server/unstable-core-do-not-import'

export type IpcLinkOptions<TRouter extends AnyRouter> = {
  transport: TrpcIpcTransport
} & TransformerOptions<inferClientTypes<TRouter>>

/** Terminating tRPC link that talks to the main process over a single IPC channel. */
export function ipcLink<TRouter extends AnyRouter>(opts: IpcLinkOptions<TRouter>): TRPCLink<TRouter> {
  return () => {
    const { transport } = opts
    const transformer = getTransformer(opts.transformer)
    const handlers = new Map<number, (message: TrpcIpcResponse) => void>()
    let nextId = 0

    transport.onMessage((message) => handlers.get(message.id)?.(message))

    return ({ op }) =>
      observable((observer) => {
        const id = ++nextId

        handlers.set(id, (message) => {
          switch (message.kind) {
            case 'started':
              observer.next({ result: { type: 'started' } })
              break
            case 'data':
              observer.next({ result: { type: 'data', data: transformer.output.deserialize(message.data) } })
              if (op.type !== 'subscription') {
                handlers.delete(id)
                observer.complete()
              }
              break
            case 'stopped':
              handlers.delete(id)
              observer.next({ result: { type: 'stopped' } })
              observer.complete()
              break
            case 'error':
              handlers.delete(id)
              observer.error(TRPCClientError.from<TRouter>({ error: transformer.output.deserialize(message.error) }))
              break
          }
        })

        transport.send({
          kind: 'request',
          id,
          type: op.type,
          path: op.path,
          input: transformer.input.serialize(op.input),
        })

        return () => {
          // Still registered means the server hasn't finished: tell it to abort.
          if (handlers.delete(id)) transport.send({ kind: 'stop', id })
        }
      })
  }
}
