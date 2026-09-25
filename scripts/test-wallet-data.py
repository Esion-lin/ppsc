#!/usr/bin/env python3
"""Exercise upload -> registered dataId -> deposit -> real balance on a running wallet.
Uses Anvil account #10 and adds 12 test PPSC; never restarts/stops the environment.
"""
import base64
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('wallet_environment_test', ROOT / 'scripts/test-wallet-environment.py')
helpers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helpers)
request, wait = helpers.request, helpers.wait


def main():
    state = wait()
    env = state['environment']
    assert env['ready'], 'Start the wallet local environment first'
    deployment = next(item for item in env['deployments'] if item['status'] == 'ready' and item['walletCompatible'])

    def rpc(method, params):
        code, result = request('rpc', {'method': method, 'params': params, 'session': env['id']})
        assert code == 200, result
        return result['result']

    owner = rpc('eth_accounts', [])[9]
    context = {'environmentId': env['id'], 'gateway': deployment['gateway'], 'owner': owner}
    cast_path = Path.home() / '.foundry/bin/cast'
    process_env = {k: v for k, v in os.environ.items() if k.lower() not in ('http_proxy', 'https_proxy', 'all_proxy')}
    process_env.update(NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost')

    def cast(*args):
        return subprocess.check_output([cast_path, *args], env=process_env, text=True).strip()

    def call(target, signature, *args):
        return cast('call', target, signature, *args, '--rpc-url', env['rpc'], '--no-proxy')

    def transact(signature, *args):
        nonce, deadline = str(time.time_ns() // 1000), str(int(time.time()) + 3600)
        data = cast('calldata', signature, *args, nonce, deadline)
        tx = {'from': owner, 'to': deployment['gateway'], 'data': data}
        execution = rpc('eth_call', [tx, 'latest'])
        tx_hash = rpc('eth_sendTransaction', [tx])
        assert rpc('eth_getTransactionReceipt', [tx_hash])['status'] == '0x1'
        for _ in range(180):
            status = call(deployment['control'], 'executionStatus(bytes32)(uint8)', execution)
            assert status not in ('7', '8'), status
            if status == '6':
                return
            time.sleep(0.5)
        raise AssertionError('daemon transaction timed out')

    def job(kind, failure=False, **fields):
        payload = {**context, 'id': str(uuid.uuid4()), 'kind': kind, **fields}
        code, result = request('data', payload)
        assert code == 202, result
        # Replaying the same request returns the same job without another transaction.
        assert request('data', payload)[1]['id'] == result['id']
        assert request('data', {**payload, 'owner': '0x' + '12' * 20})[0] == 409
        for _ in range(200):
            result = request('data?id=' + result['id'])[1]
            if result['status'] in ('completed', 'failed'):
                break
            time.sleep(0.5)
        assert result['status'] == ('failed' if failure else 'completed'), result
        wait()
        return result

    invalid = {**context, 'id': str(uuid.uuid4()), 'kind': 'input', 'mode': 'amount', 'amount': '-1'}
    assert request('data', invalid)[0] == 400
    assert request('data', {**invalid, 'amount': '499122177'})[0] == 400
    assert request('data', invalid, origin='https://example.com')[0] == 403
    assert request('data', {**invalid, 'amount': '1', 'environmentId': '0' * 32})[0] == 409

    variable = call(deployment['gateway'], 'balanceVariable(address)(bytes32)', owner)
    reference = call(deployment['control'], 'stateVariables(bytes32)(bytes32,bytes32,uint8,uint32,uint64,bool)', variable)
    if reference.splitlines()[-1] == 'false':
        transact('createAccount(uint64,uint64)')
    before = job('balance')
    amount = job('input', mode='amount', amount='7')
    assert call(deployment['control'], 'dataStatus(bytes32)(uint8)', amount['dataId']) == '1'
    transact('deposit(bytes32,uint64,uint64)', amount['dataId'])
    after = job('balance')
    assert int(after['amount']) == int(before['amount']) + 7
    assert after['dataId'] != before['dataId']
    print('PASS amount encryption, signed upload, on-chain dataId, deposit, actual balance +7', flush=True)

    with tempfile.TemporaryDirectory(prefix='ppsc-wallet-data-test-') as folder:
        ciphertext = Path(folder) / 'five.bfv'
        encryptor = ROOT / 'target/debug/manifest_encrypt_input'
        subprocess.run([encryptor, deployment['publicKeyPath'], '5', ciphertext], check=True, capture_output=True)
        uploaded = job('input', mode='file', ciphertext=base64.b64encode(ciphertext.read_bytes()).decode())
        transact('deposit(bytes32,uint64,uint64)', uploaded['dataId'])
        assert int(job('balance')['amount']) == int(before['amount']) + 12
        job('input', failure=True, mode='file', ciphertext=base64.b64encode(b'invalid' * 200).decode())
        other = next((item for item in env['deployments'] if item['id'] != deployment['id'] and item['status'] == 'ready'), None)
        if other:
            subprocess.run([encryptor, other['publicKeyPath'], '5', ciphertext], check=True, capture_output=True)
            job('input', failure=True, mode='file', ciphertext=base64.b64encode(ciphertext.read_bytes()).decode())
    final = request()[1]['environment']
    assert final['id'] == env['id'] and final['deployments'] == env['deployments']
    print('PASS BFV file upload, balance +12, invalid/wrong-key rejection, idempotency and origin/session guards', flush=True)


if __name__ == '__main__':
    main()
