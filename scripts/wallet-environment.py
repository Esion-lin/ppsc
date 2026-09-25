#!/usr/bin/env python3
"""Owned wallet environment: isolated chain/DB and one real daemon per contract.

Only fixed JSONL actions from the local server are accepted. No client-supplied
RPC endpoints, command paths, keys or Solidity artifacts are executed.
"""
import atexit
import importlib.util
import json
from pathlib import Path
import signal
import sys
import threading
import uuid

ROOT = Path(__file__).resolve().parents[1]
lock = threading.RLock()
state = dict(id=None, rpc=None, database=None, ready=False, deployments=[])
instances = []


def emit(kind, **values):
    with lock:
        print(json.dumps(dict(type=kind, **values), ensure_ascii=False), flush=True)


def publish():
    with lock:
        emit('state', environment=state)


def module():
    spec = importlib.util.spec_from_file_location('wallet_runtime_' + uuid.uuid4().hex, ROOT / 'scripts/web-demo.py')
    instance = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(instance)
    # The owner below controls cleanup order, including all extra daemons.
    atexit.unregister(instance.cleanup)
    return instance


base = module()
base.emit = lambda kind, **values: emit(kind, **values) if kind == 'log' else None


def cleanup():
    state['ready'] = False
    for instance in reversed(instances):
        instance.cleanup()
    instances.clear()
    base.cleanup()


def deploy(source, expected_hash=None, artifact_id=None, default=False):
    instance = module()
    instances.append(instance)
    instance.directory = base.directory / ('deployment-' + uuid.uuid4().hex)
    instance.directory.mkdir()
    instance.rpc = base.rpc
    instance.ENV.update(base.ENV)
    instance.initialized = True
    record = dict(id=uuid.uuid4().hex, artifactId=artifact_id, status='deploying', name='正在编译')
    state['deployments'].append(record)

    def events(kind, **values):
        if kind == 'log':
            emit('log', **values)
        elif kind == 'runtime' and values['runtime'] is None:
            record['status'] = 'failed'
            record['runtime'] = None
            publish()
        elif kind == 'error':
            record['error'] = values['message']
            publish()
    instance.emit = events
    publish()
    path = instance.directory / 'Contract.ppsc'
    path.write_text(source)
    try:
        result = instance.deploy(path, expected_hash, bootstrap=False)
        manifest = json.loads(Path(instance.ENV['MANIFEST_PATH']).read_text())
        signatures = {function['signature'] for function in manifest['functions']}
        compatible = {'createAccount()', 'deposit(bytes32)', 'withdraw(bytes32)', 'transfer(address,bytes32)', 'getBalance()'}.issubset(signatures)
        compatible = compatible and any(slot['name'] == 'balance' and slot['key_type'] == 'address' and slot['value_type'] == 'fhe_uint' for slot in manifest['state'])
        if default:
            instance.invoke('createAccount(uint64,uint64)')
            instance.invoke('createAccount(uint64,uint64)', key=instance.RECEIVER_KEY)
        record.update(result, walletCompatible=compatible, status='ready')
    except Exception as error:
        record.update(status='failed', error=instance.redact(str(error)))
        instance.cleanup()
        raise
    finally:
        publish()


atexit.register(cleanup)
def stop_signal(signum, frame):
    raise SystemExit(0)
signal.signal(signal.SIGTERM, stop_signal)
signal.signal(signal.SIGINT, stop_signal)

for line in sys.stdin:
    try:
        command = json.loads(line)
        if command['action'] == 'start':
            if state['id']:
                raise RuntimeError('环境已经启动，请勿重复启动')
            base.initialize()
            state.update(id=uuid.uuid4().hex, rpc=base.rpc, database=str(base.directory / 'pg'))
            publish()
            deploy((ROOT / 'examples/contracts/ConfidentialToken.ppsc').read_text(), default=True)
            state['ready'] = True
        elif command['action'] == 'deploy':
            if not state['ready'] or command.get('environmentId') != state['id']:
                raise RuntimeError('本地环境未就绪或会话已改变')
            deploy(command['source'], command['manifestHash'], command['artifactId'])
        elif command['action'] == 'stop':
            cleanup()
            state.update(id=None, rpc=None, database=None, ready=False, deployments=[])
        else:
            raise RuntimeError('不支持的命令')
        publish()
        emit('done')
        if command['action'] == 'stop':
            break
    except Exception as error:
        emit('error', message=base.redact(str(error)))
        emit('done')
