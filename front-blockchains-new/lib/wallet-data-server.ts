import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'
import { resolve } from 'node:path'
import { createHash } from 'node:crypto'
import { getWalletEnvironment, lockWalletDataJob } from './wallet-environment-server'
import type { WalletDataJob } from './wallet-data-types'

export type DataRequest = { id: string; environmentId: string; gateway: string; owner: string; kind: 'input' | 'balance'; mode?: 'amount' | 'file'; amount?: string; ciphertext?: string }
const globalJobs = globalThis as typeof globalThis & { walletDataJobs?: Map<string, { job: WalletDataJob; digest: string }> }
const jobs = globalJobs.walletDataJobs ??= new Map()
export function getWalletDataJob(id: string) { return jobs.get(id)?.job }

export function startWalletDataJob(request: DataRequest) {
  const digest = createHash('sha256').update(JSON.stringify(request)).digest('hex')
  const previous = jobs.get(request.id)
  if (previous) {
    if (previous.digest !== digest) throw new Error('请求 ID 已用于另一份数据')
    return previous.job
  }
  const env = getWalletEnvironment().environment
  if (!env.ready || request.environmentId !== env.id) throw new Error('请先启动并连接钱包本地环境')
  const deployment = env.deployments.find(item => item.gateway?.toLowerCase() === request.gateway.toLowerCase())
  if (!deployment || deployment.status !== 'ready' || !deployment.runtime) throw new Error('当前合约的 daemon 未就绪')
  if (request.kind === 'balance' && !deployment.walletCompatible) throw new Error('当前合约不支持钱包余额查询')
  const unlock = lockWalletDataJob()
  while (jobs.size >= 100) jobs.delete(jobs.keys().next().value!)
  const job: WalletDataJob = { id: request.id, kind: request.kind, environmentId: request.environmentId,
    gateway: request.gateway, owner: request.owner, status: 'pending', created: Date.now() }
  jobs.set(request.id, { job, digest })
  const child = spawn('python3', ['-u', resolve(process.cwd(), '../scripts/wallet-data.py')], { detached: true, stdio: ['pipe', 'pipe', 'pipe'] })
  let released = false
  const finish = () => { if (!released) { released = true; clearTimeout(timer); unlock() } }
  const timer = setTimeout(() => {
    job.status = 'failed'; job.error = '数据操作超时，请检查本地 daemon'
    if (child.pid) { try { process.kill(-child.pid, 'SIGTERM') } catch { child.kill('SIGTERM') } }
  }, 150000)
  createInterface({ input: child.stdout }).on('line', line => {
    if (job.status === 'failed') return
    try {
      const event = JSON.parse(line)
      for (const field of ['status', 'dataId', 'amount', 'version', 'executionId', 'transactionHash', 'error'] as const) {
        if (event[field] !== undefined) Object.assign(job, { [field]: event[field] })
      }
    } catch { job.status = 'failed'; job.error = '无法解析数据服务结果' }
  })
  child.stderr.resume() // Never expose keys, ciphertext or native-process stderr.
  child.on('error', () => { job.status = 'failed'; job.error = '无法启动本地数据服务'; finish() })
  child.on('close', () => {
    if (!['completed', 'failed'].includes(job.status)) { job.status = 'failed'; job.error = '数据服务提前退出' }
    finish()
  })
  child.stdin.on('error', () => { job.status = 'failed'; job.error = '发送数据失败' })
  child.stdin.end(JSON.stringify({ ...request, config: { rpc: env.rpc, gateway: deployment.gateway,
    control: deployment.control, contractId: deployment.contractId, publicKeyPath: deployment.publicKeyPath,
    uploadUrl: deployment.uploadUrl, daemonPid: deployment.runtime.daemonPid, cryptoPid: deployment.runtime.cryptoPid } }) + '\n')
  return job
}
