#!/usr/bin/env python3
"""Replay a frozen original Dialyzer oracle through the actual Rust pipeline."""
import argparse
import hashlib
from itertools import zip_longest
import json
from pathlib import Path
from replay_jobs import positive_jobs, replay


def sha(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def named(rows):
    result = {
        (r['module'], r['function'], r['arity']): (r['arguments'], r['result'])
        for r in rows
    }
    if len(result) != len(rows):
        raise ValueError('duplicate named signature')
    return result


def main():
    parser = argparse.ArgumentParser()
    for option in ['root', 'proof', 'probe', 'profile', 'out']:
        parser.add_argument('--' + option, required=True)
    parser.add_argument('--case', action='append', default=[])
    parser.add_argument('--jobs', default=1, type=positive_jobs)
    args = parser.parse_args()
    root, proof, probe, profile, out = (
        Path(getattr(args, key)).resolve()
        for key in ['root', 'proof', 'probe', 'profile', 'out']
    )
    out.mkdir(parents=True, exist_ok=True)
    manifest = proof / 'checkpoint.json'
    frozen = json.loads(manifest.read_text())['sha256']
    for filename, expected in frozen.items():
        if sha(proof / filename) != expected:
            raise ValueError('changed oracle checkpoint: ' + filename)
    library = proof / 'library.json'
    baseline = Path(json.loads(library.read_text())['source']['path'])
    sources = list((root / 'src').rglob('*.rs')) + [
        root / 'Cargo.toml', root / 'Cargo.lock', root / 'corpus/bifs.json',
        root / 'corpus/dependency_policy.json', Path(__file__).resolve(),
        profile, library, baseline, probe, manifest,
        Path(__file__).with_name('replay_jobs.py'),
    ]
    before = {str(p): sha(p) for p in sources}
    native = proof / 'pipeline-native.jsonl'
    if args.case:
        selected = out / 'selected-native.jsonl'
        requested = set(args.case)
        found = set()
        with selected.open('w') as output:
            for line in native.open():
                row = json.loads(line)
                if row['id'] in requested:
                    output.write(line)
                    found.add(row['id'])
        if found != requested:
            raise ValueError(('unknown selected cases', requested - found))
        native = selected
        sources.append(native)
        before[str(native)] = sha(native)
    actual = out / 'rust.jsonl'
    replay([str(probe), str(library), str(profile)], native, actual, root, args.jobs)
    exact_combined_order = 0
    cases = signatures = documentation = warnings = 0
    with native.open() as reference, actual.open() as result:
        for expected_line, actual_line in zip_longest(reference, result):
            if expected_line is None or actual_line is None:
                raise ValueError('oracle row count differs')
            left, right = json.loads(expected_line), json.loads(actual_line)
            for key in ['id', 'external_edges', 'raw_warning_count']:
                if left[key] != right[key]:
                    raise ValueError((left['id'], key))
            for key in ['signatures', 'documentation']:
                if named(left[key]) != named(right[key]):
                    raise ValueError((left['id'], key))
            if right.get('raw_module_warnings_match') is not True:
                raise ValueError((left['id'], 'ordered per-module raw warnings'))
            if len(left['source_module_order']) == 1 and right.get('raw_warnings_match') is not True:
                raise ValueError((left['id'], 'single-module combined raw warnings'))
            exact_combined_order += right.get('raw_warnings_match') is True
            cases += 1
            signatures += len(left['signatures'])
            documentation += len(left['documentation'])
            warnings += left['raw_warning_count']
    if cases == 0:
        raise ValueError('empty oracle')
    if before != {str(p): sha(p) for p in sources}:
        raise ValueError('candidate source or input changed during replay')
    for filename, expected_hash in frozen.items():
        if sha(proof / filename) != expected_hash:
            raise ValueError('changed oracle checkpoint: ' + filename)
    report = {
        'cases': cases,
        'named_signatures': signatures,
        'documentation_signatures': documentation,
        'raw_warning_events': warnings,
        'combined_order_matches': exact_combined_order,
        'replay_jobs': args.jobs,
        'source_sha256': before, 'output_sha256': sha(actual),
        'scope': {'cold_pipeline': True, 'original_scheduler_and_workers': True,
                  'selected_cases': args.case, 'complete_checkpoint': not args.case,
                  'target_native_facts_input': False, 'public_warnings': False,
                  'persistence': False, 'ordered_per_module_raw_warnings': True,
                  'native_cross_module_completion_order_is_nondeterministic': True},
    }
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print({k: v for k, v in report.items() if not k.endswith('sha256')})


if __name__ == '__main__':
    main()
