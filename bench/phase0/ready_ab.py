#!/usr/bin/env python3
"""Compare frozen analyzers on one preserved Core cache per project.

Run under lockf -k /tmp/rust-dialyzer-run.lock. CONFIG contains baseline and
candidate binary paths, inventory/reference roots, and project manifest paths.
This measures the Rust process only, never cold Core export.
"""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess

from compare_warnings import expand, items


def signatures(result):
    rows = result['signatures']
    keyed = {(row['module'], row['function'], row['arity']): row for row in rows}
    assert len(keyed) == len(rows), 'duplicate named signatures'
    return keyed


def warnings(result):
    return collections.Counter(json.dumps(row, sort_keys=True)
                               for row in items(expand(result['warnings'])))


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('config', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--repetitions', type=int, default=3)
args = parser.parse_args()
assert args.repetitions > 0
root = Path(__file__).resolve().parents[2]
config = json.loads(args.config.read_text())
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=False)
env = dict(os.environ, RUST_BACKTRACE='1')
for key in ('RUST_DIALYZER_TIMINGS', 'RUST_DIALYZER_VERIFY_FASTPATH',
            'RUST_DIALYZER_SEQUENTIAL'):
    env.pop(key, None)
references = {}
for project in config['manifests']:
    result = json.loads((Path(config['reference']) / project / 'run-1/rust.stdout').read_text())
    references[project] = (signatures(result), warnings(result))
metadata = {
    'config': config, 'repetitions': args.repetitions,
    'metric': 'Rust process on preserved Core cache; excludes Core export',
    'commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip(),
    'binary_sha256': {variant: hashlib.sha256(Path(config[variant]).read_bytes()).hexdigest()
                      for variant in ('baseline', 'candidate')},
    'manifest_sha256': {project: hashlib.sha256(Path(manifest).read_bytes()).hexdigest()
                        for project, manifest in config['manifests'].items()},
}
(output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
rows = []
for repeat in range(1, args.repetitions + 1):
    for project, manifest in config['manifests'].items():
        order = ('baseline', 'candidate') if repeat % 2 else ('candidate', 'baseline')
        for variant in order:
            directory = output / project / f'{repeat}-{variant}'
            directory.mkdir(parents=True)
            load_before = os.getloadavg()
            command = ['/usr/bin/time', '-l', config[variant],
                       root / '.cache/phase0/rust-cone/out/prior-library.json',
                       manifest, Path(config['inventory']) / project / 'options.json']
            with (directory / 'rust.stdout').open('w') as stdout, \
                    (directory / 'rust.stderr').open('w') as stderr:
                subprocess.run(list(map(str, command)), cwd=root, env=env,
                               stdout=stdout, stderr=stderr, check=True)
            counters = (directory / 'rust.stderr').read_text()

            def number(pattern):
                return float(re.search(pattern, counters).group(1))

            result = json.loads((directory / 'rust.stdout').read_text())
            assert result['status'] == 'ok'
            expected_signatures, expected_warnings = references[project]
            assert signatures(result) == expected_signatures, (project, variant, 'signatures changed')
            assert warnings(result) == expected_warnings, (project, variant, 'warnings changed')
            row = {'project': project, 'variant': variant, 'repeat': repeat,
                   'wall_s': number(r'([\d.]+) real'),
                   'cpu_s': number(r'([\d.]+) user') + number(r'([\d.]+) sys'),
                   'instructions_G': number(r'(\d+)\s+instructions retired') / 1e9,
                   'peak_GB': number(r'(\d+)\s+peak memory footprint') / 1e9,
                   'load_seconds': result['load_seconds'],
                   'analysis_seconds': result['analysis_seconds'],
                   'load_before': load_before, 'load_after': os.getloadavg(),
                   'warnings': result['public_warning_count'],
                   'signatures': len(result['signatures']), 'exact_parity': True}
            rows.append(row)
            (output / 'measurements.json').write_text(json.dumps(rows, indent=2) + '\n')
            print(json.dumps(row), flush=True)
summary = {}
for project in config['manifests']:
    summary[project] = {}
    for variant in ('baseline', 'candidate'):
        selected = [row for row in rows if row['project'] == project and row['variant'] == variant]
        summary[project][variant] = {
            key: statistics.median(row[key] for row in selected)
            for key in ('wall_s', 'cpu_s', 'instructions_G', 'peak_GB',
                        'load_seconds', 'analysis_seconds')}
    summary[project]['paired_wall_ratios'] = [
        next(row['wall_s'] for row in rows if row['project'] == project and row['variant'] == 'candidate' and row['repeat'] == repeat) /
        next(row['wall_s'] for row in rows if row['project'] == project and row['variant'] == 'baseline' and row['repeat'] == repeat)
        for repeat in range(1, args.repetitions + 1)]
(output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps(summary, indent=2), flush=True)
