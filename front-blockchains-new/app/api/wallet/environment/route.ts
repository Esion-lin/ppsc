import { NextRequest, NextResponse } from 'next/server'
import { executeWalletEnvironment, getWalletEnvironment } from '@/lib/wallet-environment-server'

export const runtime = 'nodejs'
export const dynamic = 'force-dynamic'
function allowed(request: NextRequest, mutation = false) {
  const host = request.headers.get('host') || ''
  const origin = request.headers.get('origin')
  return process.env.NODE_ENV === 'development' && /^(localhost|127\.0\.0\.1)(:\d+)?$/.test(host) &&
    (!mutation || origin === `http://${host}`) && (!origin || origin === `http://${host}`) &&
    [null, 'same-origin', 'none'].includes(request.headers.get('sec-fetch-site'))
}
export async function GET(request: NextRequest) {
  if (!allowed(request)) return NextResponse.json({ error: '仅开放给本机开发页面' }, { status: 403 })
  return NextResponse.json(getWalletEnvironment(), { headers: { 'Cache-Control': 'no-store' } })
}
export async function POST(request: NextRequest) {
  if (!allowed(request, true)) return NextResponse.json({ error: '仅允许本机 development 同源请求' }, { status: 403 })
  let body
  try {
    const reader = request.body?.getReader()
    if (!reader) throw new Error()
    const chunks = []; let size = 0
    while (true) {
      const { done, value } = await reader.read()
      if (done) break
      size += value.length
      if (size > 4096) { await reader.cancel(); return NextResponse.json({ error: '请求过大' }, { status: 413 }) }
      chunks.push(value)
    }
    body = JSON.parse(Buffer.concat(chunks).toString())
    if (!['start', 'deploy', 'stop'].includes(body.action) ||
      (body.action !== 'start' && (typeof body.environmentId !== 'string' || !/^[a-f0-9]{32}$/.test(body.environmentId))) ||
      (body.action === 'deploy' && (typeof body.artifactId !== 'string' || !/^[a-f0-9-]{36}$/.test(body.artifactId)))) throw new Error()
  } catch { return NextResponse.json({ error: '环境命令格式错误' }, { status: 400 }) }
  try {
    executeWalletEnvironment(body.action, body.environmentId, body.artifactId)
    return NextResponse.json({ accepted: true }, { status: 202 })
  } catch (error) {
    return NextResponse.json({ error: error instanceof Error ? error.message : '操作失败' }, { status: 409 })
  }
}
