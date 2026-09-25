export type DemoAction = 'init' | 'deploy' | 'deposit' | 'transfer' | 'withdraw' | 'status' | 'stop'
export type DemoEnvironment = { rpc: string; database: string }
export type DemoDeployment = {
  contracts: { name: string; address: string; transactionHash: string; blockNumber: number }[]
}
export type DemoSnapshot = {
  completed: number
  rpc: string
  database: string
  gateway: string
  control: string
  privateSender: string
  privateReceiver: string
  senderDataId: string
  receiverDataId: string
  root: string
}
export type DemoRuntime = {
  backend: string
  daemonPid: number
  cryptoPid: number
  running: boolean
  publicKeyFingerprint: string
}
export type DemoState = {
  runtime: DemoRuntime | null
  busy: boolean
  ready: boolean
  failed: boolean
  error: string | null
  environment: DemoEnvironment | null
  deployment: DemoDeployment | null
  snapshot: DemoSnapshot | null
  logs: { id: number; text: string }[]
}
