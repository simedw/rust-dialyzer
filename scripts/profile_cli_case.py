#!/usr/bin/env python3
"""Profile one real CLI case and verify baseline warning/type outputs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--candidate', type=Path, required=True)
p.add_argument('--baseline', type=Path, required=True)
p.add_argument('--out', type=Path, required=True)
p.add_argument('--rounds', type=int, default=3)
trace = p.add_mutually_exclusive_group()
trace.add_argument('--trace-baseline', action='store_true')
trace.add_argument('--no-trace', action='store_true')
p.add_argument('beams', nargs='+', type=Path)
a = p.parse_args()
a.out.mkdir(parents=True, exist_ok=False)
root = Path(__file__).resolve().parents[1]
env = os.environ.copy()
erlang = Path('/Users/simedw/.asdf/installs/erlang/28.3')
elixir = Path('/Users/simedw/.asdf/installs/elixir/1.19.4-otp-28')
env['PATH'] = f'{erlang}/bin:{elixir}/bin:' + env['PATH']
args = ['--no_check_plt', '--plt', str(root/'.cache/corpus/otp/base.plt'), '-Wno_unknown', *map(str,a.beams)]
paths = [Path(__file__).resolve(), a.candidate.resolve(), a.baseline.resolve(), root/'.cache/corpus/otp/base.plt', *[p.resolve() for p in a.beams]]
for directory in ['src', 'scripts', 'corpus']:
    paths.extend(p for p in (a.candidate.resolve().parents[2]/directory).rglob('*') if p.is_file())
def inventory():
    return {str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(set(paths))}
before = inventory()
(a.out/'provenance.json').write_text(json.dumps(before, indent=2)+'\n')
rows = []
reference = None
for cycle in range(a.rounds + 1):
    for backend in (['baseline', 'candidate'] if cycle % 2 == 0 else ['candidate', 'baseline']):
        binary = getattr(a, backend)
        run_env = dict(env)
        run_env.pop('RUST_DIALYZER_TRACE', None)
        if not a.no_trace and (backend == 'candidate' or a.trace_baseline):
            run_env['RUST_DIALYZER_TRACE'] = '1'
        start = time.perf_counter()
        run = subprocess.run([str(binary), '--json', *args], capture_output=True, env=run_env, timeout=600)
        elapsed = time.perf_counter() - start
        name = f'{cycle}-{backend}'
        (a.out/(name+'.stdout')).write_bytes(run.stdout)
        (a.out/(name+'.stderr')).write_bytes(run.stderr)
        assert run.returncode in (0, 2), run.stderr.decode()
        data = json.loads(run.stdout)
        data.pop('implementation_sha256', None)
        if reference is None:
            reference = data
        assert data == reference, (backend, cycle, 'warnings, signatures or documentation changed')
        spans = {}
        for line in run.stderr.decode().splitlines():
            if line.startswith('PERF '):
                _, stage, seconds = line.split()
                spans.setdefault(stage, []).append(float(seconds))
        row = dict(cycle=cycle, backend=backend, seconds=elapsed, spans=spans)
        rows.append(row)
        print(json.dumps(row), flush=True)
assert inventory() == before, "candidate source, inputs or binaries changed during measurement"
report = dict(trace='none' if a.no_trace else 'both' if a.trace_baseline else 'candidate', inputs_unchanged=True, runs=rows, median_seconds={backend:statistics.median(r['seconds'] for r in rows if r['backend']==backend and r['cycle']) for backend in ['baseline','candidate']}, exact_warnings_signatures_documentation=True)
(a.out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report['median_seconds']),flush=True)
