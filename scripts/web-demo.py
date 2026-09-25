#!/usr/bin/env python3
"""Local demo bridge. stdin accepts fixed actions; stdout is redacted JSONL.

Owns an isolated Anvil and PostgreSQL cluster. Never attaches to an existing chain.
Uses the compiled manifest, a persistent committee daemon and real OpenFHE BFV/Shamir.
"""
import atexit
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import hashlib
import threading

ROOT = Path(__file__).resolve().parents[1]
SENDER_KEY = '0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80'
RECEIVER_KEY = '0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d'
NODE_KEY = '0x' + format(0xb0b, '064x')
ENV = os.environ.copy()
ENV['PATH'] = os.pathsep.join([str(Path.home() / '.foundry/bin'), str(Path.home() / '.cargo/bin'), '/opt/homebrew/bin', '/usr/local/bin', ENV.get('PATH', '')])
for name in ('HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'http_proxy', 'https_proxy', 'all_proxy'):
    ENV.pop(name, None)
ENV.update(NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost', CARGO_TERM_COLOR='never', NO_COLOR='1')
anvil = None
active = None
directory = None
ready = False
initialized = False
failed = False
completed = 0
rpc = ''
accounts = {}
CONTROL = ''
GATEWAY = ''
daemon = None
crypto_pid = None
finalized = set()
daemon_started = False
nonces = {}
output_lock = threading.Lock()


def emit(kind, **values):
    with output_lock:
        print(json.dumps(dict(type=kind, **values), ensure_ascii=False), flush=True)


def redact(text):
    for key in (SENDER_KEY, RECEIVER_KEY, NODE_KEY):
        text = text.replace(key, '<local-demo-key>')
    return re.sub(r'\x1b\[[0-9;]*[a-zA-Z]', '', text)


def run(args, timeout=180, quiet=False):
    global active
    args = [str(arg) for arg in args]
    if not quiet:
        emit('log', text='$ ' + redact(shlex.join(args)))
    active = subprocess.Popen(args, cwd=ROOT, env=ENV, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, start_new_session=True)
    # Stream line-by-line, while a timer enforces a bound even if stdout is silent.
    import threading
    expired = threading.Event()
    process = active
    def terminate():
        expired.set()
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    timer = threading.Timer(timeout, terminate)
    timer.start()
    output = []
    try:
        for line in process.stdout:
            output.append(line)
            if not quiet:
                emit('log', text=redact(line.rstrip()))
        code = process.wait()
        if expired.is_set():
            raise RuntimeError(f'{args[0]} 执行超时，已终止')
        if code:
            raise RuntimeError(f'{args[0]} 退出码 {code}，请查看终端输出')
        return ''.join(output).strip()
    finally:
        timer.cancel()
        active = None


def cast(*args):
    return run(['cast', *args])


def call(address, signature, *args):
    return cast('call', address, signature, *args, '--rpc-url', rpc, '--no-proxy')


def send(address, signature, *args, key=SENDER_KEY, value=None):
    extra = ['--value', value] if value else []
    return cast('send', address, signature, *args, *extra, '--private-key', key, '--rpc-url', rpc, '--no-proxy', '--json')


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def cleanup():
    global anvil, ready, initialized, directory, daemon, crypto_pid
    ready = False
    if daemon:
        process = daemon
        daemon = None
        try:
            # A reaped daemon has no process group to signal. On macOS a signal
            # to a vanished group may report EPERM instead of ESRCH.
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
        except ProcessLookupError:
            pass
    crypto_pid = None
    if active and active.poll() is None:
        try:
            os.killpg(active.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    if anvil:
        if anvil.poll() is None:
            anvil.terminate()
            try:
                anvil.wait(timeout=5)
            except subprocess.TimeoutExpired:
                anvil.kill()
                anvil.wait()
        anvil = None
    if directory and (directory / 'pg/postmaster.pid').exists():
        subprocess.run(['pg_ctl', '-D', str(directory / 'pg'), '-m', 'immediate', '-w', 'stop'], env=ENV, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=20)
    ready = False
    initialized = False
    # Retain the private temp directory for diagnosis; never remove user databases.


def initialize():
    global directory, anvil, rpc, initialized, failed, completed, accounts, CONTROL, GATEWAY
    cleanup()
    completed = 0
    accounts = {}
    CONTROL = GATEWAY = ''
    nonces.clear()
    finalized.clear()
    for command in ('anvil', 'cast', 'forge', 'cargo', 'initdb', 'pg_ctl'):
        if not shutil.which(command, path=ENV['PATH']):
            raise RuntimeError('缺少本地依赖: ' + command)
    directory = Path(tempfile.mkdtemp(prefix='ppsc-web-'))
    emit('log', text='创建独立演示环境：' + str(directory))
    run(['cargo', 'build', '-p', 'ppsc-runtime', '--bin', 'manifest_committee_daemon', '--bin', 'manifest_input_client'], timeout=600)
    run(['cargo', 'build', '-p', 'ppsc-compiler', '--bin', 'ppsc'], timeout=600)
    run(['cargo', 'build', '-p', 'ppsc-fhe', '--bin', 'manifest_crypto_service', '--bin', 'manifest_encrypt_input'], timeout=600)
    run(['initdb', '-D', directory / 'pg', '-U', 'ppsc_demo', '-A', 'trust', '--no-locale', '--encoding=UTF8'])
    pg_port = free_port()
    run(['pg_ctl', '-D', directory / 'pg', '-l', directory / 'postgres.log', '-o', f'-h 127.0.0.1 -p {pg_port} -k {directory}', '-w', 'start'])
    ENV['DATABASE_URL'] = f'postgres://ppsc_demo@127.0.0.1:{pg_port}/postgres'
    chain_port = free_port()
    rpc = f'http://127.0.0.1:{chain_port}'
    emit('log', text=f'$ anvil --host 127.0.0.1 --port {chain_port} --chain-id 31337')
    with open(directory / 'anvil.log', 'w') as log:
        anvil = subprocess.Popen(['anvil', '--host', '127.0.0.1', '--port', str(chain_port), '--chain-id', '31337', '--silent'], env=ENV, stdout=log, stderr=log)
    for _ in range(100):
        if anvil.poll() is not None:
            raise RuntimeError('Anvil 启动失败：' + (directory / 'anvil.log').read_text())
        try:
            with socket.create_connection(('127.0.0.1', chain_port), timeout=0.1):
                break
        except OSError:
            time.sleep(0.1)
    else:
        raise RuntimeError('Anvil 启动超时')
    if cast('chain-id', '--rpc-url', rpc) != '31337':
        raise RuntimeError('仅允许独立本地演示链')
    ENV.update(RPC_URL=rpc, NODE_TX_KEY=NODE_KEY, DEPLOYER_PRIVATE_KEY=SENDER_KEY,
               FOUNDRY_BROADCAST=str(directory / 'broadcast'))
    initialized = True
    failed = False
    emit('environment', environment=dict(rpc=rpc, database=str(directory / 'pg')))
    emit('log', text='本地链与数据库已就绪。下一步执行 deploy，部署演示合约。')


def health():
    if not daemon or daemon.poll() is not None:
        raise RuntimeError('committee daemon 已退出，请重新初始化；不会回退到明文计算')
    if crypto_pid:
        try:
            os.kill(crypto_pid, 0)
        except ProcessLookupError:
            raise RuntimeError('OpenFHE 服务已退出，密钥已失效，请重新初始化')


def runtime_status():
    return dict(backend='OpenFHE BFV + Shamir', daemonPid=daemon.pid,
                cryptoPid=crypto_pid, running=True,
                publicKeyFingerprint=hashlib.sha256((directory / 'committee.pub').read_bytes()).hexdigest())


def wait_for(predicate, timeout=120):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        health()
        if predicate():
            return
        time.sleep(0.2)
    raise RuntimeError('等待 daemon / 链上确认超时，请查看日志并重新初始化')


def daemon_output(process):
    global crypto_pid, daemon_started, ready, failed
    for line in process.stdout:
        if daemon is not process:
            break
        text = redact(line.rstrip())
        emit('log', text='[daemon] ' + text)
        if text.startswith('manifest committee daemon started:'):
            daemon_started = True
        match = re.search(r'OpenFHE BFV/Shamir service ready: pid=(\d+)', text)
        if match:
            crypto_pid = int(match.group(1))
        match = re.search(r'result finalized on chain: execution=(0x[0-9a-f]+)', text)
        if match:
            finalized.add(match.group(1))
    if daemon is process:
        ready = False
        failed = True
        emit('runtime', runtime=None)
        emit('error', message='committee daemon 已退出，请重新初始化')
        emit('done', ready=False, failed=True)


def create_contract(name, path, *arguments):
    args = ['forge', 'create', path + ':' + name, '--root', directory / 'contracts',
            '--rpc-url', rpc, '--private-key', SENDER_KEY, '--broadcast', '--json', '--no-proxy']
    if arguments:
        args += ['--constructor-args', *arguments]
    output = run(args, timeout=600)
    result = json.loads(output[output.index('{'):])
    address, transaction = result['deployedTo'], result['transactionHash']
    receipt = json.loads(cast('receipt', transaction, '--json', '--rpc-url', rpc))
    if int(str(receipt['status']), 0) != 1 or receipt['contractAddress'].lower() != address.lower():
        raise RuntimeError('合约部署回执未通过校验：' + name)
    if int(cast('codesize', address, '--rpc-url', rpc)) == 0:
        raise RuntimeError('部署地址不存在合约：' + name)
    return dict(name=name, address=address, transactionHash=transaction,
                blockNumber=int(str(receipt['blockNumber']), 0))


def invoke(signature, *arguments, key=SENDER_KEY):
    health()
    owner = accounts['receiver'] if key == RECEIVER_KEY else accounts['sender']
    nonce = nonces.get(owner, 0) + 1
    nonces[owner] = nonce
    args = [*arguments, str(nonce), str(int(time.time()) + 3600)]
    execution = call(GATEWAY, signature + '(bytes32)', *args, '--from', owner)
    receipt = json.loads(send(GATEWAY, signature, *args, key=key))
    if int(str(receipt['status']), 0) != 1:
        raise RuntimeError('Gateway 交易失败')
    emit('log', text='链上任务已登记，等待常驻 daemon：' + execution)
    wait_for(lambda: execution in finalized)
    if call(CONTROL, 'executionStatus(bytes32)(uint8)', execution) != '6':
        raise RuntimeError('daemon 返回后链上任务尚未完成')
    return execution


def deploy(contract_source=None, expected_manifest_hash=None, bootstrap=True):
    global ready, failed, accounts, CONTROL, GATEWAY, daemon, daemon_started
    if not initialized or failed:
        raise RuntimeError('请先初始化独立演示环境')
    if ready:
        raise RuntimeError('本次环境已部署合约，请勿重复部署')
    # Build in the isolated environment so demo compilation never overwrites user sources.
    project = directory / 'contracts'
    source = project / 'src'
    source.mkdir(parents=True)
    shutil.copy(ROOT / 'contracts/src/PpscControlPlane.sol', source)
    shutil.copytree(ROOT / 'contracts/src/interfaces', source / 'interfaces')
    (source / 'examples').mkdir()
    shutil.copy(ROOT / 'contracts/src/examples/LocalAcceptAllSortitionVerifier.sol', source / 'examples')
    (project / 'foundry.toml').write_text('[profile.default]\nsrc="src"\nout="out"\nsolc_version="0.8.24"\noptimizer=true\noptimizer_runs=200\nevm_version="cancun"\n')
    artifact = directory / 'artifacts'
    run([ROOT / 'target/debug/ppsc', 'build', contract_source or ROOT / 'examples/contracts/ConfidentialToken.ppsc',
         '--out', artifact, '--sol-out', source / 'generated'])
    artifacts = [entry for entry in artifact.iterdir() if entry.is_dir()]
    if len(artifacts) != 1 or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', artifacts[0].name):
        raise RuntimeError('编译产物名称无效')
    artifact = artifacts[0]
    contract_name = artifact.name
    hashes = dict(line.split('=', 1) for line in (artifact / 'hashes.env').read_text().splitlines() if '=' in line)
    if expected_manifest_hash and hashes['MANIFEST_HASH'] != expected_manifest_hash:
        raise RuntimeError('重新编译的 manifest 与预览不一致，请重新编译')
    sender = cast('wallet', 'address', '--private-key', SENDER_KEY)
    receiver = cast('wallet', 'address', '--private-key', RECEIVER_KEY)
    node = cast('wallet', 'address', '--private-key', NODE_KEY)
    accounts = dict(sender=sender, receiver=receiver)
    cast('rpc', 'anvil_setBalance', node, '0x8AC7230489E80000', '--rpc-url', rpc)
    verifier = create_contract('LocalAcceptAllSortitionVerifier', 'src/examples/LocalAcceptAllSortitionVerifier.sol')
    control = create_contract('PpscControlPlane', 'src/PpscControlPlane.sol', sender, node, verifier['address'])
    CONTROL = control['address']
    committee = cast('keccak', 'PPSC_LOCAL_COMMITTEE_V1')
    send(CONTROL, 'finalizeCommittee(bytes32,bytes32,uint64,address[],uint16,bytes)',
         committee, cast('keccak', 'PPSC_LOCAL_SEED_V1'), '1', '[' + node + ']', '1', '0x01', key=NODE_KEY)
    gateway = create_contract(contract_name + 'Gateway', 'src/generated/' + contract_name + 'Gateway.sol',
        CONTROL, cast('keccak', 'PPSC_CONFIDENTIAL_TOKEN_DEMO_V1'), hashes['MANIFEST_HASH'], hashes['RUNTIME_HASH'],
        cast('keccak', 'PPSC_EMPTY_CONFIDENTIAL_STATE_V1'), (artifact / 'manifest.json').as_uri())
    GATEWAY = gateway['address']
    contract_id = call(GATEWAY, 'confidentialContractId()(bytes32)')
    if call(CONTROL, 'contractManifestHash(bytes32)(bytes32)', contract_id) != hashes['MANIFEST_HASH']:
        raise RuntimeError('链上 manifest hash 不匹配')
    upload_port = free_port()
    ENV.update(CONTROL=CONTROL, GATEWAY=GATEWAY, CONTRACT_ID=contract_id,
        ACTIVE_COMMITTEE_ID=committee, NODE_ADDRESS=node, NODE_ID='web-demo-node',
        MANIFEST_PATH=str(artifact / 'manifest.json'), POLL_INTERVAL_SECONDS='1',
        MANIFEST_CRYPTO_BACKEND='process', MANIFEST_CRYPTO_COMMAND=str(ROOT / 'target/debug/manifest_crypto_service'),
        MANIFEST_PUBLIC_KEY_PATH=str(directory / 'committee.pub'),
        UPLOAD_LISTEN=f'127.0.0.1:{upload_port}', UPLOAD_URL=f'http://127.0.0.1:{upload_port}/v1/inputs')
    daemon_started = False
    daemon = subprocess.Popen([ROOT / 'target/debug/manifest_committee_daemon'], cwd=ROOT, env=ENV,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, start_new_session=True)
    threading.Thread(target=daemon_output, args=(daemon,), daemon=True).start()
    wait_for(lambda: daemon_started and crypto_pid and (directory / 'committee.pub').exists())
    emit('runtime', runtime=runtime_status())
    emit('deployment', deployment=dict(contracts=[verifier, control, gateway]))
    failed = False
    ready = True
    if bootstrap:
        invoke('createAccount(uint64,uint64)')
        invoke('createAccount(uint64,uint64)', key=RECEIVER_KEY)
        emit('log', text='编译合约已部署，OpenFHE 密钥已生成，daemon 常驻运行；Alice / Bob 账户已创建。')
        snapshot()
    return dict(name=contract_name, gateway=GATEWAY, control=CONTROL, contractId=contract_id,
        manifestHash=hashes['MANIFEST_HASH'], runtimeHash=hashes['RUNTIME_HASH'],
        contracts=[verifier, control, gateway], runtime=runtime_status(),
        uploadUrl=ENV['UPLOAD_URL'], publicKeyPath=ENV['MANIFEST_PUBLIC_KEY_PATH'])


def open_balance(key):
    execution = invoke('getBalance(uint64,uint64)', key=key)
    value = bytes.fromhex(call(GATEWAY, 'openingResults(bytes32)(bytes)', execution)[2:])
    prefix = b'PPSC_MANIFEST_OPENED_U128_V1'
    if not value.startswith(prefix) or len(value) != len(prefix) + 16:
        raise RuntimeError('无法识别实际 opening 结果')
    return str(int.from_bytes(value[len(prefix):], 'big'))


def snapshot():
    if not ready:
        raise RuntimeError('请先部署并启动 daemon')
    health()
    sender_balance = open_balance(SENDER_KEY)
    receiver_balance = open_balance(RECEIVER_KEY)
    data_ids = []
    for owner in (accounts['sender'], accounts['receiver']):
        variable = call(GATEWAY, 'balanceVariable(address)(bytes32)', owner)
        reference = call(CONTROL, 'stateVariables(bytes32)(bytes32,bytes32,uint8,uint32,uint64,bool)', variable).splitlines()
        data_ids.append(reference[1])
    values = dict(completed=completed, rpc=rpc, database=str(directory / 'pg'), gateway=GATEWAY,
        control=CONTROL, privateSender=sender_balance, privateReceiver=receiver_balance,
        senderDataId=data_ids[0], receiverDataId=data_ids[1],
        root=call(CONTROL, 'contractStateRoot(bytes32)(bytes32)', ENV['CONTRACT_ID']))
    emit('runtime', runtime=runtime_status())
    emit('snapshot', snapshot=values)


def encrypted_input(amount, key):
    health()
    path = directory / 'input.bfv'
    run([ROOT / 'target/debug/manifest_encrypt_input', directory / 'committee.pub', str(amount), path])
    ciphertext = path.read_bytes()
    if len(ciphertext) < 1024 or ciphertext.startswith(b'PPSC_DEV'):
        raise RuntimeError('输入不是有效的 OpenFHE 序列化密文')
    ENV['USER_PRIVATE_KEY'] = key
    try:
        output = run([ROOT / 'target/debug/manifest_input_client', 'fhe-file', path,
                      str(time.time_ns() // 1000), str(int(time.time()) + 3600)])
    finally:
        ENV.pop('USER_PRIVATE_KEY', None)
        path.unlink(missing_ok=True)
    match = re.search(r'dataId=(0x[0-9a-f]{64})', output)
    if not match:
        raise RuntimeError('未收到密文 dataId')
    data_id = match.group(1)
    wait_for(lambda: run(['cast', 'call', CONTROL, 'dataStatus(bytes32)(uint8)', data_id,
                         '--rpc-url', rpc, '--no-proxy'], quiet=True) == '1')
    return data_id


def transact(action):
    global completed
    if not ready or failed:
        raise RuntimeError('环境尚未就绪或上次执行失败，请重新初始化')
    expected = {'deposit': 0, 'transfer': 1, 'withdraw': 2}[action]
    if completed != expected:
        raise RuntimeError('请按入账 → 转账 → 出账的顺序执行，不可重复提交')
    amount = {'deposit': 100, 'transfer': 30, 'withdraw': 20}[action]
    key = RECEIVER_KEY if action == 'withdraw' else SENDER_KEY
    data_id = encrypted_input(amount, key)
    if action == 'transfer':
        invoke('transfer(address,bytes32,uint64,uint64)', accounts['receiver'], data_id)
    else:
        invoke(action + '(bytes32,uint64,uint64)', data_id, key=key)
    completed += 1
    snapshot()


atexit.register(cleanup)
def on_signal(signum, frame):
    raise SystemExit(0)
signal.signal(signal.SIGTERM, on_signal)
signal.signal(signal.SIGINT, on_signal)

if __name__ == '__main__':
    for line in sys.stdin:
        try:
            action = json.loads(line)['action']
            if action == 'init':
                initialize()
            elif action == 'deploy':
                deploy()
            elif action == 'stop':
                cleanup()
                failed = False
                emit('stopped')
                emit('log', text='本次 daemon、OpenFHE 子进程、Anvil 与 PostgreSQL 已停止。')
            elif action == 'status':
                snapshot()
            elif action in ('deposit', 'transfer', 'withdraw'):
                transact(action)
            else:
                raise RuntimeError('不支持的演示命令')
            emit('done', ready=ready, failed=failed)
        except Exception as error:
            failed = True
            emit('error', message=redact(str(error)))
            emit('done', ready=ready, failed=True)
