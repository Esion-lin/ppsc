import { NextRequest, NextResponse } from 'next/server'
import { getWalletDataJob, startWalletDataJob, type DataRequest } from '@/lib/wallet-data-server'

export const runtime = 'nodejs'
export const dynamic = 'force-dynamic'
function allowed(request: NextRequest, mutation = false) {
  const host = request.headers.get('host') || '', origin = request.headers.get('origin')
  return process.env.NODE_ENV === 'development' && /^(localhost|127\.0\.0\.1)(:\d+)?$/.test(host) &&
    (!mutation || origin === `http://${host}`) && (!origin || origin === `http://${host}`) &&
    [null, 'same-origin', 'none'].includes(request.headers.get('sec-fetch-site'))
}
export async function GET(request: NextRequest) {
  if (!allowed(request)) return NextResponse.json({ error: '仅开放给本机开发页面' }, { status: 403 })
  const job = getWalletDataJob(request.nextUrl.searchParams.get('id') || '')
  return NextResponse.json(job || { error: '数据任务不存在或已过期' }, { status: job ? 200 : 404, headers: { 'Cache-Control': 'no-store' } })
}
export async function POST(request: NextRequest) {
  if (!allowed(request, true)) return NextResponse.json({ error: '仅允许本机 development 同源请求' }, { status: 403 })
  let body: DataRequest
  try {
    const reader = request.body?.getReader()
    if (!reader) throw new Error()
    let size = 0; const chunks = []
    while (true) {
      const { value, done } = await reader.read()
      if (done) break
      size += value.length
      if (size > 6 * 1024 * 1024) { await reader.cancel(); return NextResponse.json({ error: '密文文件最大 4 MB' }, { status: 413 }) }
      chunks.push(value)
    }
    body = JSON.parse(Buffer.concat(chunks).toString())
    if (!body || !['id', 'environmentId', 'gateway', 'owner'].every(field => typeof (body as unknown as Record<string, unknown>)[field] === 'string') ||
        !/^[a-f0-9-]{36}$/.test(body.id) || !/^[a-f0-9]{32}$/.test(body.environmentId) ||
        !/^0x[a-f0-9]{40}$/i.test(body.gateway) || !/^0x[a-f0-9]{40}$/i.test(body.owner) ||
        !['input', 'balance'].includes(body.kind)) throw new Error()
    if (body.kind === 'input') {
      if (body.mode === 'amount') {
        if (typeof body.amount !== 'string' || !/^(0|[1-9][0-9]{0,8})$/.test(body.amount) || BigInt(body.amount) > 499122176n) throw new Error('金额必须为 0–499122176 的整数')
      } else if (body.mode === 'file') {
        if (typeof body.ciphertext !== 'string' || !/^[A-Za-z0-9+/]+={0,2}$/.test(body.ciphertext) || body.ciphertext.length % 4 !== 0) throw new Error('密文文件格式无效')
        const bytes = Buffer.from(body.ciphertext, 'base64')
        if (bytes.length < 1024 || bytes.length > 4 * 1024 * 1024) throw new Error('请选择 1 KB–4 MB 的 BFV 密文文件')
      } else throw new Error()
    }
    // Strip unknown fields. Only fixed server-side commands and resolved targets are used.
    body = { id: body.id, environmentId: body.environmentId, gateway: body.gateway.toLowerCase(), owner: body.owner.toLowerCase(), kind: body.kind,
      ...(body.kind === 'input' ? body.mode === 'amount' ? { mode: body.mode, amount: body.amount } : { mode: body.mode, ciphertext: body.ciphertext } : {}) }
  } catch (error) { return NextResponse.json({ error: error instanceof Error && error.message ? error.message : '数据请求格式错误' }, { status: 400 }) }
  try { return NextResponse.json(startWalletDataJob(body), { status: 202 }) }
  catch (error) { return NextResponse.json({ error: error instanceof Error ? error.message : '无法处理数据' }, { status: 409 }) }
}
