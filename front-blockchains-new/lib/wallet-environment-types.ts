import type { DemoRuntime, DemoDeployment } from './ppsc-types'

export type WalletDeployment = {
  id: string; artifactId?: string; name: string; status: 'deploying' | 'ready' | 'failed'
  error?: string; gateway?: string; control?: string; contractId?: string
  manifestHash?: string; runtimeHash?: string; walletCompatible?: boolean
  contracts?: DemoDeployment['contracts']; runtime?: DemoRuntime | null
  uploadUrl?: string; publicKeyPath?: string
}
export type WalletEnvironment = {
  id: string | null; rpc: string | null; database: string | null; ready: boolean
  deployments: WalletDeployment[]
}
export type WalletEnvironmentState = {
  busy: boolean; error: string | null; environment: WalletEnvironment
  logs: { id: number; text: string }[]
}
