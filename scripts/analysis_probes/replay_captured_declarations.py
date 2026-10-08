#!/usr/bin/env python3
"""Replay original duplicate errors and shared declaration regressions."""
import argparse
import json
from pathlib import Path
import subprocess

from replay_composed_pipeline import sha
from replay_raw_metadata import verify


def main():
    parser = argparse.ArgumentParser()
    for name in ('root', 'proof', 'raw-proof', 'library', 'out'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--global-proof', type=Path)
    args = parser.parse_args()
    root, proof, raw, library, out = (getattr(args, n).resolve()
        for n in ('root', 'proof', 'raw_proof', 'library', 'out'))
    out.mkdir(parents=True, exist_ok=True)
    oracle = {}
    checkpoints = [proof, raw]
    if args.global_proof:
        checkpoints.append(args.global_proof.resolve())
    for checkpoint in checkpoints:
        manifest = json.loads((checkpoint / 'checkpoint.json').read_text())
        oracle.update({str(checkpoint / p): h for p, h in manifest['files'].items()})
    evidence = json.loads((proof / 'verification.json').read_text())
    oracle.update(evidence['original_beams'])
    provenance = json.loads((proof / 'proof/source-before.json').read_text())
    library_hashes = {h for p, h in provenance.items() if p.endswith('/library-v3.json')}
    assert library_hashes == {sha(library)}, 'original proof library differs'
    verify(oracle)
    jobs = (
        ('captured', 'captured_declaration_probe', 74,
         [proof / 'proof' / p for p in ('native-duplicates.jsonl',
          'native-duplicate-boundaries.jsonl', 'missing-profile-cases.jsonl')], ['matched']),
        ('diagnostics', 'target_mode_diagnostics_probe', 288,
         [raw / 'proof/native-mode-diagnostics.jsonl'], ['matched',
          'callback_order_matched', 'imported_contracts_matched',
          'schema_boundaries', 'event_boundaries']),
        ('groups', 'raw_callback_groups_probe', 17,
         [raw / 'proof/fresh-native-groups.jsonl'], ['matched']),
    )
    if args.global_proof:
        global_proof = args.global_proof.resolve()
        for name in ('source-before.json', 'hamt-source-before.json'):
            source = json.loads((global_proof / 'proof' / name).read_text())
            oracle.update({p: h for p, h in source.items()
                if p.endswith('.beam') and '/.asdf/installs/erlang/' in p})
            hashes = {h for p, h in source.items() if p.endswith('/library-v3.json')}
            assert hashes == {sha(library)}, 'global proof library differs'
        verify(oracle)
        jobs += (('global-finalization', 'global_contract_finalization_probe', 44,
            [global_proof / 'proof' / p for p in ('native-finalization.jsonl',
                'native-hamt.jsonl')], ['matched']),)
    paths = list((root / 'src').rglob('*.rs')) + [root / 'Cargo.toml', root / 'Cargo.lock',
        library, Path(__file__).resolve(), Path(__file__).with_name('replay_composed_pipeline.py'),
        Path(__file__).with_name('replay_raw_metadata.py')]
    for _, binary, _, inputs, _ in jobs:
        paths += inputs + [root / 'target/release' / binary]
    before = {str(p): sha(p) for p in paths}
    results = {}
    for name, binary, count, inputs, checks in jobs:
        source = out / (name + '-inputs.jsonl')
        with source.open('wb') as target:
            for original in inputs:
                with original.open('rb') as rows:
                    for line in rows:
                        target.write(line)
        output = out / (name + '.jsonl')
        with source.open('rb') as stdin, output.open('wb') as stdout:
            subprocess.run([str(root / 'target/release' / binary), str(library)],
                stdin=stdin, stdout=stdout, cwd=root, check=True)
        actual = 0
        with source.open() as expected, output.open() as observed:
            for line in expected:
                reference = json.loads(line)
                result = json.loads(next(observed))
                assert result['id'] == reference['id'], (name, actual)
                assert all(result.get(key) is True for key in checks), (name, result)
                actual += 1
            assert next(observed, None) is None, 'extra result rows'
        assert actual == count, (name, actual, count)
        results[name] = dict(cases=count, checks=checks, input_sha256=sha(source),
            output_sha256=sha(output))
        print(name, count, 'passed', flush=True)
    verify(before)
    verify(oracle)
    report = dict(cases=sum(r['cases'] for r in results.values()), results=results,
        source_sha256=before, oracle_sha256=oracle,
        scope=dict(original_duplicate_boundaries=70, typed_profile_lifecycle_cases=4,
            complete_public_cli_formatter=False,
            global_successful_completion_chronology=bool(args.global_proof),
            concurrent_error_race_selection=False))
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
