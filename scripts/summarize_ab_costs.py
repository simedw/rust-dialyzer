#!/usr/bin/env python3
"""Summarize non-warmup A/B costs and the observed host load, without claiming causality."""
import argparse
import json
from pathlib import Path
import statistics


def summarize(directory):
    rows = json.loads((directory / 'measurements.json').read_text())
    result = {}
    for project in sorted({row['project'] for row in rows}):
        measured = [row for row in rows if row['project'] == project and not row['warmup']]
        if not measured:
            continue
        variants = {}
        for variant in ['baseline', 'candidate']:
            metrics = [row[variant] for row in measured]
            costs = {key: statistics.median(item[key] for item in metrics) for key in
                     ['wall_s', 'user_s', 'sys_s', 'instructions', 'peak_bytes', 'max_rss_bytes']}
            costs['cpu_s'] = statistics.median(item['user_s'] + item['sys_s'] for item in metrics)
            loads = [sample['load'][0] for item in metrics for sample in item['load_samples']]
            loads += [item[key][0] for item in metrics for key in ['load_before', 'load_after']]
            costs['one_minute_load_range'] = [min(loads), max(loads)]
            variants[variant] = costs
        result[project] = {
            **variants,
            'measured_pairs': len(measured),
            'exact_warning_and_signature_parity': all(row['exact_warning_and_signature_parity'] for row in measured),
            'signatures': sorted({row['signatures'] for row in measured}),
            'warnings': sorted({row['warnings'] for row in measured}),
            'baseline_over_candidate': {key: variants['baseline'][key] / variants['candidate'][key]
                                        for key in ['wall_s', 'cpu_s', 'instructions']},
        }
    return {'projects': result, 'caveat': 'Wall times depend on recorded host load and paging. CPU and instruction medians are reported separately; no 3x OTP claim follows from Rust A/B ratios.'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    text = json.dumps(summarize(args.directory), indent=2) + '\n'
    if args.output:
        args.output.write_text(text)
    else:
        print(text, end='')
