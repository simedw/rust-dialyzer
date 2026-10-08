#!/usr/bin/env python3
"""Execute each Original generated registry case with its boundary bridged to Rust."""
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
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    otp = root / '.cache/upstream/otp'
    test = otp / 'lib/dialyzer/test'
    inventory = json.loads((root / 'corpus/inventory.json').read_text())
    suites = inventory['otp_generated_suites']
    assert len(suites) == 22
    operational = json.loads((root / 'corpus/operational_cases.json').read_text())
    assert operational['total'] == len(operational['cases']) == 150
    registered = {r['id'] for r in operational['cases'] if r['feature_set'] == 'suite_registry_consistency'}
    assert registered == {'consistency/' + suite['suite'] for suite in suites}
    for path, expected in operational['source_sha256'].items():
        assert sha(root / path) == expected, path
    source = json.loads((root / 'corpus/sources.json').read_text())['otp']
    assert subprocess.check_output(['git', '-C', str(otp), 'rev-parse', 'HEAD'], text=True).strip() == source['revision']
    subprocess.run(['git', '-C', str(otp), 'diff', '--quiet', 'HEAD', '--'], check=True)
    binary = root / 'target/release/corpus_registry_probe'
    env = dict(os.environ, ASDF_ERLANG_VERSION='28.3', ASDF_ELIXIR_VERSION='1.19.4-otp-28', ERL_FLAGS='+S 2:2')
    lib = Path(subprocess.check_output(['erl', '-noshell', '-eval', 'io:format("~s", [code:lib_dir()]), halt().'], env=env, text=True).strip())
    assert lib.is_dir()
    support = [lib / name for name in ('stdlib-7.2', 'compiler-9.0.4', 'common_test-1.29.1')]
    for directory in support:
        assert directory.is_dir(), directory
    fixtures = [test / (suite['suite'] + '_SUITE_data') for suite in suites]

    def hashes():
        files = [root / 'Cargo.toml', root / 'Cargo.lock', binary,
                 test / 'dialyzer_common.erl', test / 'file_utils.erl', test / 'dialyzer_test_constants.hrl']
        for directory in [root / 'src', root / 'scripts', root / 'corpus', *fixtures, *support]:
            files.extend(p for p in directory.rglob('*') if p.is_file())
        return {str(p): sha(p) for p in sorted(set(files))}

    before = hashes()
    subprocess.run(['elixir', str(root / 'scripts/operational_consistency.exs'),
                    str(root), str(binary), str(out / 'generated')], cwd=root, env=env, check=True)
    rows = json.loads((out / 'generated/cases.json').read_text())
    expected = {'consistency/' + suite['suite'] for suite in suites}
    assert len(rows) == len(expected) and {r['id'] for r in rows} == expected
    assert all(r['passed'] and r['original_generated_case_executed'] and r['omission_exact'] for r in rows)
    assert hashes() == before
    report = dict(cases=22, passed=22, omitted_case_negative_checks=22, exact_original_case_assertions=True,
                  source_inputs_binaries_unchanged=True, source_inputs_binaries_sha256=before,
                  scope='Original generated case functions with new_tests/2 bridged to Rust; registry-only case data supplied directly; no analyzer semantics or PLT setup claim')
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print({k: v for k, v in report.items() if not k.endswith('sha256')})


if __name__ == '__main__':
    main()
