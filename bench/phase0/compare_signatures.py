#!/usr/bin/env python3
"""Compare every named inferred signature, including gzip archived results.

Canonical JSON strings keep the comparison exact while retaining much less
memory than two full trees of decoded type objects.
"""
import argparse
import gzip
import hashlib
import json
from pathlib import Path


def index(path):
    return index_result(path)[0]


def index_result(path):
    opener = gzip.open if path.suffix == '.gz' else open
    with opener(path, 'rt') as stream:
        result = json.load(stream)
    if 'status' in result:
        assert result['status'] == 'ok', result['status']
    rows = result.pop('signatures')
    keyed = {}
    while rows:
        row = rows.pop()
        key = (row['module'], row['function'], row['arity'])
        assert key not in keyed, f'duplicate named signature: {key}'
        keyed[key] = json.dumps({k: row[k] for k in ('arguments', 'result')},
                                sort_keys=True, separators=(',', ':'))
    return keyed, result


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        while block := stream.read(1024 * 1024):
            h.update(block)
    return h.hexdigest()


def compare(native_path, oracle_path):
    native, oracle = index(native_path), index(oracle_path)
    def mfa(key):
        return dict(zip(('module', 'function', 'arity'), key))
    common = native.keys() & oracle.keys()
    differences = [{**mfa(k), 'native': json.loads(native[k]), 'otp': json.loads(oracle[k])}
                   for k in sorted(common) if native[k] != oracle[k]]
    return {'native_signature_count': len(native), 'otp_signature_count': len(oracle),
            'common_count': len(common), 'exact_matches': len(common) - len(differences),
            'missing_in_native': [mfa(k) for k in sorted(oracle.keys() - native.keys())],
            'extra_in_native': [mfa(k) for k in sorted(native.keys() - oracle.keys())],
            'differences': differences, 'complete_exact_parity': native == oracle,
            'sha256': {str(p): digest(p) for p in (native_path, oracle_path)}}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('native', type=Path)
    parser.add_argument('oracle', type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    report = compare(args.native, args.oracle)
    args.out.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: v for k, v in report.items()
                      if k not in ('differences', 'missing_in_native', 'extra_in_native', 'sha256')}
                     | {'missing_count': len(report['missing_in_native']),
                        'extra_count': len(report['extra_in_native'])}))
    raise SystemExit(0 if report['complete_exact_parity'] else 1)
