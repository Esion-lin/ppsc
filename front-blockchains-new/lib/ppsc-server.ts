import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import { resolve } from 'node:path'
import { createInterface } from 'node:readline'
import type { DemoAction, DemoState } from './ppsc-types'

type Bridge = { child?: ChildProcessWithoutNullStreams; state: DemoState; sequence: number }
const globalBridge = globalThis as typeof globalThis & { ppscBridge?: Bridge }
const bridge = globalBridge.ppscBridge ??= {
  state: { runtime: null, busy: false, ready: false, failed: false, error: null, environment: null, deployment: null, snapshot: null, logs: [] },
  sequence: 0,
}
// Preserve the current session when Next reloads this module during development.
bridge.state.environment ??= null
bridge.state.deployment ??= null
bridge.state.runtime ??= null

function log(text: string) {
  bridge.state.logs.push({ id: ++bridge.sequence, text })
  if (bridge.state.logs.length > 1500) bridge.state.logs.splice(0, bridge.state.logs.length - 1500)
}

function worker() {
  if (bridge.child) return bridge.child
  const child = spawn('python3', ['-u', resolve(process.cwd(), '../scripts/web-demo.py')], {
    cwd: resolve(process.cwd(), '..'),
    env: process.env,
    stdio: ['pipe', 'pipe', 'pipe'],
  })
  bridge.child = child
  let stopping = false
  createInterface({ input: child.stdout }).on('line', line => {
    try {
      const event = JSON.parse(line)
      if (event.type === 'log') log(event.text)
      if (event.type === 'runtime') bridge.state.runtime = event.runtime
      if (event.type === 'snapshot') bridge.state.snapshot = event.snapshot
      if (event.type === 'environment') bridge.state.environment = event.environment
      if (event.type === 'deployment') bridge.state.deployment = event.deployment
      if (event.type === 'stopped') {
        stopping = true
        bridge.state.runtime = null
        bridge.state.snapshot = null
        bridge.state.ready = false
        bridge.state.environment = null
        bridge.state.deployment = null
      }
      if (event.type === 'error') {
        bridge.state.error = event.message
        bridge.state.failed = true
        log('ERROR: ' + event.message)
      }
      if (event.type === 'done') {
        bridge.state.busy = false
        bridge.state.ready = event.ready
        bridge.state.failed = event.failed
        if (stopping) {
          bridge.child = undefined
          child.stdin.end()
        }
      }
    } catch {
      log('无法解析执行器输出')
    }
  })
  child.stderr.on('data', () => log('执行器 stderr 异常，请检查本地服务日志。'))
  child.on('error', error => {
    bridge.state.error = `无法启动执行器：${error.message}`
    log(bridge.state.error)
  })
  child.on('close', () => {
    if (bridge.child !== child) return
    bridge.child = undefined
    bridge.state.busy = false
    bridge.state.ready = false
    bridge.state.environment = null
    bridge.state.deployment = null
    bridge.state.runtime = null
    bridge.state.snapshot = null
    bridge.state.failed = true
    bridge.state.error ??= '演示执行器已退出，请重新初始化。'
  })
  return child
}

export function getDemoState() { return bridge.state }

export function executeDemo(action: DemoAction) {
  if (bridge.state.busy) throw new Error('当前命令仍在执行，请等待完成')
  if (action === 'deploy') {
    if (!bridge.state.environment) throw new Error('请先初始化演示环境')
    if (bridge.state.ready || bridge.state.deployment) throw new Error('合约已部署；如需重新部署，请重新初始化环境')
    if (bridge.state.failed) throw new Error('上次执行失败，请重新初始化以避免重复部署')
  } else if (!['init', 'stop'].includes(action) && !bridge.state.ready) {
    throw new Error(bridge.state.environment ? '请先部署演示合约' : '请先初始化演示环境')
  }
  if (['deposit', 'transfer', 'withdraw'].includes(action)) {
    if (bridge.state.failed) throw new Error('上次执行失败，请重新初始化以避免重复交易')
    const expected = { deposit: 0, transfer: 1, withdraw: 2 }[action as 'deposit' | 'transfer' | 'withdraw']
    if (bridge.state.snapshot?.completed !== expected) throw new Error('请按存款、转账、提款顺序执行')
  }
  const child = worker()
  bridge.state.busy = true
  bridge.state.error = null
  if (action === 'init') {
    bridge.state.ready = false
    bridge.state.runtime = null
    bridge.state.snapshot = null
    bridge.state.environment = null
    bridge.state.deployment = null
    bridge.state.failed = false
  }
  log(`ppsc> ${action}`)
  child.stdin.write(JSON.stringify({ action }) + '\n', error => {
    if (error) {
      bridge.state.busy = false
      bridge.state.failed = true
      bridge.state.error = '发送命令失败，请重新初始化'
    }
  })
}
