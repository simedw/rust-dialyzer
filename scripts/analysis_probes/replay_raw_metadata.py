#!/usr/bin/env python3
"""Replay the frozen original Dialyzer raw metadata cases against this build."""
import argparse
import json
from pathlib import Path
import subprocess

from replay_composed_pipeline import sha


JOBS = (
    ('general-hashes', 'graph_primitives_probe', 'fresh-native-hashes.jsonl', ('matches', 'key_roundtrip')),
    ('primitives', 'graph_primitives_probe', None, ('matches',)),
    ('maps', 'graph_maps_probe', None, ('matches',)),
    ('ets', 'graph_ets_probe', None, ('matches',)),
    ('keys', 'declaration_key_probe', 'fresh-native-keys.jsonl', ('matched', 'optional_keys_matched')),
    ('boundaries', 'target_contract_mode_probe', 'fresh-native-boundaries.jsonl', ('matched_status',)),
    ('groups', 'raw_callback_groups_probe', 'fresh-native-groups.jsonl', ('matched',)),
    ('public', 'raw_declaration_public_probe', 'public-inputs.jsonl', ('matched',)),
    ('public-callback', 'raw_declaration_public_probe', 'public-callback-backend/native.jsonl', ('matched',)),
    ('mode-diagnostics', 'target_mode_diagnostics_probe', 'native-mode-diagnostics.jsonl',
     ('matched', 'callback_order_matched', 'schema_boundaries', 'event_boundaries', 'imported_contracts_matched')),
)


def verify(hashes):
    for path, expected in hashes.items():
        if sha(path) != expected:
            raise ValueError('changed proof input: ' + str(path))


def main():
    parser = argparse.ArgumentParser()
    for name in ('root', 'proof', 'library', 'baselines', 'out'):
        parser.add_argument('--' + name, required=True, type=Path)
    args = parser.parse_args()
    root, proof, library, baselines, out = (getattr(args, n).resolve()
        for n in ('root', 'proof', 'library', 'baselines', 'out'))
    out.mkdir(parents=True, exist_ok=True)
    checkpoint = json.loads((proof / 'checkpoint.json').read_text())
    oracle = {str(proof / p): h for p, h in checkpoint['files'].items()}
    native = json.loads((proof / 'proof/fresh-report.json').read_text())
    assert native['source_freshness'] is True
    original = {p: h for p, h in native['hashes'].items()
                if '/.asdf/installs/erlang/' in p and p.endswith('.beam')}
    assert original, 'missing original Dialyzer BEAM provenance'
    oracle.update(original)
    library_hashes = {h for p, h in native['hashes'].items() if p.endswith('/library-v3.json')}
    assert library_hashes == {sha(library)}, 'library differs from original proof input'
    verify(oracle)
    inputs = {}
    for name, binary, relative, checks in JOBS:
        if relative is None:
            singular = {'primitives': 'primitive', 'maps': 'map', 'ets': 'ets'}[name]
            source = baselines / (singular + '-baseline.jsonl')
        else:
            source = proof / 'proof' / relative
        assert sha(source) == native['results'][name]['input_sha256'], name
        inputs[name] = source
    paths = list((root / 'src').rglob('*.rs')) + [root / 'Cargo.toml', root / 'Cargo.lock',
        Path(__file__).resolve(), Path(__file__).with_name('replay_composed_pipeline.py'),
        proof / 'checkpoint.json', library]
    paths += list(inputs.values())
    paths += [root / 'target/release' / binary for _, binary, _, _ in JOBS]
    before = {str(p): sha(p) for p in paths}
    results = {}
    for name, binary, relative, checks in JOBS:
        command = [str(root / 'target/release' / binary)]
        if name in ('boundaries', 'groups', 'public', 'public-callback', 'mode-diagnostics'):
            command.append(str(library))
        output = out / (name + '.jsonl')
        with inputs[name].open('rb') as stdin, output.open('wb') as stdout:
            subprocess.run(command, stdin=stdin, stdout=stdout, cwd=root, check=True)
        rows = [json.loads(line) for line in output.read_text().splitlines()]
        assert len(rows) == native['results'][name]['cases'], (name, len(rows))
        for index, row in enumerate(rows):
            assert all(row.get(key) is True for key in checks), (name, index, row)
        results[name] = dict(cases=len(rows), output_sha256=sha(output), checks=list(checks))
        print(name, len(rows), 'passed', flush=True)
    verify(before)
    verify(oracle)
    report = dict(results=results, cases=sum(r['cases'] for r in results.values()),
        source_sha256=before, oracle_sha256=oracle,
        scope=dict(full_pipeline=False, duplicate_captured_error=False,
                   nonatom_remote_module_dependencies=False))
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print('verified', report['cases'], 'raw metadata cases', flush=True)


if __name__ == '__main__':
    main()
