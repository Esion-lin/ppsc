'use client'

import Link from 'next/link'
import { useCallback, useEffect, useRef, useState, type FormEvent, type ReactNode } from 'react'
import ThemeSwitcher from './ThemeSwitcher'
import { call, controlAddress, encodeCall, errorMessage, executionLabels, invocationNonce, isDataId, networks, readWallet, restoreActivities, rpc, setWalletSession, short, validAddress, words, type Activity, type CompiledContract, type NetworkId } from '@/lib/wallet'

import type { WalletEnvironmentState, WalletDeployment } from '@/lib/wallet-environment-types'
import WalletEnvironmentPanel from './WalletEnvironmentPanel'
import type { WalletDataJob } from '@/lib/wallet-data-types'

type View = 'overview' | 'contracts' | 'activity' | 'settings'
type Action = 'transfer' | 'deposit' | 'receive' | 'withdraw'
type Notice = { text: string; error?: boolean }
type PrivateState = { exists: boolean; dataId: string; version: string } | null
const nav: { id: View; title: string; icon: string }[] = [
  { id: 'overview', title: '钱包概览', icon: 'grid' }, { id: 'contracts', title: '加密合约', icon: 'code' },
  { id: 'activity', title: '交易记录', icon: 'clock' }, { id: 'settings', title: '连接配置', icon: 'settings' },
]
function Icon({ name, size = 20 }: { name: string; size?: number }) {
  const paths: Record<string, ReactNode> = {
    grid: <><rect x="3" y="3" width="7" height="7" rx="1.5" /><rect x="14" y="3" width="7" height="7" rx="1.5" /><rect x="3" y="14" width="7" height="7" rx="1.5" /><rect x="14" y="14" width="7" height="7" rx="1.5" /></>,
    code: <><path d="m8 7-5 5 5 5m8-10 5 5-5 5m-3-13-2 20" /></>,
    clock: <><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></>,
    settings: <><path d="M4 7h16M4 17h16" /><circle cx="8" cy="7" r="3" /><circle cx="16" cy="17" r="3" /></>,
    arrow: <path d="M6 18 18 6M6 6h12v12" />,
    down: <path d="M12 3v14m-5-5 5 5 5-5M4 17v4h16v-4" />,
    upload: <path d="M12 17V3m-5 5 5-5 5 5M4 16v5h16v-5" />,
    shield: <><path d="m12 3 8 3v6c0 5-8 9-8 9s-8-4-8-9V6l8-3Z" /><path d="m8 12 3 3 5-6" /></>,
    wallet: <><path d="M20 8V5H5a2 2 0 0 0 0 4h16v11H5a2 2 0 0 1-2-2V7" /><path d="M21 12h-6v5h6" /></>,
    copy: <><rect x="8" y="8" width="12" height="13" rx="2" /><path d="M16 8V3H3v13h5" /></>,
    refresh: <><path d="M20 7v5h-5M4 17v-5h5" /><path d="M6 6a8 8 0 0 1 13 3M18 18A8 8 0 0 1 5 15" /></>,
    plus: <path d="M12 5v14M5 12h14" />,
    lock: <><rect x="5" y="10" width="14" height="11" rx="2" /><path d="M8 10V7a4 4 0 0 1 8 0v3m-4 5v2" /></>,
  }
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name] || paths.wallet}</svg>
}
function download(name: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }))
  const link = document.createElement('a'); link.href = url; link.download = name; link.click()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}

