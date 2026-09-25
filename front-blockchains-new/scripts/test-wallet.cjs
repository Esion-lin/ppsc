// Run with `npm run test:wallet`; uses the existing TypeScript installation, no test dependencies.
const assert = require('node:assert/strict')
const { test, after } = require('node:test')
const { readFileSync } = require('node:fs')
const { resolve } = require('node:path')
const { execFileSync } = require('node:child_process')
const Module = require('node:module')
const ts = require('typescript')
process.chdir(resolve(__dirname, '..'))
const originalResolve = Module._resolveFilename
Module._resolveFilename = function (request, ...rest) {
  return originalResolve.call(this, request.startsWith('@/') ? resolve(request.slice(2)) : request, ...rest)
}
require.extensions['.ts'] = (module, filename) => module._compile(ts.transpileModule(readFileSync(filename, 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, esModuleInterop: true },
}).outputText, filename)
const { NextRequest } = require('next/server')
const wallet = require('../lib/wallet.ts')
const rpcRoute = require('../app/api/wallet/rpc/route.ts')
const compileRoute = require('../app/api/wallet/compile/route.ts')
const dataRoute = require('../app/api/wallet/data/route.ts')
const originalFetch = global.fetch
const originalMode = process.env.NODE_ENV
after(() => { global.fetch = originalFetch; process.env.NODE_ENV = originalMode })
const account = '0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266'
const gateway = '0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0'
function request(path, body, origin = 'http://127.0.0.1:3000') {
  return new NextRequest(`http://127.0.0.1:3000/api/wallet/${path}`, { method: 'POST', headers: { host: '127.0.0.1:3000', origin, 'Content-Type': 'application/json' }, body: JSON.stringify(body) })
}

test('Gateway encoding agrees with Foundry for all browser-supported calls', () => {
  const cast = process.env.CAST || resolve(process.env.HOME, '.foundry/bin/cast')
  const id = '0x' + '12'.repeat(32)
  const cases = [
    ['controlPlane', 'controlPlane()', []], ['balanceVariable', 'balanceVariable(address)', [account]],
    ['stateVariables', 'stateVariables(bytes32)', [id]], ['executionStatus', 'executionStatus(bytes32)', [id]],
    ['invocationNonceUsed', 'invocationNonceUsed(address,uint64)', [account, 42n]],
    ['createAccount', 'createAccount(uint64,uint64)', [42n, 2000000000n]],
    ['deposit', 'deposit(bytes32,uint64,uint64)', [id, 42n, 2000000000n]],
    ['withdraw', 'withdraw(bytes32,uint64,uint64)', [id, 42n, 2000000000n]],
    ['transfer', 'transfer(address,bytes32,uint64,uint64)', [account, id, 42n, 2000000000n]],
  ]
  for (const [name, signature, args] of cases) assert.equal(wallet.encodeCall(name, args), execFileSync(cast, ['calldata', signature, ...args.map(String)], { encoding: 'utf8' }).trim())
  assert.throws(() => wallet.encodeCall('deposit', [-1n]))
  assert.throws(() => wallet.encodeCall('deposit', [2n ** 64n]))
  assert.throws(() => wallet.words('0x', 6))
})

test('RPC only accepts local development origins and narrow, value-free Gateway calls', async () => {
  process.env.NODE_ENV = 'development'
  const forwarded = []
  global.fetch = async (url, options) => {
    assert.equal(url, 'http://127.0.0.1:8545')
    const body = JSON.parse(options.body); forwarded.push(body)
    const result = body.method === 'eth_chainId' ? '0x7a69' : body.method === 'web3_clientVersion' ? 'anvil/v1' : '0x' + 'ab'.repeat(32)
    return Response.json({ result })
  }
  assert.equal((await rpcRoute.POST(request('rpc', { method: 'eth_accounts', params: [] }, 'https://evil.example'))).status, 403)
  process.env.NODE_ENV = 'production'
  assert.equal((await rpcRoute.POST(request('rpc', { method: 'eth_accounts', params: [] }))).status, 403)
  process.env.NODE_ENV = 'development'
  for (const body of [
    { method: 'anvil_reset', params: [] },
    { method: 'eth_sendTransaction', params: [{ from: account, to: gateway, value: '0x1' }] },
    { method: 'eth_sendTransaction', params: [{ from: account, to: gateway, data: wallet.encodeCall('createAccount', [1n, 2000000000n]), value: '0x1' }] },
    { method: 'eth_call', params: [{ to: gateway, data: '0xdeadbeef' }, 'latest'] },
  ]) assert.equal((await rpcRoute.POST(request('rpc', body))).status, 400)
  assert.equal(forwarded.length, 0)
  const result = await rpcRoute.POST(request('rpc', { method: 'eth_sendTransaction', params: [{ from: account, to: gateway, data: wallet.encodeCall('createAccount', [1n, 2000000000n]) }] }))
  assert.equal(result.status, 200)
  assert.equal(forwarded.at(-1).method, 'eth_sendTransaction')
  global.fetch = async () => Response.json({ result: '0x1' })
  assert.equal((await rpcRoute.POST(request('rpc', { method: 'eth_accounts', params: [] }))).status, 409)
  global.fetch = originalFetch
})

