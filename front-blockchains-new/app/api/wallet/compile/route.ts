import { NextRequest, NextResponse } from 'next/server'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { mkdtemp, writeFile, readFile, readdir, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { resolve, join } from 'node:path'
import { registerWalletArtifact } from '@/lib/wallet-environment-server'

export const runtime = 'nodejs'
export const dynamic = 'force-dynamic'
const execute = promisify(execFile)
const compilerState = globalThis as typeof globalThis & { walletCompilerBusy?: boolean }
const MAX_BODY = 384 * 1024

export async function POST(request: NextRequest) {
  const host = request.headers.get('host') || ''
  if (process.env.NODE_ENV !== 'development' || !/^(localhost|127\.0\.0\.1)(:\d+)?$/.test(host) ||
    request.headers.get('origin') !== `http://${host}` ||
    ![null, 'same-origin'].includes(request.headers.get('sec-fetch-site'))) {
    return NextResponse.json({ error: '合约编译仅在本机 development 模式的同源页面开放。' }, { status: 403 })
  }
  if (compilerState.walletCompilerBusy) return NextResponse.json({ error: '编译器正在处理其他请求，请稍后重试。' }, { status: 409 })
  let directory: string | undefined
  compilerState.walletCompilerBusy = true
  try {
    const reader = request.body?.getReader()
    if (!reader) throw new Error('请求内容为空')
    const chunks: Uint8Array[] = []
    let length = 0
    while (true) {
      const { done, value } = await reader.read()
      if (done) break
      length += value.length
      if (length > MAX_BODY) { await reader.cancel(); return NextResponse.json({ error: '文件过大，最多支持 256 KB 源码。' }, { status: 413 }) }
      chunks.push(value)
    }
    const { source, name } = JSON.parse(Buffer.concat(chunks).toString('utf8'))
    if (typeof name !== 'string' || !name.toLowerCase().endsWith('.ppsc') || typeof source !== 'string' || !source.trim() || Buffer.byteLength(source) > 256 * 1024) {
      return NextResponse.json({ error: '请选择非空的 .ppsc 文件，大小不超过 256 KB。' }, { status: 400 })
    }
    directory = await mkdtemp(join(tmpdir(), 'ppsc-wallet-'))
    const input = join(directory, 'Contract.ppsc')
    const output = join(directory, 'artifacts')
    const solidity = join(directory, 'solidity')
    await writeFile(input, source, { mode: 0o600 })
    // Fixed executable and argument array: uploaded content is never passed to a shell.
    await execute(resolve(process.cwd(), '../target/debug/ppsc'), ['build', input, '--out', output, '--sol-out', solidity], { timeout: 30000, maxBuffer: 1024 * 1024 })
    const entries = await readdir(output, { withFileTypes: true })
    const contract = entries.find(entry => entry.isDirectory())?.name
    if (!contract || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(contract)) throw new Error('编译产物名称无效')
    const files: Record<string, string> = {}
    for (const file of ['manifest.json', 'abi.json', 'operators.json', 'hashes.env']) files[file] = await readFile(join(output, contract, file), 'utf8')
    files[`${contract}Gateway.sol`] = await readFile(join(solidity, `${contract}Gateway.sol`), 'utf8')
    const manifestHash = files['hashes.env'].match(/^MANIFEST_HASH=(.+)$/m)?.[1]
    if (!manifestHash) throw new Error('编译产物缺少 manifest hash')
    const artifactId = registerWalletArtifact(source, manifestHash)
    return NextResponse.json({ artifactId, name: contract, manifestHash, runtimeHash: files['hashes.env'].match(/^RUNTIME_HASH=(.+)$/m)?.[1], files }, { headers: { 'Cache-Control': 'no-store' } })
  } catch (error) {
    const e = error as { code?: string; stderr?: string; message?: string }
    const message = e.code === 'ENOENT' ? '未找到 PPSC 编译器。请在项目根目录执行 cargo build -p ppsc-compiler --bin ppsc。' :
      e.stderr ? `编译失败：${e.stderr.slice(0, 2000).replaceAll(directory || '\0', '[upload]')}` : '无法编译，请检查源码语法和请求格式。'
    return NextResponse.json({ error: message }, { status: e.code === 'ENOENT' ? 503 : 400 })
  } finally {
    try { if (directory) await rm(directory, { recursive: true, force: true }) }
    finally { compilerState.walletCompilerBusy = false }
  }
}
