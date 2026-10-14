#!/usr/bin/env python3
"""Sample the native analyzer with phase timings; diagnostic, never speed acceptance."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'bench/phase0'))
from three_projects import digest, host_pressure, write, check_same
from storage import require_disk_reserve


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('frozen', type=Path)
    parser.add_argument('binary', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--project', default='elixir')
    parser.add_argument('--reference', type=Path, required=True)
    args = parser.parse_args()
    with open('/private/tmp/rust-dialyzer-run.lock', 'a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        require_disk_reserve(args.output)
        args.output.mkdir(parents=True, exist_ok=False)
        inventory = args.frozen / args.project
        for entry in json.loads((inventory / 'inputs.json').read_text()):
            assert digest(entry['frozen']) == entry['sha256']
        binary = args.output.resolve() / 'core_cache_analyze'
        shutil.copyfile(args.binary, binary)
        binary.chmod(0o555)
        env = dict(os.environ, RUST_DIALYZER_SETUP_JOBS='14', RUST_DIALYZER_TIMINGS='1')
        for key in ['RUST_DIALYZER_VERIFY_FASTPATH', 'RUST_DIALYZER_GENERATION_PROBE', 'RUST_DIALYZER_SEQUENTIAL']:
            env.pop(key, None)
        samples, stop = [], threading.Event()
        started = time.monotonic()
        def sample_load():
            while not stop.is_set():
                samples.append({'elapsed_s': time.monotonic() - started,
                                'load': os.getloadavg(), **host_pressure()})
                stop.wait(1)
        sampler = threading.Thread(target=sample_load, daemon=True)
        sampler.start()
        try:
            with (args.output / 'result.json').open('w') as stdout, (args.output / 'timings.log').open('w') as stderr:
                process = subprocess.Popen([binary, args.frozen / 'empty-prior.json',
                                            inventory / 'manifest.json', inventory / 'options.json'],
                                           cwd=ROOT, env=env, stdout=stdout, stderr=stderr)
                sample = subprocess.run(['/usr/bin/sample', str(process.pid), '5', '1', '-file',
                                         str((args.output / 'stacks.sample').resolve())],
                                        capture_output=True, text=True)
                (args.output / 'sample.log').write_text(sample.stdout + sample.stderr)
                returncode = process.wait()
        finally:
            stop.set()
            sampler.join()
            write(args.output / 'load.json', samples)
        write(args.output / 'metadata.json', {
            'binary_sha256': digest(binary), 'pid': process.pid, 'returncode': returncode,
            'sample_returncode': sample.returncode, 'diagnostic_only': True,
            'warning_and_signature_reference': str(args.reference),
        })
        if returncode:
            raise RuntimeError(f'analyzer exited {returncode}')
        write(args.output / 'proof.json', check_same(args.reference, args.output / 'result.json'))
        print(args.output, flush=True)