export default function WalletDashboard() {
  const [view, setView] = useState<View>('overview')
  const [action, setAction] = useState<Action>('transfer')
  const chain: NetworkId = '31337'
  const [actualChain, setActualChain] = useState('')
  const [account, setAccount] = useState('')
  const [gateway, setGateway] = useState('')
  const [gatewayDraft, setGatewayDraft] = useState('')
  const [accounts, setAccounts] = useState<string[]>([])
  const [privateState, setPrivateState] = useState<PrivateState>(null)
  const [recipient, setRecipient] = useState('')
  const [dataId, setDataId] = useState('')
  const [inputAmount, setInputAmount] = useState('')
  const [inputMode, setInputMode] = useState<'amount' | 'file'>('amount')
  const [ciphertextFile, setCiphertextFile] = useState<File | null>(null)
  const [inputJob, setInputJob] = useState<WalletDataJob | null>(null)
  const [balanceJob, setBalanceJob] = useState<WalletDataJob | null>(null)
  const [balanceVisible, setBalanceVisible] = useState(false)
  const [taskBusy, setBusy] = useState('')
  const [environmentState, setEnvironmentState] = useState<WalletEnvironmentState | null>(null)
  const [environmentSession, setEnvironmentSession] = useState<string | undefined>()
  const [environmentError, setEnvironmentError] = useState('')
  const observedDeployment = useRef('')
  const activeSession = useRef<string | undefined>()
  const busy = taskBusy || (environmentState?.busy ? '本地环境操作中…' : '')
  const [notice, setNotice] = useState<Notice | null>(null)
  const [readError, setReadError] = useState('')
  const [activities, setActivities] = useState<Activity[]>([])
  const [loaded, setLoaded] = useState(false)
  const [file, setFile] = useState<File | null>(null)
  const [compiled, setCompiled] = useState<CompiledContract | null>(null)
  const [dragging, setDragging] = useState(false)
  const actionLock = useRef(false)
  const scope = useRef(0)
  const fileInput = useRef<HTMLInputElement>(null)
  const wrongNetwork = !!account && actualChain !== chain
  const visibleActivities = activities.filter(a => a.chain === chain && a.session === environmentSession && a.account.toLowerCase() === account.toLowerCase())
  const pending = visibleActivities.filter(a => a.status === 'pending' || a.status === 'confirmed' && a.execution && (a.stage ?? 0) < 6).length

  useEffect(() => {
    try { setActivities(restoreActivities(localStorage.getItem('ppsc-wallet-activity-v1'))) } catch { /* Storage may be disabled. */ }
    setLoaded(true)
  }, [])
  useEffect(() => {
    scope.current++; setPrivateState(null); setReadError('')
    let address = ''
    try { address = localStorage.getItem(`ppsc-wallet-gateway-${chain}`) || '' } catch { /* Optional persistence. */ }
    setGateway(address); setGatewayDraft(address)
  }, [chain])
  useEffect(() => {
    if (loaded) try { localStorage.setItem('ppsc-wallet-activity-v1', JSON.stringify(activities.slice(0, 100))) } catch { /* Session still works without storage. */ }
  }, [activities, loaded])

  useEffect(() => {
    setDataId(''); setInputJob(null); setBalanceJob(null); setBalanceVisible(false)
    setInputAmount(''); setCiphertextFile(null)
  }, [account, gateway, environmentSession])

  const managedContract = environmentState?.environment.deployments.find(item => item.gateway?.toLowerCase() === gateway.toLowerCase() && item.status === 'ready')
  const canUseDataService = !!environmentSession && !!managedContract && !!account && !wrongNetwork
  const balanceFresh = balanceJob?.status === 'completed' && balanceJob.dataId === privateState?.dataId
  const displayedBalance = !privateState?.exists ? '—' : balanceVisible && balanceFresh ? balanceJob.amount : '••••••'

  async function dataOperation(kind: 'input' | 'balance', resumeId?: string) {
    const current = scope.current
    await run(kind === 'input' ? '正在准备数据' : '正在查询余额', async () => {
      if (!canUseDataService) throw new Error('请先在概览启动本地环境并连接已部署的合约。')
      const update = (job: WalletDataJob) => {
        if (current !== scope.current) return
        if (kind === 'input') setInputJob(job); else setBalanceJob(job)
        const labels: Record<string, string> = { pending: '正在准备', encrypting: '正在加密 / 校验密文', uploading: '正在签名上传', registering: '等待链上登记', querying: '正在提交余额查询', opening: '等待 daemon 返回余额' }
        if (labels[job.status]) setBusy(labels[job.status])
      }
      let job: WalletDataJob
      if (resumeId) {
        const response = await fetch(`/api/wallet/data?id=${encodeURIComponent(resumeId)}`, { cache: 'no-store' })
        const result = await response.json()
        if (!response.ok) throw new Error(result.error || '无法读取数据任务')
        job = result
      } else {
        const payload: Record<string, unknown> = { id: crypto.randomUUID(), kind, environmentId: environmentSession, gateway, owner: account }
        if (kind === 'input') {
          payload.mode = inputMode
          if (inputMode === 'amount') {
            if (!/^(0|[1-9][0-9]{0,8})$/.test(inputAmount) || BigInt(inputAmount) > 499122176n) throw new Error('金额必须为 0–499122176 的整数。')
            payload.amount = inputAmount
          } else {
            if (!ciphertextFile || !ciphertextFile.name.toLowerCase().endsWith('.bfv') || ciphertextFile.size < 1024 || ciphertextFile.size > 4 * 1024 * 1024) throw new Error('请选择 1 KB–4 MB 的 .bfv 密文文件。')
            const bytes = new Uint8Array(await ciphertextFile.arrayBuffer())
            let binary = ''
            for (let start = 0; start < bytes.length; start += 8192) binary += String.fromCharCode(...bytes.subarray(start, start + 8192))
            payload.ciphertext = btoa(binary)
          }
        }
        const response = await fetch('/api/wallet/data', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload), signal: AbortSignal.timeout(15000) })
        const result = await response.json()
        if (!response.ok) throw new Error(result.error || '数据任务提交失败')
        job = result
      }
      update(job)
      const until = Date.now() + 170000
      while (!['completed', 'failed'].includes(job.status) && Date.now() < until) {
        await new Promise(resolve => setTimeout(resolve, 900))
        const response = await fetch(`/api/wallet/data?id=${encodeURIComponent(job.id)}`, { cache: 'no-store', signal: AbortSignal.timeout(10000) })
        const result = await response.json()
        if (!response.ok) throw new Error(result.error || '无法读取任务进度，可稍后刷新任务状态')
        job = result; update(job)
      }
      if (job.status !== 'completed') throw new Error(job.error || '等待超时，请刷新任务状态，不必重复上传。')
      if (current !== scope.current || job.environmentId !== environmentSession || job.owner.toLowerCase() !== account.toLowerCase() || job.gateway.toLowerCase() !== gateway.toLowerCase()) return
      if (kind === 'input' && job.dataId) {
        setDataId(job.dataId); setNotice({ text: '数据已加密上传并完成链上登记，dataId 已填入交易表单。' })
      } else if (kind === 'balance') {
        setBalanceVisible(true); await refresh()
        setNotice({ text: '已取得 daemon 返回的实际余额；开发 opening 已写入本地链。' })
      }
    })
  }

  async function attachDeployment(deployment: WalletDeployment, session: string) {
    if (!deployment.gateway || deployment.status !== 'ready' || !deployment.walletCompatible) return
    const current = ++scope.current
    setWalletSession(session); activeSession.current = session; setEnvironmentSession(session)
    setPrivateState(null); setReadError(''); setDataId('')
    const available = await rpc<string[]>('eth_accounts')
    if (scope.current !== current) return
    setAccounts(available); setAccount(previous => available.includes(previous) ? previous : available[0] || '')
    setActualChain(chain); setGateway(deployment.gateway); setGatewayDraft(deployment.gateway)
    try { localStorage.setItem(`ppsc-wallet-gateway-${session}`, deployment.gateway) } catch { /* Optional persistence. */ }
  }
  useEffect(() => {
    let cancelled = false
    let timer: ReturnType<typeof setTimeout>
    async function pollEnvironment() {
      try {
        const response = await fetch('/api/wallet/environment', { cache: 'no-store', signal: AbortSignal.timeout(10000) })
        if (!response.ok) throw new Error('本地环境接口不可用，请使用本机开发服务器')
        const next: WalletEnvironmentState = await response.json()
        if (cancelled) return
        setEnvironmentState(next); setEnvironmentError('')
        const env = next.environment
        if (activeSession.current && (!env.ready || env.id !== activeSession.current)) {
          scope.current++; setAccount(''); setAccounts([]); setPrivateState(null); setGateway(''); setGatewayDraft('')
          // Keep the old session on RPC until an explicit connection: no silent fallback to another chain.
          if (!env.id) { observedDeployment.current = ''; activeSession.current = undefined }
        }
        const latest = [...env.deployments].reverse().find(item => item.status === 'ready' && item.walletCompatible)
        if (env.ready && env.id && latest && latest.id !== observedDeployment.current && !actionLock.current && !next.busy) {
          await attachDeployment(latest, env.id)
          observedDeployment.current = latest.id
          setNotice({ text: `${latest.name} 已部署，daemon 运行中，钱包已自动连接。` })
        }
      } catch (error) { if (!cancelled) setEnvironmentError(errorMessage(error)) }
      finally { if (!cancelled) timer = setTimeout(pollEnvironment, 1500) }
    }
    void pollEnvironment()
    return () => { cancelled = true; clearTimeout(timer) }
  }, [])

  async function environmentAction(action: 'start' | 'deploy' | 'stop') {
    await run(action === 'start' ? '启动本地环境' : action === 'stop' ? '停止环境' : '提交合约部署', async () => {
      const response = await fetch('/api/wallet/environment', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ action, environmentId: environmentState?.environment.id, artifactId: compiled?.artifactId }),
      })
      const result = await response.json()
      if (!response.ok) throw new Error(result.error || '环境操作失败')
      if (action === 'stop') {
        scope.current++; setAccount(''); setAccounts([]); setPrivateState(null); setGateway(''); setGatewayDraft('')
      }
      setEnvironmentState(previous => previous ? { ...previous, busy: true, error: null } : previous)
      setNotice({ text: action === 'deploy' ? '部署已提交，进度与回执将在下方更新。' : action === 'start' ? '正在准备本地链、数据库、合约与真实密码 daemon。首次构建需要一些时间。' : '正在停止钱包创建的本地环境。' })
    })
  }

  const refresh = useCallback(async () => {
    if (!account || actualChain !== chain) return
    const current = scope.current
    try {
      const state = await readWallet(account, gateway)
      if (current === scope.current) { setPrivateState(state); setReadError('') }
    } catch (error) { if (current === scope.current) { setReadError(errorMessage(error)); setPrivateState(null) } }
  }, [account, actualChain, chain, gateway, environmentSession])
  useEffect(() => { void refresh(); const timer = setInterval(() => void refresh(), 10000); return () => clearInterval(timer) }, [refresh])
  useEffect(() => {
    if (!account || wrongNetwork || !visibleActivities.some(a => a.status === 'pending' || a.execution && a.status === 'confirmed' && (a.stage ?? 0) < 6)) return
    let cancelled = false
    let polling = false
    const poll = async () => {
      if (polling) return
      polling = true
      try {
        if (BigInt(await rpc('eth_chainId')).toString() !== chain) return
        const updates = await Promise.all(visibleActivities.map(async item => {
          if (item.status === 'failed' || item.status === 'confirmed' && (!item.execution || (item.stage ?? 0) >= 6)) return item
          try {
            const receipt = await rpc<{ status: string } | null>('eth_getTransactionReceipt', [item.hash])
            if (!receipt) return item
            const status = receipt.status === '0x1' ? 'confirmed' as const : 'failed' as const
            let stage = item.stage
            if (status === 'confirmed' && item.execution && item.control) {
              stage = Number(BigInt(words(await call(item.control, 'executionStatus', [item.execution]), 1)[0]))
            }
            return { ...item, status, stage }
          } catch { return item }
        }))
        if (!cancelled) setActivities(previous => previous.map(item => updates.find(update => update.hash === item.hash && update.chain === item.chain && update.session === item.session) || item))
      } catch { /* Keep pending state on transient RPC failure; never infer confirmation. */ }
      finally { polling = false }
    }
    void poll(); const timer = setInterval(() => void poll(), 8000)
    return () => { cancelled = true; clearInterval(timer) }
    // Restart only when the set of pending transactions or wallet scope changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [account, chain, environmentSession, wrongNetwork, activities.map(a => `${a.hash}:${a.status}:${a.stage}`).join('|')])

  async function run(label: string, task: () => Promise<void>) {
    if (actionLock.current) return
    actionLock.current = true; setBusy(label); setNotice(null)
    try { await task() } catch (error) { setNotice({ text: errorMessage(error), error: true }) }
    finally { actionLock.current = false; setBusy('') }
  }
  async function connect() {
    await run('连接本地链', async () => {
      const env = environmentState?.environment
      const deployment = env?.deployments.find(item => item.status === 'ready' && item.walletCompatible)
      if (env?.ready && env.id && deployment) { await attachDeployment(deployment, env.id); return }
      if (env?.id) throw new Error('钱包环境尚未就绪，请等待部署完成或先停止环境。')
      setWalletSession(); setEnvironmentSession(undefined); activeSession.current = undefined
      const id = BigInt(await rpc('eth_chainId')).toString()
      if (id !== chain) throw new Error('请启动 chain ID 为 31337 的本地 Anvil 链。')
      const available = await rpc<string[]>('eth_accounts')
      if (!available.length) throw new Error('本地链没有可用账户，请检查 Anvil 是否正常启动。')
      scope.current++; setAccounts(available); setAccount(available[0]); setActualChain(id)
    })
  }
  async function send(kind: 'createAccount' | 'deposit' | 'transfer' | 'withdraw') {
    await run('正在提交交易', async () => {
      if (!account) throw new Error('请先连接本地账户。')
      if (environmentSession && environmentState?.environment.deployments.some(item => item.gateway?.toLowerCase() === gateway.toLowerCase() && item.status !== 'ready')) throw new Error('当前合约 daemon 未就绪，请查看部署记录。')
      if (BigInt(await rpc('eth_chainId')).toString() !== chain) throw new Error('仅允许向本地 Anvil 链提交交易。')
      if (!validAddress(gateway)) throw new Error('请先在连接配置中填写已部署的 Gateway 地址。')
      if (await rpc('eth_getCode', [gateway, 'latest']) === '0x') throw new Error('本地链上找不到 Gateway 合约。')
      const control = await controlAddress(gateway)
      if (kind === 'createAccount') {
        const state = await readWallet(account, gateway)
        if (state?.exists) throw new Error('当前密态账户已存在，无需再次创建。')
        if (visibleActivities.some(a => a.control?.toLowerCase() === control.toLowerCase() && a.title === '创建密态账户' && (a.status === 'pending' || a.status === 'confirmed' && (a.stage ?? 0) < 6))) throw new Error('账户创建仍在处理中，请等待委员会回写。')
      } else if (!isDataId(dataId.trim())) throw new Error('请输入已登记的 32 字节金额输入 dataId。')
      if (kind === 'transfer' && !validAddress(recipient.trim())) throw new Error('请输入有效的非零收款地址。')
      const nonce = invocationNonce()
      if (BigInt(words(await call(control, 'invocationNonceUsed', [account, nonce]), 1)[0]) !== 0n) throw new Error('调用 nonce 已使用，请重试。')
      const deadline = BigInt(Math.floor(Date.now() / 1000) + 3600)
      const args = kind === 'createAccount' ? [nonce, deadline] : kind === 'transfer' ? [recipient.trim(), dataId.trim(), nonce, deadline] : [dataId.trim(), nonce, deadline]
      const [execution] = words(await call(gateway, kind, args, account), 1)
      const hash = await rpc('eth_sendTransaction', [{ from: account, to: gateway, data: encodeCall(kind, args) }])
      const title = { createAccount: '创建密态账户', deposit: '密态入账', transfer: '密态转账', withdraw: '密态出账' }[kind]
      setActivities(previous => [{ hash, account, chain, session: environmentSession, title, time: Date.now(), status: 'pending' as const, execution, control }, ...previous].slice(0, 100))
      setNotice({ text: '交易已提交至本地链，正在等待确认与委员会执行。' })
      setDataId('')
    })
  }
  async function copy(value: string) {
    try { await navigator.clipboard.writeText(value); setNotice({ text: '已复制到剪贴板。' }) }
    catch { setNotice({ text: '复制失败，请手动选择并复制地址。', error: true }) }
  }
  function chooseFile(value: File | undefined) {
    setCompiled(null); setFile(null)
    if (!value) return
    if (!value.name.toLowerCase().endsWith('.ppsc') || value.size > 256 * 1024 || value.size === 0) { setNotice({ text: '请选择非空的 .ppsc 文件，大小不超过 256 KB。', error: true }); return }
    setFile(value); setNotice(null)
  }
  async function compile() {
    if (!file) return
    await run('正在编译合约', async () => {
      setCompiled(null)
      const response = await fetch('/api/wallet/compile', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name: file.name, source: await file.text() }), signal: AbortSignal.timeout(45000) })
      const result = await response.json()
      if (!response.ok) throw new Error(result.error || '编译失败')
      setCompiled(result); setNotice({ text: '编译成功，manifest 与 Gateway 已生成。可点击「自动部署合约」，部署后自动配置 Gateway。' })
    })
  }
  function saveGateway(event: FormEvent) {
    event.preventDefault()
    const address = gatewayDraft.trim()
    if (address && !validAddress(address)) { setNotice({ text: 'Gateway 地址格式不正确。', error: true }); return }
    scope.current++; setPrivateState(null); setGateway(address)
    try { localStorage.setItem(`ppsc-wallet-gateway-${environmentSession || chain}`, address) } catch { /* Configuration remains usable in memory. */ }
    setNotice({ text: '连接配置已保存，将从当前网络读取合约状态。' })
  }
  const isPrivate = action !== 'receive'
  const actionTitle = { transfer: '发起转账', deposit: '密态入账', receive: '收款地址', withdraw: '密态出账' }[action]

  function activityList(full = false) {
    return <div className="wallet-activity-list">
      {visibleActivities.length === 0 ? <div className="wallet-empty"><span className="wallet-empty-icon"><Icon name="clock" size={27} /></span><strong>交易从这里开始</strong><p>连接本地链并发起第一笔交易，进度将在这里显示。</p></div> :
        (full ? visibleActivities : visibleActivities.slice(0, 4)).map(item => {
          const failed = item.status === 'failed' || item.stage === 7 || item.stage === 8
          const done = item.status === 'confirmed' && (!item.execution || item.stage === 6)
          const label = item.status === 'pending' ? '等待链上确认' : item.status === 'failed' ? '链上交易失败' : item.execution ? executionLabels[item.stage ?? 0] || '状态未知' : '已确认'
          return <div className="wallet-activity" key={`${item.chain}-${item.hash}`}>
            <span className="wallet-activity-icon"><Icon name={item.title.includes('入账') ? 'down' : 'arrow'} /></span>
            <div className="wallet-activity-name"><strong>{item.title}</strong><span>{new Date(item.time).toLocaleString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })} · <button onClick={() => void copy(item.hash)} title={item.hash}>{short(item.hash)}</button></span>{full && item.execution && <small title={item.execution}>Execution: {short(item.execution)}</small>}</div>
            <div className="wallet-activity-status"><span className={`wallet-status ${failed ? 'error' : done ? 'success' : 'waiting'}`}>{label}</span></div>
          </div>
        })}
    </div>
  }

  return <div className="wallet-app">
    <aside className="wallet-sidebar">
      <Link href="/" className="wallet-brand"><span className="wallet-brand-mark"><Icon name="shield" size={25} /></span><span>PPSC<span className="wallet-brand-sub">CONFIDENTIAL NETWORK</span></span></Link>
      <div className="wallet-space-label">WORKSPACE <span>01</span></div>
      <nav aria-label="钱包导航">{nav.map(item => <button key={item.id} className={`wallet-nav-item ${view === item.id ? 'active' : ''}`} aria-current={view === item.id ? 'page' : undefined} onClick={() => { setView(item.id); setNotice(null) }}><Icon name={item.icon} />{item.title}{item.id === 'contracts' && <span className="wallet-nav-tag">PPSC</span>}</button>)}</nav>
      <div className="wallet-sidebar-bottom"><div className="wallet-security-note"><Icon name="shield" /><strong>验证合约流程，<br />观察状态流转。</strong><p>链上验证 · 本地密码计算</p><Link href="/topic-one">探索 PPSC 协议 <span>↗</span></Link></div><Link href="/" className="wallet-back">← 返回示范平台</Link><div className="wallet-sidebar-foot"><span>DEVNET / v0.1</span><span className="wallet-dot" /></div></div>
    </aside>
    <div className="wallet-workspace">
      <header className="wallet-topbar"><div className="wallet-breadcrumb">工作空间 <span>/</span> <strong>{nav.find(item => item.id === view)?.title}</strong></div><div className="wallet-top-actions"><label className="wallet-network"><span className="wallet-dot" /><span className="sr-only">本地网络</span><span>Anvil Local</span></label><ThemeSwitcher />{account && <select className="wallet-account-select" aria-label="选择本地账户" value={account} disabled={!!busy} onChange={event => { scope.current++; setAccount(event.target.value); setPrivateState(null); setNotice(null); setDataId('') }}>{accounts.map((address, index) => <option value={address} key={address}>账户 {index + 1} · {short(address)}</option>)}</select>}<button className="wallet-connect" onClick={() => account ? void copy(account) : void connect()} disabled={!!busy}><Icon name="wallet" size={17} /><span>{account ? '已连接' : '连接本地链'}</span></button></div></header>
      <main className="wallet-main">
        <div className="wallet-page-heading"><div><div className="wallet-eyebrow">LOCAL CONTRACT WORKSPACE</div><h1>{view === 'overview' ? '我的钱包' : nav.find(item => item.id === view)?.title}<span className="wallet-beta">BETA</span></h1><p>{view === 'overview' ? '一键启动本地环境，部署隐私合约并管理密态账户。' : view === 'contracts' ? '从隐私合约源码，到可验证的编译产物。' : view === 'activity' ? '追踪链上确认与委员会执行的每一步。' : '连接你的网络与已部署的隐私合约。'}</p></div><span className="wallet-mode"><span className="wallet-dot" />本地开发工作空间</span></div>
        <div className="wallet-form-note" role="note"><Icon name="code" size={18} /><p><strong>本地 OpenFHE / BFV + Shamir 后端。</strong> 在概览一键启动隔离本地链、数据库与 daemon，钱包会自动连接。也支持手动准备的本机 8545 环境。</p></div>
        {notice && <div className={`wallet-notice ${notice.error ? 'error' : ''}`} role={notice.error ? 'alert' : 'status'}><span>{notice.text}</span><button aria-label="关闭提示" onClick={() => setNotice(null)}>×</button></div>}
        {balanceJob && !['completed', 'failed'].includes(balanceJob.status) && !busy && <button className="wallet-secondary" onClick={() => void dataOperation('balance', balanceJob.id)}>刷新余额查询任务</button>}
        {wrongNetwork && <div className="wallet-notice error" role="alert"><span>钱包当前网络与 {networks[chain].name} 不一致，交易已暂停。</span><button disabled={!!busy} onClick={() => void connect()}>重新连接 ↗</button></div>}
        {readError && account && !wrongNetwork && <div className="wallet-notice error" role="alert"><span>{readError}</span><button onClick={() => setView('settings')}>检查配置</button></div>}

        {view === 'overview' && <>
          <WalletEnvironmentPanel state={environmentState} error={environmentError} busy={!!busy} onAction={environmentAction} />
          <section className="wallet-summary-grid" aria-label="资产概览">
            <div className="wallet-balance-card"><div className="wallet-card-top"><span><Icon name="wallet" size={17} />密态资产余额</span><span className="wallet-outline-badge">{networks[chain].name}</span></div><div className="wallet-balance">{displayedBalance}<span>PPSC</span></div><p className="wallet-balance-caption" >{balanceVisible && !balanceFresh && balanceJob ? '链上状态已变化，请刷新余额' : balanceVisible && balanceFresh ? 'daemon 返回的实际账本余额' : '点击显示余额，读取当前账户的实际余额'}</p><div className="wallet-balance-controls"><button type="button" disabled={!!busy || !canUseDataService || !privateState?.exists || pending > 0} onClick={() => balanceVisible ? setBalanceVisible(false) : void dataOperation('balance')}>{balanceVisible ? '隐藏余额' : '显示余额'}</button><button type="button" disabled={!!busy || !canUseDataService || !privateState?.exists || pending > 0} onClick={() => void dataOperation('balance')}>刷新余额</button></div><p className="wallet-opening-note">查询将通过开发 opening 把余额公开写入本地链。{pending > 0 && '请等待当前交易执行完成。'}</p><div className="wallet-balance-bottom"><span className="wallet-address">{account ? short(account) : '尚未连接本地链'}{account && <button aria-label="复制钱包地址" onClick={() => void copy(account)}><Icon name="copy" size={14} /></button>}</span><button onClick={() => { setAction('receive') }}><Icon name="plus" size={15} />收款地址</button></div><div className="wallet-orbit" aria-hidden="true"><div /><div /><span>⌘</span></div></div>
            <div className="wallet-private-card"><div className="wallet-card-top"><span><Icon name="shield" size={17} />密态账户</span><span className="wallet-label">DEV</span></div><div className="wallet-private-value">{displayedBalance}<Icon name="lock" size={21} /></div><p>{privateState?.exists ? `状态版本 ${privateState.version} · 链上状态引用` : '创建账户后可查询余额与链上状态'}</p><div className="wallet-private-bottom"><span className={`wallet-status ${privateState?.exists ? 'success' : ''}`}>{!account ? '等待连接' : !gateway ? '待配置 Gateway' : privateState?.exists ? '账户已就绪' : privateState ? '尚未创建' : '等待读取'}</span><button disabled={!!busy || wrongNetwork} onClick={() => !account ? void connect() : !gateway ? setView('settings') : privateState?.exists ? void copy(privateState.dataId) : void send('createAccount')}>{privateState?.exists ? '复制 dataId' : '设置账户'} ↗</button></div></div>
            <div className="wallet-stat-card"><span className="wallet-stat-icon"><Icon name="clock" /></span><span>进行中的交易</span><strong>{pending.toString().padStart(2, '0')}</strong><button onClick={() => setView('activity')}>查看执行进度 <span>↗</span></button></div>
          </section>
          <section className="wallet-quick-actions" aria-label="快捷操作">{([{ id: 'transfer', label: '转账', detail: '发送 PPSC 密态资产', icon: 'arrow' }, { id: 'deposit', label: '入金', detail: '登记密态资产入账', icon: 'down' }, { id: 'receive', label: '收款', detail: '复制本地账户地址', icon: 'wallet' }] as const).map(item => <button key={item.id} onClick={() => setAction(item.id)} className={action === item.id ? 'selected' : ''}><span className="wallet-action-icon"><Icon name={item.icon} /></span><span><strong>{item.label}</strong><small>{item.detail}</small></span><span className="wallet-action-arrow">↗</span></button>)}<button onClick={() => setView('contracts')}><span className="wallet-action-icon"><Icon name="code" /></span><span><strong>上传合约</strong><small>构建你的隐私应用</small></span><span className="wallet-action-arrow">↗</span></button></section>
          <div className="wallet-content-grid"><section className="wallet-panel wallet-transfer-panel"><div className="wallet-panel-heading"><h2>{actionTitle}</h2><span className="wallet-soft-label"><Icon name={isPrivate ? 'lock' : 'wallet'} size={13} />{isPrivate ? 'dataId 调用' : '链上资产'}</span></div><div className="wallet-tabs" aria-label="交易类型">{(['transfer', 'deposit', 'receive', 'withdraw'] as const).map(id => <button key={id} className={action === id ? 'active' : ''} onClick={() => setAction(id)}>{({ transfer: '转账', deposit: '入金', receive: '收款', withdraw: '出账' })[id]}</button>)}</div>
            {action === 'receive' ? <div className="wallet-receive"><span className="wallet-receive-symbol"><Icon name="wallet" size={36} /></span><h3>密态资产收款地址</h3><p>请先创建密态账户，再将此地址提供给本地链的转账方。</p><div className="wallet-receive-address">{account || '连接本地链后生成收款地址'}</div><span className="wallet-soft-label">{networks[chain].name} · PPSC</span><button className="wallet-primary" disabled={!!busy || wrongNetwork} onClick={() => account ? void copy(account) : void connect()}><Icon name="copy" size={17} />{account ? '复制收款地址' : '连接本地链'}</button></div> : <form onSubmit={event => { event.preventDefault(); if (!account) void connect(); else void send(action) }}>
              {action === 'transfer' && <label className="wallet-field">收款地址<input value={recipient} onChange={event => setRecipient(event.target.value)} placeholder="0x… 输入收款钱包地址" spellCheck={false} autoComplete="off" required={!!account} disabled={!!busy} /></label>}
              <div className="wallet-data-upload">
                <div className="wallet-panel-heading"><h3>上传数据 · 获取 dataId</h3><span className="wallet-soft-label">BFV</span></div>
                <div className="wallet-tabs"><button type="button" disabled={!!busy} className={inputMode === 'amount' ? 'active' : ''} onClick={() => setInputMode('amount')}>输入金额</button><button type="button" disabled={!!busy} className={inputMode === 'file' ? 'active' : ''} onClick={() => setInputMode('file')}>上传密文文件</button></div>
                {inputMode === 'amount' ? <label className="wallet-field">待加密金额<input value={inputAmount} onChange={event => setInputAmount(event.target.value)} inputMode="numeric" placeholder="例如 100 · 仅支持整数" disabled={!!busy} /><span className="wallet-field-hint">0–499122176；由本机原生客户端使用当前合约公钥加密</span></label> : <label className="wallet-field">BFV 密文文件<input type="file" accept=".bfv" disabled={!!busy} onChange={event => { setCiphertextFile(event.target.files?.[0] || null); event.target.value = '' }} /><span className="wallet-field-hint">{ciphertextFile?.name || '选择用当前合约委员会公钥生成的 .bfv 文件，最大 4 MB'}</span></label>}
                <button className="wallet-secondary" type="button" disabled={!!busy || !canUseDataService} onClick={() => void dataOperation('input')}>{busy && inputJob && !['completed', 'failed'].includes(inputJob.status) ? busy : '上传并获取 dataId'}</button>
                <p className="wallet-form-footnote">{canUseDataService ? '以当前 Anvil 测试账户签名。链上登记完成后自动填入下方；只上传数据，不自动转账。' : '请先启动钱包本地环境并连接合约。手工环境仍可填写已有 dataId。'}</p>
                {inputJob && <div className="wallet-data-result" role="status"><p>{({ pending: '准备中', encrypting: '正在加密或校验密文', uploading: '正在签名上传', registering: '已上传，等待链上登记', completed: '已登记 · dataId 可用于交易', failed: '上传失败', querying: '', opening: '' })[inputJob.status]}</p>{inputJob.error && <p>{inputJob.error}</p>}{inputJob.dataId && <code className="wallet-hash">{inputJob.dataId}</code>}{inputJob.status === 'completed' && inputJob.dataId && <button type="button" className="wallet-text-button" onClick={() => void copy(inputJob.dataId!)}>复制 dataId</button>}{!['completed', 'failed'].includes(inputJob.status) && <button type="button" className="wallet-text-button" disabled={!!busy} onClick={() => void dataOperation('input', inputJob.id)}>刷新任务状态</button>}</div>}
              </div>
              <label className="wallet-field">金额输入 dataId <span className="wallet-field-hint">已上传并登记至当前合约</span><input value={dataId} onChange={event => setDataId(event.target.value)} placeholder="0x… 32 字节输入引用" spellCheck={false} autoComplete="off" required={!!account} disabled={!!busy} /></label>
              <div className="wallet-form-note"><Icon name="shield" size={16} /><p>可在上方上传数据自动生成，也可粘贴当前账户、当前合约已登记的 dataId。</p></div>
              <div className="wallet-form-detail"><span>执行网络</span><strong>{networks[chain].name}</strong></div><div className="wallet-form-detail"><span>Gateway</span><strong title={gateway}>{short(gateway)}</strong></div>
              <button className="wallet-primary" type="submit" disabled={!!busy || wrongNetwork}>{busy || (!account ? '连接本地链以继续' : `提交${action === 'transfer' ? '转账' : action === 'deposit' ? '入账' : '出账'}`)}<Icon name="arrow" size={17} /></button>
              {isPrivate && <p className="wallet-form-footnote">交易由 Anvil 本地解锁账户发送，仅用于本机开发验证。</p>}
            </form>}
          </section><div className="wallet-right-column"><section className="wallet-panel"><div className="wallet-panel-heading"><h2>最近交易</h2><button className="wallet-text-button" onClick={() => setView('activity')}>查看全部 ↗</button></div>{activityList()}</section><section className="wallet-developer-card"><div><span className="wallet-eyebrow">BUILD & VALIDATE</span><h2>从合约源码，<br />到可验证的流程。</h2><p>上传 .ppsc 源码，生成 manifest<br />与 Solidity Gateway。</p><button onClick={() => setView('contracts')}>上传加密合约 <Icon name="arrow" size={16} /></button></div><span className="wallet-code-art" aria-hidden="true">{'{ }'}<span>LOCAL DEVELOPMENT</span></span></section></div></div>
        </>}

        {view === 'contracts' && <div className="wallet-contract-grid"><section className="wallet-panel"><div className="wallet-panel-heading"><h2>上传隐私合约</h2><span className="wallet-soft-label">.ppsc</span></div><div className={`wallet-dropzone ${dragging ? 'dragging' : ''}`} onDragOver={event => { event.preventDefault(); if (!busy) setDragging(true) }} onDragLeave={() => setDragging(false)} onDrop={event => { event.preventDefault(); setDragging(false); if (!busy) chooseFile(event.dataTransfer.files[0]) }}><span className="wallet-upload-icon"><Icon name="upload" size={32} /></span><h3>{file ? file.name : '将合约文件拖放到这里'}</h3><p>{file ? `${(file.size / 1024).toFixed(1)} KB · 准备编译` : '支持 .ppsc 源文件，最大 256 KB'}</p><button className="wallet-secondary" onClick={() => fileInput.current?.click()} disabled={!!busy}>{file ? '重新选择文件' : '选择文件'}</button><input ref={fileInput} type="file" accept=".ppsc" className="sr-only" tabIndex={-1} aria-label="选择 PPSC 合约文件" disabled={!!busy} onChange={event => { chooseFile(event.target.files?.[0]); event.target.value = '' }} /></div><div className="wallet-form-note"><Icon name="code" size={18} /><p>源码发送到本机编译服务。生成的 manifest 描述密态计算流程，上传本身不会部署合约或发起交易。</p></div><button className="wallet-primary" onClick={() => void compile()} disabled={!file || !!busy}>{busy || '上传并编译合约'}<Icon name="arrow" size={17} /></button></section><section className="wallet-panel"><div className="wallet-panel-heading"><h2>编译产物</h2><span className={`wallet-status ${compiled ? 'success' : ''}`}>{compiled ? environmentState?.environment.deployments.some(item => item.artifactId === compiled.artifactId && item.status === 'ready') ? '部署完成' : '编译完成' : '等待上传'}</span></div>{compiled ? <><h3 className="wallet-compiled-name">{compiled.name}</h3><button className="wallet-primary" onClick={() => void environmentAction('deploy')} disabled={!!busy || !environmentState?.environment.ready || environmentState.environment.deployments.some(item => item.artifactId === compiled.artifactId)}>{environmentState?.environment.deployments.some(item => item.artifactId === compiled.artifactId) ? '已提交部署 · 查看下方记录' : '自动部署合约'}<Icon name="upload" size={17} /></button><p className="wallet-form-footnote">{environmentState?.environment.ready ? '部署当前编译版本，启动独立 daemon；兼容钱包的合约会自动连接。' : '请先在钱包概览点击「一键部署本地环境」。'}</p><p className="wallet-small">Manifest hash</p><code className="wallet-hash">{compiled.manifestHash}</code><div className="wallet-artifacts">{Object.entries(compiled.files).map(([name, value]) => <button key={name} onClick={() => download(name, value)}><span><Icon name="code" size={17} />{name}</span><Icon name="down" size={16} /></button>)}</div><p className="wallet-small">自动部署完成后会显示链上回执，并连接兼容的钱包合约。也可下载产物用于手工部署。</p><button className="wallet-secondary" onClick={() => setView('settings')}>配置已部署的合约 ↗</button></> : <div className="wallet-empty"><Icon name="code" size={38} /><strong>准备好构建了吗？</strong><p>编译通过后，可下载 manifest、ABI、<br />操作序列、哈希与 Gateway 源码。</p></div>}</section></div>}
        {view === 'contracts' && <WalletEnvironmentPanel state={environmentState} error={environmentError} busy={!!busy} onAction={environmentAction} contractsOnly onConnect={deployment => { if (environmentState?.environment.id) void run('连接合约', () => attachDeployment(deployment, environmentState.environment.id!)) }} />}
        {view === 'activity' && <section className="wallet-panel"><div className="wallet-panel-heading"><h2>当前账户交易 <span className="wallet-count">{visibleActivities.length}</span></h2><span className="wallet-small">本浏览器发起的最近 100 笔记录</span></div>{activityList(true)}<div className="wallet-activity-note"><Icon name="clock" size={16} />链上确认后，任务继续等待 daemon 计算与结果回写；页面每 8 秒查询链上进度；受管环境的 daemon 状态见合约部署记录。</div></section>}
        {view === 'settings' && <div className="wallet-contract-grid"><section className="wallet-panel"><div className="wallet-panel-heading"><h2>合约连接</h2><span className="wallet-soft-label">{networks[chain].name}</span></div><form onSubmit={saveGateway}><label className="wallet-field">ConfidentialToken Gateway<input value={gatewayDraft} onChange={event => setGatewayDraft(event.target.value)} placeholder="0x… 实际部署的 Gateway 地址" spellCheck={false} disabled={!!busy} /></label><p className="wallet-small">自动部署后已填写 Gateway，也可手动填写当前链上的兼容地址。钱包会自动读取 ControlPlane。</p><button className="wallet-primary" disabled={!!busy} type="submit">保存连接配置<Icon name="arrow" size={17} /></button></form></section><section className="wallet-panel"><div className="wallet-panel-heading"><h2>开始使用</h2><Icon name="shield" /></div><ol className="wallet-steps"><li><span>01</span><div><strong>启动网络与 Runtime</strong><p>在钱包概览点击「一键部署本地环境」，自动启动 Anvil、PostgreSQL、默认 Gateway 和 OpenFHE / Shamir daemon。</p></div></li><li><span>02</span><div><strong>选择本地账户，创建密态账户</strong><p>连接 Anvil 后选择账户并填写 Gateway；收款方也需要已创建的密态账户。</p></div></li><li><span>03</span><div><strong>加密并登记输入后发起交易</strong><p>使用 manifest_encrypt_input 加密金额，再通过 manifest_input_client fhe-file 签名上传，等待 dataId 登记后入账或转账。</p></div></li></ol><p className="wallet-form-footnote">此页面适配编译生成的 ConfidentialToken Gateway。daemon 的执行方式与密码计算后端是独立配置；常驻运行不表示已启用真实加密。</p></section></div>}
        <footer className="wallet-footer"><span><Icon name="shield" size={14} />PPSC · 隐私保护智能合约</span><span>仅连接本机 Anvil · Chain ID 31337 <span className="wallet-footer-separator">/</span> <button disabled={!account || wrongNetwork} onClick={() => void refresh()}>刷新状态 <Icon name="refresh" size={12} /></button></span></footer>
      </main>
    </div>
  </div>
}
