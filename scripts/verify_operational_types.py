#!/usr/bin/env python3
"""Run Original type assertions and property contracts against the current Rust build."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', required=True, type=Path)
    out = parser.parse_args().out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    root = Path(__file__).resolve().parents[1]
    otp, proper = (root / '.cache/upstream' / name for name in ('otp', 'proper'))
    sources = json.loads((root / 'corpus/sources.json').read_text())
    for name, directory in (('otp', otp), ('proper', proper)):
        revision = subprocess.check_output(['git', '-C', str(directory), 'rev-parse', 'HEAD'], text=True).strip()
        assert revision == sources[name]['revision'], name
        subprocess.run(['git', '-C', str(directory), 'diff', '--quiet', 'HEAD', '--'], check=True)
    registry = json.loads((root / 'corpus/operational_cases.json').read_text())
    for path, expected in registry['source_sha256'].items():
        assert sha(root / path) == expected, path
    assert (proper / 'ebin/proper.beam').is_file(), 'Build the pinned PropEr dependency first'
    env = dict(os.environ, ASDF_ERLANG_VERSION='28.3', ASDF_ELIXIR_VERSION='1.19.4-otp-28',
               ERL_FLAGS='+S 2:2', CARGO_TARGET_DIR=str(out / 'cargo-target'))
    lib = Path(subprocess.check_output(['erl', '-noshell', '-eval',
        'io:format("~s", [code:lib_dir()]), halt().'], env=env, text=True).strip())
    support = [lib / name for name in ('dialyzer-5.4', 'stdlib-7.2', 'compiler-9.0.4', 'common_test-1.29.1')]
    assert all(p.is_dir() for p in support)
    suite_source = otp / 'lib/dialyzer/test/erl_types_SUITE.erl'
    property_source = otp / 'lib/dialyzer/test/property_test/erl_types_prop.erl'

    def inventory():
        files = [root / 'Cargo.toml', root / 'Cargo.lock', suite_source, property_source]
        for directory in [root / 'src', root / 'scripts', root / 'corpus', proper, *support]:
            files.extend(p for p in directory.rglob('*') if p.is_file() and '.git' not in p.parts)
        return {str(p): sha(p) for p in sorted(set(files))}

    before = inventory()
    subprocess.run(['cargo', 'build', '--release', '--offline', '--locked', '--bin', 'type_probe'],
                   cwd=root, env=env, check=True)
    binary = out / 'cargo-target/release/type_probe'
    binary_hash = sha(binary)
    subprocess.run(['elixir', str(root / 'scripts/type_suite_bridge.exs'), str(otp), str(binary), str(out / 'suite')],
                   cwd=root, env=env, check=True)
    subprocess.run(['elixir', str(root / 'scripts/verify_properties.exs'), str(proper), str(otp),
                    str(out / 'properties'), str(binary), '5000'], cwd=root, env=env, check=True)
    suite = json.loads((out / 'suite/report.json').read_text())
    properties = json.loads((out / 'properties/report.json').read_text())
    expected_cases = {'consistency_and_to_string', 'misc', 'map_multiple_representations'}
    expected_properties = {'absorption', 'associativity', 'commutativity', 'idempotence', 'identity', 'limit'}
    assert suite['requested'] == suite['passed'] == 3
    assert {r['case'] for r in suite['cases']} == expected_cases
    assert not suite['bridge_unsupported'] and suite['native_algebra_fallback'] is False
    assert properties['unsupported'] == 0 and len(properties['properties']) == 6
    assert {r['property'] for r in properties['properties']} == expected_properties
    assert all(r['requested'] == r['passed'] == 5000 and not r['failures'] for r in properties['properties'])
    rows = [dict(id='erl_types_SUITE/' + r['case'], passed=r['passed'],
                 scope='Original suite case assertions; erl_types calls bridged to Rust') for r in suite['cases']]
    rows += [dict(id='erl_types_SUITE/' + r['property'], passed=True, trials=5000,
                  scope='Original property generator and law contract evaluated over Rust; CT property wrapper not executed')
             for r in properties['properties']]
    expected_ids = {r['id'] for r in registry['cases'] if r['feature_set'] in ('lattice_deterministic', 'lattice_properties')}
    assert len(rows) == 9 and {r['id'] for r in rows} == expected_ids
    assert before == inventory() and sha(binary) == binary_hash
    (out / 'cases.json').write_text(json.dumps(rows, indent=2) + '\n')
    report = dict(original_suite_cases=3, original_properties=6, property_trials=30000,
                  source_inputs_binaries_unchanged=True, source_inputs_sha256=before,
                  binary_sha256=binary_hash,
                  scope='Exact Original deterministic assertions and property contracts; no public CLI/cache/CT property wrapper claim')
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print({k: v for k, v in report.items() if not k.endswith('sha256')})


if __name__ == '__main__':
    main()
