#!/usr/bin/env python3
"""Capture stage timings and an optional macOS CPU sample under the run lock.

profile_two_x.py BINARY MANIFEST OPTIONS OUT_DIR [--sample]
Run with lockf -k /tmp/rust-dialyzer-run.lock. Outputs remain in OUT_DIR.
"""
import collections
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

if sys.argv[1] == '--exec':
    Path(sys.argv[2]).write_text(str(os.getpid()))
    os.execv(sys.argv[3], sys.argv[3:])

ROOT = Path(__file__).resolve().parents[2]
binary, manifest, options, destination = map(lambda p: Path(p).resolve(), sys.argv[1:5])
destination.mkdir(parents=True, exist_ok=False)
env = dict(os.environ, RUST_DIALYZER_TIMINGS='1', RUST_BACKTRACE='1')
with (destination / 'result.json').open('w') as output, (destination / 'stderr').open('w') as errors:
    process = subprocess.Popen(['/usr/bin/time', '-l', sys.executable, __file__, '--exec',
        str(destination / 'pid'), str(binary), str(ROOT / '.cache/phase0/rust-cone/out/prior-library.json'),
        str(manifest), str(options)], stdout=output, stderr=errors, env=env, cwd=ROOT)
    sampler = None
    if '--sample' in sys.argv:
        while not (destination / 'pid').exists() and process.poll() is None:
            time.sleep(0.02)
        if (destination / 'pid').exists():
            with (destination / 'sample-log').open('w') as log:
                sampler = subprocess.Popen(['/usr/bin/sample', (destination / 'pid').read_text(),
                    '60', '10', '-file', str(destination / 'sample.txt')], stdout=log, stderr=log)
    code = process.wait()
    if sampler:
        sampler.wait()
    if code:
        raise SystemExit(code)

stderr = (destination / 'stderr').read_text()
records = [json.loads(line.split(' ', 1)[1]) for line in stderr.splitlines()
           if line.startswith('RUST_DIALYZER_TIMING ')]
grouped = collections.defaultdict(list)
for row in records:
    grouped[row['scope']].append(row)
summary = {'scopes': {scope: {'count': len(rows),
    'sum_ms': sum(r['total_ms'] for r in rows),
    'max_ms': max(r['total_ms'] for r in rows)} for scope, rows in grouped.items()},
    'top_modules': sorted(grouped['diagnostic_module_components'],
                          key=lambda r: -r['total_ms'])[:20],
    'stages': [r for r in records if r['scope'] in {
        'core_pipeline', 'compiled_analysis', 'probe_run', 'diagnostic_components',
        'scc_graph', 'module_batch', 'module_memo', 'setup_branch'}]}
for name, pattern, scale in [('wall_s', r'([\d.]+) real', 1),
                             ('instructions_G', r'(\d+)\s+instructions retired', 1e9),
                             ('peak_GB', r'(\d+)\s+peak memory footprint', 1e9)]:
    summary[name] = float(re.search(pattern, stderr).group(1)) / scale
summary['note'] = 'Nested stage times overlap. Module sums are worker elapsed time, not CPU time.'
(destination / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps({k: v for k, v in summary.items() if k not in {'top_modules', 'stages'}}, indent=2))
