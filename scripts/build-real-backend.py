#!/usr/bin/env python3
"""Build the pinned OpenFHE static libraries locally, then the real demo binaries.

Requires git, Python/pip, a C++17 compiler and Cargo. Downloads stay in target/;
no system installation. Run from any directory: python3 scripts/build-real-backend.py
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'target/openfhe-development'
COMMIT = '1306d14f8c26bb6150d3e6ad54f28dfe1007689e'  # OpenFHE v1.5.1


def run(*args):
    subprocess.run([str(arg) for arg in args], cwd=ROOT, check=True)


def main():
    if not SOURCE.exists():
        run('git', 'clone', '--depth', '1', '--branch', 'v1.5.1', '--recurse-submodules',
            '--shallow-submodules', 'https://github.com/openfheorg/openfhe-development.git', SOURCE)
    revision = subprocess.check_output(['git', '-C', str(SOURCE), 'rev-parse', 'HEAD'], text=True).strip()
    if revision != COMMIT:
        raise SystemExit('target/openfhe-development is not pinned v1.5.1; keep it intact and configure OPENFHE_DIR manually.')
    cmake = shutil.which('cmake') or ROOT / 'target/build-tools/cmake/data/bin/cmake'
    if not Path(cmake).exists():
        run(sys.executable, '-m', 'pip', 'install', '--target', ROOT / 'target/build-tools', 'cmake==3.31.6')
    run(cmake, '-S', SOURCE, '-B', SOURCE / 'build', '-DCMAKE_BUILD_TYPE=Release',
        '-DBUILD_STATIC=ON', '-DBUILD_SHARED=OFF', '-DBUILD_UNITTESTS=OFF',
        '-DBUILD_EXAMPLES=OFF', '-DBUILD_BENCHMARKS=OFF', '-DWITH_OPENMP=OFF', '-DGIT_SUBMOD_AUTO=OFF')
    run(cmake, '--build', SOURCE / 'build', '-j', str(min(os.cpu_count() or 2, 6)))
    cargo = shutil.which('cargo') or Path.home() / '.cargo/bin/cargo'
    os.environ['OPENFHE_DIR'] = str(SOURCE)
    run(cargo, 'build', '-p', 'ppsc-fhe', '--bin', 'manifest_crypto_service', '--bin', 'manifest_encrypt_input')
    run(cargo, 'build', '-p', 'ppsc-runtime', '--bin', 'manifest_committee_daemon', '--bin', 'manifest_input_client')
    run(cargo, 'build', '-p', 'ppsc-compiler', '--bin', 'ppsc')
    print('Real backend ready. Run python3 scripts/test-real-crypto.py, then open /topic-one.')


if __name__ == '__main__':
    main()
