import { NextRequest, NextResponse } from 'next/server'
import { executeDemo, getDemoState } from '@/lib/ppsc-server'
import type { DemoAction } from '@/lib/ppsc-types'

export const runtime = 'nodejs'
export const dynamic = 'force-dynamic'

function allowed(request: NextRequest, mutation = false) {
  // This endpoint launches local development processes; never expose it on a public host.
  if (process.env.NODE_ENV !== 'development') return false
  const host = request.headers.get('host') ?? ''
  if (!/^(localhost|127\.0\.0\.1)(:\d+)?$/.test(host)) return false
  const origin = request.headers.get('origin')
  if (mutation && origin !== `http://${host}`) return false
  if (origin && origin !== `http://${host}`) return false
  const site = request.headers.get('sec-fetch-site')
  return !site || site === 'same-origin' || site === 'none'
}

export async function GET(request: NextRequest) {
  if (!allowed(request)) return NextResponse.json({ error: '演示接口仅开放给本机开发页面' }, { status: 403 })
  return NextResponse.json(getDemoState(), { headers: { 'Cache-Control': 'no-store' } })
}

export async function POST(request: NextRequest) {
  if (!allowed(request, true)) return NextResponse.json({ error: '仅允许本机页面的同源命令' }, { status: 403 })
  let action: unknown
  try { action = (await request.json()).action } catch {
    return NextResponse.json({ error: '请求格式错误' }, { status: 400 })
  }
  if (typeof action !== 'string' || !['init', 'deploy', 'deposit', 'transfer', 'withdraw', 'status', 'stop'].includes(action)) {
    return NextResponse.json({ error: '不支持的命令' }, { status: 400 })
  }
  try {
    executeDemo(action as DemoAction)
    return NextResponse.json({ accepted: true }, { status: 202 })
  } catch (error) {
    return NextResponse.json({ error: error instanceof Error ? error.message : '执行失败' }, { status: 409 })
  }
}
