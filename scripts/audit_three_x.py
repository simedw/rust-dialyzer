#!/usr/bin/env python3
"""Audit the native PGO benchmark and its exact-output evidence before delivery."""
import argparse
import json
import os
from pathlib import Path
import statistics
import sys

ROOT = Path(__file__).resolve().parents[1]
# Name of the private large benchmark project; set locally.
LARGE_APP = os.environ.get('RUST_DIALYZER_LARGE_APP', 'large-app')
sys.path.insert(0, str(ROOT / 'bench/phase0'))
from three_projects import digest


def audit(report):
    frozen = report / 'frozen'
    pgo = json.loads((report / 'native-pgo/metadata.json').read_text())
    matched = frozen / 'native-pgo-matched'
    metadata = json.loads((matched / 'metadata.json').read_text())
    baseline = json.loads((frozen / 'node-outline-package/metadata.json').read_text())
    assert pgo['source_sha256'] == metadata['source_sha256'] == baseline['source_sha256']
    test_log = (report / 'node-outline-build-test-fixed.log').read_text()
    assert '362 passed; 0 failed; 2 ignored' in test_log
    assert 'Finished `release`' in (report / 'node-outline-all-bins-fixed.log').read_text()
    assert 'Finished `release`' in (report / 'native-pgo/use.stderr').read_text()
    assert pgo['use_flags'].startswith('-Cprofile-use=')
    for path, checksum in pgo['source_sha256'].items():
        assert digest(ROOT / path) == checksum, f'Source changed: {path}'
    for name, checksum in pgo['binary_sha256'].items():
        assert digest(report / 'native-pgo' / name) == checksum, f'Binary changed: {name}'
    assert digest(report / 'native-pgo/merged.profdata') == pgo['profile_sha256']
    assert digest(matched / 'core_cache_analyze') == metadata['binary_sha256'] == pgo['binary_sha256']['core_cache_analyze']
    assert metadata['signature_export'] is False and not metadata['timings_enabled']
    rows = json.loads((matched / 'measurements.json').read_text())
    costs = json.loads((frozen / 'native-pgo-ab/cost-summary.json').read_text())['projects']
    expected = {'elixir': (846, 39126, 215), 'phoenix': (600, 31050, 325),
                LARGE_APP: tuple(int(n) for n in os.environ['RUST_DIALYZER_LARGE_APP_EXPECTED'].split(','))}
    results = {}
    for project, (modules, signatures, warnings) in expected.items():
        measured = [r for r in rows if r['project'] == project and not r['warmup']]
        assert len(measured) == 3, f'{project}: three measured pairs required'
        assert {r['repeat'] for r in measured} == {1, 2, 3}
        assert all(r['raw_warning_parity'] and r['modules'] == modules for r in measured)
        rust = statistics.median(r['rust']['wall_s'] for r in measured)
        otp = statistics.median(r['otp']['wall_s'] for r in measured)
        for r in measured:
            for engine in ['rust', 'otp']:
                m = r[engine]
                assert m['load_samples'] and m['stdout_sha256']
                assert all('swap' in s and 'memory_pressure' in s for s in m['load_samples'])
                output = matched / project / str(r['repeat']) / (engine + '.stdout')
                if output.exists():
                    assert digest(output) == m['stdout_sha256'], f'{project}: output changed'
        if project == LARGE_APP:
            proof = json.loads((report / 'native-pgo-full-large/proof.json').read_text())
            full_meta = json.loads((report / 'native-pgo-full-large/metadata.json').read_text())
            assert full_meta['binary_sha256'] == pgo['binary_sha256']['core_cache_analyze']
            assert proof['modules'] == modules and proof['signatures'] == signatures and proof['warnings'] == warnings
            assert proof['exact_warning_and_signature_parity']
        else:
            proof = costs[project]
            assert proof['exact_warning_and_signature_parity'] and proof['measured_pairs'] == 3
            assert proof['signatures'] == [signatures] and proof['warnings'] == [warnings]
        inventory = frozen / project
        for entry in json.loads((inventory / 'inputs.json').read_text()):
            assert digest(Path(entry['frozen'])) == entry['sha256'], f'{project}: BEAM changed'
        assert digest(inventory / 'options.json') == metadata['resume_contract']['options_sha256'][project]
        assert digest(inventory / 'manifest.json') == metadata['resume_contract']['manifest_sha256'][project]
        assert digest(inventory / 'inputs.json') == metadata['resume_contract']['inventory_sha256'][project]
        loads = [s['load'][0] for r in measured for engine in ['rust', 'otp'] for s in r[engine]['load_samples']]
        results[project] = {'rust_s': rust, 'otp_s': otp, 'speedup': otp / rust,
                            'at_least_3x': otp >= 3 * rust, 'measured_pairs': 3,
                            'modules_inferred': modules, 'full_named_signatures_preserved': signatures,
                            'raw_stock_warning_parity': True, 'load_recorded': True,
                            'one_minute_load_range': [min(loads), max(loads)]}
    assert digest(frozen / 'empty-prior.json') == metadata['resume_contract']['prior_sha256']
    assert digest(ROOT / 'bench/phase0/cold_oracle.exs') == metadata['resume_contract']['oracle_sha256']
    result = {'projects': results, 'all_three_at_least_3x': all(r['at_least_3x'] for r in results.values()),
              'binary_sha256': pgo['binary_sha256'], 'profile_sha256': pgo['profile_sha256'],
              'scope': 'Native frontend and analysis through core_cache_analyze; empty Rust prior and fresh OTP PLT; 14 workers/schedulers; filesystem caches not flushed. Profile-guided build required.',
              'signature_reference': 'Preserved verified Rust T output; warnings also compared with fresh OTP. No claim of new full-signature equality against OTP.',
              'tests': '362 library tests passed, 2 ignored; exact source matches tested T; PGO release analyzer and CLI builds passed.'}
    (report / 'completion-audit.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    args = parser.parse_args()
    result = audit(args.report.resolve())
    print(json.dumps(result, indent=2))
    raise SystemExit(0 if result['all_three_at_least_3x'] else 1)
