#!/usr/bin/env python3
"""Replay original retained-PLT lifecycle and phase outputs in an integrated build."""
import argparse
import json
from pathlib import Path
import subprocess

from replay_core_components import rows, sha
from replay_composed_pipeline import named


def main():
    parser = argparse.ArgumentParser()
    for name in ('root', 'proof', 'profile', 'out'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    root, proof, profile, out = (getattr(args, n).resolve()
                                for n in ('root', 'proof', 'profile', 'out'))
    out.mkdir(parents=True, exist_ok=True)
    manifest = proof / 'checkpoint.json'
    oracle = {str(proof / p): h for p, h in json.loads(manifest.read_text())['files'].items()}
    assert all(sha(p) == h for p, h in oracle.items()), 'changed original checkpoint'
    library = proof / 'proof/prior.json'
    cold = Path('/private/tmp/rust-dialyzer-composed-oracle-checkpoint-1')
    cold_manifest = cold / 'checkpoint.json'
    oracle.update({str(cold / p): h for p, h in
                   json.loads(cold_manifest.read_text())['sha256'].items()})
    assert all(sha(p) == h for p, h in oracle.items())
    jobs = [('lifecycle', proof / 'proof/lifecycle-native.jsonl', 'core_pipeline_probe',
             [library, profile], 3),
            ('projection', proof / 'proof/lifecycle-native.jsonl', 'import_lifecycle_probe',
             [library], 3),
            ('cold', cold / 'pipeline-native.jsonl', 'core_pipeline_probe',
             [cold / 'library.json', profile], 6)]
    paths = list((root / 'src').rglob('*.rs')) + [root / p for p in
             ('Cargo.toml', 'Cargo.lock', 'corpus/bifs.json', 'corpus/dependency_policy.json')]
    paths += [Path(__file__).resolve(), Path(__file__).with_name('replay_core_components.py'),
              Path(__file__).with_name('replay_composed_pipeline.py'), manifest, cold_manifest]
    for _, native, binary, extra, _ in jobs:
        paths += [native, root / 'target/release' / binary, *extra]
    before = {str(p): sha(p) for p in paths}
    results = {}
    for name, native, binary, extra, count in jobs:
        actual = out / (name + '.jsonl')
        with native.open('rb') as stdin, actual.open('wb') as stdout:
            subprocess.run([str(root / 'target/release' / binary), *map(str, extra)],
                           stdin=stdin, stdout=stdout, cwd=root, check=True)
        expected, got = rows(native), rows(actual)
        assert len(expected) == len(got) == count
        if name == 'projection':
            for row in got:
                assert all(row[k] for k in ('info_exact', 'contracts_exact', 'type_modules_exact',
                           'exports_exact', 'cold_removed_absent', 'cold_callbacks_absent')), row
                if row['id'] == 'checked':
                    assert not row['callbacks_retained'] and not row['removed_contract']
                    assert row['removed_result'] is None and row['prior_current_members'] == 0
                else:
                    assert row['callbacks_retained'] and row['removed_contract']
                    assert row['removed_result'] == {'kind': 'atoms', 'values': ['stale']}
                    assert row['prior_current_members'] > 0
        else:
            for n, row in zip(expected, got):
                assert n['id'] == row['id'] and row['raw_warnings_match']
                assert n['raw_warning_count'] == row['raw_warning_count']
                for field in ('signatures', 'documentation'):
                    assert named(n[field]) == named(row[field]), (n['id'], field)
        results[name] = dict(cases=count, exact=True, output_sha256=sha(actual))
    assert before == {str(p): sha(p) for p in paths}, 'candidate changed'
    assert all(sha(p) == h for p, h in oracle.items()), 'oracle changed'
    report = dict(results=results, source_sha256=before, oracle_sha256=oracle,
                  physical_persistence=False, public_warning_publication=False)
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(results)


if __name__ == '__main__':
    main()
