export const TRPC_IPC_CHANNEL = 'trpc'

export type TrpcIpcRequest =
  | {
      kind: 'request'
      id: number
      type: 'query' | 'mutation' | 'subscription'
      path: string
      /** Serialized with the router's transformer. */
      input: unknown
    }
  | { kind: 'stop'; id: number }

export type TrpcIpcResponse =
  | { kind: 'data'; id: number; data: unknown }
  | { kind: 'started'; id: number }
  | { kind: 'stopped'; id: number }
  | { kind: 'error'; id: number; error: unknown }

export interface TrpcIpcTransport {
  send(message: TrpcIpcRequest): void
  /** Returns an unsubscribe function. */
  onMessage(listener: (message: TrpcIpcResponse) => void): () => void
}