test('Real PPSC upload compilation produces matching manifest and Gateway; invalid inputs fail', async () => {
  process.env.NODE_ENV = 'development'
  const source = readFileSync('../examples/contracts/ConfidentialToken.ppsc', 'utf8')
  assert.equal((await compileRoute.POST(request('compile', { name: 'Token.ppsc', source }, 'https://evil.example'))).status, 403)
  const response = await compileRoute.POST(request('compile', { name: '../../Token.ppsc', source }))
  assert.equal(response.status, 200)
  const output = await response.json()
  assert.equal(output.name, 'ConfidentialToken')
  assert.match(output.manifestHash, /^0x[0-9a-f]{64}$/)
  assert.equal(JSON.parse(output.files['manifest.json']).contract, 'ConfidentialToken')
  assert.match(output.files['ConfidentialTokenGateway.sol'], /function transfer\(address to, bytes32 amountDataId, uint64 nonce, uint64 deadline\)/)
  assert.equal(Object.keys(output.files).length, 5)
  for (const body of [{ name: 'Token.sol', source }, { name: 'Token.ppsc', source: '' }, { name: 'Token.ppsc', source: 'not a contract' }]) {
    assert.equal((await compileRoute.POST(request('compile', body))).status, 400)
  }
  assert.equal((await compileRoute.POST(request('compile', { name: 'Token.ppsc', source: 'x'.repeat(400 * 1024) }))).status, 413)
})

test('Activity storage rejects malformed entries and preserves pending execution IDs', () => {
  assert.deepEqual(wallet.restoreActivities('invalid'), [])
  assert.deepEqual(wallet.restoreActivities(JSON.stringify([{ chain: '1', hash: 'bad' } ])), [])
  const entry = { hash: '0x' + 'ab'.repeat(32), account, chain: '31337', title: '密态入账', time: 1, status: 'pending', execution: '0x' + 'cd'.repeat(32), control: gateway }
  assert.deepEqual(wallet.restoreActivities(JSON.stringify([entry])), [entry])
})

test('Data uploads enforce origin, amount/file bounds, field types and managed session', async () => {
  process.env.NODE_ENV = 'development'
  const body = { id: '11111111-1111-4111-8111-111111111111', environmentId: 'a'.repeat(32), gateway, owner: account, kind: 'input', mode: 'amount', amount: '1' }
  assert.equal((await dataRoute.POST(request('data', body, 'https://evil.example'))).status, 403)
  for (const change of [{ amount: '-1' }, { amount: '499122177' }, { amount: 1 }, { amount: '1.5' }, { owner: [account] }, { kind: 'shell' }, { mode: 'file', ciphertext: 'YWJj' }]) {
    assert.equal((await dataRoute.POST(request('data', { ...body, ...change }))).status, 400)
  }
  assert.equal((await dataRoute.POST(request('data', body))).status, 409)
  process.env.NODE_ENV = 'production'
  assert.equal((await dataRoute.POST(request('data', body))).status, 403)
  process.env.NODE_ENV = 'development'
})

