#!/usr/bin/env python3
"""Replay frozen original compiler-mode outcomes against a candidate build."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time

from replay_core_components import normalize_graph, rows, sha


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--proof', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    root, proof, out = (getattr(args, k).resolve() for k in ('root', 'proof', 'out'))
    out.mkdir(parents=True, exist_ok=True)
    manifest = proof / 'manifest.json'
    frozen = {str(proof / p): h for p, h in json.loads(manifest.read_text())['files'].items()}
    assert all(sha(p) == h for p, h in frozen.items()), 'changed original checkpoint'
    library = Path('/private/tmp/rust-dialyzer-diagnostic-adapter/library-v3.json')
    jobs = [('diagnostics', 'target_mode_diagnostics_probe', 288),
            ('boundaries', 'target_contract_mode_probe', 20),
            ('scc', 'generation_probe', 27)]
    paths = list((root / 'src').rglob('*.rs')) + [root / p for p in
             ('Cargo.toml', 'Cargo.lock', 'corpus/bifs.json', 'corpus/dependency_policy.json')]
    paths += [Path(__file__).resolve(), Path(__file__).with_name('replay_core_components.py'),
              manifest, library, Path(json.loads(library.read_text())['source']['path'])]
    paths += [root / 'target/release' / binary for _, binary, _ in jobs]
    before = {str(p): sha(p) for p in paths}
    results = {}
    start = time.monotonic()
    for name, binary, count in jobs:
        native, actual = proof / f'proof/native_{name}.jsonl', out / f'{name}.jsonl'
        with native.open('rb') as stdin, actual.open('wb') as stdout:
            subprocess.run([str(root / 'target/release' / binary)] +
                           ([] if name == 'scc' else [str(library)]),
                           stdin=stdin, stdout=stdout, cwd=root, check=True)
        expected, got = rows(native), rows(actual)
        assert len(expected) == len(got) == count, (name, 'row count')
        result = {'cases': count, 'output_sha256': sha(actual)}
        if name == 'diagnostics':
            assert all(all(r[k] for k in ('matched', 'event_boundaries', 'schema_boundaries',
                       'callback_order_matched', 'imported_contracts_matched')) for r in got)
            result['matches'] = count
        elif name == 'scc':
            assert all(normalize_graph(n['native']) == normalize_graph(r) and
                       r['source_state_matched'] for n, r in zip(expected, got))
            result.update(matches=count, runtime_from_production_constructor=True)
        else:
            gaps = [r['id'] for r in got if not r['matched_status']]
            assert not gaps, gaps
            result.update(ignore_matches=sum(not r['use_contracts'] for r in got),
                          use_status_matches=sum(r['use_contracts'] for r in got),
                          known_raw_metadata_gaps=gaps, exact_error_reasons_claimed=False)
        results[name] = result
    assert before == {str(p): sha(p) for p in paths}, 'candidate changed during replay'
    assert all(sha(p) == h for p, h in frozen.items()), 'original checkpoint changed'
    report = dict(results=results, source_sha256=before, oracle_sha256=frozen,
                  elapsed_seconds=time.monotonic() - start,
                  full_pipeline=False, cli_migration=False)
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(results)


if __name__ == '__main__':
    main()
