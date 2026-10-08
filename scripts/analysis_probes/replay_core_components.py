#!/usr/bin/env python3
"""Verify integrated Core components against frozen original runtime results."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def rows(path):
    return [json.loads(line) for line in Path(path).read_text().splitlines()]


def normalize_graph(value):
    if isinstance(value, dict):
        if 'derived' in value:
            return {'derived': True}
        return {k: normalize_graph(v) for k, v in value.items()
                if k not in ['closure_checks', 'source_state_matched']}
    if isinstance(value, list):
        return [normalize_graph(v) for v in value]
    return value


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--proof', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--kind', choices=['utf', 'cleanup', 'publication', 'overflow'],
                        required=True)
    args = parser.parse_args()
    root, proof, out = [getattr(args, key).resolve() for key in ['root', 'proof', 'out']]
    out.mkdir(parents=True, exist_ok=True)
    manifest = proof / ('SHA256.json' if args.kind == 'cleanup' else 'checkpoint.json')
    checkpoint = json.loads(manifest.read_text())
    frozen = checkpoint.get('sha256', checkpoint.get('files', checkpoint))
    oracle_paths = {str(proof / name): digest for name, digest in frozen.items()
                    if isinstance(digest, str) and len(digest) == 64}
    for path, digest in oracle_paths.items():
        if sha(path) != digest:
            raise ValueError('changed checkpoint: ' + path)
    jobs = []
    if args.kind == 'utf':
        jobs = [('scc', proof / 'proof/native_scc.jsonl', 'generation_probe', []),
                ('generation', proof / 'proof/native_generation.jsonl', 'generation_probe', [])]
    elif args.kind == 'overflow':
        jobs = [('overflow', proof / 'native.jsonl', 'arithmetic_limit_probe', []),
                ('shift', proof / 'shift-native.jsonl', 'arithmetic_limit_probe', [])]
    elif args.kind == 'publication':
        library = Path('/private/tmp/rust-dialyzer-arithmetic-limit-checkpoint-1/library.json')
        jobs = [('publication', proof / 'proof/native.jsonl', 'warning_publication_probe', [library])]
    else:
        historical = json.loads((proof / 'proof/input_hashes.json').read_text())
        oracle_paths.update(historical)
        for path, digest in historical.items():
            if sha(path) != digest:
                raise ValueError('changed original cleanup proof: ' + path)
        native = next(Path(p) for p in historical if p.endswith('/data/native.jsonl'))
        generated = next(Path(p) for p in historical if p.endswith('/data/cases_native.jsonl'))
        jobs = [('upstream', native, 'cleanup_probe', []),
                ('generated', generated, 'cleanup_probe', [])]
    paths = list((root / 'src').rglob('*.rs')) + [
        root / 'Cargo.toml', root / 'Cargo.lock', root / 'corpus/bifs.json',
        root / 'corpus/dependency_policy.json', root / 'corpus/warnings.json',
        Path(__file__).resolve(), manifest,
    ]
    for _, native, binary, extra in jobs:
        paths.extend([native, root / 'target/release' / binary, *extra])
    if args.kind == 'publication':
        paths.extend([proof / 'compare_publication.exs', root / 'scripts/core_export.exs'])
    before = {str(path): sha(path) for path in paths}
    results = {}
    for name, native, binary, extra in jobs:
        actual = out / (name + '.jsonl')
        with native.open('rb') as stdin, actual.open('wb') as stdout:
            subprocess.run([str(root / 'target/release' / binary), *map(str, extra)],
                           stdin=stdin, stdout=stdout, cwd=root, check=True)
        expected, got = rows(native), rows(actual)
        if len(expected) != len(got) or not got:
            raise ValueError((name, 'row count'))
        for index, (left, right) in enumerate(zip(expected, got)):
            if args.kind == 'utf':
                matches = normalize_graph(left['native']) == normalize_graph(right)
                if name == 'scc':
                    matches = matches and right.get('source_state_matched') is True
            elif args.kind == 'overflow':
                matches = left['id'] == right['id'] and right.get('matched') is True
            elif args.kind == 'cleanup':
                matches = left['id'] == right['id']
                if 'error' in left:
                    matches = matches and right.get('error') == {
                        'kind': 'error', 'class': 'function_clause'}
                else:
                    matches = matches and all(right.get(key) is True for key in
                                              ['missing', 'external', 'unknown', 'dependencies'])
            else:
                matches = left['id'] == right['id']
            if not matches:
                raise ValueError((name, index, 'original result differs', right))
        if args.kind == 'publication':
            env = dict(os.environ, ASDF_ERLANG_VERSION='28.3',
                       ASDF_ELIXIR_VERSION='1.19.4-otp-28')
            subprocess.run(['elixir', str(proof / 'compare_publication.exs'),
                            str(root), str(native), str(actual)],
                           cwd=root, env=env, check=True)
        results[name] = {'cases': len(got), 'matches': len(got), 'sha256': sha(actual)}
        if args.kind == 'utf' and name == 'generation':
            captures = sum(row.get('closure_checks', 0) for row in got)
            required = sum(1 for row in expected for function in row['native']['functions']
                           for node in function['graph']['nodes']
                           if node.get('right', {}).get('derived'))
            if captures != required:
                raise ValueError('generation capture count differs')
            results[name]['closure_captures'] = captures
    if before != {str(path): sha(path) for path in paths}:
        raise ValueError('candidate source or binary changed')
    for path, digest in oracle_paths.items():
        if sha(path) != digest:
            raise ValueError('original checkpoint changed')
    report = {'kind': args.kind, 'results': results, 'source_sha256': before,
              'oracle_sha256': oracle_paths, 'whole_pipeline': False}
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print({'kind': args.kind, 'results': results})


if __name__ == '__main__':
    main()
