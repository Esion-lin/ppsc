import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import { createInterface } from 'node:readline'
import { resolve } from 'node:path'
import { randomUUID } from 'node:crypto'
import type { WalletEnvironmentState } from './wallet-environment-types'

type Artifact = { source: string; manifestHash: string; created: number }
type Bridge = { child?: ChildProcessWithoutNullStreams; state: WalletEnvironmentState; sequence: number; artifacts: Map<string, Artifact> }
const globalState = globalThis as typeof globalThis & { ppscWalletEnvironment?: Bridge }
const empty = () => ({ id: null, rpc: null, database: null, ready: false, deployments: [] })
const bridge: Bridge = globalState.ppscWalletEnvironment ??= {
  state: { busy: false, error: null, environment: empty(), logs: [] }, sequence: 0, artifacts: new Map(),
}
function log(text: string) {
  bridge.state.logs.push({ id: ++bridge.sequence, text })
  bridge.state.logs = bridge.state.logs.slice(-500)
}
export function registerWalletArtifact(source: string, manifestHash: string) {
  for (const [id, artifact] of bridge.artifacts) if (Date.now() - artifact.created > 3600000) bridge.artifacts.delete(id)
  while (bridge.artifacts.size >= 16) bridge.artifacts.delete(bridge.artifacts.keys().next().value!)
  const id = randomUUID()
  bridge.artifacts.set(id, { source, manifestHash, created: Date.now() })
  return id
}
function worker() {
  if (bridge.child) return bridge.child
  const child = spawn('python3', ['-u', resolve(process.cwd(), '../scripts/wallet-environment.py')], {
    cwd: resolve(process.cwd(), '..'), env: process.env, stdio: ['pipe', 'pipe', 'pipe'],
  })
  bridge.child = child
  createInterface({ input: child.stdout }).on('line', line => {
    if (bridge.child !== child) return
    try {
      const event = JSON.parse(line)
      if (event.type === 'log') log(event.text)
      if (event.type === 'state') bridge.state.environment = event.environment
      if (event.type === 'error') { bridge.state.error = event.message; log('ERROR: ' + event.message) }
      if (event.type === 'done') {
        bridge.state.busy = false
        if (!bridge.state.environment.id) { bridge.child = undefined; child.stdin.end() }
      }
    } catch { log('无法解析本地环境输出') }
  })
  child.stderr.on('data', () => log('本地环境执行器出现 stderr 输出'))
  const fail = () => {
    if (bridge.child !== child) return
    bridge.child = undefined
    bridge.state.busy = false
    bridge.state.environment.ready = false
    for (const deployment of bridge.state.environment.deployments) { deployment.status = 'failed'; deployment.runtime = null }
    bridge.state.error = '环境执行器已退出；请停止环境后重新启动。'
  }
  child.on('error', fail)
  child.on('close', fail)
  return child
}
export function getWalletEnvironment() { return bridge.state }

// Share the mutation lock with input uploads/openings so shutdown or deployment
// cannot race a data job on the same local environment.
export function lockWalletDataJob() {
  if (bridge.state.busy) throw new Error('本地环境正在处理其他操作，请稍后重试')
  bridge.state.busy = true
  return () => { bridge.state.busy = false }
}

export function walletRpcTarget(session?: string) {
  if (!session) return 'http://127.0.0.1:8545'
  const environment = bridge.state.environment
  if (session !== environment.id || !environment.ready || !environment.rpc || !/^http:\/\/127\.0\.0\.1:\d+$/.test(environment.rpc)) {
    throw new Error('钱包环境已停止或会话已改变，请在概览重新连接。')
  }
  return environment.rpc
}

export function executeWalletEnvironment(action: 'start' | 'deploy' | 'stop', environmentId?: string, artifactId?: string) {
  if (bridge.state.busy) throw new Error('环境操作正在执行，请等待完成')
  const environment = bridge.state.environment
  let payload: Record<string, unknown> = { action }
  if (action === 'start') {
    if (environment.id || bridge.child) throw new Error('环境已启动；可直接连接或先停止环境')
  } else {
    if (!environment.id || environmentId !== environment.id) throw new Error('环境会话已改变，请刷新页面')
    if (action === 'deploy') {
      if (!environment.ready) throw new Error('请先在钱包概览启动本地环境')
      const artifact = artifactId && bridge.artifacts.get(artifactId)
      if (!artifact || Date.now() - artifact.created > 3600000) throw new Error('编译产物已过期，请重新编译后部署')
      if (environment.deployments.some(item => item.artifactId === artifactId)) throw new Error('该编译产物已提交部署，请查看部署记录')
      payload = { action, environmentId, artifactId, source: artifact.source, manifestHash: artifact.manifestHash }
    } else if (!bridge.child) {
      bridge.state.environment = empty(); bridge.state.error = null
      return
    }
  }
  bridge.state.busy = true
  bridge.state.error = null
  log('wallet> ' + action)
  worker().stdin.write(JSON.stringify(payload) + '\n', error => {
    if (error) { bridge.state.busy = false; bridge.state.error = '发送环境命令失败，请停止后重试' }
  })
}
