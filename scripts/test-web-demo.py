#!/usr/bin/env python3
"""Integration test for a running localhost Next development server.
Creates and stops one isolated demo; do not run during someone else's demo.
"""
import json
import os
import signal
import time
from urllib.request import Request, urlopen
from urllib.error import HTTPError

BASE = 'http://127.0.0.1:3000'


def request(action=None, origin=BASE):
    data = json.dumps({'action': action}).encode() if action else None
    req = Request(BASE + '/api/ppsc', data=data, headers={'Content-Type': 'application/json', 'Origin': origin})
    try:
        with urlopen(req, timeout=15) as response:
            return response.status, json.load(response)
    except HTTPError as error:
        return error.code, json.load(error)


def wait():
    deadline = time.monotonic() + 700
    while time.monotonic() < deadline:
        code, state = request()
        assert code == 200
        if not state['busy']:
            assert not state['error'], '\n'.join(line['text'] for line in state['logs'][-20:])
            return state
        time.sleep(0.5)
    raise AssertionError('Demo timed out')


if __name__ == '__main__':
    initial = request()[1]
    assert not initial['busy'] and not initial['ready'] and not initial.get('environment'), 'Stop the existing demo before running this test'
    first_log_id = max((line['id'] for line in initial['logs']), default=0)
    assert request('init', 'http://example.com')[0] == 403
    assert request('arbitrary-shell-command')[0] == 400
    assert request('deposit')[0] == 409
    assert request('deploy')[0] == 409
    assert request('init')[0] == 202
    assert request('init')[0] == 409
    try:
        state = wait()
        assert state['environment']['rpc'].startswith('http://127.0.0.1:')
        assert not state['ready'] and state['snapshot'] is None and state['deployment'] is None
        assert request('deposit')[0] == 409, 'Trading before deployment must be rejected'
        assert request('status')[0] == 409
        assert request('deploy')[0] == 202
        assert request('deploy')[0] == 409
        state = wait()
        assert state['ready']
        contracts = state['deployment']['contracts']
        assert {item['name'] for item in contracts} == {'LocalAcceptAllSortitionVerifier', 'PpscControlPlane', 'ConfidentialTokenGateway'}
        assert len({item['address'] for item in contracts}) == 3
        for item in contracts:
            assert len(item['address']) == 42 and len(item['transactionHash']) == 66 and item['blockNumber'] > 0
        assert request('deploy')[0] == 409, 'Already deployed contracts must not be redeployed'
        assert state['snapshot']['privateSender'] == '0'
        runtime = state['runtime']
        assert runtime['running'] and runtime['cryptoPid'] != runtime['daemonPid']
        print('PASS init → deploy / three real deployment receipts + real crypto daemon / pre-deploy and repeated-deploy guards', flush=True)
        expected = [
            ('deposit', {'privateSender': '100', 'privateReceiver': '0'}),
            ('transfer', {'privateSender': '70', 'privateReceiver': '30'}),
            ('withdraw', {'privateSender': '70', 'privateReceiver': '10'}),
        ]
        roots = set()
        for index, (action, values) in enumerate(expected, 1):
            assert request(action)[0] == 202
            state = wait()
            snapshot = state['snapshot']
            assert snapshot['completed'] == index
            assert state['runtime'] == runtime, 'Daemon, crypto PID and public key must remain stable'
            for key, value in values.items():
                assert snapshot[key] == value, (action, key, snapshot[key], value)
            roots.add(snapshot['root'])
            assert request(action)[0] == 409, 'Repeated transaction must be rejected'
            print('PASS ' + action + ' ' + json.dumps(values), flush=True)
        assert len(roots) == 3
        assert request('status')[0] == 202
        state = wait()
        assert state['snapshot']['completed'] == 3
        logs = '\n'.join(line['text'] for line in state['logs'] if line['id'] > first_log_id)
        assert 'transactionHash' in logs and 'result finalized on chain' in logs
        assert 'BFV ciphertext written:' in logs
        assert 'confidential_swap_worker' not in logs and 'dev-fhe' not in logs
        from importlib.util import spec_from_file_location, module_from_spec
        spec = spec_from_file_location('demo', __file__.replace('test-web-demo.py', 'web-demo.py'))
        demo = module_from_spec(spec)
        spec.loader.exec_module(demo)
        for key in (demo.SENDER_KEY, demo.RECEIVER_KEY, demo.NODE_KEY):
            assert key not in logs, 'Private key leaked in browser logs'
        print('PASS fresh state / distinct roots / real receipts / log redaction', flush=True)
        # Kill only the crypto process created by this test. The idle daemon must
        # detect the loss, fail closed, and reject further business transactions.
        os.kill(runtime['cryptoPid'], signal.SIGTERM)
        for _ in range(40):
            state = request()[1]
            if state['failed'] and not state['ready'] and state['runtime'] is None:
                break
            time.sleep(0.5)
        else:
            raise AssertionError('Crypto process exit was not detected')
        assert request('deposit')[0] == 409
        print('PASS crypto process exit detected without a new task; no plaintext fallback', flush=True)
        assert request('init')[0] == 202
        reset = wait()
        assert reset['environment'] and not reset['ready'] and reset['deployment'] is None and reset['snapshot'] is None
        assert request('deposit')[0] == 409
        print('PASS reinitialization clears deployed contracts and blocks trading until redeployed', flush=True)
    finally:
        assert request('stop')[0] == 202
        state = wait()
        assert not state['ready'] and state['snapshot'] is None and state['environment'] is None and state['deployment'] is None
        print('PASS stop', flush=True)
