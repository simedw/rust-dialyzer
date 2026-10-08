#!/usr/bin/env python3
"""Prepare every pinned compiler case using repository-owned inputs."""
import argparse
import hashlib
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--manifest', default='.cache/core-v6/report.json')
    parser.add_argument('--environment', required=True)
    parser.add_argument('--out', default='.cache/preparation/corpus')
    args = parser.parse_args()
    manifest = pathlib.Path(args.manifest).resolve()
    before = digest(manifest)
    cases = json.loads(manifest.read_text())['cases']
    if len(cases) != 327 or any(c['status'] != 'roundtrip' for c in cases):
        raise ValueError('expected all 327 compiler cases')
    environment = str(pathlib.Path(args.environment).resolve())
    out = pathlib.Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    requests = out / 'requests.jsonl'
    requests.write_text(''.join(json.dumps({
        'case': case['id'], 'core': case['core_json'],
        'environment': environment, 'finalized': True,
    }) + '\n' for case in cases))
    subprocess.run([
        'cargo', 'build', '--offline', '--release',
        '--bin', 'compiled_definitions_probe',
    ], cwd=ROOT, check=True)
    subprocess.run([
        sys.executable, str(ROOT / 'scripts/verify_compiled_definitions.py'),
        '--requests', str(requests),
        '--probe', str(ROOT / 'target/release/compiled_definitions_probe'),
        '--out', str(out),
    ], cwd=ROOT, check=True)
    if digest(manifest) != before:
        raise ValueError('compiler manifest changed during proof')
    report_path = out / 'report.json'
    report = json.loads(report_path.read_text())
    report['inputs_sha256'][str(manifest)] = before
    report['implementation_sha256'][str(pathlib.Path(__file__).relative_to(ROOT))] = digest(pathlib.Path(__file__))
    report_path.write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
