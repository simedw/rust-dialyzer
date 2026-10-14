#!/usr/bin/env python3
"""Report an optimistic reuse bound from RUST_DIALYZER_GENERATION_PROBE.

Equal seeds and propagated reads alone do not establish valid replay. Library
values, generator prefix effects, admission and merge cost can only reduce the
reported bound. Elapsed time is nested worker time, not end-to-end wall savings.
"""
import argparse
import gzip
from collections import Counter
import json
from pathlib import Path


def summarize(path):
    previous = {}
    counts = Counter()
    durations = Counter()
    shape_counts, shape_durations = Counter(), Counter()
    opener = gzip.open(path, 'rt') if path.suffix == '.gz' else path.open()
    for line in opener:
        row = json.loads(line)
        identity = row['identity']
        key = (identity['program'], tuple(map(tuple, identity['scc'])), tuple(identity['entry']))
        old = previous.get(key)
        category = 'first'
        if old is not None:
            same_seeds = all(old['identity'][k] == identity[k] for k in ('fresh', 'synthetic'))
            same_reads = old['reads'] == row['reads']
            category = ('optimistic_hit' if same_seeds and same_reads else
                        'changed_reads' if same_seeds else 'changed_seeds')
            if row['succeeded'] and all(field in row and field in old for field in
                    ('domains', 'shape', 'prefix_constraints', 'library_queries')):
                same_shape = old['shape'] == row['shape'] and old['prefix_constraints'] == row['prefix_constraints']
                same_domains = old['domains'] == row['domains']
                if same_seeds and same_shape:
                    shape_counts['same_shape_any_domain'] += 1
                    shape_durations['same_shape_any_domain'] += row['elapsed_ms']
                if same_seeds and same_shape and same_domains:
                    shape_counts['same_domain_shape'] += 1
                    shape_durations['same_domain_shape'] += row['elapsed_ms']
                    if not same_reads and old['library_queries'] == row['library_queries'] == 0:
                        shape_counts['changed_return_no_library'] += 1
                        shape_durations['changed_return_no_library'] += row['elapsed_ms']
        if not row['succeeded']:
            category = 'failure'
        else:
            previous[key] = row
        counts[category] += 1
        durations[category] += row['elapsed_ms']
    total = sum(durations.values())
    opener.close()
    return {'entries': dict(counts), 'generation_elapsed_ms': dict(durations),
            'optimistic_reusable_fraction': durations['optimistic_hit'] / total if total else 0,
            'scope': 'Sequential generation entries only; excludes parallel chunks and completed SCC memo hits.',
            'shape_candidates': {'entries': dict(shape_counts), 'observed_elapsed_ms': dict(shape_durations),
                'fractions': {key: value / total if total else 0 for key, value in shape_durations.items()}},
            'limitation': 'No library/prefix/admission equality proof. Shape erases values/captures and misses constructor attempts and failure paths. No retained fragments, CPU measurement, or end-to-end speed claim.'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input', type=Path)
    parser.add_argument('--out', type=Path)
    args = parser.parse_args()
    report = summarize(args.input)
    text = json.dumps(report, indent=2) + '\n'
    if args.out:
        args.out.write_text(text)
    print(text, end='')
