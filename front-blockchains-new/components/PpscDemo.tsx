'use client'

import Link from 'next/link'
import { useEffect, useRef, useState, type FormEvent } from 'react'
import type { DemoAction, DemoState } from '@/lib/ppsc-types'

const steps = [
  { action: 'deploy', title: '部署合约', amount: '3', unit: '合约', detail: '从 .ppsc 编译 Gateway，部署 Verifier 与 ControlPlane，启动常驻 daemon 和 OpenFHE 服务。' },
  { action: 'deposit', title: '加密入账', amount: '100', unit: '单位', detail: '使用委员会公钥加密 100，经签名上传后调用 Gateway，由 daemon 更新 Alice 余额。' },
  { action: 'transfer', title: '密态转账', amount: '30', unit: '单位', detail: 'Alice 向 Bob 转账 30，执行 BFV 密文加减与 Shamir 比较，自动回写链上状态。' },
  { action: 'withdraw', title: '密态出账', amount: '20', unit: '单位', detail: 'Bob 从合约账本扣减 20，余额变为 10。本例展示密态记账，不兑换外部资产。' },
] as const
const panel = 'rounded-2xl border border-[var(--default-border-color)] bg-[var(--secondary-background)]'
const muted = 'text-[var(--desc-color)]'
const initial: DemoState = { runtime: null, busy: false, ready: false, failed: false, error: null, environment: null, deployment: null, snapshot: null, logs: [] }
const commands: DemoAction[] = ['init', 'deploy', 'deposit', 'transfer', 'withdraw', 'status', 'stop']

