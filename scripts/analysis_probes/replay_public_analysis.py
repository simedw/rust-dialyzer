#!/usr/bin/env python3
"""Compare the compiled production entry with original public Dialyzer runs."""
import argparse
import json
from itertools import zip_longest
from pathlib import Path

from replay_composed_pipeline import sha
from replay_jobs import positive_jobs, replay


def verify(hashes):
    for path, expected in hashes.items():
        if sha(path) != expected:
            raise ValueError('changed proof input: ' + str(path))


def main():
    parser = argparse.ArgumentParser()
    for name in ('root', 'proof', 'library', 'profile', 'out'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--jobs', default=1, type=positive_jobs)
    parser.add_argument('--input-archive', type=Path)
    args = parser.parse_args()
    root, proof, library, profile, out = (getattr(args, n).resolve()
        for n in ('root', 'proof', 'library', 'profile', 'out'))
    out.mkdir(parents=True, exist_ok=True)
    frozen = json.loads((proof / 'checkpoint.json').read_text())['sha256']
    oracle = {str(proof / p): h for p, h in frozen.items()}
    provenance = proof / 'public-native.jsonl.provenance.json'
    evidence = json.loads(provenance.read_text())
    assert evidence['actual_public_run'] and evidence['original_beams']
    assert not evidence['native_analysis_facts_as_rust_inputs']
    assert not evidence['checked_update_claimed']
    assert sha(library) in evidence['inputs'].values(), \
        'prior library is not bound to the original public oracle inputs'
    oracle.update(evidence['inputs'])
    archived_inputs = {}
    if args.input_archive:
        archive = args.input_archive.resolve()
        replacements = json.loads(archive.read_text())
        for original, saved in replacements.items():
            assert oracle.get(original) == saved['sha256'], 'archive differs from original provenance'
            target = str(Path(saved['archive']).resolve())
            assert sha(target) == saved['sha256'], 'archived bytes changed'
            oracle.pop(original)
            oracle[target] = saved['sha256']
            archived_inputs[original] = target
    verify(oracle)
    probe = root / 'target/release/compiled_analysis_probe'
    native = proof / 'public-native.jsonl'
    paths = list((root / 'src').rglob('*.rs')) + [root / n for n in
        ('Cargo.toml', 'Cargo.lock', 'corpus/bifs.json', 'corpus/warnings.json',
         'corpus/dependency_policy.json')]
    paths += [Path(__file__).resolve(), Path(__file__).with_name('replay_composed_pipeline.py'),
              Path(__file__).with_name('replay_jobs.py'),
              library, profile, probe, proof / 'checkpoint.json']
    if args.input_archive:
        paths.append(args.input_archive.resolve())
    before = {str(p): sha(p) for p in paths}
    actual = out / 'rust.jsonl'
    replay([str(probe), str(library), str(profile)], native, actual, root, args.jobs)
    cases = warning_count = 0
    with native.open() as reference, actual.open() as result:
        for expected_line, actual_line in zip_longest(reference, result):
            assert expected_line is not None and actual_line is not None, 'oracle row count differs'
            left, right = json.loads(expected_line), json.loads(actual_line)
            assert left['id'] == right.get('id'), (left['id'], right)
            assert right.get('public_warnings_match') is True, (left['id'], right)
            assert left['warning_count'] == right['public_warning_count'], left['id']
            cases += 1
            warning_count += right['public_warning_count']
    assert cases, 'empty oracle'
    verify(before)
    verify(oracle)
    report = dict(cases=cases, exact_public_warning_lists=True,
        jobs=args.jobs,
        warnings=warning_count,
        source_sha256=before, oracle_sha256=oracle, output_sha256=sha(actual),
        archived_original_inputs=archived_inputs,
        scope=dict(actual_original_public_api=True, unchecked_prior=True,
                   physical_persistence=False, checked_update=False,
                   complete_corpus=False, target_native_facts_input=False))
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print({k: v for k, v in report.items() if not k.endswith('sha256')})


if __name__ == '__main__':
    main()
