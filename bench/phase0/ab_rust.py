#!/usr/bin/env python3
"""Interleave two fresh-cache Rust variants under lockf -k rust-dialyzer-run.lock.

ab_rust.py CONFIG_JSON OUTPUT_DIR [--repetitions N] [--project phoenix|elixir]
CONFIG_JSON names baseline/candidate binary and cache_script, inventory, reference.
Each process reports macOS time counters. Every warning and named signature is checked.
"""
import argparse
import collections
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import sys

from compare_warnings import expand, items
from storage import require_disk_reserve

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('config', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--repetitions', type=int, default=3)
parser.add_argument('--keep-caches', action='store_true')
parser.add_argument('--project', choices=['phoenix', 'elixir'], action='append')
args = parser.parse_args()
assert args.repetitions > 0
config = json.loads(args.config.read_text())
out = args.output.resolve()
require_disk_reserve(out)
out.mkdir(parents=True, exist_ok=False)
env = dict(os.environ, ERL_FLAGS='+S 14:14', RUST_BACKTRACE='1')
projects = args.project or ['phoenix', 'elixir']


def signatures(result):
    return {(row['module'], row['function'], row['arity']): row
            for row in result['signatures']}


def warnings(result):
    return collections.Counter(json.dumps(row, sort_keys=True)
                               for row in items(expand(result['warnings'])))


def measure(label, command, directory):
    require_disk_reserve(directory)
    load_before = os.getloadavg()
    with (directory / (label + '.stdout')).open('w') as stdout, \
            (directory / (label + '.stderr')).open('w') as stderr:
        subprocess.run(['/usr/bin/time', '-l', *map(str, command)], cwd=ROOT,
                       env=env, stdout=stdout, stderr=stderr, check=True)
    text = (directory / (label + '.stderr')).read_text()
    def number(pattern):
        return float(re.search(pattern, text).group(1))
    return {'wall_s': number(r'([\d.]+) real'),
            'cpu_s': number(r'([\d.]+) user') + number(r'([\d.]+) sys'),
            'instructions_G': number(r'(\d+)\s+instructions retired') / 1e9,
            'peak_GB': number(r'(\d+)\s+peak memory footprint') / 1e9,
            'load_before': load_before, 'load_after': os.getloadavg()}


references = {}
for project in projects:
    result = json.loads((Path(config['reference']) / project / 'run-1/rust.stdout').read_text())
    references[project] = (signatures(result), warnings(result))
metadata = {'config': config, 'repetitions': args.repetitions,
            'commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
            'binary_sha256': {name: hashlib.sha256(Path(config[name]['binary']).read_bytes()).hexdigest()
                              for name in ['baseline', 'candidate']},
            'metric': 'fresh Core cache plus Rust process, all warnings and named signatures',
            'note': 'Empty tool caches; filesystem cache not flushed. Peaks are sequential maxima.'}
(out / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
rows = []
for repeat in range(1, args.repetitions + 1):
    for project in projects:
        variants = ['baseline', 'candidate'] if repeat % 2 else ['candidate', 'baseline']
        for variant in variants:
            directory = out / project / f'{repeat}-{variant}'
            directory.mkdir(parents=True)
            inventory = Path(config['inventory']) / project
            cache = measure('cache', [ROOT / '.cache/real-projects/elixir-v1.20.4/bin/elixir',
                config[variant]['cache_script'], ROOT, inventory / 'beams.txt',
                directory / 'cache', directory / 'manifest.json', '--jobs', '14'], directory)
            details = json.loads((directory / 'cache.stdout').read_text().splitlines()[-1])
            assert details['compiled'] == details['modules'] and details['reused'] == 0
            rust = measure('rust', [config[variant]['binary'],
                ROOT / '.cache/phase0/rust-cone/out/prior-library.json',
                directory / 'manifest.json', inventory / 'options.json'], directory)
            result = json.loads((directory / 'rust.stdout').read_text())
            assert result['status'] == 'ok'
            expected_signatures, expected_warnings = references[project]
            assert signatures(result) == expected_signatures, (project, variant, 'signatures changed')
            assert warnings(result) == expected_warnings, (project, variant, 'warnings changed')
            row = {'project': project, 'variant': variant, 'repeat': repeat,
                   'cache': cache, 'rust': rust, 'fresh_wall_s': cache['wall_s'] + rust['wall_s'],
                   'fresh_cpu_s': cache['cpu_s'] + rust['cpu_s'],
                   'fresh_instructions_G': cache['instructions_G'] + rust['instructions_G'],
                   'fresh_peak_GB': max(cache['peak_GB'], rust['peak_GB']),
                   'warnings': result['public_warning_count'], 'signatures': len(result['signatures']),
                   'exact_parity': True}
            rows.append(row)
            (out / 'measurements.json').write_text(json.dumps(rows, indent=2) + '\n')
            print(json.dumps(row), flush=True)
            if not args.keep_caches:
                cache_path = directory / 'cache'
                for attempt in range(3):
                    try:
                        shutil.rmtree(cache_path)
                        break
                    except FileNotFoundError:
                        break
                    except OSError as error:
                        if error.errno != errno.ENOTEMPTY or attempt == 2:
                            raise

summary = {}
for project in projects:
    summary[project] = {}
    for variant in ['baseline', 'candidate']:
        selected = [row for row in rows if row['project'] == project and row['variant'] == variant]
        metrics = {key: round(statistics.median(row[key] for row in selected), 4)
                   for key in ['fresh_wall_s', 'fresh_cpu_s', 'fresh_instructions_G', 'fresh_peak_GB']}
        metrics['rust_wall_s'] = statistics.median(row['rust']['wall_s'] for row in selected)
        metrics['cache_wall_s'] = statistics.median(row['cache']['wall_s'] for row in selected)
        metrics['fresh_wall_range_s'] = [min(row['fresh_wall_s'] for row in selected),
                                       max(row['fresh_wall_s'] for row in selected)]
        summary[project][variant] = metrics
    summary[project]['paired_wall_ratios'] = [
        next(row['fresh_wall_s'] for row in rows if row['project'] == project and row['variant'] == 'candidate' and row['repeat'] == repeat) /
        next(row['fresh_wall_s'] for row in rows if row['project'] == project and row['variant'] == 'baseline' and row['repeat'] == repeat)
        for repeat in range(1, args.repetitions + 1)]
(out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps(summary, indent=2), flush=True)