export default function PpscDemo() {
  const [state, setState] = useState<DemoState>(initial)
  const [connected, setConnected] = useState(false)
  const [requesting, setRequesting] = useState(false)
  const [error, setError] = useState('')
  const [command, setCommand] = useState('')
  const [revealed, setRevealed] = useState(false)
  const [follow, setFollow] = useState(true)
  const [localOutput, setLocalOutput] = useState('')
  const terminal = useRef<HTMLDivElement>(null)
  const requestLock = useRef(false)
  const snapshot = state.snapshot
  const completed = snapshot?.completed ?? 0
  const busy = state.busy || requesting

  useEffect(() => {
    let stopped = false
    let timer: ReturnType<typeof setTimeout>
    let controller: AbortController
    async function poll() {
      controller = new AbortController()
      const timeout = setTimeout(() => controller.abort(), 10000)
      try {
        const response = await fetch('/api/ppsc', { cache: 'no-store', signal: controller.signal })
        const data = await response.json()
        if (!response.ok) throw new Error(data.error || '无法连接演示服务')
        if (!stopped) { setState(data); setConnected(true) }
      } catch {
        if (!stopped) setConnected(false)
      } finally {
        clearTimeout(timeout)
        if (!stopped) timer = setTimeout(poll, 700)
      }
    }
    poll()
    return () => { stopped = true; clearTimeout(timer); controller?.abort() }
  }, [])

  useEffect(() => {
    if (follow && terminal.current) terminal.current.scrollTop = terminal.current.scrollHeight
  }, [state.logs, follow, localOutput])

  async function execute(action: DemoAction) {
    if (requestLock.current || state.busy) return
    requestLock.current = true
    setRequesting(true)
    setError('')
    setLocalOutput('')
    try {
      const response = await fetch('/api/ppsc', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ action }) })
      const data = await response.json()
      if (!response.ok) throw new Error(data.error || '命令提交失败')
      const status = await fetch('/api/ppsc', { cache: 'no-store' })
      if (status.ok) setState(await status.json())
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : '连接失败，请先查询状态再重试')
    } finally {
      setRequesting(false)
      requestLock.current = false
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault()
    const value = command.trim().toLowerCase()
    if (value === 'help') setLocalOutput('init 初始化独立本地链与数据库\ndeploy 编译并部署三个合约并启动 daemon\ndeposit 存入 100 单位\ntransfer 转账 30 单位\nwithdraw 提取 20 单位\nstatus 查询链上与 Runtime 余额\nstop 停止本次环境\n依次执行 init → deploy → deposit → transfer → withdraw；init 创建全新演示。')
    else if (commands.includes(value as DemoAction)) execute(value as DemoAction)
    else setLocalOutput(`未知命令：${value}。输入 help 查看可用命令。此终端只接受演示命令。`)
    setCommand('')
  }

  return <main className="mx-auto max-w-7xl px-5 pb-12 pt-7 md:px-6">
    <div className={`mb-7 flex items-center gap-3 text-xs ${muted}`}><Link href="/">首页</Link><span>/</span><span>课题一</span><span>/</span><span>PPSC 实机演示</span></div>
    <section className="relative overflow-hidden rounded-2xl bg-[linear-gradient(115deg,var(--hero-from),var(--hero-via),#684626)] px-6 py-9 text-white md:px-10">
      <div className="tech-grid pointer-events-none absolute inset-0 opacity-30" />
      <div className="relative grid items-center gap-8 lg:grid-cols-[1.5fr_1fr]">
        <div><p className="text-xs tracking-[0.22em] text-[#e1b68e]">RESEARCH 01 / PPSC</p><h1 className="mt-4 text-3xl font-bold md:text-4xl">隐私保护智能合约演示</h1><p className="mt-4 max-w-xl text-sm leading-7 text-white/65">在独立本地链部署编译合约，以 OpenFHE / BFV 密文和 Shamir 分享执行入账、转账与出账。常驻 daemon 自动读取任务、计算并提交链上结果。</p></div>
        <div className="rounded-xl border border-white/15 bg-black/15 p-5"><div className="flex flex-wrap items-center justify-between gap-2"><span className="text-sm font-semibold">本地开发环境</span><span className="rounded-full bg-white/10 px-3 py-1 text-xs">{!connected ? '服务未连接' : busy ? '命令执行中' : state.failed ? '执行异常' : state.ready ? '合约已部署 · 可交易' : state.environment ? '环境已启动 · 待部署' : '等待初始化'}</span></div><p className="mt-3 text-sm leading-6 text-white/65">Anvil + PostgreSQL + Rust Runtime<br />计算后端：OpenFHE BFV + Shamir<br />执行方式：常驻 committee daemon</p><p className="mt-3 break-all font-mono text-xs text-[#e1b68e]">{state.environment?.rpc ?? '初始化后分配独立本地链端口'}</p></div>
      </div>
    </section>

    <section aria-label="演示后端与执行方式" className={`${panel} mt-5 grid gap-5 p-5 md:grid-cols-2`}>
      <div><h2 className="text-sm font-semibold">OpenFHE · 实际密码运算</h2><p className={`mt-2 text-xs leading-6 ${muted}`}>输入由独立客户端使用委员会公钥加密；BFV 密文与 Shamir 分享保存在 PostgreSQL。当前委员会各方仍在同一密码服务进程内，H2S / S2H 使用解密再分享或重加密。</p></div>
      <div><h2 className="text-sm font-semibold">Runtime · 常驻自动执行</h2><p className={`mt-2 text-xs leading-6 ${muted}`}>部署后自动启动 manifest_committee_daemon 和密码子进程，连续处理任务并保持同一套密钥。停止或重新初始化会关闭这些进程。链上使用本地单节点开发委员会。</p></div>
    </section>

    <section className="mt-7" aria-labelledby="demo-title">
      <div className="mb-5 flex flex-wrap items-center justify-between gap-3"><div><h2 id="demo-title" className="text-2xl font-semibold">合约部署与密态资产流转</h2><p className={`mt-2 text-xs ${muted}`}>先初始化本地环境，再按顺序部署和交易；刷新页面不会重复部署或执行交易。</p></div><div className="flex flex-wrap gap-2"><button disabled={busy || !connected} onClick={() => execute('init')} className="btn-primary disabled:cursor-not-allowed disabled:opacity-40">{state.environment || state.failed ? '重新初始化' : '初始化演示环境'}</button><button disabled={busy || !state.ready || !connected} onClick={() => execute('status')} className="min-h-11 rounded-full border border-[var(--default-border-color)] px-4 text-sm disabled:opacity-40">刷新余额</button><button disabled={busy || !connected} onClick={() => execute('stop')} className={`min-h-11 px-3 text-sm disabled:opacity-40 ${muted}`}>停止环境</button></div></div>
      {(!connected || error || state.error) && <p role="alert" className="mb-4 rounded-lg border border-red-400/30 bg-red-400/10 p-4 text-sm">{error || state.error || '无法连接本地演示接口，请确认开发服务器在本机运行。'}{state.failed && ' 执行可能已部分完成，请重新初始化后重试。'}</p>}
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">{steps.map((step, index) => {
        const done = step.action === 'deploy' ? !!state.deployment : completed >= index
        const available = step.action === 'deploy' ? !!state.environment && !state.deployment : state.ready && completed === index - 1
        return <article key={step.action} className={`${panel} p-5 ${available && !state.failed ? 'ring-1 ring-[var(--default-color)]' : ''}`}>
          <div className="flex justify-between"><span className="text-xs text-[var(--default-color)]">STEP 0{index + 1}</span><span className={`text-xs ${muted}`}>{done ? step.action === 'deploy' ? '已部署' : '已结算' : '待执行'}</span></div>
          <h3 className="mt-3 text-lg font-semibold">{step.title}<span className="ml-2 text-[var(--default-color)]">{step.amount} <small className="text-xs font-normal">{step.unit}</small></span></h3>
          <p className={`mt-3 min-h-[96px] text-xs leading-6 ${muted}`}>{step.detail}</p>
          <button onClick={() => execute(step.action)} disabled={!connected || state.failed || busy || !available} className="mt-4 min-h-11 w-full rounded-lg border border-[var(--default-border-color)] text-sm font-semibold text-[var(--default-color)] transition hover:bg-[var(--other-background)] disabled:cursor-not-allowed disabled:opacity-40">{done ? '✓ 已完成' : step.action === 'deploy' ? '部署演示合约 →' : `执行${step.title} →`}</button>
        </article>
      })}</div>
    </section>

    <section className="mt-5 grid gap-5 lg:grid-cols-[1fr_2fr]">
      <div className={`${panel} p-5`}><div className="flex items-center justify-between"><h2 className="font-semibold">实际账户余额</h2><button aria-pressed={revealed} onClick={() => setRevealed(value => !value)} className="min-h-11 text-xs text-[var(--default-color)]">{revealed ? '隐藏数值' : '显示数值'}</button></div><p className={`text-xs leading-5 ${muted}`}>余额通过 getBalance → daemon 解密 opening 获取。本地演示的 opening 会公开写入链上；隐藏按钮仅控制显示。</p>{[{ label: 'Alice · 发送方', balance: snapshot?.privateSender, dataId: snapshot?.senderDataId }, { label: 'Bob · 接收方', balance: snapshot?.privateReceiver, dataId: snapshot?.receiverDataId }].map(account => <div key={account.label} className="mt-4 border-t border-[var(--default-border-color)] pt-4"><h3 className="text-sm font-semibold">{account.label}</h3><p className="mt-3 text-2xl tabular-nums text-[var(--default-color)]">{account.balance == null ? '—' : revealed ? account.balance : '••••'}<small className={`ml-2 text-xs ${muted}`}>账本单位</small></p><p className={`mt-2 break-all font-mono text-[10px] leading-5 ${muted}`}>dataId: {account.dataId ?? '—'}</p></div>)}<p className={`mt-5 text-xs leading-5 ${muted}`}>{busy ? '余额为最近一次成功查询。' : snapshot ? `已结算 ${completed} / 3 笔业务任务` : '部署合约后显示链上查询结果。'}</p></div>
      <div className="min-w-0 overflow-hidden rounded-2xl border border-[#354047] bg-[#0b1014] text-[#d5e0e5]">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b border-white/10 bg-[#151d23] px-5 py-3"><h2 className="font-mono text-sm"><span className="mr-3 text-[#bd7c40]">❯_</span>PPSC Terminal</h2><label className="flex min-h-8 items-center gap-2 text-xs text-slate-400"><input type="checkbox" checked={follow} onChange={event => setFollow(event.target.checked)} />自动滚动</label></div>
        <div ref={terminal} tabIndex={0} aria-label="实际命令与执行输出" className="h-[390px] overflow-auto p-5 font-mono text-[11px] leading-6 md:text-xs">
          {state.logs.length === 0 && <p className="text-slate-400">PPSC 本地演示终端<br />输入 help 查看命令，输入 init 初始化环境，再输入 deploy 部署合约。<br />首次运行需要构建 Rust 和 Solidity，请等待命令完成。</p>}
          {state.logs.map(line => <pre key={line.id} className={`whitespace-pre-wrap break-all ${line.text.startsWith('$') || line.text.startsWith('ppsc>') ? 'text-[#e9b786]' : line.text.startsWith('ERROR') ? 'text-red-400' : 'text-[#aabac5]'}`}>{line.text}</pre>)}
          {localOutput && <pre className="mt-3 whitespace-pre-wrap break-all text-[#80d1c3]">{localOutput}</pre>}
          {busy && <p className="mt-2 text-[#80d1c3]">进程执行中…</p>}
        </div>
        <form onSubmit={submit} className="flex items-center gap-3 border-t border-white/10 px-4 py-3"><label htmlFor="ppsc-command" className="font-mono text-sm text-[#bd7c40]">ppsc&gt;</label><input id="ppsc-command" autoComplete="off" spellCheck={false} value={command} onChange={event => setCommand(event.target.value)} placeholder="help / init / deploy / deposit / transfer / withdraw / status / stop" className="min-h-10 min-w-0 flex-1 rounded bg-transparent px-1 font-mono text-xs text-white outline-none focus-visible:ring-1 focus-visible:ring-[#bd7c40]" /><button disabled={busy || !connected || !command.trim()} className="min-h-10 rounded-md bg-white/10 px-4 text-xs disabled:opacity-40">执行 ↵</button></form>
      </div>
    </section>
    <div className="mt-5 grid grid-cols-2 gap-3 md:grid-cols-4">{[['密码后端', state.runtime?.backend], ['Daemon PID', state.runtime?.daemonPid], ['密码服务 PID', state.runtime?.cryptoPid], ['运行状态', state.runtime?.running ? '运行中' : '未运行']].map(([label, value]) => <div key={label} className={`${panel} p-5`}><p className={`text-xs ${muted}`}>{label}</p><p className="mt-2 break-all text-lg font-semibold tabular-nums">{value ?? '—'}</p></div>)}</div>
    <section className={`${panel} mt-5 p-5`} aria-labelledby="deployment-title">
      <div className="flex flex-wrap items-center justify-between gap-2"><h2 id="deployment-title" className="text-sm font-semibold">部署回执与状态</h2><span className={`text-xs ${muted}`}>{state.deployment ? '3 / 3 合约已确认' : state.environment ? '本地环境已启动，等待部署' : '等待初始化'}</span></div>
      {state.deployment ? <div className="mt-4 grid gap-3 lg:grid-cols-2">{state.deployment.contracts.map(contract => <article key={contract.address} className="min-w-0 rounded-lg border border-[var(--default-border-color)] bg-[var(--other-background)] p-4">
        <div className="flex flex-wrap justify-between gap-2"><h3 className="break-all text-xs font-semibold">{contract.name}</h3><span className="text-xs text-[var(--success-color)]">已确认 · 区块 {contract.blockNumber}</span></div>
        <dl className="mt-3 space-y-2 text-xs"><div><dt className={muted}>合约地址</dt><dd className="mt-1 break-all font-mono">{contract.address}</dd></div><div><dt className={muted}>部署交易哈希</dt><dd className="mt-1 break-all font-mono">{contract.transactionHash}</dd></div></dl>
      </article>)}</div> : <p className={`mt-3 text-xs leading-6 ${muted}`}>点击「部署演示合约」后，展示实际部署地址、交易哈希与确认区块；部署完成前不能执行资金流转。</p>}
      <dl className="mt-4 grid gap-3 border-t border-[var(--default-border-color)] pt-4 text-xs md:grid-cols-2">{[['独立本地链 RPC', state.environment?.rpc], ['独立 PostgreSQL 数据目录', state.environment?.database], ['链上状态根', snapshot?.root], ['委员会公钥 SHA-256', state.runtime?.publicKeyFingerprint]].map(([label, value]) => <div key={label}><dt className={muted}>{label}</dt><dd className="mt-1 break-all font-mono leading-6">{value ?? '—'}</dd></div>)}</dl>
    </section>
    <p className={`mt-5 text-xs leading-6 ${muted}`}>本页使用本地测试账户、独立 Anvil 和 PostgreSQL，实际调用 OpenFHE 密码运算，无明文后端回退。输入加密在本机原生客户端执行。密钥仅保存在密码进程内，进程退出后需重新初始化；这不是多机委员会或生产部署。终端私钥参数已脱敏。</p>
    <footer className={`mt-8 flex justify-between text-xs ${muted}`}><span>PPSC · 课题一成果演示</span><Link href="/">返回首页 ↗</Link></footer>
  </main>
}
