#!/usr/bin/env python3
"""One bounded wallet data job; configuration is resolved by the local server.
Never prints signing keys, plaintext input, ciphertext or subprocess stderr.
"""
import base64
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
ENV = os.environ.copy()
ENV['PATH'] = os.pathsep.join([str(Path.home() / '.foundry/bin'), ENV.get('PATH', '')])
for key in ('HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'http_proxy', 'https_proxy', 'all_proxy'):
    ENV.pop(key, None)
ENV.update(NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost')


def interrupted(_signum, _frame):
    raise RuntimeError('数据操作已取消')


signal.signal(signal.SIGTERM, interrupted)


def emit(**values):
    print(json.dumps(values), flush=True)


def run(args, error='本地命令执行失败', timeout=25):
    result = subprocess.run([str(arg) for arg in args], env=ENV, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(error)
    return result.stdout.strip()


def main():
    request = json.loads(sys.stdin.readline())
    config = request['config']
    rpc, control, gateway = config['rpc'], config['control'], config['gateway']

    def cast(*args):
        return run(['cast', *args, '--rpc-url', rpc, '--no-proxy'], '本地链调用失败，请检查账户、合约与 daemon')

    def health():
        for pid in (config['daemonPid'], config['cryptoPid']):
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                raise RuntimeError('合约 daemon 或密码进程已停止')

    health()
    if cast('chain-id') != '31337' or 'anvil' not in cast('client').lower():
        raise RuntimeError('仅支持受管本地 Anvil 环境')
    accounts = json.loads(cast('rpc', 'eth_accounts'))
    owner = request['owner'].lower()
    index = next((i for i, address in enumerate(accounts[:10]) if address.lower() == owner), None)
    if index is None:
        raise RuntimeError('请选择本地 Anvil 的前十个测试账户')
    key = run(['cast', 'wallet', 'private-key', 'test test test test test test test test test test test junk', str(index)], '无法获取本地测试账户签名')
    derived = run(['cast', 'wallet', 'address', '--private-key', key], '无法验证本地测试账户')
    if derived.lower() != owner:
        raise RuntimeError('当前账户与本地测试账户不匹配')
    nonce, deadline = time.time_ns() // 1000, int(time.time()) + 3600

    if request['kind'] == 'input':
        with tempfile.TemporaryDirectory(prefix='ppsc-wallet-input-') as folder:
            path = Path(folder) / 'input.bfv'
            emit(status='encrypting')
            if request['mode'] == 'amount':
                amount = request['amount']
                if not re.fullmatch(r'0|[1-9][0-9]{0,8}', amount) or int(amount) > 499122176:
                    raise RuntimeError('金额必须为 0–499122176 的整数')
                run([ROOT / 'target/debug/manifest_encrypt_input', config['publicKeyPath'], amount, path], '金额加密失败')
            else:
                data = base64.b64decode(request['ciphertext'], validate=True)
                if not 1024 <= len(data) <= 4 * 1024 * 1024:
                    raise RuntimeError('密文文件大小无效')
                path.write_bytes(data)
                run([ROOT / 'target/debug/manifest_encrypt_input', '--validate', config['publicKeyPath'], path], '密文格式无效，或不是用当前合约公钥生成的 BFV 密文')
            emit(status='uploading')
            ENV.update(USER_PRIVATE_KEY=key, CONTROL=control, CONTRACT_ID=config['contractId'], UPLOAD_URL=config['uploadUrl'])
            result = run([ROOT / 'target/debug/manifest_input_client', 'fhe-file', path, str(nonce), str(deadline)], '密文签名上传失败')
            ENV.pop('USER_PRIVATE_KEY', None)
        match = re.search(r'dataId=(0x[0-9a-f]{64})', result)
        if not match:
            raise RuntimeError('上传服务未返回 dataId')
        data_id = match.group(1)
        emit(status='registering', dataId=data_id)
        for _ in range(180):
            health()
            if cast('call', control, 'dataStatus(bytes32)(uint8)', data_id) == '1':
                emit(status='completed', dataId=data_id)
                return
            time.sleep(0.5)
        raise RuntimeError('已上传，但等待链上登记超时；请检查 daemon，勿将未登记的 dataId 用于交易')

    emit(status='querying')
    variable = cast('call', gateway, 'balanceVariable(address)(bytes32)', owner)
    def balance_reference():
        return cast('call', control, 'stateVariables(bytes32)(bytes32,bytes32,uint8,uint32,uint64,bool)', variable).splitlines()
    before = balance_reference()
    execution = cast('call', gateway, 'getBalance(uint64,uint64)(bytes32)', str(nonce), str(deadline), '--from', owner)
    receipt = json.loads(cast('send', gateway, 'getBalance(uint64,uint64)', str(nonce), str(deadline), '--private-key', key, '--json'))
    if int(str(receipt['status']), 0) != 1:
        raise RuntimeError('余额查询交易失败')
    emit(status='opening', executionId=execution, transactionHash=receipt['transactionHash'])
    for _ in range(180):
        health()
        status = cast('call', control, 'executionStatus(bytes32)(uint8)', execution)
        if status in ('7', '8'):
            raise RuntimeError('余额查询任务执行失败')
        if status == '6':
            opened = cast('call', gateway, 'openingResults(bytes32)(bytes)', execution)
            if opened != '0x':
                data = bytes.fromhex(opened[2:])
                prefix = b'PPSC_MANIFEST_OPENED_U128_V1'
                if not data.startswith(prefix) or len(data) != len(prefix) + 16:
                    raise RuntimeError('余额 opening 格式不匹配')
                reference = balance_reference()
                if before != reference:
                    raise RuntimeError('查询期间余额发生变化，请等待交易完成后刷新余额')
                emit(status='completed', amount=str(int.from_bytes(data[-16:], 'big')), dataId=reference[1], version=reference[4].split()[0])
                return
        time.sleep(0.5)
    raise RuntimeError('等待余额 opening 超时，请检查 daemon 和账户是否已创建')


try:
    main()
except Exception as error:
    emit(status='failed', error=str(error) if isinstance(error, RuntimeError) else '数据处理失败，请检查文件或本地服务')
    sys.exit(1)
