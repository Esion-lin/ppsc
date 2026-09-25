export const networks = { '31337': { name: 'Anvil Local', hex: '0x7a69', explorer: '' } } as const
export type NetworkId = keyof typeof networks
export const selectors = {
  controlPlane: '0xe9035955', balanceVariable: '0x6d8082cd', stateVariables: '0x94d94c83',
  executionStatus: '0xb2973c91', invocationNonceUsed: '0xd1db9205',
  createAccount: '0x649aab54', deposit: '0x007b8887', withdraw: '0x5c5453cd', transfer: '0x926c81aa',
} as const
export type Invocation = 'createAccount' | 'deposit' | 'withdraw' | 'transfer'
export const executionLabels = ['未登记', '等待委员会', '已分配委员会', '密态计算中', '协议切换中', '等待结果回写', '执行完成', '执行失败', '已取消']
export type Activity = {
  hash: string; account: string; chain: NetworkId; session?: string; title: string; time: number
  status: 'pending' | 'confirmed' | 'failed'; execution?: string; control?: string; stage?: number
}
export type CompiledContract = { artifactId: string; name: string; manifestHash: string; runtimeHash: string; files: Record<string, string> }
const walletClient = globalThis as typeof globalThis & { ppscWalletRpcSession?: string }
export function setWalletSession(session?: string) { walletClient.ppscWalletRpcSession = session }
export const short = (value: string) => value ? `${value.slice(0, 6)}…${value.slice(-4)}` : '未配置'
export function validAddress(value: string) { return /^0x[\da-f]{40}$/i.test(value) && !/^0x0{40}$/.test(value) }
export function isDataId(value: string) { return /^0x[\da-f]{64}$/i.test(value) && !/^0x0{64}$/.test(value) }
export function errorMessage(error: unknown): string { return error instanceof Error ? error.message : '操作失败，请稍后重试。' }
export async function rpc<T = string>(method: string, params: unknown[] = []): Promise<T> {
  const response = await fetch('/api/wallet/rpc', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ method, params, session: walletClient.ppscWalletRpcSession }), signal: AbortSignal.timeout(15000) })
  const result = await response.json()
  if (!response.ok || result.error) throw new Error(result.error || '本地链请求失败')
  return result.result as T
}
// ABI encoding is intentionally limited to the fixed-width types in the generated Gateway.
export function encodeCall(method: keyof typeof selectors, args: (string | bigint)[] = []) {
  return selectors[method] + args.map(value => {
    if (typeof value === 'bigint') {
      if (value < 0n || value >= 2n ** 64n) throw new Error('uint64 参数超出范围')
      return value.toString(16).padStart(64, '0')
    }
    if (!/^0x(?:[\da-f]{40}|[\da-f]{64})$/i.test(value)) throw new Error('合约参数格式错误')
    return value.slice(2).toLowerCase().padStart(64, '0')
  }).join('')
}
export async function call(to: string, method: keyof typeof selectors, args: (string | bigint)[] = [], from?: string) {
  return rpc('eth_call', [{ to, data: encodeCall(method, args), ...(from ? { from } : {}) }, 'latest'])
}
export function words(value: string, count: number) {
  if (!new RegExp(`^0x[0-9a-fA-F]{${64 * count}}$`).test(value)) throw new Error('合约返回数据不匹配，请核实 Gateway 地址。')
  return Array.from({ length: count }, (_, index) => '0x' + value.slice(2 + index * 64, 66 + index * 64))
}
export async function controlAddress(gateway: string) {
  const [word] = words(await call(gateway, 'controlPlane'), 1)
  const address = '0x' + word.slice(-40)
  if (!validAddress(address)) throw new Error('ControlPlane 地址无效')
  return address
}
export async function readWallet(account: string, gateway: string) {
  if (BigInt(await rpc('eth_chainId')) !== 31337n) throw new Error('仅支持 chain ID 31337 的本地 Anvil 链。')
  if (!validAddress(gateway)) return null
  if (await rpc('eth_getCode', [gateway, 'latest']) === '0x') throw new Error('本地链上找不到 Gateway，请填写实际部署地址。')
  const control = await controlAddress(gateway)
  const [variable] = words(await call(gateway, 'balanceVariable', [account]), 1)
  const state = words(await call(control, 'stateVariables', [variable]), 6)
  return { exists: BigInt(state[5]) !== 0n, dataId: state[1], version: BigInt(state[4]).toString() }
}
// Random uint48 nonces avoid collisions with sequential CLI invocations.
export function invocationNonce() {
  const bytes = crypto.getRandomValues(new Uint8Array(6))
  return bytes.reduce((value, byte) => (value << 8n) | BigInt(byte), 0n)
}
export function restoreActivities(value: string | null): Activity[] {
  try {
    const items: unknown = JSON.parse(value || '[]')
    if (!Array.isArray(items)) return []
    return items.filter((a): a is Activity => a && /^0x[\da-f]{64}$/i.test(a.hash) &&
      typeof a.account === 'string' && validAddress(a.account) && a.chain === '31337' &&
      typeof a.title === 'string' && typeof a.time === 'number' && Number.isFinite(a.time) &&
      ['pending', 'confirmed', 'failed'].includes(a.status) &&
      (a.session === undefined || typeof a.session === 'string' && /^[a-f0-9]{32}$/.test(a.session)) &&
      (!a.execution || isDataId(a.execution)) && (!a.control || validAddress(a.control)) &&
      (a.stage === undefined || Number.isInteger(a.stage) && a.stage >= 0 && a.stage <= 8)).slice(0, 100)
  } catch { return [] }
}
