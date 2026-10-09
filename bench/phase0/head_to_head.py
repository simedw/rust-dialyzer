#!/usr/bin/env python3
"""Measure prepared head-to-head inputs. Run under rust-dialyzer-run.lock.

Usage: python3 bench/phase0/head_to_head.py OUTPUT_DIR
OUTPUT_DIR contains otp.exs and PROJECT/{beams.txt,options.json}.
Each repetition builds a fresh Core cache and a fresh OTP PLT.
"""
import argparse
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('out', type=Path)
parser.add_argument('--binary', type=Path, default=ROOT / 'target/perf/release/core_cache_analyze')
parser.add_argument('--cache-script', type=Path, default=ROOT / 'prototypes/core_cache/core_cache.exs')
parser.add_argument('--reference', type=Path)
parser.add_argument('--repetitions', type=int, default=3)
args = parser.parse_args()
OUT = args.out.resolve()
ELIXIR = ROOT / '.cache/real-projects/elixir-v1.20.4/bin/elixir'
BINARY = args.binary.resolve()
PRIOR = ROOT / '.cache/phase0/rust-cone/out/prior-library.json'
ENV = dict(os.environ, ERL_FLAGS='+S 14:14', RUST_BACKTRACE='1')
rows = []


def measure(label, command, directory):
    stdout = directory / f'{label}.stdout'
    stderr = directory / f'{label}.stderr'
    with stdout.open('w') as output, stderr.open('w') as errors:
        subprocess.run(['/usr/bin/time', '-l', *map(str, command)], cwd=ROOT,
                       env=ENV, stdout=output, stderr=errors, check=True)
    text = stderr.read_text()
    def stat(pattern):
        return float(re.search(pattern, text).group(1))
    result = dict(wall_s=stat(r'([\d.]+) real'),
                  user_s=stat(r'([\d.]+) user'), sys_s=stat(r'([\d.]+) sys'),
                  peak_GB=stat(r'(\d+)\s+peak memory footprint') / 1e9,
                  maxrss_GB=stat(r'(\d+)\s+maximum resident set size') / 1e9,
                  instructions_G=stat(r'(\d+)\s+instructions retired') / 1e9)
    print(label, json.dumps(result), flush=True)
    return result


def signatures(result):
    return {(s['module'], s['function'], s['arity']): s for s in result['signatures']}


references = {}
if args.reference:
    for project in ['phoenix', 'elixir']:
        references[project] = signatures(json.loads(
            (args.reference / project / 'run-1/rust.stdout').read_text()))

for repeat in range(1, args.repetitions + 1):
    for project in ['phoenix', 'elixir']:
        base = OUT / project
        run = base / f'run-{repeat}'
        run.mkdir()  # Never silently reuse a previous PLT or cold cache.
        otp_cmd = [ELIXIR, OUT / 'otp.exs', base / 'beams.txt', run / 'otp.plt',
                   run / 'otp-warnings.json']
        cache_cmd = [ELIXIR, args.cache_script.resolve(), ROOT,
                     base / 'beams.txt', run / 'cache', run / 'manifest.json', '--jobs', '14']
        rust_cmd = [BINARY, PRIOR, run / 'manifest.json', base / 'options.json']
        print(f'{project} repetition {repeat}', flush=True)
        if repeat % 2:
            otp = measure('otp', otp_cmd, run)
        cache = measure('cold-cache', cache_cmd, run)
        rust = measure('rust', rust_cmd, run)
        if not repeat % 2:
            otp = measure('otp', otp_cmd, run)
        warm = measure('warm-cache', cache_cmd, run)
        cache_details = json.loads((run / 'cold-cache.stdout').read_text().splitlines()[-1])
        warm_details = json.loads((run / 'warm-cache.stdout').read_text().splitlines()[-1])
        assert cache_details['compiled'] == cache_details['modules']
        output = json.loads((run / 'rust.stdout').read_text())
        if output['status'] != 'ok':
            raise RuntimeError(output)
        signature_parity = None
        if project in references:
            signature_parity = signatures(output) == references[project]
            if not signature_parity:
                raise RuntimeError(f'{project}: inferred signatures changed')
        parity = subprocess.run([sys.executable, ROOT / 'bench/phase0/compare_warnings.py',
                                 run / 'otp-warnings.json', run / 'rust.stdout'],
                                capture_output=True, text=True, check=True).stdout
        print(parity.splitlines()[0], flush=True)
        row = dict(project=project, repeat=repeat, modules=output['modules'],
                   warnings=output['public_warning_count'], otp=otp, cold_cache=cache,
                   warm_cache=warm, rust=rust, parity=parity,
                   cold_cache_details=cache_details, warm_cache_details=warm_details,
                   signature_parity=signature_parity,
                   cold_rust_wall_s=cache['wall_s'] + rust['wall_s'],
                   cache_check_plus_rust_wall_s=warm['wall_s'] + rust['wall_s'],
                   cold_rust_peak_GB=max(cache['peak_GB'], rust['peak_GB']))
        rows.append(row)
        (OUT / 'measurements.json').write_text(json.dumps(rows, indent=2) + '\n')

summary = {}
for project in ['phoenix', 'elixir']:
    selected = [r for r in rows if r['project'] == project]
    median = lambda values: round(statistics.median(values), 3)
    summary[project] = dict(modules=selected[0]['modules'], warnings=selected[0]['warnings'],
        otp_wall_s=median([r['otp']['wall_s'] for r in selected]),
        otp_peak_GB=median([r['otp']['peak_GB'] for r in selected]),
        rust_wall_s=median([r['rust']['wall_s'] for r in selected]),
        rust_peak_GB=median([r['rust']['peak_GB'] for r in selected]),
        cold_rust_wall_s=median([r['cold_rust_wall_s'] for r in selected]),
        cache_check_plus_rust_wall_s=median([r['cache_check_plus_rust_wall_s'] for r in selected]),
        cold_cache_wall_s=median([r['cold_cache']['wall_s'] for r in selected]))
(OUT / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps(summary, indent=2), flush=True)
