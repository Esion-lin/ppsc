#!/usr/bin/env python3
"""Real HTTP integration: one-click environment, upload deployment and wallet RPC.
Requires localhost:3000 development server and no active wallet environment.
"""
import json
import os
from pathlib import Path
import socket
import time
from urllib.error import HTTPError
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
BASE = 'http://127.0.0.1:3000'


def request(path='environment', body=None, origin=BASE):
    req = Request(BASE + '/api/wallet/' + path, data=json.dumps(body).encode() if body is not None else None,
                  headers={'Content-Type': 'application/json', 'Origin': origin})
    try:
        with urlopen(req, timeout=45) as response:
            return response.status, json.load(response)
    except HTTPError as error:
        return error.code, json.load(error)


def wait():
    for _ in range(900):
        code, state = request()
        assert code == 200
        if not state['busy']:
            assert not state['error'], '\n'.join(line['text'] for line in state['logs'][-25:])
            return state
        time.sleep(0.5)
    raise AssertionError('Environment timed out')


def main():
    initial = request()[1]
    assert not initial['busy'] and not initial['environment']['id'], 'Stop the existing wallet environment first'
    assert request(body={'action': 'start'}, origin='https://example.com')[0] == 403
    assert request(body={'action': 'shell'})[0] == 400
    assert request(body={'action': 'start'})[0] == 202
    assert request(body={'action': 'start'})[0] == 409
    session = None
    pids = []
    try:
        state = wait()
        env = state['environment']; session = env['id']
        assert env['ready'] and len(env['deployments']) == 1
        default = env['deployments'][0]
        assert default['status'] == 'ready' and default['walletCompatible']
        assert len(default['contracts']) == 3
        pids.extend([default['runtime']['daemonPid'], default['runtime']['cryptoPid']])
        assert request(body={'action': 'start'})[0] == 409
        print('PASS one-click Anvil + PostgreSQL + default Gateway + real daemon', flush=True)

        def rpc(method, params):
            code, result = request('rpc', {'method': method, 'params': params, 'session': session})
            assert code == 200, result
            return result['result']

        assert rpc('eth_chainId', []) == '0x7a69'
        account = rpc('eth_accounts', [])[0]
        source = (ROOT / 'examples/contracts/ConfidentialToken.ppsc').read_text().replace('ConfidentialToken', 'WalletToken')
        code, compiled = request('compile', {'name': 'WalletToken.ppsc', 'source': source})
        assert code == 200 and compiled['name'] == 'WalletToken'
        payload = {'action': 'deploy', 'environmentId': session, 'artifactId': compiled['artifactId']}
        assert request(body={**payload, 'environmentId': '0' * 32})[0] == 409
        assert request(body={**payload, 'artifactId': 'a' * 36})[0] == 409
        assert request(body=payload)[0] == 202
        assert request(body=payload)[0] == 409
        state = wait(); uploaded = state['environment']['deployments'][-1]
        assert uploaded['name'] == 'WalletToken' and uploaded['status'] == 'ready'
        assert uploaded['manifestHash'] == compiled['manifestHash']
        assert uploaded['gateway'] != default['gateway']
        assert state['environment']['deployments'][0]['runtime'] == default['runtime']
        pids.extend([uploaded['runtime']['daemonPid'], uploaded['runtime']['cryptoPid']])
        assert len(set(pids)) == 4
        assert request(body=payload)[0] == 409
        for deployment in (default, uploaded):
            for contract in deployment['contracts']:
                receipt = rpc('eth_getTransactionReceipt', [contract['transactionHash']])
                assert receipt['status'] == '0x1' and receipt['contractAddress'].lower() == contract['address'].lower()
        print('PASS uploaded contract compiled/deployed, six on-chain receipts, independent stable daemons', flush=True)

        nonce, deadline = 1234567, int(time.time()) + 3600
        data = '0x649aab54' + f'{nonce:064x}{deadline:064x}'
        tx = {'from': account, 'to': uploaded['gateway'], 'data': data}
        execution = rpc('eth_call', [tx, 'latest'])
        tx_hash = rpc('eth_sendTransaction', [tx])
        assert rpc('eth_getTransactionReceipt', [tx_hash])['status'] == '0x1'
        for _ in range(180):
            status = rpc('eth_call', [{'to': uploaded['control'], 'data': '0xb2973c91' + execution[2:]}, 'latest'])
            if int(status, 16) == 6:
                break
            time.sleep(0.5)
        else:
            raise AssertionError('Uploaded contract daemon did not complete wallet invocation')
        logs = '\n'.join(line['text'] for line in request()[1]['logs'])
        assert 'ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80' not in logs
        print('PASS wallet RPC reaches managed chain; uploaded Gateway invocation completed by its daemon', flush=True)
    finally:
        current = request()[1]['environment']
        if current['id']:
            assert request(body={'action': 'stop', 'environmentId': current['id']})[0] == 202
            stopped = wait()
            assert not stopped['environment']['id']
            assert request('rpc', {'method': 'eth_accounts', 'params': [], 'session': current['id']})[0] != 200
            time.sleep(1)
            for pid in pids:
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    continue
                raise AssertionError(f'Owned process {pid} was left running')
            with socket.socket() as sock:
                assert sock.connect_ex(('127.0.0.1', int(current['rpc'].rsplit(':', 1)[1]))) != 0
            print('PASS stop cleans up daemons and chain; stale session never falls back to another chain', flush=True)


if __name__ == '__main__':
    main()
