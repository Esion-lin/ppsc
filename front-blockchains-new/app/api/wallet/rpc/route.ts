import { NextRequest, NextResponse } from 'next/server'
import { selectors, validAddress } from '@/lib/wallet'
import { walletRpcTarget } from '@/lib/wallet-environment-server'

export const runtime = 'nodejs'
export const dynamic = 'force-dynamic'
const lengths: Record<string, number> = {
  [selectors.controlPlane]: 0, [selectors.balanceVariable]: 1, [selectors.stateVariables]: 1,
  [selectors.executionStatus]: 1, [selectors.invocationNonceUsed]: 2,
  [selectors.createAccount]: 2, [selectors.deposit]: 3, [selectors.withdraw]: 3, [selectors.transfer]: 4,
}
const mutations: string[] = [selectors.createAccount, selectors.deposit, selectors.withdraw, selectors.transfer]
async function upstream(rpcUrl: string, method: string, params: unknown[]) {
  const response = await fetch(rpcUrl, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }), cache: 'no-store', signal: AbortSignal.timeout(10000) })
  if (!response.ok) throw new Error('本地 Anvil 服务请求失败')
  const data = await response.json()
  if (data.error) throw new Error(String(data.error.message || '本地合约调用失败'))
  return data.result
}
function validCall(value: unknown, mutation: boolean) {
  if (!value || typeof value !== 'object') return false
  const tx = value as Record<string, unknown>
  if (Object.keys(tx).some(key => !['to', 'from', 'data'].includes(key))) return false
  if (typeof tx.to !== 'string' || !validAddress(tx.to) || typeof tx.data !== 'string') return false
  if (tx.from !== undefined && (typeof tx.from !== 'string' || !validAddress(tx.from))) return false
  const selector = tx.data.slice(0, 10)
  const count = lengths[selector]
  return count !== undefined && new RegExp(`^0x[0-9a-fA-F]{${8 + count * 64}}$`).test(tx.data) &&
    (!mutation || typeof tx.from === 'string' && mutations.includes(selector))
}
export async function POST(request: NextRequest) {
  const host = request.headers.get('host') || ''
  if (process.env.NODE_ENV !== 'development' || !/^(localhost|127\.0\.0\.1)(:\d+)?$/.test(host) ||
    request.headers.get('origin') !== `http://${host}` || ![null, 'same-origin'].includes(request.headers.get('sec-fetch-site'))) {
    return NextResponse.json({ error: '本地链接口仅开放给本机 development 同源页面。' }, { status: 403 })
  }
  try {
    const reader = request.body?.getReader()
    if (!reader) throw new Error('请求为空')
    const chunks: Uint8Array[] = []; let size = 0
    while (true) {
      const { value, done } = await reader.read()
      if (done) break
      size += value.length
      if (size > 4096) { await reader.cancel(); return NextResponse.json({ error: '请求过大' }, { status: 413 }) }
      chunks.push(value)
    }
    const { method, params, session } = JSON.parse(Buffer.concat(chunks).toString('utf8'))
    if (session !== undefined && (typeof session !== 'string' || !/^[a-f0-9]{32}$/.test(session))) return NextResponse.json({ error: '环境会话格式错误' }, { status: 400 })
    if (!Array.isArray(params)) return NextResponse.json({ error: '参数格式错误' }, { status: 400 })
    const valid = (['eth_chainId', 'eth_accounts'].includes(method) && params.length === 0) ||
      (method === 'eth_call' && params.length === 2 && params[1] === 'latest' && validCall(params[0], false)) ||
      (method === 'eth_sendTransaction' && params.length === 1 && validCall(params[0], true)) ||
      (method === 'eth_getCode' && params.length === 2 && typeof params[0] === 'string' && validAddress(params[0]) && params[1] === 'latest') ||
      (method === 'eth_getTransactionReceipt' && params.length === 1 && typeof params[0] === 'string' && /^0x[\da-f]{64}$/i.test(params[0]))
    if (!valid) return NextResponse.json({ error: '仅支持钱包所需的本地链查询与密态 Gateway 调用。' }, { status: 400 })
    const rpcUrl = walletRpcTarget(session)
    const chainId = await upstream(rpcUrl, 'eth_chainId', [])
    if (BigInt(chainId) !== 31337n) return NextResponse.json({ error: '仅支持 chain ID 31337 的本地链。' }, { status: 409 })
    const client = await upstream(rpcUrl, 'web3_clientVersion', [])
    if (typeof client !== 'string' || !client.toLowerCase().includes('anvil')) return NextResponse.json({ error: '请连接本地 Anvil 开发节点。' }, { status: 409 })
    const result = method === 'eth_chainId' ? chainId : await upstream(rpcUrl, method, params)
    return NextResponse.json({ result }, { headers: { 'Cache-Control': 'no-store' } })
  } catch (error) {
    const message = error instanceof Error ? error.message : '请求失败'
    return NextResponse.json({ error: message === 'fetch failed' || message.includes('abort') ? '无法连接本地链，请先运行 anvil --port 8545 --chain-id 31337。' : message.slice(0, 500) }, { status: 502 })
  }
}
