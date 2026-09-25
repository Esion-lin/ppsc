#!/usr/bin/env python3
"""Exercise public-key-only encryption across processes and the daemon wire protocol."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / 'target/debug'


def main():
    with tempfile.TemporaryDirectory(prefix='ppsc-crypto-test-') as folder:
        key = Path(folder) / 'committee.pub'
        service = subprocess.Popen([BIN / 'manifest_crypto_service'],
            env={**os.environ, 'MANIFEST_PUBLIC_KEY_PATH': str(key)},
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        try:
            for _ in range(300):
                if key.exists():
                    break
                assert service.poll() is None, 'Crypto service exited'
                time.sleep(0.1)
            assert key.exists(), 'Public key export timed out'

            def request(value):
                service.stdin.write(json.dumps(value) + '\n')
                service.stdin.flush()
                response = json.loads(service.stdout.readline())
                assert 'error' not in response, response
                return response

            def evaluate(opcode, *arguments):
                return request({'evaluate': {'opcode': opcode, 'arguments': arguments}})['value']

            def encrypt(amount):
                path = Path(folder) / 'ciphertext'
                subprocess.run([BIN / 'manifest_encrypt_input', key, str(amount), path], check=True, capture_output=True)
                data = path.read_bytes()
                assert len(data) > 1024 and not data.startswith(b'PPSC_DEV')
                subprocess.run([BIN / 'manifest_encrypt_input', '--validate', key, path], check=True, capture_output=True)
                return {'value_type': 'fhe_uint', 'payload': data.hex()}

            def opening(value):
                shares = evaluate('convert.h2s', value)
                opened = request({'pick': {'value': shares}})['value']
                data = bytes.fromhex(opened['payload'].removeprefix('0x'))
                assert data.startswith(b'PPSC_MANIFEST_OPENED_U128_V1')
                return int.from_bytes(data[-16:], 'big')

            hundred, thirty = encrypt(100), encrypt(30)
            assert hundred != encrypt(100), 'BFV encryption must be randomized'
            assert opening(hundred) == 100
            assert opening(evaluate('fhe.add', hundred, thirty)) == 130
            assert opening(evaluate('fhe.sub', hundred, thirty)) == 70
            for left, right, expected in [(hundred, thirty, True), (thirty, hundred, False), (hundred, hundred, True)]:
                condition = evaluate('convert.h2s', evaluate('fhe.ge', left, right))
                assert request({'secret_bool': {'value': condition}})['boolean'] is expected
            invalid = subprocess.run([BIN / 'manifest_encrypt_input', key, '499122177', Path(folder) / 'bad'], capture_output=True)
            assert invalid.returncode != 0
            corrupt = Path(folder) / 'corrupt.bfv'
            corrupt.write_bytes(b'not a BFV ciphertext' * 100)
            assert subprocess.run([BIN / 'manifest_encrypt_input', '--validate', key, corrupt], capture_output=True).returncode != 0
            print('PASS public-key-only BFV encryption, randomized ciphertexts, add/sub, Shamir comparison and opening, amount bound')
        finally:
            service.stdin.close()
            try:
                service.wait(timeout=5)
            except subprocess.TimeoutExpired:
                service.kill()
                service.wait()


if __name__ == '__main__':
    main()
