'use client'

import type { WalletDeployment, WalletEnvironmentState } from '@/lib/wallet-environment-types'

export default function WalletEnvironmentPanel({ state, error, busy, onAction, contractsOnly = false, onConnect }: {
  state: WalletEnvironmentState | null; error: string; busy: boolean; contractsOnly?: boolean
  onAction: (action: 'start' | 'deploy' | 'stop') => Promise<void>
  onConnect?: (deployment: WalletDeployment) => void
}) {
  const env = state?.environment
  return <section className="wallet-panel wallet-environment" aria-label={contractsOnly ? '合约部署记录' : '本地环境'}>
    <div className="wallet-panel-heading"><div><h2>{contractsOnly ? '合约部署记录' : '本地开发环境'}</h2><p className="wallet-small">{contractsOnly ? '每个合约独立运行 daemon，部署记录在当前环境会话中保留。' : 'Anvil · PostgreSQL · 编译合约 · OpenFHE / Shamir daemon'}</p></div><span className={`wallet-status ${env?.ready ? 'success' : ''}`}>{state?.busy ? '正在执行' : env?.ready ? '环境已就绪' : env?.id ? '环境未就绪' : '尚未启动'}</span></div>
    {!contractsOnly && <>
      <div className="wallet-environment-actions"><button className="wallet-primary" disabled={busy || !state || !!env?.id} onClick={() => void onAction('start')}>{env?.ready ? '本地环境已创建' : state?.busy ? '正在部署本地环境…' : env?.id ? '本地环境已创建' : '一键部署本地环境'}</button><button className="wallet-secondary" disabled={busy || !env?.id} onClick={() => void onAction('stop')}>停止本地环境</button></div>
      <p className="wallet-form-footnote">自动使用空闲本地端口，并连接默认 ConfidentialToken Gateway。启动时自动创建 Alice / Bob 账户。停止后内存密钥失效，再次启动会创建全新环境。</p>
      {env?.rpc && <div className="wallet-environment-grid"><div><span className="wallet-small">本地 RPC</span><code className="wallet-hash">{env.rpc}</code></div><div><span className="wallet-small">环境会话</span><code className="wallet-hash">{env.id}</code></div></div>}
      {env?.deployments[0]?.runtime && <p className="wallet-small">默认 daemon PID {env.deployments[0].runtime.daemonPid} · 密码进程 PID {env.deployments[0].runtime.cryptoPid} · {env.deployments[0].status === 'ready' ? '运行中' : '正在部署'}</p>}
      {env?.deployments[0]?.status === 'failed' && <p role="alert" className="wallet-notice error">默认合约 daemon 已停止，请停止并重新启动环境。</p>}
    </>}
    {(error || state?.error) && <p className="wallet-notice error" role="alert">{error || state?.error}</p>}
    {contractsOnly && <div className="wallet-deployment-list">{env?.deployments.length ? [...env.deployments].reverse().map(item => <article className="wallet-deployment" key={item.id}>
      <div className="wallet-panel-heading"><h3>{item.name}</h3><span className={`wallet-status ${item.status === 'ready' ? 'success' : ''}`}>{item.status === 'ready' ? '已部署 · daemon 运行中' : item.status === 'failed' ? '执行异常' : '部署中…'}</span></div>
      {item.error && <p role="alert" className="wallet-small">{item.error}</p>}
      {item.gateway && <><span className="wallet-small">Gateway</span><code className="wallet-hash">{item.gateway}</code><span className="wallet-small">Contract ID</span><code className="wallet-hash">{item.contractId}</code><span className="wallet-small">Manifest hash</span><code className="wallet-hash">{item.manifestHash}</code></>}
      {item.runtime && <p className="wallet-small">Daemon PID {item.runtime.daemonPid} · OpenFHE PID {item.runtime.cryptoPid}</p>}
      {item.status === 'ready' && <>{item.walletCompatible ? <button className="wallet-secondary" disabled={busy} onClick={() => onConnect?.(item)}>连接此合约</button> : <p className="wallet-form-footnote">已部署并启动 Runtime。此合约没有钱包所需的 balance / 转账接口，可使用生成的 Gateway ABI 调用。</p>}
        <details><summary>部署回执与输入上传配置</summary>{item.contracts?.map(contract => <div key={contract.address}><p className="wallet-small">{contract.name} · 区块 {contract.blockNumber}</p><code className="wallet-hash">{contract.address}</code><code className="wallet-hash">{contract.transactionHash}</code></div>)}<span className="wallet-small">密文上传地址</span><code className="wallet-hash">{item.uploadUrl}</code><span className="wallet-small">本机委员会公钥</span><code className="wallet-hash">{item.publicKeyPath}</code></details>
      </>}
    </article>) : <p className="wallet-small">启动环境后显示默认合约；上传编译后的合约可在上方自动部署。</p>}</div>}
    {!!state?.logs.length && <details className="wallet-environment-logs"><summary>本地环境日志</summary><div tabIndex={0}>{state.logs.slice(-120).map(line => <pre key={line.id}>{line.text}</pre>)}</div></details>}
  </section>
}