test('Real Anvil deployment, Gateway invocation and receipt through the wallet API', { skip: process.env.WALLET_CHAIN_TEST !== '1' }, async () => {
  const { spawn } = require('node:child_process')
  const { setTimeout: delay } = require('node:timers/promises')
  const cast = process.env.CAST || resolve(process.env.HOME, '.foundry/bin/cast')
  const anvilPath = process.env.ANVIL || resolve(process.env.HOME, '.foundry/bin/anvil')
  global.fetch = originalFetch
  async function raw(method, params = []) {
    const response = await originalFetch('http://127.0.0.1:8545', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }), signal: AbortSignal.timeout(2000) })
    const data = await response.json()
    if (data.error) throw new Error(data.error.message)
    return data.result
  }
  let occupied = false
  try { await raw('eth_chainId'); occupied = true } catch { /* No running chain. */ }
  assert.equal(occupied, false, 'Port 8545 is occupied; this test never touches an existing chain.')
  const child = spawn(anvilPath, ['--host', '127.0.0.1', '--port', '8545', '--chain-id', '31337', '--silent'], { stdio: 'ignore' })
  let spawnError
  child.on('error', error => { spawnError = error })
  try {
    let ready = false
    for (let i = 0; i < 50; i++) {
      if (spawnError) throw spawnError
      try { await raw('eth_chainId'); ready = true; break } catch { await delay(100) }
    }
    assert.ok(ready, 'Anvil did not start')
    const accounts = await raw('eth_accounts')
    async function mined(hash) {
      for (let i = 0; i < 100; i++) {
        const receipt = await raw('eth_getTransactionReceipt', [hash])
        if (receipt) return receipt
        await delay(30)
      }
      throw new Error('Transaction confirmation timed out')
    }
    const encode = (signature, args) => execFileSync(cast, ['abi-encode', signature, ...args], { encoding: 'utf8' }).trim().slice(2)
    async function deploy(name, signature, args = []) {
      const artifact = JSON.parse(readFileSync(`../contracts/out/${name}.sol/${name}.json`, 'utf8'))
      const data = artifact.bytecode.object + (signature ? encode(signature, args) : '')
      const hash = await raw('eth_sendTransaction', [{ from: accounts[0], data, gas: '0xb71b00' }])
      const receipt = await mined(hash)
      assert.equal(receipt.status, '0x1', `${name} deployment failed`)
      return receipt.contractAddress
    }
    const verifier = await deploy('LocalAcceptAllSortitionVerifier')
    const control = await deploy('PpscControlPlane', 'constructor(address,address,address)', [accounts[0], accounts[2], verifier])
    const hashes = readFileSync('../target/ppsc/ConfidentialToken/hashes.env', 'utf8')
    const hash = key => hashes.match(new RegExp(`^${key}=(.+)$`, 'm'))[1]
    const gatewayAddress = await deploy('ConfidentialTokenGateway', 'constructor(address,bytes32,bytes32,bytes32,bytes32,string)', [control, '0x' + '11'.repeat(32), hash('MANIFEST_HASH'), hash('RUNTIME_HASH'), '0x' + '22'.repeat(32), 'file://target/ppsc/ConfidentialToken/manifest.json'])
    process.env.NODE_ENV = 'development'
    global.fetch = (url, options) => url === '/api/wallet/rpc' ? rpcRoute.POST(request('rpc', JSON.parse(options.body))) : originalFetch(url, options)
    assert.equal(await wallet.controlAddress(gatewayAddress), control)
    assert.equal((await wallet.readWallet(accounts[0], gatewayAddress)).exists, false)
    const deadline = BigInt(Math.floor(Date.now() / 1000) + 3600)
    const args = [1n, deadline]
    const [execution] = wallet.words(await wallet.call(gatewayAddress, 'createAccount', args, accounts[0]), 1)
    const txHash = await wallet.rpc('eth_sendTransaction', [{ from: accounts[0], to: gatewayAddress, data: wallet.encodeCall('createAccount', args) }])
    await mined(txHash)
    assert.equal((await wallet.rpc('eth_getTransactionReceipt', [txHash])).status, '0x1')
    assert.equal(BigInt(wallet.words(await wallet.call(control, 'executionStatus', [execution]), 1)[0]), 1n)
    assert.equal(BigInt(wallet.words(await wallet.call(control, 'invocationNonceUsed', [accounts[0], 1n]), 1)[0]), 1n)
    await assert.rejects(() => wallet.call(gatewayAddress, 'createAccount', args, accounts[0]))
    await assert.rejects(() => wallet.call(gatewayAddress, 'deposit', ['0x' + 'ff'.repeat(32), 2n, deadline], accounts[0]))
  } finally {
    global.fetch = originalFetch
    child.kill('SIGTERM')
    await new Promise(resolve => { if (child.exitCode !== null) resolve(); else child.once('close', resolve) })
  }
})
